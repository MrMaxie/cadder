import { isAbsolute, join, parse, resolve } from 'node:path';
import { types } from 'node:util';
import type { CaddyConfig, PortResult, RuntimeOwner } from '../contracts/ports.ts';
import { assertProtected, prepareRuntime } from '../platform/runtime-security.ts';
import { loadOrCreateAdminMaterial } from './certificates.ts';
import type { CaddyRoutePlan } from './composition.ts';
import { canonicalizeDomain, compareText, type JsonObject, type JsonValue } from './domains.ts';
import {
  assertCandidate,
  assertJsonBody,
  CaddyPreparationError,
  configHash,
  maxCaddyConfigurationBytes,
} from './preparation.ts';

export type SecureAdminPolicyOptions = Readonly<{
  runtimeBase: string;
  owner: RuntimeOwner;
  materialDirectory: string;
  adminPort: number;
  plan: CaddyRoutePlan;
  baseEnvironment?: Readonly<NodeJS.ProcessEnv>;
}>;
export type SecureAdminPolicy = Readonly<{
  config: CaddyConfig;
  paths: Readonly<{
    base: string;
    data: string;
    config: string;
    scratch: string;
    home: string;
    defaultStorage: string;
    autosaveDirectory: string;
  }>;
  environment: Readonly<NodeJS.ProcessEnv>;
  /** Use normal Node HTTPS identity verification, never a custom bypass or ambient CA. */
  client: Readonly<{
    host: '127.0.0.1';
    port: number;
    servername: 'localhost';
    rejectUnauthorized: true;
    ca: string;
    cert: string;
    key: string;
  }>;
}>;

function invalidCandidate(): never {
  throw new CaddyPreparationError(
    'caddy_candidate_invalid',
    'Expected a complete, bounded JSON object with safe JSON values.',
  );
}

/** No stringify(object), getters, toJSON, prototype assignment or proxy traversal. */
function canonicalBody(input: unknown): string {
  const parts: string[] = [];
  const active = new WeakSet<object>();
  let bytes = 0;
  let visits = 0;
  function emit(text: string): void {
    bytes += Buffer.byteLength(text, 'utf8');
    if (bytes > maxCaddyConfigurationBytes) invalidCandidate();
    parts.push(text);
  }
  function visit(value: unknown, depth: number): void {
    // Explicit bounds precede native recursion/serialization limits.
    if (depth > 128 || ++visits > 1_000_000) invalidCandidate();
    if (value === null || typeof value === 'boolean') {
      emit(String(value));
    } else if (typeof value === 'string') {
      if (Buffer.byteLength(value, 'utf8') > maxCaddyConfigurationBytes) invalidCandidate();
      emit(JSON.stringify(value));
    } else if (typeof value === 'number' && Number.isFinite(value)) {
      emit(JSON.stringify(value));
    } else if (typeof value === 'object') {
      if (types.isProxy(value) || active.has(value)) invalidCandidate();
      const array = Array.isArray(value);
      const prototype: unknown = Object.getPrototypeOf(value);
      if (
        array ? prototype !== Array.prototype : prototype !== null && prototype !== Object.prototype
      )
        invalidCandidate();
      const keys = Reflect.ownKeys(value);
      if (keys.length > 1_000_000) invalidCandidate();
      const descriptors = Object.getOwnPropertyDescriptors(value);
      if (keys.some((key) => typeof key !== 'string')) invalidCandidate();
      active.add(value);
      if (array) {
        const length = descriptors.length!.value as number;
        if (length > 1_000_000 || keys.length !== length + 1) invalidCandidate();
        emit('[');
        for (let index = 0; index < length; index++) {
          const entry = descriptors[String(index)];
          if (!entry || !entry.enumerable || !('value' in entry)) invalidCandidate();
          if (index > 0) emit(',');
          visit(entry.value, depth + 1);
        }
        emit(']');
      } else {
        emit('{');
        const ordered = (keys as string[]).sort(compareText);
        for (let index = 0; index < ordered.length; index++) {
          const key = ordered[index]!;
          const entry = descriptors[key]!;
          if (!entry.enumerable || !('value' in entry)) invalidCandidate();
          if (index > 0) emit(',');
          if (Buffer.byteLength(key, 'utf8') > maxCaddyConfigurationBytes) invalidCandidate();
          emit(JSON.stringify(key));
          emit(':');
          visit(entry.value, depth + 1);
        }
        emit('}');
      }
      active.delete(value);
    } else invalidCandidate();
  }
  try {
    visit(input, 0);
    return parts.join('');
  } catch {
    // Native failures and attacker-controlled exception messages never enter diagnostics.
    return invalidCandidate();
  }
}

function candidate(body: string): CaddyConfig {
  const config: CaddyConfig = Object.freeze({
    adaptedConfig: Object.freeze({ format: 'json', body }),
    effectiveConfigHash: configHash(body),
  });
  assertCandidate(config);
  return config;
}

