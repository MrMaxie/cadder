import 'reflect-metadata';
import { webcrypto } from 'node:crypto';
import { constants } from 'node:fs';
import { open, unlink } from 'node:fs/promises';
import { join } from 'node:path';
import {
  AuthorityKeyIdentifierExtension,
  BasicConstraintsExtension,
  ExtendedKeyUsage,
  ExtendedKeyUsageExtension,
  KeyUsageFlags,
  KeyUsagesExtension,
  PemConverter,
  SubjectKeyIdentifierExtension,
  X509Certificate,
  X509CertificateGenerator,
} from '@peculiar/x509';
import type { PortResult, RuntimeOwner } from '../contracts/ports.ts';
import {
  assertProtected,
  createProtectedFile,
  prepareRuntime,
} from '../platform/runtime-security.ts';
import { errorCode, type ProtocolError } from '../protocol/errors.ts';

// SAFETY: Node 24 WebCrypto implements the standard Crypto API expected by x509;
// its declaration uses Node-specific BufferSource types instead of DOM ones.
const crypto = webcrypto as unknown as Crypto;
const curve = { name: 'ECDSA', namedCurve: 'P-256' };
const signingAlgorithm = { name: 'ECDSA', hash: 'SHA-256' };
const day = 24 * 60 * 60 * 1000;
const roles = ['root', 'intermediate', 'client'] as const;
type Role = (typeof roles)[number];
type PemIdentity = Readonly<{ certificate: string; key: string }>;
type Bundle = Record<Role, PemIdentity>;
const maxFileBytes = 16 * 1024;
const issuerRoles = { root: 'root', intermediate: 'root', client: 'intermediate' } as const;
const pathLengths = { root: 1, intermediate: 0, client: undefined };
const caUsage = KeyUsageFlags.keyCertSign | KeyUsageFlags.cRLSign;
const filenames = {
  root: { certificate: 'root.crt.pem', key: 'root.key.pem' },
  intermediate: { certificate: 'intermediate.crt.pem', key: 'intermediate.key.pem' },
  client: { certificate: 'client.crt.pem', key: 'client.key.pem' },
} as const;

export type AdminMaterial = Readonly<{
  paths: Readonly<Record<Role, PemIdentity>>;
  certificates: Readonly<Record<Role, string>>;
  /** Caddy access_control.public_keys expects leaf certificate DER, not SPKI. */
  authorizedClientCertificateBase64: string;
  /** Internal TLS adapter credentials; never log or include this object in diagnostics. */
  tls: Readonly<{ ca: string; cert: string; key: string }>;
}>;

const names: Record<Role, string> = {
  root: 'CN=Cadder Admin Root',
  intermediate: 'CN=Cadder Admin Intermediate',
  client: 'CN=Cadder Admin Client',
};

class MaterialError extends Error {
  constructor(readonly code: string) {
    super('Caddy administration material was refused.');
  }
}

function pemData(pem: string, tag: string): ArrayBuffer {
  // Accept only one complete PEM block, not parser-tolerated trailing material.
  const blocks = PemConverter.decodeWithHeaders(pem);
  if (
    blocks.length !== 1 ||
    blocks[0]!.type !== tag ||
    blocks[0]!.headers.length !== 0 ||
    PemConverter.encode(blocks[0]!.rawData, tag).replace(/\s/g, '') !== pem.replace(/\s/g, '')
  )
    throw new MaterialError('caddy_material_invalid');
  return blocks[0]!.rawData;
}

async function generateBundle(): Promise<Bundle> {
  // Fixed validity policy: 365-day years, root 10 years, intermediate 5,
  // client 1. Backdate five minutes for initial skew. Caddy owns repeated
  // short-lived server issuance; this module never rotates expired material.
  const now = Date.now();
  const notBefore = new Date(now - 5 * 60 * 1000);
  const years = { root: 10, intermediate: 5, client: 1 };
  const keys = {} as Record<Role, CryptoKeyPair>;
  const bundle = {} as Bundle;
  for (const role of roles) {
    keys[role] = await crypto.subtle.generateKey(curve, true, ['sign', 'verify']);
    const issuer = issuerRoles[role];
    const ca = role !== 'client';
    const certificate = await X509CertificateGenerator.create(
      {
        subject: names[role],
        issuer: names[issuer],
        publicKey: keys[role].publicKey,
        signingKey: keys[issuer].privateKey,
        signingAlgorithm,
        notBefore,
        notAfter: new Date(now + years[role] * 365 * day),
        extensions: [
          new BasicConstraintsExtension(ca, pathLengths[role], true),
          new KeyUsagesExtension(ca ? caUsage : KeyUsageFlags.digitalSignature, true),
          ...(ca ? [] : [new ExtendedKeyUsageExtension([ExtendedKeyUsage.clientAuth], true)]),
          await SubjectKeyIdentifierExtension.create(keys[role].publicKey, false, crypto),
          await AuthorityKeyIdentifierExtension.create(keys[issuer].publicKey, false, crypto),
        ],
      },
      crypto,
    );
    bundle[role] = {
      certificate: certificate.toString('pem'),
      key: PemConverter.encode(
        await crypto.subtle.exportKey('pkcs8', keys[role].privateKey),
        'PRIVATE KEY',
      ),
    };
  }
  return bundle;
}

