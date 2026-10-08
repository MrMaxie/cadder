import { X509Certificate } from 'node:crypto';
import * as fs from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  normalizeCaddyConfig,
  prepareSecureAdminPolicy,
  type SecureAdminPolicy,
  type SecureAdminPolicyOptions,
} from '../src/caddy/admin-policy.ts';
import { loadOrCreateAdminMaterial, type AdminMaterial } from '../src/caddy/certificates.ts';
import { composeRoutePlan, type CaddyRoutePlan } from '../src/caddy/composition.ts';
import type { CompositionRegistration, JsonObject } from '../src/caddy/domains.ts';
import {
  assertCandidate,
  configHash,
  maxCaddyConfigurationBytes,
} from '../src/caddy/preparation.ts';
import * as security from '../src/platform/runtime-security.ts';
import { powershell } from '../src/platform/powershell.ts';
import { protocolErrorSchema } from '../src/protocol/errors.ts';

vi.mock('../src/platform/runtime-security.ts', async (original) => {
  const actual = await original<typeof security>();
  return {
    ...actual,
    prepareRuntime: vi.fn(actual.prepareRuntime),
    assertProtected: vi.fn(actual.assertProtected),
  };
});
vi.mock('../src/caddy/certificates.ts', async (original) => {
  const actual = await original<typeof import('../src/caddy/certificates.ts')>();
  return { ...actual, loadOrCreateAdminMaterial: vi.fn(actual.loadOrCreateAdminMaterial) };
});
vi.mock('node:fs/promises', async (original) => {
  const actual = await original<typeof fs>();
  return { ...actual, lstat: vi.fn(actual.lstat) };
});
vi.mock('../src/platform/powershell.ts', async (original) => {
  const actual = await original<typeof import('../src/platform/powershell.ts')>();
  return { ...actual, powershell: vi.fn(actual.powershell) };
});
const actualSecurity = await vi.importActual<typeof security>(
  '../src/platform/runtime-security.ts',
);
const actualMaterial = await vi.importActual<typeof import('../src/caddy/certificates.ts')>(
  '../src/caddy/certificates.ts',
);
const actualFs = await vi.importActual<typeof fs>('node:fs/promises');
const actualPowerShell = await vi.importActual<typeof import('../src/platform/powershell.ts')>(
  '../src/platform/powershell.ts',
);
const nativePlatform = process.platform;
const uidDescriptor = Object.getOwnPropertyDescriptor(process, 'getuid');
const owner = { id: 'uid:42', uid: 42, elevated: false };
const base = join(tmpdir(), 'cadder-policy-modeled');
const materialDirectory = join(tmpdir(), 'cadder-policy-material-modeled');
const mockMaterial: AdminMaterial = {
  paths: {
    root: {
      certificate: join(materialDirectory, 'root.crt.pem'),
      key: join(materialDirectory, 'root.key.pem'),
    },
    intermediate: {
      certificate: join(materialDirectory, 'intermediate.crt.pem'),
      key: join(materialDirectory, 'intermediate.key.pem'),
    },
    client: {
      certificate: join(materialDirectory, 'client.crt.pem'),
      key: join(materialDirectory, 'client.key.pem'),
    },
  },
  certificates: {
    root: 'ROOT PUBLIC PEM',
    intermediate: 'INTERMEDIATE PUBLIC PEM',
    client: 'CLIENT PUBLIC PEM',
  },
  authorizedClientCertificateBase64: 'CLIENT_LEAF_DER',
  tls: { ca: 'ROOT PUBLIC PEM', cert: 'CLIENT CHAIN PUBLIC PEM', key: 'SECRET CLIENT KEY' },
};
let cleanup: string | undefined;
beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  Object.defineProperty(process, 'getuid', { configurable: true, value: () => 42 });
  vi.mocked(security.prepareRuntime).mockResolvedValue();
  vi.mocked(security.assertProtected).mockResolvedValue();
  vi.mocked(loadOrCreateAdminMaterial).mockResolvedValue({ ok: true, value: mockMaterial });
  vi.mocked(powershell).mockResolvedValue('');
  vi.mocked(fs.lstat).mockImplementation(async () => stat());
});
afterEach(async () => {
  vi.restoreAllMocks();
  if (uidDescriptor) Object.defineProperty(process, 'getuid', uidDescriptor);
  else Reflect.deleteProperty(process, 'getuid');
  if (cleanup) {
    await actualFs.rm(cleanup, { recursive: true, force: true });
    cleanup = undefined;
  }
});
function stat(
  changes: Partial<{ uid: number; mode: number; link: boolean; directory: boolean }> = {},
): Awaited<ReturnType<typeof fs.lstat>> {
  return {
    uid: changes.uid ?? 42,
    mode: changes.mode ?? 0o700,
    isSymbolicLink: () => changes.link ?? false,
    isDirectory: () => changes.directory ?? true,
    isFile: () => false,
  } as Awaited<ReturnType<typeof fs.lstat>>;
}
function registration(id: string, hosts = ['app.localhost']): CompositionRegistration {
  return {
    registrationId: id,
    activationState: 'active',
    sourceConfigPath: { raw: `${id}/Caddyfile`, canonical: null },
    registeredDomains: hosts.map((host) => ({
      name: { raw: host, canonical: host },
      activationState: 'active',
      logStream: { streamId: id, domainKey: host, channel: 'domain' },
    })),
  };
}
function routePlan(projectExtras: JsonObject = {}): CaddyRoutePlan {
  const body = JSON.stringify({
    ...projectExtras,
    apps: {
      ...(projectExtras.apps as JsonObject),
      http: {
        servers: {
          untrusted: {
            listen: [':9999'],
            routes: [
              {
                '@id': 'opaque',
                match: [{ host: ['app.localhost', 'other.localhost'] }],
                handle: [
                  {
                    handler: 'static_response',
                    body: 'original',
                    opaque: {
                      host: 'opaque',
                      admin: { disabled: false },
                      routes: [2, 1],
                      __proto__: null,
                    },
                  },
                ],
              },
            ],
          },
        },
      },
    },
  });
  const result = composeRoutePlan([
    { registration: registration('b', ['other.localhost']), config: normalizeCaddyConfig(body) },
    { registration: registration('a'), config: normalizeCaddyConfig(body) },
  ]);
  if (!result.ok) throw new Error('fixture composition failed');
  return result.value;
}
function options(overrides: Partial<SecureAdminPolicyOptions> = {}): SecureAdminPolicyOptions {
  return {
    runtimeBase: base,
    materialDirectory,
    owner,
    adminPort: 32451,
    plan: overrides.plan ?? routePlan(),
    baseEnvironment: { PATH: '/safe/bin' },
    ...overrides,
  };
}
async function policy(
  overrides: Partial<SecureAdminPolicyOptions> = {},
): Promise<SecureAdminPolicy> {
  const result = await prepareSecureAdminPolicy(options(overrides));
  if (!result.ok) throw new Error(`Policy failed: ${result.error.code}`);
  return result.value;
}
async function refused(overrides: Partial<SecureAdminPolicyOptions>, code?: string): Promise<void> {
  const result = await prepareSecureAdminPolicy(options(overrides));
  expect(result).toMatchObject({
    ok: false,
    error: { retryable: false, ...(code ? { code } : {}) },
  });
  if (result.ok) throw new Error('unexpected success');
  expect(protocolErrorSchema.safeParse(result.error).success).toBe(true);
  expect(result.error.message.length).toBeLessThan(256);
  expect(JSON.stringify(result)).not.toMatch(/SECRET|PRIVATE KEY|attacker|parser/);
}

