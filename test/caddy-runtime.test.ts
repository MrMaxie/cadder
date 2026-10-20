import { mkdtemp, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { normalizeCaddyConfig, type SecureAdminPolicy } from '../src/caddy/admin-policy.ts';
import { CaddyRuntime } from '../src/caddy/runtime.ts';
import { configHash } from '../src/caddy/preparation.ts';
import {
  ConfigurationTransactions,
  type ConfigurationSnapshot,
} from '../src/daemon/configuration-transactions.ts';
import { assertProtected } from '../src/platform/runtime-security.ts';
import { backend, fixtureResolver } from './fixtures/caddy-runtime-backend.ts';

// Only this test's policy/path and HTTPS seams are fake. The real validator and queue are exercised.
vi.mock('../src/platform/runtime-security.ts', () => ({
  assertProtected: vi.fn(async () => undefined),
  assertRuntimeDescendant: vi.fn(async () => undefined),
  createProtectedFile: async (path: string, _owner: unknown, body: string) => {
    await writeFile(path, body, { flag: 'wx', mode: 0o600 });
    return true;
  },
}));
vi.mock('../src/caddy/admin-client.ts', async (original) => {
  const actual = await original<typeof import('../src/caddy/admin-client.ts')>();
  return {
    CaddyAdminClient: class extends actual.CaddyAdminClient {
      override apply = backend.apply.bind(backend);
      override readActive = backend.readActive.bind(backend);
      override stop = backend.stop.bind(backend);
    },
  };
});
let directory: string;
const runtimes: CaddyRuntime[] = [];
function candidate(tag = 'first') {
  return normalizeCaddyConfig(
    JSON.stringify({
      admin: { disabled: true, remote: { listen: 'fixture' } },
      apps: {
        pki: { certificate_authorities: { local: { install_trust: false } } },
        http: { tag },
      },
    }),
  );
}
function policy(): SecureAdminPolicy {
  return {
    config: candidate(),
    paths: {
      base: directory,
      data: join(directory, 'data'),
      config: join(directory, 'config'),
      home: join(directory, 'home'),
      scratch: join(directory, 'scratch'),
      defaultStorage: join(directory, 'data', 'caddy'),
      autosaveDirectory: join(directory, 'config', 'caddy'),
    },
    environment: { FIXTURE_STARTUP: 'immutable' },
    client: {
      host: '127.0.0.1',
      port: 12345,
      servername: 'localhost',
      rejectUnauthorized: true,
      ca: 'fixture-ca',
      cert: 'fixture-cert',
      key: 'fixture-key',
    },
  };
}
function runtime(input = policy()): CaddyRuntime {
  const value = new CaddyRuntime(
    input,
    { id: 'fixture-owner', elevated: false },
    fixtureResolver(),
    { startupMs: 150, stopMs: 30, pollMs: 2 },
  );
  runtimes.push(value);
  return value;
}
beforeEach(async () => {
  backend.reset();
  vi.mocked(assertProtected).mockReset().mockResolvedValue(undefined);
  directory = await mkdtemp(join(tmpdir(), 'cadder-runtime-test-'));
  const { mkdir } = await import('node:fs/promises');
  await mkdir(join(directory, 'scratch'));
});
afterEach(async () => {
  for (const value of runtimes.splice(0)) await value.close().catch(() => undefined);
  await rm(directory, { recursive: true, force: true });
});

it('idle -> secure initial run -> independently observed active -> reload -> actual idle', async () => {
  const value = runtime();
  expect(await value.readActive()).toEqual({ ok: true, value: null });
  expect(await value.apply(candidate())).toEqual({ status: 'applied' });
  expect(backend.runCalls[0]!.args.slice(0, 2)).toEqual(['run', '--config']);
  expect(backend.runCalls[0]!.args).not.toContain('--resume');
  expect(backend.runCalls[0]!.options.longLived).toBe(true);
  expect(await value.readActive()).toEqual({ ok: true, value: candidate().effectiveConfigHash });
  expect(await value.apply(candidate('reload'))).toEqual({ status: 'applied' });
  expect(await value.readActive()).toEqual({
    ok: true,
    value: candidate('reload').effectiveConfigHash,
  });
  expect(backend.runCalls).toHaveLength(1);
  expect(await value.apply(null)).toEqual({ status: 'applied' });
  expect(backend.stops).toBe(1);
  expect(await value.readActive()).toEqual({ ok: true, value: null });
  expect(await readdir(join(directory, 'scratch'))).toEqual([]);
});

it('performs only a conflict check before verified native startup', async () => {
  let release!: () => void;
  backend.launchGate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const value = runtime();
  const pending = value.apply(candidate());
  await vi.waitFor(() => expect(backend.runCalls).toHaveLength(1), { interval: 2 });
  expect(backend.reads).toBe(1);
  expect(backend.stops).toBe(0);
  release();
  expect(await pending).toEqual({ status: 'applied' });
  expect(backend.reads).toBeGreaterThan(0);
});

it.each(['ready', 'silent', 'exit', 'launch-failed'] as const)(
  'refuses an existing matching endpoint without launching or stopping it (%s)',
  async (mode) => {
    backend.active = candidate().effectiveConfigHash;
    backend.startup = mode;
    const value = runtime();
    expect(await value.apply(candidate())).toEqual({ status: 'definitely-rejected' });
    await value.close();
    expect(backend.runCalls).toHaveLength(0);
    expect(backend.reads).toBe(1);
    expect(backend.stops).toBe(0);
    expect(backend.active).toBe(candidate().effectiveConfigHash);
    expect(await readdir(join(directory, 'scratch'))).toEqual([]);
  },
);

it('close cancels delayed native startup without administering an unstarted process', async () => {
  backend.launchGate = new Promise<void>(() => undefined);
  const value = runtime();
  const pending = value.apply(candidate());
  await vi.waitFor(() => expect(backend.runCalls).toHaveLength(1), { interval: 2 });
  await value.close();
  expect(await pending).toEqual({ status: 'ambiguous' });
  expect(backend.reads).toBe(1);
  expect(backend.stops).toBe(0);
  expect(await readdir(join(directory, 'scratch'))).toEqual([]);
});

it('rejection and ambiguous reload never substitute receipts for independent state', async () => {
  const value = runtime();
  await value.apply(candidate());
  backend.loadStatus = 'definitely-rejected';
  expect(await value.apply(candidate('rejected'))).toEqual({ status: 'definitely-rejected' });
  expect(await value.readActive()).toEqual({ ok: true, value: candidate().effectiveConfigHash });
  backend.loadStatus = 'ambiguous';
  backend.uncertainLoadChanges = true;
  expect(await value.apply(candidate('uncertain'))).toEqual({ status: 'ambiguous' });
  expect(await value.readActive()).toEqual({
    ok: true,
    value: candidate('uncertain').effectiveConfigHash,
  });
  backend.active = null;
  expect((await value.readActive()).ok).toBe(false);
});

it('snapshots candidates and startup environment before asynchronous effects', async () => {
  const original = policy();
  const value = runtime(original);
  Object.assign(original.environment, { FIXTURE_STARTUP: 'mutated' });
  const first = structuredClone(candidate());
  const hash = first.effectiveConfigHash;
  const pending = value.apply(first);
  Object.assign(first.adaptedConfig, { body: '{}' });
  Object.assign(first, { effectiveConfigHash: 'mutated' });
  expect(await pending).toEqual({ status: 'applied' });
  expect(backend.runCalls[0]!.options.env!.FIXTURE_STARTUP).toBe('immutable');
  expect(await value.readActive()).toEqual({ ok: true, value: hash });
});

it('refuses malformed, noncanonical and security-changing candidates before any effects', async () => {
  const value = runtime();
  expect(await value.apply({ ...candidate(), effectiveConfigHash: 'wrong' })).toEqual({
    status: 'definitely-rejected',
  });
  const changed = normalizeCaddyConfig('{"admin":{"disabled":false},"apps":{"pki":{}}}');
  expect(await value.apply(changed)).toEqual({ status: 'definitely-rejected' });
  expect(await value.apply(normalizeCaddyConfig('{}'))).toEqual({ status: 'definitely-rejected' });
  const spaced = ` ${candidate().adaptedConfig.body}`;
  expect(
    await value.apply({
      adaptedConfig: { format: 'json', body: spaced },
      effectiveConfigHash: configHash(spaced),
    }),
  ).toEqual({ status: 'definitely-rejected' });
  expect(backend.runCalls).toEqual([]);
  expect(backend.reads).toBe(0);
  expect(vi.mocked(assertProtected)).not.toHaveBeenCalled();
});

it.each(['silent', 'mismatch', 'exit'] as const)(
  'settles startup %s and supports explicit retry only',
  async (mode) => {
    backend.startup = mode;
    const value = runtime();
    expect(await value.apply(candidate())).toEqual({ status: 'ambiguous' });
    expect(await value.readActive()).toEqual({ ok: true, value: null });
    expect(await readdir(join(directory, 'scratch'))).toEqual([]);
    expect(backend.runCalls).toHaveLength(1);
    backend.startup = 'ready';
    expect(await value.apply(candidate('retry'))).toEqual({ status: 'applied' });
    expect(backend.runCalls).toHaveLength(2);
  },
);

it('unexpected exit is unavailable, never cached active/idle, until explicit reconciliation', async () => {
  const value = runtime();
  await value.apply(candidate());
  backend.settle?.();
  await Promise.resolve();
  await Promise.resolve();
  expect((await value.readActive()).ok).toBe(false);
  expect(backend.runCalls).toHaveLength(1);
  expect(await value.apply(candidate('explicit'))).toEqual({ status: 'applied' });
  expect(backend.runCalls).toHaveLength(2);
});

it.each(['ignored', 'hanging'] as const)(
  'forces only owned work after bounded %s stop',
  async (mode) => {
    const value = runtime();
    await value.apply(candidate());
    backend.stopMode = mode;
    expect(await value.apply(null)).toEqual({ status: 'applied' });
    expect(await value.readActive()).toEqual({ ok: true, value: null });
    expect(await readdir(join(directory, 'scratch'))).toEqual([]);
  },
);

it('close cancels startup readback, refuses queued admissions and drains before return', async () => {
  backend.startup = 'silent';
  backend.blockRead = true;
  const value = runtime();
  const starting = value.apply(candidate());
  await vi.waitFor(() => expect(backend.runCalls).toHaveLength(1));
  const queued = value.apply(candidate('never'));
  const closing = value.close();
  expect(await value.apply(candidate('closed'))).toEqual({ status: 'definitely-rejected' });
  expect(await queued).toEqual({ status: 'definitely-rejected' });
  expect(await starting).toEqual({ status: 'ambiguous' });
  await closing;
  expect(value.close()).toBe(closing);
  expect(await readdir(join(directory, 'scratch'))).toEqual([]);
  expect((await value.readActive()).ok).toBe(false);
});

it.each(['graceful', 'ignored', 'hanging'] as const)(
  'healthy close attempts bounded %s stop before owned force',
  async (mode) => {
    const value = runtime();
    await value.apply(candidate());
    backend.stopMode = mode;
    await value.close();
    expect(backend.stops).toBe(1);
    expect(await readdir(join(directory, 'scratch'))).toEqual([]);
    expect(await value.apply(candidate('closed'))).toEqual({ status: 'definitely-rejected' });
  },
);

it('close aborts a reload in flight without waiting for its normal admin deadline', async () => {
  const value = runtime();
  await value.apply(candidate());
  backend.blockRead = true;
  const loading = value.apply(candidate('reload'));
  await vi.waitFor(() => expect(backend.loads).toBe(1));
  await value.close();
  expect(await loading).toEqual({ status: 'ambiguous' });
  expect(await readdir(join(directory, 'scratch'))).toEqual([]);
});

it('cleanup faults stay ambiguous, retain staging and prevent false idle/restart', async () => {
  const value = runtime();
  await value.apply(candidate());
  backend.stopMode = 'ignored';
  backend.cleanupFault = true;
  expect(await value.apply(null)).toEqual({ status: 'ambiguous' });
  expect((await value.readActive()).ok).toBe(false);
  expect(await value.apply(candidate('unsafe-restart'))).toEqual({ status: 'ambiguous' });
  expect(await readdir(join(directory, 'scratch'))).toHaveLength(1);
  await expect(value.close()).rejects.toThrow('teardown could not be proved');
});

it('refuses unsafe roots and validator rejection without a native start', async () => {
  vi.mocked(assertProtected).mockRejectedValue(new Error('private ACL parser diagnostic'));
  const value = runtime();
  expect(await value.apply(candidate())).toEqual({ status: 'definitely-rejected' });
  expect(backend.runCalls).toEqual([]);
  vi.mocked(assertProtected).mockResolvedValue(undefined);
  backend.validationFails = true;
  expect(await value.apply(candidate())).toEqual({ status: 'definitely-rejected' });
  expect(backend.runCalls).toEqual([]);
});

it('close cancels between strict root checks without acquiring native or admin work', async () => {
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => {
    release = resolve;
  });
  vi.mocked(assertProtected).mockImplementationOnce(() => blocked);
  const value = runtime();
  const pending = value.apply(candidate());
  await vi.waitFor(() => expect(assertProtected).toHaveBeenCalledTimes(1), { interval: 2 });
  const closing = value.close();
  release();
  expect(await pending).toEqual({ status: 'definitely-rejected' });
  await closing;
  expect(assertProtected).toHaveBeenCalledTimes(1);
  expect(backend.runCalls).toEqual([]);
  expect(backend.reads).toBe(0);
});