/**
 * Normalize a complete submitted or independently fetched JSON object, not a receipt.
 * Object keys sort lexically (including numeric keys); arrays and opaque data stay intact.
 * No Caddy default-field elision: native effective-config differences remain differences.
 * Depth >128 and >1M visited values are refused even below the 32 MiB byte bound.
 */
export function normalizeCaddyConfig(body: string): CaddyConfig {
  try {
    if (typeof body !== 'string') invalidCandidate();
    assertJsonBody(body);
    const value: unknown = JSON.parse(body);
    object(value);
    return candidate(canonicalBody(value));
  } catch {
    return invalidCandidate();
  }
}

function object(value: unknown): JsonObject {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) invalidCandidate();
  return value as JsonObject;
}
function exactKeys(value: JsonObject, expected: readonly string[]): void {
  const keys = Object.keys(value).sort(compareText);
  if (keys.length !== expected.length || keys.some((key, index) => key !== expected[index]))
    invalidCandidate();
}
function sameArray(value: JsonValue | undefined, expected: readonly string[]): boolean {
  return (
    Array.isArray(value) &&
    value.length === expected.length &&
    value.every((v, i) => v === expected[i])
  );
}

/** Only the existing outer composeRoutePlan guards are policy; nested handlers are opaque. */
function guardedPlan(input: CaddyRoutePlan): CaddyRoutePlan {
  const plan = object(JSON.parse(canonicalBody(input)) as unknown);
  exactKeys(plan, ['kind', 'servers', 'tlsSubjects']);
  if (plan.kind !== 'caddy-route-plan' || !Array.isArray(plan.tlsSubjects)) invalidCandidate();
  const subjects = plan.tlsSubjects;
  if (
    subjects.some((host) => typeof host !== 'string' || !host || canonicalizeDomain(host) !== host)
  )
    invalidCandidate();
  const subjectSet = new Set(subjects as string[]);
  const sorted = [...subjectSet].sort(compareText);
  if (!sameArray(subjects, sorted)) invalidCandidate();
  const servers = object(plan.servers);
  exactKeys(servers, ['cadder_http', 'cadder_https']);
  const guards: string[][][] = [];
  for (const [name, port] of [
    ['cadder_http', 80],
    ['cadder_https', 443],
  ] as const) {
    const server = object(servers[name]);
    exactKeys(server, ['listen', 'routes']);
    if (
      !sameArray(server.listen, [`127.0.0.1:${port}`, `[::1]:${port}`]) ||
      !Array.isArray(server.routes)
    )
      invalidCandidate();
    const seen = new Set<string>();
    const hosts: string[][] = [];
    for (const entry of server.routes) {
      const route = object(entry);
      exactKeys(route, ['handle', 'match', 'terminal']);
      if (
        route.terminal !== true ||
        !Array.isArray(route.match) ||
        route.match.length !== 1 ||
        !Array.isArray(route.handle) ||
        route.handle.length !== 1
      )
        invalidCandidate();
      const matcher = object(route.match[0]);
      const handler = object(route.handle[0]);
      exactKeys(matcher, ['host']);
      exactKeys(handler, ['handler', 'routes']);
      if (
        !Array.isArray(matcher.host) ||
        matcher.host.length === 0 ||
        handler.handler !== 'subroute' ||
        !Array.isArray(handler.routes) ||
        handler.routes.some(
          (child) => child === null || typeof child !== 'object' || Array.isArray(child),
        )
      )
        invalidCandidate();
      for (const host of matcher.host) {
        if (typeof host !== 'string' || !subjectSet.has(host) || seen.has(host)) invalidCandidate();
        seen.add(host);
      }
      hosts.push(matcher.host as string[]);
    }
    guards.push(hosts);
  }
  if (canonicalBody(guards[0]) !== canonicalBody(guards[1])) invalidCandidate();
  // SAFETY: the private JSON tree passed the exact route-plan/listener/guard checks above.
  return plan as unknown as CaddyRoutePlan;
}

const isolatedKeys = new Set([
  'XDG_DATA_HOME',
  'XDG_CONFIG_HOME',
  'XDG_CACHE_HOME',
  'XDG_STATE_HOME',
  'XDG_RUNTIME_DIR',
  'HOME',
  'USERPROFILE',
  'HOMEDRIVE',
  'HOMEPATH',
  'APPDATA',
  'LOCALAPPDATA',
  'TEMP',
  'TMP',
  'TMPDIR',
  'CADDY_ADMIN',
]);
function childEnvironment(
  paths: SecureAdminPolicy['paths'],
  base: Readonly<NodeJS.ProcessEnv>,
): Readonly<NodeJS.ProcessEnv> {
  const environment: NodeJS.ProcessEnv = Object.create(null) as NodeJS.ProcessEnv;
  // Also remove differently cased keys on Unix: do not leave Windows aliases in the carrier.
  for (const [key, value] of Object.entries(base)) {
    if (!isolatedKeys.has(key.toUpperCase())) environment[key] = value;
  }
  Object.assign(environment, {
    XDG_DATA_HOME: paths.data,
    XDG_CONFIG_HOME: paths.config,
    XDG_CACHE_HOME: paths.data,
    XDG_STATE_HOME: paths.data,
    XDG_RUNTIME_DIR: paths.scratch,
    HOME: paths.home,
    TEMP: paths.scratch,
    TMP: paths.scratch,
    TMPDIR: paths.scratch,
  });
  if (process.platform === 'win32') {
    const drive = parse(paths.home).root.replace(/[\\/]+$/, '');
    Object.assign(environment, {
      USERPROFILE: paths.home,
      HOMEDRIVE: drive,
      HOMEPATH: paths.home.slice(drive.length),
      APPDATA: paths.config,
      LOCALAPPDATA: paths.data,
    });
  }
  return Object.freeze(environment);
}