it('assembles native mutual-TLS policy, imported CA paths and real internal web issuance', async () => {
  const value = await policy();
  const config = JSON.parse(value.config.adaptedConfig.body);
  expect(config.admin).toEqual({
    disabled: true,
    identity: { identifiers: ['localhost'], issuers: [{ module: 'internal', ca: 'cadder-admin' }] },
    remote: { listen: '127.0.0.1:32451', access_control: [{ public_keys: ['CLIENT_LEAF_DER'] }] },
  });
  expect(config.apps.pki.certificate_authorities).toEqual({
    'cadder-admin': {
      install_trust: false,
      root: { format: 'pem_file', certificate: mockMaterial.paths.root.certificate },
      intermediate: {
        format: 'pem_file',
        certificate: mockMaterial.paths.intermediate.certificate,
        private_key: mockMaterial.paths.intermediate.key,
      },
    },
    local: { install_trust: false },
  });
  expect(config.apps.tls).toEqual({
    automation: {
      policies: [
        {
          subjects: ['app.localhost', 'other.localhost'],
          issuers: [{ module: 'internal', ca: 'local' }],
        },
      ],
    },
    certificates: { automate: ['app.localhost', 'other.localhost'] },
  });
  expect(config.apps.http.servers.cadder_https.tls_connection_policies).toEqual([{}]);
  expect(config.apps.http.servers.cadder_http.tls_connection_policies).toBeUndefined();
  expect(config.apps.http.servers.cadder_https.automatic_https).toEqual({ disable: true });
  expect(value.config.adaptedConfig.body).not.toMatch(
    /SECRET|PUBLIC PEM|root.key.pem|client.key.pem|acme/,
  );
  expect(value.client).toEqual({
    host: '127.0.0.1',
    port: 32451,
    servername: 'localhost',
    rejectUnauthorized: true,
    ...mockMaterial.tls,
  });
  expect(loadOrCreateAdminMaterial).toHaveBeenCalledWith(materialDirectory, owner);
  assertCandidate(value.config);
  expect(normalizeCaddyConfig(value.config.adaptedConfig.body)).toEqual(value.config);
  expect(Object.isFrozen(value)).toBe(true);
  expect(Object.isFrozen(value.client)).toBe(true);
  expect(Object.isFrozen(value.config.adaptedConfig)).toBe(true);
});