it('staging deletion failure remains unavailable and close refuses false success', async () => {
  const value = runtime();
  await value.apply(candidate());
  vi.mocked(assertProtected).mockRejectedValue(new Error('staging replaced'));
  expect(await value.apply(null)).toEqual({ status: 'ambiguous' });
  expect((await value.readActive()).ok).toBe(false);
  await expect(value.close()).rejects.toThrow('teardown could not be proved');
});

it('real transaction queue rolls persistence-failed first start back to actual idle and reconciles', async () => {
  const value = runtime();
  const idle: ConfigurationSnapshot = {
    config: null,
    desiredState: { projects: [] },
    registrations: [],
  };
  const queue = new ConfigurationTransactions(
    idle,
    {
      validate: async (config) => ({
        ok: true,
        value: { effectiveConfigHash: config.effectiveConfigHash, diagnostics: [] },
      }),
    },
    {
      persistDesiredState: async () => ({
        ok: false,
        error: {
          kind: 'storage',
          code: 'fixture_persist_failed',
          message: 'Fixture persistence failure.',
          guidance: null,
          retryable: false,
          requestId: null,
        },
      }),
    },
    value,
  );
  expect(
    (await queue.submit(() => ({ ok: true, value: { ...idle, config: candidate() } }))).ok,
  ).toBe(false);
  expect(queue.readCommitted()).toEqual(idle);
  expect(await value.readActive()).toEqual({ ok: true, value: null });
  expect((await queue.reconcile()).ok).toBe(true);
  await queue.close();
});

