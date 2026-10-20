import 'reflect-metadata';
import { constants } from 'node:fs';
import * as fs from 'node:fs/promises';
import { createPrivateKey, webcrypto, X509Certificate as NativeCertificate } from 'node:crypto';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import {
  BasicConstraintsExtension,
  ExtendedKeyUsage,
  ExtendedKeyUsageExtension,
  Extension,
  KeyUsageFlags,
  KeyUsagesExtension,
  PemConverter,
  X509Certificate,
  X509CertificateGenerator,
  type X509CertificateCreateParams,
} from '@peculiar/x509';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { loadOrCreateAdminMaterial, type AdminMaterial } from '../src/caddy/certificates.ts';
import { assertProtected, runtimeOwner } from '../src/platform/runtime-security.ts';
import { powershell } from '../src/platform/powershell.ts';
import { protocolErrorSchema } from '../src/protocol/errors.ts';

vi.mock('node:fs/promises', async (original) => {
  const actual = await original<typeof import('node:fs/promises')>();
  return {
    ...actual,
    lstat: vi.fn(actual.lstat),
    open: vi.fn(actual.open),
    unlink: vi.fn(actual.unlink),
  };
});
vi.mock('../src/platform/powershell.ts', async (original) => {
  const actual = await original<typeof import('../src/platform/powershell.ts')>();
  return { ...actual, powershell: vi.fn(actual.powershell) };
});
const actualFs = await vi.importActual<typeof fs>('node:fs/promises');
const actualPowerShell = await vi.importActual<typeof import('../src/platform/powershell.ts')>(
  '../src/platform/powershell.ts',
);
const owner = { id: 'uid:42', uid: 42, elevated: false };
const directory = join(tmpdir(), 'cadder-mocked-admin');
const roles = ['root', 'intermediate', 'client'] as const;
type Role = (typeof roles)[number];
type CertificateChanges = Partial<Extract<X509CertificateCreateParams, { signingKey: CryptoKey }>>;
type File = { bytes: Buffer; uid: number; mode: number; linked?: boolean };
const files = new Map<string, File>();
const handles = new Map<string, ReturnType<typeof fileHandle>>();
const uidDescriptor = Object.getOwnPropertyDescriptor(process, 'getuid');
const day = 24 * 60 * 60 * 1000;
const nativeDirectories: string[] = [];
let writeFailure: 'writeFile' | 'sync' | 'close' | undefined;
let failurePath: string | undefined;

function missing() {
  return Object.assign(new Error('missing'), { code: 'ENOENT' });
}
function diskFailure() {
  return Object.assign(new Error('SECRET private parser payload'), { code: 'ENOSPC' });
}
function fileHandle(path: string) {
  let offset = 0;
  const phases = {
    writeFile: vi.fn(async (data: string) => {
      files.get(path)!.bytes = Buffer.from(data);
      if (writeFailure === 'writeFile' && failurePath === path) throw diskFailure();
    }),
    sync: vi.fn(async () => {
      if (writeFailure === 'sync' && failurePath === path) throw diskFailure();
    }),
    close: vi.fn(async () => {
      if (writeFailure === 'close' && failurePath === path) throw diskFailure();
    }),
    stat: vi.fn(async () => ({
      isFile: (): boolean => true,
      uid: files.get(path)!.uid,
      mode: files.get(path)!.mode,
      size: files.get(path)!.bytes.length,
    })),
    read: vi.fn(async (buffer: Buffer, target: number, length: number) => {
      const source = files.get(path)!.bytes;
      const bytesRead = Math.min(length, source.length - offset);
      source.copy(buffer, target, offset, offset + bytesRead);
      offset += bytesRead;
      return { bytesRead, buffer };
    }),
  };
  return phases;
}