it('preserves listeners, outer guards, original nested opaque handlers, ordering and input immutability', async () => {
  const plan = routePlan();
  const before = structuredClone(plan);
  const value = await policy({ plan });
  const servers = JSON.parse(value.config.adaptedConfig.body).apps.http.servers;
  for (const name of ['cadder_http', 'cadder_https'] as const) {
    expect(servers[name].listen).toEqual(before.servers[name].listen);
    expect(servers[name].routes).toEqual(before.servers[name].routes);
    expect(
      servers[name].routes.map((r: { match: { host: string[] }[] }) => r.match[0]!.host),
    ).toEqual([['app.localhost'], ['other.localhost']]);
  }
  expect(plan).toEqual(before);
  expect(Object.isFrozen(plan)).toBe(false);
});

it('does not import project admin, remote, PKI, storage or server settings', async () => {
  const plan = routePlan({
    admin: {
      disabled: false,
      remote: { listen: ':2021' },
      identity: { issuers: [{ module: 'acme' }] },
    },
    storage: { module: 'attacker' },
    apps: {
      pki: { certificate_authorities: { evil: { install_trust: true } } },
      tls: { automation: { policies: [{ issuers: [{ module: 'acme' }] }] } },
    },
  });
  const config = JSON.parse((await policy({ plan })).config.adaptedConfig.body);
  expect(Object.keys(config).sort()).toEqual(['admin', 'apps']);
  expect(Object.keys(config.apps).sort()).toEqual(['http', 'pki', 'tls']);
  expect(Object.keys(config.apps.pki.certificate_authorities).sort()).toEqual([
    'cadder-admin',
    'local',
  ]);
  expect(config.apps.http.servers.untrusted).toBeUndefined();
  expect(config.admin.remote.listen).toBe('127.0.0.1:32451');
});

