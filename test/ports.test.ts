import { readFile } from 'node:fs/promises';
import { expect, expectTypeOf, it } from 'vitest';
import type { ProtocolError } from '../src/protocol/errors.ts';
import type {
  CaddyApplyReceipt,
  CaddyConfig,
  CaddyPort,
  CaddyValidation,
  ClientServicePort,
  DesiredState,
  DomainActivationResult,
  PlatformPort,
  PortResult,
  ProtectedPathKind,
  ProjectActivationResult,
  QueryLogsResult,
  QueryStateResult,
  RuntimeOwner,
  ShutdownResult,
  StoragePort,
} from '../src/contracts/ports.ts';
import type { RpcParams } from '../src/protocol/rpc.ts';

const validConfig: CaddyConfig = {
  effectiveConfigHash: 'config-hash',
  adaptedConfig: {
    format: 'json',
    body: JSON.stringify({
      apps: { http: { servers: { app: { routes: [{ match: [{ host: ['app.localhost'] }] }] } } } },
    }),
  },
};
const validation: CaddyValidation = { effectiveConfigHash: 'config-hash', diagnostics: [] };
const error: ProtocolError = {
  kind: 'configuration',
  code: 'invalid_configuration',
  message: 'The fixture rejected the candidate.',
  guidance: null,
  retryable: false,
  requestId: null,
};
const ok = <T>(value: T): PortResult<T> => ({ ok: true, value });
const fail = <T>(): PortResult<T> => ({ ok: false, error });

it('keeps the four ports independently mockable with typed result/error contracts', async () => {
  const caddy: CaddyPort = {
    validate: async (config) => (config === validConfig ? ok(validation) : fail()),
    apply: async (config) =>
      config === validConfig ? ok({ effectiveConfigHash: config.effectiveConfigHash }) : fail(),
  };
  const platform: PlatformPort = {
    currentOwner: async () => ok({ id: 'uid:1000', uid: 1000, elevated: false }),
    assertProtected: async () => ok(undefined),
  };
  const desired: DesiredState = {
    projects: [
      {
        projectKey: 'project-a',
        sourceWorkingDirectory: '/workspace/project-a',
        sourceConfigPath: '/workspace/project-a/Caddyfile',
        enabled: true,
        domains: [{ canonicalDomain: 'app.localhost', upstream: '127.0.0.1:3000', enabled: true }],
      },
    ],
  };
  const storage: StoragePort = {
    loadDesiredState: async () => ok(desired),
    persistDesiredState: async (state) => (state === desired ? ok(undefined) : fail()),
  };
  const service: ClientServicePort = {
    queryState: async () =>
      ok({
        requestId: '00000000-0000-4000-8000-000000000001',
        accepted: true,
        message: 'ok',
        snapshot: null,
      }),
    queryLogs: async (params) =>
      ok({
        requestId: '00000000-0000-4000-8000-000000000001',
        accepted: true,
        message: 'ok',
        stream: { ...params.stream, domainKey: params.stream.domainKey ?? null },
        streamStatus: 'empty',
        entries: [],
      }),
    setProjectEnabled: async (params) =>
      ok({
        ...params,
        requestId: '00000000-0000-4000-8000-000000000001',
        accepted: true,
        message: 'ok',
      }),
    setDomainEnabled: async (params) =>
      ok({
        ...params,
        requestId: '00000000-0000-4000-8000-000000000001',
        accepted: true,
        message: 'ok',
      }),
    shutdown: async () =>
      ok({ requestId: '00000000-0000-4000-8000-000000000001', accepted: true, message: 'ok' }),
  };

  expect((await caddy.validate(validConfig)).ok).toBe(true);
  expect((await platform.currentOwner()).ok).toBe(true);
  expect((await storage.loadDesiredState()).ok).toBe(true);
  expect((await service.shutdown()).ok).toBe(true);
  const rejected = fail<CaddyValidation>();
  expect(rejected.ok).toBe(false);
  if (!rejected.ok) expect(rejected.error).toEqual(error);
});