beforeEach(() => {
  files.clear();
  handles.clear();
  writeFailure = undefined;
  failurePath = undefined;
  vi.clearAllMocks();
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  Object.defineProperty(process, 'getuid', { configurable: true, value: () => 42 });
  vi.mocked(powershell).mockResolvedValue('');
  vi.mocked(fs.lstat).mockImplementation(async (path) => {
    const file = files.get(String(path));
    if (!file && String(path).endsWith('.pem')) throw missing();
    return {
      isSymbolicLink: () => file?.linked ?? false,
      isFile: () => !!file,
      isDirectory: () => !file,
      uid: file?.uid ?? 42,
      mode: file?.mode ?? 0o700,
    } as never;
  });
  vi.mocked(fs.open).mockImplementation(async (path, flags) => {
    const name = String(path);
    if (Number(flags) & constants.O_EXCL) {
      if (files.has(name)) throw Object.assign(new Error('exists'), { code: 'EEXIST' });
      files.set(name, { bytes: Buffer.alloc(0), uid: 42, mode: 0o600 });
    } else if (!files.has(name)) throw missing();
    const handle = fileHandle(name);
    handles.set(name, handle);
    return handle as never;
  });
  vi.mocked(fs.unlink).mockImplementation(async (path) => {
    if (!files.delete(String(path))) throw missing();
  });
});
afterEach(async () => {
  vi.restoreAllMocks();
  if (uidDescriptor) Object.defineProperty(process, 'getuid', uidDescriptor);
  else Reflect.deleteProperty(process, 'getuid');
  await Promise.all(
    nativeDirectories.splice(0).map((path) => actualFs.rm(path, { recursive: true, force: true })),
  );
});

async function material(): Promise<AdminMaterial> {
  const result = await loadOrCreateAdminMaterial(directory, owner);
  if (!result.ok) throw new Error(`Material setup failed: ${result.error.code}`);
  return result.value;
}
function text(path: string): string {
  return files.get(path)!.bytes.toString('utf8');
}
function replace(path: string, value: string | Buffer) {
  files.get(path)!.bytes = Buffer.from(value);
}
async function expectFailure(code: string) {
  const result = await loadOrCreateAdminMaterial(directory, owner);
  expect(result).toMatchObject({ ok: false, error: { code, retryable: false } });
  if (result.ok) throw new Error('unexpected material success');
  expect(protocolErrorSchema.safeParse(result.error).success).toBe(true);
  expect(JSON.stringify(result)).not.toMatch(/SECRET|PRIVATE KEY|parser/);
  return result.error;
}
async function reissue(value: AdminMaterial, role: Role, changes: CertificateChanges) {
  const certificate = new X509Certificate(value.certificates[role]);
  const issuer = role === 'client' ? 'intermediate' : 'root';
  const signingKey = await crypto.subtle.importKey(
    'pkcs8',
    PemConverter.decodeFirst(text(value.paths[issuer].key)),
    { name: 'ECDSA', namedCurve: 'P-256' },
    false,
    ['sign'],
  );
  const updated = await X509CertificateGenerator.create(
    {
      subject: certificate.subject,
      issuer: certificate.issuer,
      publicKey: certificate.publicKey,
      signingKey,
      signingAlgorithm: { name: 'ECDSA', hash: 'SHA-256' },
      notBefore: certificate.notBefore,
      notAfter: certificate.notAfter,
      extensions: certificate.extensions,
      ...changes,
    },
    crypto,
  );
  replace(value.paths[role].certificate, updated.toString('pem'));
}