it('preserves a composed wildcard guard and subject without introducing a new domain policy', async () => {
  const config = normalizeCaddyConfig(
    JSON.stringify({
      apps: {
        http: {
          servers: {
            project: {
              routes: [
                {
                  match: [{ host: ['*.localhost'] }],
                  handle: [{ handler: 'static_response', body: 'wildcard' }],
                },
              ],
            },
          },
        },
      },
    }),
  );
  const composed = composeRoutePlan([
    { registration: registration('wildcard', ['*.localhost']), config },
  ]);
  if (!composed.ok) throw new Error('wildcard composition failed');
  const body = JSON.parse((await policy({ plan: composed.value })).config.adaptedConfig.body);
  expect(body.apps.tls.automation.policies[0].subjects).toEqual(['*.localhost']);
  for (const name of ['cadder_http', 'cadder_https'] as const) {
    expect(body.apps.http.servers[name].routes).toEqual(composed.value.servers[name].routes);
  }
});

it('keeps an empty plan valid without catch-all certificate automation', async () => {
  const result = composeRoutePlan([]);
  if (!result.ok) throw new Error('empty composition failed');
  const config = JSON.parse((await policy({ plan: result.value })).config.adaptedConfig.body);
  expect(config.apps.tls).toEqual({ automation: { policies: [] }, certificates: { automate: [] } });
  expect(config.apps.http.servers.cadder_https.routes).toEqual([]);
});

it('retains TLS subjects when all project routes prune without requiring a matching guard', async () => {
  const config = normalizeCaddyConfig(
    JSON.stringify({
      apps: {
        http: {
          servers: {
            project: {
              routes: [
                {
                  match: [{ host: ['disabled.localhost'] }],
                  handle: [{ handler: 'static_response', body: 'disabled-only' }],
                },
              ],
            },
          },
        },
      },
    }),
  );
  const composed = composeRoutePlan([{ registration: registration('pruned'), config }]);
  if (!composed.ok) throw new Error('pruned-route composition failed');
  expect(composed.value.tlsSubjects).toEqual(['app.localhost']);
  const body = JSON.parse((await policy({ plan: composed.value })).config.adaptedConfig.body);
  expect(body.apps.tls.automation.policies[0].subjects).toEqual(['app.localhost']);
  expect(body.apps.tls.certificates.automate).toEqual(['app.localhost']);
  for (const name of ['cadder_http', 'cadder_https'] as const) {
    expect(composed.value.servers[name].routes).toEqual([]);
    expect(body.apps.http.servers[name].routes).toEqual([]);
  }
});

it.each(['linux', 'darwin', 'win32'] as const)(
  'isolates Caddy default storage, autosave, home and scratch on %s, including mixed-case env keys',
  async (platform) => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue(platform);
    const ambient = { ...process.env };
    const baseEnvironment = Object.freeze({
      PATH: 'preserved',
      unrelated: 'yes',
      XDG_DATA_HOME: '/outside',
      xdg_data_home: '/outside2',
      Xdg_Config_Home: '/outside',
      HOME: '/outside',
      home: '/outside',
      UserProfile: '/outside',
      HomeDrive: 'X:',
      HomePath: '/outside',
      AppData: '/outside',
      LOCALAPPDATA: '/outside',
      Temp: '/outside',
      TMP: '/outside',
      TmpDir: '/outside',
      XDG_CACHE_HOME: '/outside',
      XDG_STATE_HOME: '/outside',
      XDG_RUNTIME_DIR: '/outside',
      caddy_admin: ':2019',
    });
    const snapshot = { ...baseEnvironment };
    const value = await policy({ baseEnvironment });
    const env = value.environment;
    expect(env.PATH).toBe('preserved');
    expect(env.unrelated).toBe('yes');
    expect(env.XDG_DATA_HOME).toBe(value.paths.data);
    expect(env.XDG_CONFIG_HOME).toBe(value.paths.config);
    expect(join(env.XDG_DATA_HOME!, 'caddy')).toBe(value.paths.defaultStorage);
    expect(join(env.XDG_CONFIG_HOME!, 'caddy')).toBe(value.paths.autosaveDirectory);
    for (const key of ['TEMP', 'TMP', 'TMPDIR', 'XDG_RUNTIME_DIR'])
      expect(env[key]).toBe(value.paths.scratch);
    expect(env.HOME).toBe(value.paths.home);
    expect(env.XDG_CACHE_HOME).toBe(value.paths.data);
    expect(env.XDG_STATE_HOME).toBe(value.paths.data);
    expect(Object.values(env)).not.toContain('/outside');
    expect(Object.keys(env).filter((key) => key.toUpperCase() === 'XDG_DATA_HOME')).toEqual([
      'XDG_DATA_HOME',
    ]);
    expect(env.caddy_admin).toBeUndefined();
    expect(env.CADDY_ADMIN).toBeUndefined();
    if (platform === 'win32') {
      expect(env.USERPROFILE).toBe(value.paths.home);
      expect(env.HOMEDRIVE! + env.HOMEPATH!).toBe(value.paths.home);
      expect(env.APPDATA).toBe(value.paths.config);
      expect(env.LOCALAPPDATA).toBe(value.paths.data);
    } else {
      for (const key of ['USERPROFILE', 'HOMEDRIVE', 'HOMEPATH', 'APPDATA', 'LOCALAPPDATA'])
        expect(env[key]).toBeUndefined();
    }
    expect(baseEnvironment).toEqual(snapshot);
    expect({ ...process.env }).toEqual(ambient);
    expect(Object.isFrozen(env)).toBe(true);
    expect(security.prepareRuntime).toHaveBeenCalledTimes(7);
    for (const directory of [
      value.paths.data,
      value.paths.config,
      value.paths.scratch,
      value.paths.home,
      value.paths.defaultStorage,
      value.paths.autosaveDirectory,
    ]) {
      expect(security.prepareRuntime).toHaveBeenCalledWith(directory, owner);
      expect(security.assertProtected).toHaveBeenCalledWith(directory, owner, true);
    }
  },
);