async function validateBundle(bundle: Bundle): Promise<Record<Role, X509Certificate>> {
  try {
    const certificates = {} as Record<Role, X509Certificate>;
    const publicKeys: string[] = [];
    const now = Date.now();
    for (const role of roles) {
      const certificate = new X509Certificate(pemData(bundle[role].certificate, 'CERTIFICATE'));
      certificates[role] = certificate;
      const ca = role !== 'client';
      const basic = certificate.getExtension(BasicConstraintsExtension);
      const usage = certificate.getExtension(KeyUsagesExtension);
      const eku = certificate.getExtension(ExtendedKeyUsageExtension);
      const extensionTypes = certificate.extensions.map((extension) => extension.type);
      if (
        certificate.subject !== names[role] ||
        certificate.signatureAlgorithm.name !== 'ECDSA' ||
        certificate.signatureAlgorithm.hash.name !== 'SHA-256' ||
        !basic?.critical ||
        basic.ca !== ca ||
        basic.pathLength !== pathLengths[role] ||
        !usage?.critical ||
        usage.usages !== (ca ? caUsage : KeyUsageFlags.digitalSignature) ||
        (ca
          ? eku !== null
          : !eku ||
            !eku.critical ||
            eku.usages.length !== 1 ||
            eku.usages[0] !== ExtendedKeyUsage.clientAuth) ||
        new Set(extensionTypes).size !== extensionTypes.length ||
        certificate.extensions.some(
          (extension) =>
            extension.critical &&
            !(
              extension instanceof BasicConstraintsExtension ||
              extension instanceof KeyUsagesExtension ||
              extension instanceof ExtendedKeyUsageExtension
            ),
        )
      )
        throw new MaterialError('caddy_material_invalid');
      if (
        !Number.isFinite(certificate.notBefore.getTime()) ||
        !Number.isFinite(certificate.notAfter.getTime()) ||
        certificate.notBefore.getTime() > now ||
        certificate.notAfter.getTime() <= now
      )
        throw new MaterialError('caddy_material_validity');
      const publicKey = await certificate.publicKey.export(curve, ['verify'], crypto);
      publicKeys.push(Buffer.from(certificate.publicKey.rawData).toString('base64'));
      const privateKey = await crypto.subtle.importKey(
        'pkcs8',
        pemData(bundle[role].key, 'PRIVATE KEY'),
        curve,
        false,
        ['sign'],
      );
      const challenge = crypto.getRandomValues(new Uint8Array(32));
      const signature = await crypto.subtle.sign(signingAlgorithm, privateKey, challenge);
      if (!(await crypto.subtle.verify(signingAlgorithm, publicKey, signature, challenge)))
        throw new MaterialError('caddy_material_invalid');
    }
    if (new Set(publicKeys).size !== roles.length)
      throw new MaterialError('caddy_material_invalid');
    for (const role of roles) {
      const certificate = certificates[role];
      const issuer = certificates[issuerRoles[role]];
      if (
        certificate.issuer !== issuer.subject ||
        certificate.notBefore < issuer.notBefore ||
        certificate.notAfter > issuer.notAfter ||
        !(await certificate.verify({ publicKey: issuer.publicKey, signatureOnly: true }, crypto))
      )
        throw new MaterialError('caddy_material_invalid');
    }
    return certificates;
  } catch (error) {
    if (error instanceof MaterialError) throw error;
    // Never return certificate/private-key parser or WebCrypto exception text.
    throw new MaterialError('caddy_material_invalid');
  }
}