it('generates independent P-256 CAs/client, fixed PEM names, key matches and verified chain', async () => {
  const value = await material();
  const publicKeys = roles.map((role) =>
    new NativeCertificate(value.certificates[role]).publicKey
      .export({ format: 'der', type: 'spki' })
      .toString('base64'),
  );
  expect(new Set(publicKeys).size).toBe(3);
  for (const role of roles) {
    const parsed = new X509Certificate(value.certificates[role]);
    const native = new NativeCertificate(value.certificates[role]);
    const issuer = new NativeCertificate(
      value.certificates[role === 'client' ? 'intermediate' : 'root'],
    );
    expect(native.verify(issuer.publicKey)).toBe(true);
    expect(native.checkPrivateKey(createPrivateKey(text(value.paths[role].key)))).toBe(true);
    expect(native.publicKey.asymmetricKeyDetails?.namedCurve).toBe('prime256v1');
    expect(parsed.getExtension(BasicConstraintsExtension)).toMatchObject({
      ca: role !== 'client',
      critical: true,
      pathLength: role === 'root' ? 1 : role === 'intermediate' ? 0 : undefined,
    });
    expect(parsed.getExtension(KeyUsagesExtension)).toMatchObject({
      usages:
        role === 'client'
          ? KeyUsageFlags.digitalSignature
          : KeyUsageFlags.keyCertSign | KeyUsageFlags.cRLSign,
      critical: true,
    });
    expect(parsed.getExtension(ExtendedKeyUsageExtension)?.usages).toEqual(
      role === 'client' ? [ExtendedKeyUsage.clientAuth] : undefined,
    );
    expect(parsed.notBefore.getTime()).toBeLessThanOrEqual(Date.now());
    expect(Math.round((parsed.notAfter.getTime() - Date.now()) / day)).toBe(
      (role === 'root' ? 10 : role === 'intermediate' ? 5 : 1) * 365,
    );
    expect(value.paths[role]).toEqual({
      certificate: join(directory, `${role}.crt.pem`),
      key: join(directory, `${role}.key.pem`),
    });
  }
  expect(value.authorizedClientCertificateBase64).toBe(
    new X509Certificate(value.certificates.client).toString('base64'),
  );
  expect(value.tls).toEqual({
    ca: value.certificates.root,
    cert: `${value.certificates.client}\n${value.certificates.intermediate}`,
    key: text(value.paths.client.key),
  });
  expect(files.size).toBe(6);
});

it('reuses existing complete identity without writes or regeneration', async () => {
  const first = await material();
  const before = [...files].map(([path, file]) => [path, file.bytes.toString('base64')]);
  const generate = vi.spyOn(X509CertificateGenerator, 'create');
  const reused = await material();
  expect(reused).toEqual(first);
  expect(generate).not.toHaveBeenCalled();
  expect([...files].map(([path, file]) => [path, file.bytes.toString('base64')])).toEqual(before);
  for (const handle of handles.values()) {
    expect(handle.writeFile).not.toHaveBeenCalled();
    expect(handle.close).toHaveBeenCalledOnce();
  }
});

it('uses the available no-follow flag when opening existing PEM files', async () => {
  await material();
  vi.mocked(fs.open).mockClear();
  await material();
  expect(fs.open).toHaveBeenCalledTimes(6);
  for (const [, flags] of vi.mocked(fs.open).mock.calls) {
    expect(flags).toBe(constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | constants.O_NONBLOCK);
    expect(Number(flags) & constants.O_EXCL).toBe(0);
  }
});

it('redacts generation exceptions without creating files', async () => {
  vi.spyOn(X509CertificateGenerator, 'create').mockRejectedValueOnce(diskFailure());
  await expectFailure('caddy_material_generation_failed');
  expect(files.size).toBe(0);
  expect(fs.open).not.toHaveBeenCalled();
});

it('validates generated material before persisting, including independent keys', async () => {
  const keys = await webcrypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, [
    'sign',
    'verify',
  ]);
  vi.spyOn(webcrypto.subtle, 'generateKey').mockResolvedValue(keys);
  await expectFailure('caddy_material_invalid');
  expect(files.size).toBe(0);
  expect(fs.open).not.toHaveBeenCalled();
});

it.each(roles)('fails closed on expired %s without rotation', async (role) => {
  const value = await material();
  const expiry = new X509Certificate(value.certificates[role]).notAfter.getTime();
  vi.spyOn(Date, 'now').mockReturnValue(expiry);
  await expectFailure('caddy_material_validity');
  expect(fs.unlink).not.toHaveBeenCalled();
  expect(files.size).toBe(6);
});
it.each(roles)('rejects not-yet-valid %s', async (role) => {
  const value = await material();
  await reissue(value, role, { notBefore: new Date(Date.now() + day) });
  await expectFailure('caddy_material_validity');
});