it('defaults to a copy of ambient env without changing it', async () => {
  const before = { ...process.env };
  const input = options();
  const withoutEnvironment = { ...input };
  delete withoutEnvironment.baseEnvironment;
  const result = await prepareSecureAdminPolicy(withoutEnvironment);
  expect(result.ok).toBe(true);
  expect({ ...process.env }).toEqual(before);
});

it.each([0, -1, 65536, 1.1, NaN, Infinity, 80, 443, '32451', undefined, null])(
  'refuses invalid/reserved admin port %s before runtime writes',
  async (adminPort) => {
    await refused({ adminPort: adminPort as number }, 'caddy_admin_policy_invalid');
    expect(security.prepareRuntime).not.toHaveBeenCalled();
    expect(loadOrCreateAdminMaterial).not.toHaveBeenCalled();
  },
);
it.each([1, 2021, 65535])('uses only the exact explicit valid port %s', async (adminPort) => {
  const value = await policy({ adminPort });
  expect(JSON.parse(value.config.adaptedConfig.body).admin.remote.listen).toBe(
    `127.0.0.1:${adminPort}`,
  );
});
it.each(['runtimeBase', 'materialDirectory'] as const)('requires an absolute %s', async (key) => {
  await refused({ [key]: 'relative' }, 'caddy_admin_policy_invalid');
  expect(security.prepareRuntime).not.toHaveBeenCalled();
});