async function readPem(path: string, owner: RuntimeOwner): Promise<string> {
  await assertProtected(path, owner);
  const handle = await open(
    path,
    constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | constants.O_NONBLOCK,
  );
  try {
    const info = await handle.stat();
    if (
      !info.isFile() ||
      (process.platform !== 'win32' && (info.uid !== owner.uid || (info.mode & 0o777) !== 0o600))
    )
      throw new MaterialError('caddy_material_unsafe');
    if (info.size > maxFileBytes) throw new MaterialError('caddy_material_oversized');
    // One extra byte detects growth after stat. No unbounded readFile allocation.
    const bytes = Buffer.alloc(maxFileBytes + 1);
    let used = 0;
    while (used < bytes.length) {
      const { bytesRead } = await handle.read(bytes, used, bytes.length - used, null);
      if (bytesRead === 0) break;
      used += bytesRead;
    }
    if (used > maxFileBytes) throw new MaterialError('caddy_material_oversized');
    try {
      return new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(0, used));
    } catch {
      throw new MaterialError('caddy_material_invalid');
    }
  } finally {
    await handle.close();
  }
}

/**
 * Caller must hold the runtime lifetime lock, and keep it while using material.
 * Creates six fixed PEM files only when all six are absent. Existing identity
 * is validated without writes, repair or rotation. Failure cleanup removes only
 * exclusively created files; prepareRuntime's directory is left in place.
 */
export async function loadOrCreateAdminMaterial(
  directory: string,
  owner: RuntimeOwner,
): Promise<PortResult<AdminMaterial>> {
  const created: string[] = [];
  try {
    await prepareRuntime(directory, owner);
    const paths = {} as Record<Role, PemIdentity>;
    let present = 0;
    for (const role of roles) {
      paths[role] = {
        certificate: join(directory, filenames[role].certificate),
        key: join(directory, filenames[role].key),
      };
      for (const path of Object.values(paths[role])) {
        try {
          await assertProtected(path, owner);
          present += 1;
        } catch (error) {
          if (errorCode(error) !== 'ENOENT') throw error;
        }
      }
    }
    if (present !== 0 && present !== 6) throw new MaterialError('caddy_material_incomplete');
    let bundle: Bundle;
    if (present === 0) {
      try {
        bundle = await generateBundle();
      } catch {
        throw new MaterialError('caddy_material_generation_failed');
      }
    } else {
      bundle = {} as Bundle;
      for (const role of roles) {
        bundle[role] = {
          certificate: await readPem(paths[role].certificate, owner),
          key: await readPem(paths[role].key, owner),
        };
      }
    }
    const certificates = await validateBundle(bundle);
    if (present === 0) {
      for (const role of roles) {
        for (const field of ['certificate', 'key'] as const) {
          const path = paths[role][field];
          if (!(await createProtectedFile(path, owner, bundle[role][field])))
            throw new MaterialError('caddy_material_conflict');
          created.push(path);
        }
      }
    }
    return {
      ok: true,
      value: {
        paths,
        certificates: {
          root: bundle.root.certificate,
          intermediate: bundle.intermediate.certificate,
          client: bundle.client.certificate,
        },
        authorizedClientCertificateBase64: certificates.client.toString('base64'),
        tls: {
          ca: bundle.root.certificate,
          cert: `${bundle.client.certificate}\n${bundle.intermediate.certificate}`,
          key: bundle.client.key,
        },
      },
    };
  } catch (error) {
    let cleanupFailed = false;
    for (const path of created.reverse()) {
      try {
        await unlink(path);
      } catch (cleanupError) {
        if (errorCode(cleanupError) !== 'ENOENT') cleanupFailed = true;
      }
    }
    let code = 'caddy_material_io';
    if (cleanupFailed) code = 'caddy_material_cleanup_failed';
    else if (error instanceof MaterialError) code = error.code;
    else if (errorCode(error) === 'unsafe-runtime-permissions') code = 'caddy_material_unsafe';
    let kind: ProtocolError['kind'] = 'configuration';
    switch (code) {
      case 'caddy_material_unsafe':
        kind = 'accessDenied';
        break;
      case 'caddy_material_conflict':
        kind = 'conflict';
        break;
      case 'caddy_material_io':
      case 'caddy_material_cleanup_failed':
        kind = 'storage';
        break;
    }
    return {
      ok: false,
      error: {
        kind,
        code,
        message: 'Caddy administration material could not be created or validated.',
        guidance:
          'Use a complete valid owner-protected bundle. I/O or cleanup failures may leave partial files; inspect the protected directory. Existing credentials are never repaired or rotated automatically.',
        retryable: false,
        requestId: null,
      },
    };
  }
}