it.each(
  roles.flatMap((role) => (['certificate', 'key'] as const).map((field) => ({ role, field }))),
)('refuses malformed $role $field and leaves it unchanged', async ({ role, field }) => {
  const value = await material();
  replace(value.paths[role][field], 'PRIVATE KEY SECRET parser payload');
  const before = text(value.paths[role][field]);
  await expectFailure('caddy_material_invalid');
  expect(text(value.paths[role][field])).toBe(before);
  expect(fs.unlink).not.toHaveBeenCalled();
});
it.each(['certificate', 'key'] as const)(
  'rejects oversized %s before allocating/reading it',
  async (field) => {
    const value = await material();
    replace(value.paths.root[field], Buffer.alloc(16 * 1024 + 1));
    await expectFailure('caddy_material_oversized');
    expect(handles.get(value.paths.root[field])!.read).not.toHaveBeenCalled();
  },
);
it('bounds a growing file even if stat reported a small size', async () => {
  const value = await material();
  replace(value.paths.root.certificate, Buffer.alloc(32 * 1024));
  const open = vi.mocked(fs.open).getMockImplementation()!;
  vi.mocked(fs.open).mockImplementation(async (path, flags, mode) => {
    const handle = await open(path, flags, mode);
    if (path === value.paths.root.certificate)
      handles
        .get(String(path))!
        .stat.mockResolvedValue({ isFile: () => true, uid: 42, mode: 0o600, size: 1 });
    return handle;
  });
  await expectFailure('caddy_material_oversized');
  expect(handles.get(value.paths.root.certificate)!.read.mock.calls[0]![2]).toBe(16 * 1024 + 1);
});
it('rejects invalid UTF-8, extra PEM blocks and empty files', async () => {
  const value = await material();
  for (const bad of [
    Buffer.from([0xff]),
    '',
    `${value.certificates.root}\n${value.certificates.root}`,
  ]) {
    replace(value.paths.root.certificate, bad);
    await expectFailure('caddy_material_invalid');
  }
});
it.each(['certificate', 'key'] as const)('rejects PEM headers on %s', async (field) => {
  const value = await material();
  const path = value.paths.root[field];
  const pem = PemConverter.encode(
    PemConverter.decodeFirst(text(path)),
    field === 'certificate' ? 'CERTIFICATE' : 'PRIVATE KEY',
  ).replace(/^(-----BEGIN [^-]+-----)/, '$1\nProc-Type: 4,ENCRYPTED\n');
  expect(PemConverter.decodeWithHeaders(pem)[0]!.headers).toEqual([
    { key: 'Proc-Type', value: '4,ENCRYPTED' },
  ]);
  replace(path, pem);
  await expectFailure('caddy_material_invalid');
  expect(text(path)).toBe(pem);
  expect(fs.unlink).not.toHaveBeenCalled();
});

it.each(['certificate', 'key'] as const)(
  'redacts valid-PEM malformed DER %s parser exceptions',
  async (field) => {
    const value = await material();
    replace(
      value.paths.root[field],
      PemConverter.encode(
        Buffer.from('SECRET parser details'),
        field === 'certificate' ? 'CERTIFICATE' : 'PRIVATE KEY',
      ),
    );
    await expectFailure('caddy_material_invalid');
    expect(fs.unlink).not.toHaveBeenCalled();
  },
);
it('refuses a correctly signed/matched client with a wrong EC curve', async () => {
  const value = await material();
  const keys = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-384' }, true, [
    'sign',
    'verify',
  ]);
  await reissue(value, 'client', { publicKey: keys.publicKey });
  replace(
    value.paths.client.key,
    PemConverter.encode(await crypto.subtle.exportKey('pkcs8', keys.privateKey), 'PRIVATE KEY'),
  );
  await expectFailure('caddy_material_invalid');
});

it.each(roles)('refuses mismatched %s key', async (role) => {
  const value = await material();
  replace(value.paths[role].key, text(value.paths[role === 'root' ? 'client' : 'root'].key));
  await expectFailure('caddy_material_invalid');
});
it.each([1, 3, 5])(
  'refuses a partial %i-file bundle without touching existing files',
  async (count) => {
    await material();
    for (const path of [...files.keys()].slice(count)) files.delete(path);
    const before = [...files.keys()];
    await expectFailure('caddy_material_incomplete');
    expect([...files.keys()]).toEqual(before);
    expect(fs.unlink).not.toHaveBeenCalled();
  },
);