it('pins every port signature, result and shared error type', () => {
  expectTypeOf<CaddyPort['validate']>().parameters.toEqualTypeOf<[config: CaddyConfig]>();
  expectTypeOf<CaddyPort['validate']>().returns.toEqualTypeOf<
    Promise<PortResult<CaddyValidation>>
  >();
  expectTypeOf<CaddyPort['apply']>().parameters.toEqualTypeOf<[config: CaddyConfig]>();
  expectTypeOf<CaddyPort['apply']>().returns.toEqualTypeOf<
    Promise<PortResult<CaddyApplyReceipt>>
  >();

  expectTypeOf<PlatformPort['currentOwner']>().toEqualTypeOf<
    (explicitUid?: number) => Promise<PortResult<RuntimeOwner>>
  >();
  expectTypeOf<PlatformPort['currentOwner']>().returns.toEqualTypeOf<
    Promise<PortResult<RuntimeOwner>>
  >();
  expectTypeOf<PlatformPort['assertProtected']>().parameters.toEqualTypeOf<
    [path: string, owner: RuntimeOwner, kind: ProtectedPathKind]
  >();
  expectTypeOf<PlatformPort['assertProtected']>().returns.toEqualTypeOf<
    Promise<PortResult<void>>
  >();

  expectTypeOf<StoragePort['loadDesiredState']>().parameters.toEqualTypeOf<[]>();
  expectTypeOf<StoragePort['loadDesiredState']>().returns.toEqualTypeOf<
    Promise<PortResult<DesiredState>>
  >();
  expectTypeOf<StoragePort['persistDesiredState']>().parameters.toEqualTypeOf<
    [state: DesiredState]
  >();
  expectTypeOf<StoragePort['persistDesiredState']>().returns.toEqualTypeOf<
    Promise<PortResult<void>>
  >();

  expectTypeOf<ClientServicePort['queryState']>().parameters.toEqualTypeOf<[]>();
  expectTypeOf<ClientServicePort['queryState']>().returns.toEqualTypeOf<
    Promise<PortResult<QueryStateResult>>
  >();
  expectTypeOf<ClientServicePort['queryLogs']>().parameters.toEqualTypeOf<
    [params: RpcParams<'query-logs-request'>]
  >();
  expectTypeOf<ClientServicePort['queryLogs']>().returns.toEqualTypeOf<
    Promise<PortResult<QueryLogsResult>>
  >();
  expectTypeOf<ClientServicePort['setProjectEnabled']>().parameters.toEqualTypeOf<
    [params: RpcParams<'set-entrypoint-enabled-request'>]
  >();
  expectTypeOf<ClientServicePort['setProjectEnabled']>().returns.toEqualTypeOf<
    Promise<PortResult<ProjectActivationResult>>
  >();
  expectTypeOf<ClientServicePort['setDomainEnabled']>().parameters.toEqualTypeOf<
    [params: RpcParams<'set-domain-enabled-request'>]
  >();
  expectTypeOf<ClientServicePort['setDomainEnabled']>().returns.toEqualTypeOf<
    Promise<PortResult<DomainActivationResult>>
  >();
  expectTypeOf<ClientServicePort['shutdown']>().parameters.toEqualTypeOf<[]>();
  expectTypeOf<ClientServicePort['shutdown']>().returns.toEqualTypeOf<
    Promise<PortResult<ShutdownResult>>
  >();

  expectTypeOf<PortResult<never>>().toEqualTypeOf<
    Readonly<{ ok: true; value: never }> | Readonly<{ ok: false; error: ProtocolError }>
  >();
});

it('pins exact contract keys and excludes live ownership fields', () => {
  expectTypeOf<keyof CaddyConfig>().toEqualTypeOf<'effectiveConfigHash' | 'adaptedConfig'>();
  expectTypeOf<keyof CaddyConfig['adaptedConfig']>().toEqualTypeOf<'format' | 'body'>();
  expectTypeOf<keyof RuntimeOwner>().toEqualTypeOf<'id' | 'uid' | 'elevated'>();
  expectTypeOf<keyof DesiredState>().toEqualTypeOf<'projects'>();
  expectTypeOf<keyof DesiredState['projects'][number]>().toEqualTypeOf<
    'projectKey' | 'sourceWorkingDirectory' | 'sourceConfigPath' | 'enabled' | 'domains'
  >();
  expectTypeOf<keyof DesiredState['projects'][number]['domains'][number]>().toEqualTypeOf<
    'canonicalDomain' | 'upstream' | 'enabled'
  >();

  const project = {
    projectKey: 'project-a',
    sourceWorkingDirectory: '/workspace/project-a',
    sourceConfigPath: '/workspace/project-a/Caddyfile',
    enabled: false,
    domains: [{ canonicalDomain: 'app.localhost', upstream: null, enabled: false }],
  } satisfies DesiredState['projects'][number];

  expect(Object.keys(project)).not.toEqual(
    expect.arrayContaining([
      'registrationId',
      'entrypointInstance',
      'shimSessionNonce',
      'ownerProcess',
      'processId',
      'activeRoute',
      'lease',
    ]),
  );
});

it('keeps contracts as a protocol-only leaf module', async () => {
  const source = await readFile(new URL('../src/contracts/ports.ts', import.meta.url), 'utf8');
  const imports = [...source.matchAll(/\bfrom\s+['"]([^'"]+)['"]/g)].map(
    ([, specifier]) => specifier,
  );
  expect(imports).toHaveLength(2);
  expect(imports.every((specifier) => specifier?.startsWith('../protocol/'))).toBe(true);
  expect(source.match(/export interface /g)).toHaveLength(4);
});