it.each([false, true])(
  'real queue recovers an unexpected exit only after proved cleanup (cleanupFault=%s)',
  async (cleanupFault) => {
    const value = runtime();
    const idle: ConfigurationSnapshot = {
      config: null,
      desiredState: { projects: [] },
      registrations: [],
    };
    const persist = vi.fn(async () => ({ ok: true as const, value: undefined }));
    const queue = new ConfigurationTransactions(
      idle,
      {
        validate: async (config) => ({
          ok: true,
          value: { effectiveConfigHash: config.effectiveConfigHash, diagnostics: [] },
        }),
      },
      { persistDesiredState: persist },
      value,
    );
    const committed = { ...idle, config: candidate() };
    expect((await queue.submit(() => ({ ok: true, value: committed }))).ok).toBe(true);
    if (cleanupFault) {
      backend.stopMode = 'ignored';
      backend.cleanupFault = true;
      expect(await value.apply(null)).toEqual({ status: 'ambiguous' });
    } else {
      backend.settle?.();
    }
    expect((await value.readActive()).ok).toBe(false);
    expect((await queue.reconcile()).ok).toBe(!cleanupFault);
    expect(queue.readCommitted()).toEqual(committed);
    expect(persist).toHaveBeenCalledTimes(1);
    expect(backend.runCalls).toHaveLength(cleanupFault ? 1 : 2);
    if (cleanupFault) {
      expect(queue.readFence()?.code).toBe('config_reconciliation_failed');
      expect((await value.readActive()).ok).toBe(false);
    } else {
      expect(queue.readFence()).toBeNull();
      expect(await value.readActive()).toEqual({
        ok: true,
        value: committed.config.effectiveConfigHash,
      });
    }
    await queue.close();
  },
);