const outer = 'servers.cadder_http.routes.0';
it.each([
  ['kind', 'kind', 'config'],
  ['project admin', 'admin', { remote: { listen: ':2021' } }],
  ['PKI', 'apps', { pki: {} }],
  ['third server', 'servers.evil', {}],
  ['widened HTTP', 'servers.cadder_http.listen', [':80']],
  ['widened HTTPS', 'servers.cadder_https.listen', ['localhost:443']],
  ['extra listener policy', 'servers.cadder_https.listener_wrappers', []],
  ['missing routes', 'servers.cadder_http.routes', undefined],
  ['non-array routes', 'servers.cadder_http.routes', {}],
  ['malformed server', 'servers.cadder_http', null],
  ['non-array subjects', 'tlsSubjects', null],
  ['unsorted subjects', 'tlsSubjects', ['other.localhost', 'app.localhost']],
  ['duplicate subjects', 'tlsSubjects', ['app.localhost', 'app.localhost']],
  ['empty subject', 'tlsSubjects', ['']],
  ['non-string subject', 'tlsSubjects', [42]],
  ['noncanonical subject', 'tlsSubjects', ['APP.localhost']],
  ['missing guard', `${outer}.match`, []],
  ['hostless guard', `${outer}.match.0.host`, []],
  ['unguarded route', `${outer}.terminal`, false],
  ['guard matcher widening', `${outer}.match.0.path`, ['*']],
  ['extra outer handler', `${outer}.handle.1`, { handler: 'static_response' }],
  ['not a subroute', `${outer}.handle.0.handler`, 'file_server'],
  ['invalid child routes', `${outer}.handle.0.routes`, [42]],
  ['guard outside subjects', `${outer}.match.0.host`, ['evil.localhost']],
  ['duplicate guard host', `${outer}.match.0.host`, ['app.localhost', 'app.localhost']],
  ['mismatched guard order', `${outer}.match.0.host`, ['other.localhost']],
] as const)('refuses malformed/widened plan: %s', async (_name, path, value) => {
  const plan = structuredClone(routePlan());
  const keys = path.split('.');
  let target: unknown = plan;
  for (const key of keys.slice(0, -1)) target = (target as Record<string, unknown>)[key];
  if (value === undefined) Reflect.deleteProperty(target as object, keys.at(-1)!);
  else
    Object.defineProperty(target, keys.at(-1)!, {
      enumerable: true,
      configurable: true,
      writable: true,
      value,
    });
  await refused({ plan }, 'caddy_candidate_invalid');
  expect(security.prepareRuntime).not.toHaveBeenCalled();
});

it.each(['owner', 'mode', 'link', 'ancestor-link', 'not-directory', 'ACL'] as const)(
  'uses existing helpers to deny unsafe runtime %s without repairing it',
  async (failure) => {
    vi.mocked(security.prepareRuntime).mockImplementation(actualSecurity.prepareRuntime);
    if (failure === 'ACL') {
      vi.spyOn(process, 'platform', 'get').mockReturnValue('win32');
      vi.mocked(powershell).mockRejectedValue(new Error('SECRET attacker ACL details'));
    } else {
      vi.mocked(fs.lstat).mockImplementation(async (path) => {
        const target = String(path) === base || failure === 'ancestor-link';
        return target
          ? stat({
              uid: failure === 'owner' ? 99 : 42,
              mode: failure === 'mode' ? 0o755 : 0o700,
              link: failure === 'link' || failure === 'ancestor-link',
              directory: failure !== 'not-directory',
            })
          : stat();
      });
    }
    await refused({}, 'caddy_admin_policy_failed');
    expect(loadOrCreateAdminMaterial).not.toHaveBeenCalled();
  },
);
it('refuses unsafe existing child paths and material failures without replacement or diagnostic leakage', async () => {
  vi.mocked(security.assertProtected).mockRejectedValue(new Error('SECRET path parser details'));
  await refused({}, 'caddy_admin_policy_failed');
  expect(loadOrCreateAdminMaterial).not.toHaveBeenCalled();
  vi.mocked(security.assertProtected).mockResolvedValue();
  vi.mocked(loadOrCreateAdminMaterial).mockResolvedValue({
    ok: false,
    error: {
      kind: 'configuration',
      code: 'caddy_material_invalid',
      message: 'Caddy administration material was refused.',
      guidance: null,
      retryable: false,
      requestId: null,
    },
  });
  await refused({}, 'caddy_material_invalid');
});

