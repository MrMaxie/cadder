import { randomUUID } from 'node:crypto';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { DaemonApplication } from '../src/daemon/application.ts';
import { ConfigurationTransactions } from '../src/daemon/configuration-transactions.ts';
import { normalizeCaddyConfig } from '../src/caddy/admin-policy.ts';
import type { DesiredState, CaddyConfig, PortResult } from '../src/contracts/ports.ts';
import { dispatchRpc, type RpcMethod, type RpcParams } from '../src/protocol/rpc.ts';
import { fakeRegistration } from './fixtures/rpc-data.ts';
import { PROTOCOL_VERSION } from '../src/protocol/version.ts';
import { runtimeLogStream } from '../src/storage/schema.ts';

const roots: string[] = [];
afterEach(async () => {
  vi.restoreAllMocks();
  for (const path of roots.splice(0)) await rm(path, { recursive: true, force: true });
});
async function fixture(initial: DesiredState = { projects: [] }) {
  const root = await mkdtemp(join(tmpdir(), 'cadder-app-'));
  roots.push(root);
  const config = join(root, 'Caddyfile');
  await writeFile(config, 'app.localhost { respond ok }');
  let durable = structuredClone(initial);
  let active: string | null = null;
  let rejected = false;
  let persistFailed = false;
  const queue = new ConfigurationTransactions(
    { config: null, registrations: [], desiredState: durable },
    {
      validate: async (candidate) => ({
        ok: true,
        value: { effectiveConfigHash: candidate.effectiveConfigHash, diagnostics: [] },
      }),
    },
    {
      persistDesiredState: async (value) => {
        if (persistFailed)
          return {
            ok: false,
            error: {
              kind: 'storage',
              code: 'storage',
              message: 'Rejected',
              guidance: null,
              retryable: false,
              requestId: null,
            },
          };
        durable = structuredClone(value);
        return { ok: true, value: undefined };
      },
    },
    {
      apply: async (value) => {
        if (rejected) return { status: 'definitely-rejected' };
        active = value?.effectiveConfigHash ?? null;
        return { status: 'applied' };
      },
      readActive: async () => ({ ok: true, value: active }),
    },
  );
  const adapt = vi.fn(
    async (): Promise<PortResult<CaddyConfig>> => ({
      ok: true,
      value: normalizeCaddyConfig(
        JSON.stringify({
          apps: {
            http: {
              servers: {
                project: {
                  routes: [
                    {
                      match: [{ host: ['app.localhost'] }],
                      handle: [{ handler: 'static_response', body: 'ok' }],
                    },
                  ],
                },
              },
            },
          },
        }),
      ),
    }),
  );
  const stop = vi.fn();
  const app = new DaemonApplication({
    transactions: queue,
    adapt,
    secure: async (plan) => ({ ok: true, value: normalizeCaddyConfig(JSON.stringify({ plan })) }),
    queryLogs: async () => [],
    log: async () => {},
    shutdown: stop,
    storage: null,
    runtime: () => ({
      status: active ? 'running' : 'idle',
      binaryPath: null,
      version: null,
      processId: null,
      adminEndpoint: null,
      diagnostics: [],
    }),
  });
  const registration = fakeRegistration();
  registration.entrypointInstance.shimSessionNonce = registration.ownerProcess.shimSessionNonce =
    randomUUID();
  registration.sourceWorkingDirectory = { raw: root, canonical: null };
  registration.sourceConfigPath = { raw: config, canonical: null };
  async function call<M extends RpcMethod>(method: M, params: RpcParams<M>) {
    return dispatchRpc(
      { protocolVersion: PROTOCOL_VERSION, requestId: randomUUID(), method, params },
      app.handle,
    );
  }
  async function register() {
    const response = await call('register-entrypoint-request', { registration });
    expect(response).not.toHaveProperty('error');
    return app.snapshot().registrations[0]!.registrationId;
  }
  return {
    app,
    call,
    registration,
    register,
    adapt,
    queue,
    stop,
    durable: () => durable,
    active: () => active,
    reject: () => {
      rejected = true;
    },
    failPersist: () => {
      persistFailed = true;
    },
  };
}

it('registers from actual canonical sources, derives domains and rejects a competing or stale session', async () => {
  const f = await fixture();
  const id = await f.register();
  expect(f.app.snapshot().registrations[0]).toMatchObject({
    registrationId: id,
    activationState: 'active',
    registeredDomains: [{ name: { canonical: 'app.localhost' } }],
  });
  const intruder = structuredClone(f.registration);
  intruder.entrypointInstance.shimSessionNonce = intruder.ownerProcess.shimSessionNonce = 'other';
  expect(await f.call('register-entrypoint-request', { registration: intruder })).toHaveProperty(
    'error',
  );
  expect(
    await f.call('unregister-entrypoint-request', {
      registrationId: id,
      shimSessionNonce: 'other',
    }),
  ).toHaveProperty('error');
  expect(
    await f.call('heartbeat-entrypoint-request', { registrationId: id, shimSessionNonce: 'other' }),
  ).toHaveProperty('error');
  expect(f.app.snapshot().registrations).toHaveLength(1);
  expect(
    await f.call('heartbeat-entrypoint-request', {
      registrationId: id,
      shimSessionNonce: f.registration.entrypointInstance.shimSessionNonce,
    }),
  ).not.toHaveProperty('error');
  expect(await f.call('query-state-request', {})).toHaveProperty(
    'result.snapshot.registrations.0.registrationId',
    id,
  );
});