/**
 * Caller holds the runtime lifetime lock. No server, port allocation or new lock is created.
 * Pinned Caddy 2.11.4 uses XDG_* on ALL OSes for DefaultStorage/admin identity and autosave.
 * Caddy owns admin leaf issuance/renewal; startup, mTLS denial and generated-file protection
 * still need native acceptance. Directory checks here never repair existing unsafe paths.
 */
export async function prepareSecureAdminPolicy(
  options: SecureAdminPolicyOptions,
): Promise<PortResult<SecureAdminPolicy>> {
  try {
    if (
      !Number.isInteger(options.adminPort) ||
      options.adminPort < 1 ||
      options.adminPort > 65535 ||
      [80, 443].includes(options.adminPort)
    )
      throw new CaddyPreparationError(
        'caddy_admin_policy_invalid',
        'Expected an explicit non-route administration port.',
      );
    if (!isAbsolute(options.runtimeBase) || !isAbsolute(options.materialDirectory))
      throw new CaddyPreparationError(
        'caddy_admin_policy_invalid',
        'Administration runtime and material directories must be absolute.',
      );
    const plan = guardedPlan(options.plan);
    await prepareRuntime(options.runtimeBase, options.owner);
    const base = resolve(options.runtimeBase);
    const paths = Object.freeze({
      base,
      data: join(base, 'data'),
      config: join(base, 'config'),
      scratch: join(base, 'scratch'),
      home: join(base, 'home'),
      defaultStorage: join(base, 'data', 'caddy'),
      autosaveDirectory: join(base, 'config', 'caddy'),
    });
    for (const directory of [
      paths.data,
      paths.config,
      paths.scratch,
      paths.home,
      paths.defaultStorage,
      paths.autosaveDirectory,
    ]) {
      await prepareRuntime(directory, options.owner);
      await assertProtected(directory, options.owner, true);
    }
    const material = await loadOrCreateAdminMaterial(options.materialDirectory, options.owner);
    if (!material.ok) return material;
    const { paths: pem, authorizedClientCertificateBase64, tls } = material.value;
    const config = candidate(
      canonicalBody({
        admin: {
          disabled: true,
          identity: {
            identifiers: ['localhost'],
            issuers: [{ module: 'internal', ca: 'cadder-admin' }],
          },
          remote: {
            listen: `127.0.0.1:${options.adminPort}`,
            access_control: [{ public_keys: [authorizedClientCertificateBase64] }],
          },
        },
        apps: {
          pki: {
            certificate_authorities: {
              'cadder-admin': {
                install_trust: false,
                root: { format: 'pem_file', certificate: pem.root.certificate },
                intermediate: {
                  format: 'pem_file',
                  certificate: pem.intermediate.certificate,
                  private_key: pem.intermediate.key,
                },
              },
              local: { install_trust: false },
            },
          },
          http: {
            servers: {
              cadder_http: { ...plan.servers.cadder_http, automatic_https: { disable: true } },
              cadder_https: {
                ...plan.servers.cadder_https,
                automatic_https: { disable: true },
                tls_connection_policies: [{}],
              },
            },
          },
          // The retained local/internal web issuer, distinct from the imported admin CA.
          // An empty subject list must not become a catch-all issuance policy.
          tls: {
            automation: {
              policies:
                plan.tlsSubjects.length === 0
                  ? []
                  : [
                      {
                        subjects: plan.tlsSubjects,
                        issuers: [{ module: 'internal', ca: 'local' }],
                      },
                    ],
            },
            certificates: { automate: plan.tlsSubjects },
          },
        },
      }),
    );
    return {
      ok: true,
      value: Object.freeze({
        config,
        paths,
        environment: childEnvironment(paths, options.baseEnvironment ?? process.env),
        client: Object.freeze({
          host: '127.0.0.1',
          port: options.adminPort,
          servername: 'localhost',
          rejectUnauthorized: true,
          ...tls,
        }),
      }),
    };
  } catch (error) {
    return {
      ok: false,
      error: {
        kind: 'configuration',
        code: error instanceof CaddyPreparationError ? error.code : 'caddy_admin_policy_failed',
        message: 'Caddy administration policy or protected runtime was refused.',
        guidance: null,
        retryable: false,
        requestId: null,
      },
    };
  }
}