it('prepares and re-verifies an actual owned runtime, with authorized client LEAF DER', async () => {
  vi.spyOn(process, 'platform', 'get').mockReturnValue(nativePlatform);
  if (uidDescriptor) Object.defineProperty(process, 'getuid', uidDescriptor);
  else Reflect.deleteProperty(process, 'getuid');
  vi.mocked(fs.lstat).mockImplementation(actualFs.lstat);
  vi.mocked(powershell).mockImplementation(actualPowerShell.powershell);
  vi.mocked(security.prepareRuntime).mockImplementation(actualSecurity.prepareRuntime);
  vi.mocked(security.assertProtected).mockImplementation(actualSecurity.assertProtected);
  vi.mocked(loadOrCreateAdminMaterial).mockImplementation(actualMaterial.loadOrCreateAdminMaterial);
  const nativeOwner = await security.runtimeOwner(
    nativePlatform === 'win32' ? undefined : process.getuid!(),
  );
  cleanup = await actualFs.mkdtemp(join(tmpdir(), 'cadder-policy-owned-'));
  const input = options({
    runtimeBase: join(cleanup, 'runtime'),
    materialDirectory: join(cleanup, 'material'),
    owner: nativeOwner,
  });
  const first = await prepareSecureAdminPolicy(input);
  if (!first.ok) throw new Error(`Native protected preparation failed: ${first.error.code}`);
  for (const directory of Object.values(first.value.paths))
    await actualSecurity.assertProtected(directory, nativeOwner, true);
  const second = await prepareSecureAdminPolicy(input);
  expect(second).toEqual(first);
  const config = JSON.parse(first.value.config.adaptedConfig.body);
  const der = Buffer.from(config.admin.remote.access_control[0].public_keys[0], 'base64');
  const leaf = new X509Certificate(der);
  const clientPem = await actualFs.readFile(
    join(input.materialDirectory, 'client.crt.pem'),
    'utf8',
  );
  expect(leaf.raw).toEqual(new X509Certificate(clientPem).raw);
  expect(leaf.ca).toBe(false);
  expect(leaf.raw).not.toEqual(new X509Certificate(first.value.client.ca).raw);
  expect(first.value.config.adaptedConfig.body).not.toContain('PRIVATE KEY');
}, 60_000);