it('toggles project/domain intent through apply and keeps disabled domains disabled after project re-enable', async () => {
  const f = await fixture();
  const id = await f.register();
  expect(
    await f.call('set-domain-enabled-request', {
      registrationId: id,
      domainKey: 'APP.localhost',
      enabled: false,
    }),
  ).not.toHaveProperty('error');
  expect(f.active()).toBeNull();
  expect(
    await f.call('set-entrypoint-enabled-request', { registrationId: id, enabled: false }),
  ).not.toHaveProperty('error');
  expect(
    await f.call('set-entrypoint-enabled-request', { registrationId: id, enabled: true }),
  ).not.toHaveProperty('error');
  expect(f.app.snapshot().registrations[0]!.registeredDomains[0]?.activationState).toBe('inactive');
  expect(f.durable().projects[0]?.domains[0]?.enabled).toBe(false);
  expect(
    await f.call('set-domain-enabled-request', {
      registrationId: id,
      domainKey: 'missing.localhost',
      enabled: true,
    }),
  ).toHaveProperty('error');
  expect(
    await f.call('set-entrypoint-enabled-request', { registrationId: 'missing', enabled: true }),
  ).toHaveProperty('error');
  expect(
    await f.call('set-entrypoint-enabled-request', {
      registrationId: id,
      shimSessionNonce: 'stale',
      enabled: false,
    }),
  ).toHaveProperty('error');
});

it('preserves committed routes after apply rejection or storage failure and rolls back the real queue', async () => {
  for (const mode of ['reject', 'persist'] as const) {
    const f = await fixture();
    const id = await f.register();
    const before = f.active();
    if (mode === 'reject') f.reject();
    else f.failPersist();
    expect(
      await f.call('set-entrypoint-enabled-request', { registrationId: id, enabled: false }),
    ).toHaveProperty('error');
    expect(f.active()).toBe(before);
    expect(f.app.snapshot().registrations[0]?.activationState).toBe('active');
    expect(f.durable().projects[0]?.enabled).toBe(true);
  }
});

it('expires live sessions, preserves durable intent, and restores no lease on restart', async () => {
  const f = await fixture();
  const id = await f.register();
  await f.app.expire(Date.now() + 1);
  expect(f.app.snapshot().registrations).toEqual([]);
  expect(f.active()).toBeNull();
  expect(f.durable().projects).toHaveLength(1);
  const restarted = await fixture(f.durable());
  expect(restarted.app.snapshot().registrations).toEqual([]);
  await f.register();
  expect(
    await f.call('unregister-entrypoint-request', {
      registrationId: id,
      shimSessionNonce: f.registration.entrypointInstance.shimSessionNonce,
    }),
  ).not.toHaveProperty('error');
  expect(f.active()).toBeNull();
});

it('rejects forged paths/domains and accepts only bounded canonical log selections', async () => {
  const f = await fixture();
  const relative = structuredClone(f.registration);
  relative.sourceConfigPath.raw = 'Caddyfile';
  expect(await f.call('register-entrypoint-request', { registration: relative })).toHaveProperty(
    'error',
  );
  await f.register();
  for (const limit of [0, 201])
    expect(await f.call('query-logs-request', { stream: runtimeLogStream, limit })).toHaveProperty(
      'error',
    );
  expect(await f.call('query-logs-request', { stream: runtimeLogStream })).toHaveProperty(
    'result.entries',
  );
  expect(
    await f.call('query-logs-request', { stream: { ...runtimeLogStream, streamId: 'forged' } }),
  ).toHaveProperty('error');
  expect(
    await f.call('query-logs-request', {
      stream: f.app.snapshot().registrations[0]!.registeredDomains[0]!.logStream,
    }),
  ).toHaveProperty('result.entries');
});

it('acknowledges shutdown before closing admission and scheduling owned teardown', async () => {
  const f = await fixture();
  expect(await f.call('shutdown-daemon-request', {})).not.toHaveProperty('error');
  expect(await f.call('query-state-request', {})).toHaveProperty('error');
  expect(f.stop).not.toHaveBeenCalled();
  await vi.waitFor(() => expect(f.stop).toHaveBeenCalledOnce());
});