it.each([
  'ca',
  'path',
  'usage',
  'eku',
  'duplicate',
  'critical',
  'subject',
  'issuer',
  'signature',
  'containment',
] as const)('rejects invalid chain/profile: %s', async (fault) => {
  const value = await material();
  const role =
    fault === 'ca' || fault === 'path' || fault === 'usage' || fault === 'eku'
      ? 'intermediate'
      : 'client';
  const certificate = new X509Certificate(value.certificates[role]);
  let extensions = certificate.extensions;
  if (fault === 'ca' || fault === 'path')
    extensions = extensions
      .filter((extension) => !(extension instanceof BasicConstraintsExtension))
      .concat(
        new BasicConstraintsExtension(fault !== 'ca', fault === 'path' ? 1 : undefined, true),
      );
  if (fault === 'usage')
    extensions = extensions
      .filter((extension) => !(extension instanceof KeyUsagesExtension))
      .concat(new KeyUsagesExtension(KeyUsageFlags.digitalSignature, true));
  if (fault === 'eku')
    extensions = [
      ...extensions,
      new ExtendedKeyUsageExtension([ExtendedKeyUsage.clientAuth], true),
    ];
  if (fault === 'duplicate')
    extensions = [...extensions, new BasicConstraintsExtension(false, undefined, true)];
  if (fault === 'critical')
    extensions = [
      ...extensions,
      new Extension('1.2.3.4', true, new BasicConstraintsExtension(false).value),
    ];
  const changes: CertificateChanges = { extensions };
  if (fault === 'subject') changes.subject = 'CN=Another client';
  if (fault === 'issuer') changes.issuer = 'CN=Another issuer';
  if (fault === 'containment') changes.notAfter = new Date(Date.now() + 6 * 365 * day);
  if (fault === 'signature')
    changes.signingKey = await crypto.subtle.importKey(
      'pkcs8',
      PemConverter.decodeFirst(text(value.paths.root.key)),
      { name: 'ECDSA', namedCurve: 'P-256' },
      false,
      ['sign'],
    );
  await reissue(value, role, changes);
  await expectFailure('caddy_material_invalid');
});
it.each(roles)(
  'verifies the %s certificate signature rather than trusting issuer names',
  async (role) => {
    const value = await material();
    const wrongSigner = role === 'client' ? 'root' : 'client';
    const signingKey = await crypto.subtle.importKey(
      'pkcs8',
      PemConverter.decodeFirst(text(value.paths[wrongSigner].key)),
      { name: 'ECDSA', namedCurve: 'P-256' },
      false,
      ['sign'],
    );
    await reissue(value, role, { signingKey });
    await expectFailure('caddy_material_invalid');
  },
);

it.each([
  'missing-usage',
  'ca',
  'missing-eku',
  'server-eku',
  'noncritical-basic',
  'noncritical-usage',
  'noncritical-eku',
  'wrong-signature-algorithm',
] as const)('rejects invalid authorized client: %s', async (fault) => {
  const value = await material();
  let extensions = new X509Certificate(value.certificates.client).extensions;
  if (fault === 'missing-usage' || fault === 'noncritical-usage')
    extensions = extensions.filter((extension) => !(extension instanceof KeyUsagesExtension));
  if (fault === 'noncritical-usage')
    extensions.push(new KeyUsagesExtension(KeyUsageFlags.digitalSignature, false));
  if (fault === 'ca' || fault === 'noncritical-basic')
    extensions = extensions.filter(
      (extension) => !(extension instanceof BasicConstraintsExtension),
    );
  if (fault === 'ca') extensions.push(new BasicConstraintsExtension(true, 0, true));
  if (fault === 'noncritical-basic') extensions.push(new BasicConstraintsExtension(false));
  if (fault === 'missing-eku' || fault === 'server-eku' || fault === 'noncritical-eku')
    extensions = extensions.filter(
      (extension) => !(extension instanceof ExtendedKeyUsageExtension),
    );
  if (fault === 'server-eku')
    extensions.push(new ExtendedKeyUsageExtension([ExtendedKeyUsage.serverAuth]));
  if (fault === 'noncritical-eku')
    extensions.push(new ExtendedKeyUsageExtension([ExtendedKeyUsage.clientAuth], false));
  await reissue(value, 'client', {
    extensions,
    ...(fault === 'wrong-signature-algorithm'
      ? { signingAlgorithm: { name: 'ECDSA', hash: 'SHA-384' } }
      : {}),
  });
  await expectFailure('caddy_material_invalid');
});