describe('independent complete-body normalization', () => {
  it('orders all object keys deterministically, preserving arrays, primitives and opaque __proto__ data', () => {
    const left =
      '{"z":[3,1,{"b":false,"a":null}],"2":"two","10":"ten","__proto__":{"polluted":true},"toJSON":"opaque","a":{"z":"\\u00e9","a":-0}}';
    const right =
      '{"a":{"a":0,"z":"é"},"toJSON":"opaque","__proto__":{"polluted":true},"10":"ten","2":"two","z":[3,1,{"a":null,"b":false}]}';
    const first = normalizeCaddyConfig(left);
    expect(first).toEqual(normalizeCaddyConfig(right));
    const canonical =
      '{"10":"ten","2":"two","__proto__":{"polluted":true},"a":{"a":0,"z":"é"},"toJSON":"opaque","z":[3,1,{"a":null,"b":false}]}';
    expect(first.adaptedConfig.body).toBe(canonical);
    expect(first.effectiveConfigHash).toBe(configHash(canonical));
    expect(({} as { polluted?: boolean }).polluted).toBeUndefined();
    expect(normalizeCaddyConfig('{"a":[1,2]}').effectiveConfigHash).not.toBe(
      normalizeCaddyConfig('{"a":[2,1]}').effectiveConfigHash,
    );
    expect(normalizeCaddyConfig('{"a":{}}')).not.toEqual(
      normalizeCaddyConfig('{"a":{"default":false}}'),
    );
  });
  it.each([
    '',
    '{',
    '{} {}',
    '[]',
    'null',
    '42',
    '"string"',
    '{"x":NaN}',
    '{"x":Infinity}',
    '{"x":"\ud800"}',
    '{"x":1e400}',
  ])('refuses incomplete, non-object or invalid JSON body %j with bounded diagnostics', (body) => {
    expect(() => normalizeCaddyConfig(body)).toThrow('Expected a complete, bounded JSON object');
  });
  it('refuses object carriers without invoking toJSON or getters', async () => {
    const toJSON = vi.fn(() => {
      throw new Error('SECRET');
    });
    expect(() => normalizeCaddyConfig({ toJSON } as never)).toThrow();
    expect(toJSON).not.toHaveBeenCalled();
    const plan = routePlan();
    Object.defineProperty(plan, 'toJSON', { enumerable: true, value: toJSON });
    await refused({ plan }, 'caddy_candidate_invalid');
    expect(toJSON).not.toHaveBeenCalled();
    const getter = vi.fn(() => {
      throw new Error('SECRET');
    });
    Object.defineProperty(plan, 'kind', { enumerable: true, get: getter });
    await refused({ plan }, 'caddy_candidate_invalid');
    expect(getter).not.toHaveBeenCalled();
  });
  it.each([
    'cycle',
    'proxy',
    'native-failure',
    'prototype',
    'symbol',
    'nonenumerable',
    'undefined',
    'function',
    'bigint',
    'nan',
    'sparse',
    'array-property',
    'array-getter',
    'deep',
    'oversize-string',
    'oversize-key',
    'aggregate-bytes',
    'many-values',
  ] as const)('refuses unsafe/oversized plan carrier: %s', async (fault) => {
    const plan = routePlan();
    const route = plan.servers.cadder_http.routes[0]!;
    let payload: unknown;
    switch (fault) {
      case 'cycle':
        payload = plan;
        break;
      case 'proxy':
        payload = new Proxy(
          {},
          {
            ownKeys: () => {
              throw new Error('SECRET');
            },
          },
        );
        break;
      case 'native-failure':
        vi.spyOn(Object, 'getOwnPropertyDescriptors').mockImplementationOnce(() => {
          throw new Error('SECRET');
        });
        payload = {};
        break;
      case 'prototype':
        payload = new Date();
        break;
      case 'symbol':
        payload = { [Symbol('secret')]: 1 };
        break;
      case 'nonenumerable':
        payload = Object.defineProperty({}, 'secret', { value: 1 });
        break;
      case 'undefined':
        payload = undefined;
        break;
      case 'function':
        payload = () => 1;
        break;
      case 'bigint':
        payload = 1n;
        break;
      case 'nan':
        payload = NaN;
        break;
      case 'sparse':
        payload = new Array(2);
        break;
      case 'array-property':
        payload = Object.assign([1], { extra: 2 });
        break;
      case 'array-getter':
        payload = Object.defineProperty([1], '0', {
          get: () => {
            throw new Error('SECRET');
          },
        });
        break;
      case 'deep':
        payload = {};
        for (let i = 0; i < 150; i++) payload = { child: payload };
        break;
      case 'oversize-string':
        payload = 'x'.repeat(maxCaddyConfigurationBytes + 1);
        break;
      case 'oversize-key':
        payload = { ['x'.repeat(maxCaddyConfigurationBytes + 1)]: 1 };
        break;
      case 'aggregate-bytes':
        payload = {
          a: 'x'.repeat(maxCaddyConfigurationBytes / 2),
          b: 'x'.repeat(maxCaddyConfigurationBytes / 2),
        };
        break;
      case 'many-values':
        payload = new Array(1_000_001).fill(null);
        break;
    }
    Object.defineProperty(route, 'payload', { enumerable: true, value: payload });
    await refused({ plan }, 'caddy_candidate_invalid');
  });
  it('bounds full bodies, canonical expansion, depth and total visited values', () => {
    expect(() =>
      normalizeCaddyConfig('{"s":"' + 'x'.repeat(maxCaddyConfigurationBytes) + '"}'),
    ).toThrow();
    const repeated = 'é'.repeat(Math.floor(maxCaddyConfigurationBytes / 2));
    expect(() => normalizeCaddyConfig(JSON.stringify({ s: repeated }))).toThrow();
    // Compact number spellings fit the input bound; canonical spelling exceeds it.
    const expanding =
      '{"a":[' + '1e9,'.repeat(900_000) + '0],"s":"' + 'x'.repeat(25_000_000) + '"}';
    expect(Buffer.byteLength(expanding)).toBeLessThan(maxCaddyConfigurationBytes);
    expect(() => normalizeCaddyConfig(expanding)).toThrow();
    expect(() =>
      normalizeCaddyConfig(
        '{"s":"' + '\u0001'.repeat(Math.floor(maxCaddyConfigurationBytes / 6)) + '"}',
      ),
    ).toThrow();
    expect(() =>
      normalizeCaddyConfig('{"s":' + '['.repeat(150) + '0' + ']'.repeat(150) + '}'),
    ).toThrow();
    expect(() => normalizeCaddyConfig('{"s":[' + '0,'.repeat(1_000_000) + '0]}')).toThrow();
  });
});