it.each(['link', 'owner', 'mode'] as const)(
  'denies unsafe existing %s without writes, repairs or cleanup',
  async (fault) => {
    const value = await material();
    const file = files.get(value.paths.root.certificate)!;
    if (fault === 'link') file.linked = true;
    if (fault === 'owner') file.uid = 43;
    if (fault === 'mode') file.mode = 0o644;
    vi.mocked(fs.open).mockClear();
    expect((await expectFailure('caddy_material_unsafe')).kind).toBe('accessDenied');
    expect(fs.open).not.toHaveBeenCalled();
    expect(fs.unlink).not.toHaveBeenCalled();
  },
);
it('rejects linked ancestors before looking for or creating files', async () => {
  const inspect = vi.mocked(fs.lstat).getMockImplementation()!;
  vi.mocked(fs.lstat).mockImplementation(async (path, options) => {
    if (String(path) === dirname(directory))
      return { isSymbolicLink: () => true, uid: 42 } as never;
    return inspect(path, options);
  });
  await expectFailure('caddy_material_unsafe');
  expect(fs.open).not.toHaveBeenCalled();
  expect(files.size).toBe(0);
});
it('denies Windows ACL failures without repairing them', async () => {
  vi.spyOn(process, 'platform', 'get').mockReturnValue('win32');
  vi.mocked(powershell).mockRejectedValueOnce(new Error('unsafe ACL SECRET'));
  await expectFailure('caddy_material_unsafe');
  expect(fs.open).not.toHaveBeenCalled();
  expect(powershell).toHaveBeenCalledOnce();
});

it.each(['writeFile', 'sync', 'close'] as const)(
  'cleans only exclusively created paths after %s failure',
  async (phase) => {
    const unrelated = join(directory, 'unrelated.pem');
    files.set(unrelated, { bytes: Buffer.from('unowned'), uid: 43, mode: 0o644 });
    writeFailure = phase;
    failurePath = join(directory, 'intermediate.key.pem');
    await expectFailure('caddy_material_io');
    expect([...files.keys()]).toEqual([unrelated]);
    expect(fs.unlink).not.toHaveBeenCalledWith(unrelated);
    expect(handles.get(failurePath)!.close).toHaveBeenCalledOnce();
    expect(fs.unlink).toHaveBeenCalledTimes(4);
  },
);
it('never removes a pre-existing concurrent file after an exclusive-create conflict', async () => {
  const conflict = join(directory, 'intermediate.key.pem');
  const open = vi.mocked(fs.open).getMockImplementation()!;
  vi.mocked(fs.open).mockImplementation(async (path, flags, mode) => {
    if (path === conflict)
      files.set(conflict, { bytes: Buffer.from('pre-existing'), uid: 42, mode: 0o600 });
    return open(path, flags, mode);
  });
  await expectFailure('caddy_material_conflict');
  expect([...files.keys()]).toEqual([conflict]);
  expect(text(conflict)).toBe('pre-existing');
  expect(fs.unlink).not.toHaveBeenCalledWith(conflict);
});
it('reports cleanup failure while attempting every owned path and preserving unowned data', async () => {
  writeFailure = 'sync';
  failurePath = join(directory, 'intermediate.key.pem');
  const unlink = vi.mocked(fs.unlink).getMockImplementation()!;
  vi.mocked(fs.unlink).mockImplementation(async (path) => {
    if (path === join(directory, 'root.key.pem')) throw diskFailure();
    return unlink(path);
  });
  await expectFailure('caddy_material_cleanup_failed');
  expect([...files.keys()]).toEqual([join(directory, 'root.key.pem')]);
  expect(fs.unlink).toHaveBeenCalledWith(join(directory, 'root.crt.pem'));
});
it('tolerates an already-removed owned file while cleaning remaining owned paths', async () => {
  writeFailure = 'sync';
  failurePath = join(directory, 'intermediate.key.pem');
  const unlink = vi.mocked(fs.unlink).getMockImplementation()!;
  vi.mocked(fs.unlink).mockImplementation(async (path) => {
    if (path === join(directory, 'root.key.pem')) files.delete(String(path));
    return unlink(path);
  });
  await expectFailure('caddy_material_io');
  expect(files.size).toBe(0);
  expect(fs.unlink).toHaveBeenCalledTimes(4);
});

it('does not claim ownership after failed open or helper cleanup failure', async () => {
  vi.mocked(fs.open).mockRejectedValueOnce(diskFailure());
  await expectFailure('caddy_material_io');
  expect(fs.unlink).not.toHaveBeenCalled();
  writeFailure = 'writeFile';
  failurePath = join(directory, 'root.crt.pem');
  vi.mocked(fs.unlink).mockRejectedValueOnce(diskFailure());
  // The helper's aggregate error does not expose which cleanup phase failed.
  // Do not invent a precise classification or claim its residual file as ours.
  const error = await expectFailure('caddy_material_io');
  expect(error.guidance).toContain('may leave partial files');
  expect(fs.unlink).toHaveBeenCalledTimes(1);
  expect(files.size).toBe(1);
});
it.each(['read', 'close', 'stat'] as const)(
  'returns redacted I/O failures on existing-file %s',
  async (phase) => {
    const value = await material();
    const open = vi.mocked(fs.open).getMockImplementation()!;
    vi.mocked(fs.open).mockImplementation(async (path, flags, mode) => {
      const handle = await open(path, flags, mode);
      if (path === value.paths.root.certificate)
        handles.get(String(path))![phase].mockRejectedValueOnce(diskFailure());
      return handle;
    });
    await expectFailure('caddy_material_io');
    expect(handles.get(value.paths.root.certificate)!.close).toHaveBeenCalledOnce();
    expect(fs.unlink).not.toHaveBeenCalled();
  },
);
it('validates the opened object rather than trusting pre-open path checks', async () => {
  const value = await material();
  const open = vi.mocked(fs.open).getMockImplementation()!;
  vi.mocked(fs.open).mockImplementation(async (path, flags, mode) => {
    const handle = await open(path, flags, mode);
    if (path === value.paths.root.certificate)
      handles
        .get(String(path))!
        .stat.mockResolvedValue({ isFile: () => false, uid: 42, mode: 0o600, size: 1 });
    return handle;
  });
  await expectFailure('caddy_material_unsafe');
  expect(handles.get(value.paths.root.certificate)!.read).not.toHaveBeenCalled();
});

it(
  'creates, reuses and checks all six permissions in actual owned temporary runtime data',
  { timeout: 60_000 },
  async () => {
    vi.restoreAllMocks();
    if (uidDescriptor) Object.defineProperty(process, 'getuid', uidDescriptor);
    else Reflect.deleteProperty(process, 'getuid');
    vi.mocked(fs.lstat).mockImplementation(actualFs.lstat);
    vi.mocked(fs.open).mockImplementation(actualFs.open);
    vi.mocked(fs.unlink).mockImplementation(actualFs.unlink);
    vi.mocked(powershell).mockImplementation(actualPowerShell.powershell);
    const root = await actualFs.mkdtemp(join(tmpdir(), 'cadder-admin-material-'));
    nativeDirectories.push(root);
    const nativeOwner = await runtimeOwner(
      process.platform !== 'win32' && process.getuid?.() === 0 ? 0 : undefined,
    );
    const target = join(root, 'material');
    const first = await loadOrCreateAdminMaterial(target, nativeOwner);
    expect(first.ok).toBe(true);
    if (!first.ok) throw new Error(`Native creation refused: ${first.error.code}`);
    await assertProtected(target, nativeOwner, true);
    for (const paths of Object.values(first.value.paths))
      for (const path of Object.values(paths)) await assertProtected(path, nativeOwner);
    const reused = await loadOrCreateAdminMaterial(target, nativeOwner);
    expect(reused).toEqual(first);
    if (process.platform !== 'win32') {
      await actualFs.chmod(first.value.paths.client.key, 0o644);
      const unsafe = await loadOrCreateAdminMaterial(target, nativeOwner);
      expect(unsafe).toMatchObject({ ok: false, error: { kind: 'accessDenied' } });
      expect((await actualFs.lstat(first.value.paths.client.key)).mode & 0o777).toBe(0o644);
    }
  },
);
