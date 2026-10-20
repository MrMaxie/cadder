import { afterEach, expect, it, vi } from 'vitest';
import { resolve } from 'node:path';
import { buildRegistration, managedRun, shimPolicy } from '../src/shim/run.ts';
import { ClientService } from '../src/client/service.ts';
import { rpc } from '../src/client/connection.ts';
import { CadderError } from '../src/protocol/errors.ts';
import { registrationSchema } from '../src/protocol/dto.ts';
import { shimCommandCases, shimAliases } from './compatibility/catalog.ts';

vi.mock('../src/client/connection.ts', () => ({ rpc: vi.fn() }));
vi.mock('node:timers/promises', () => ({ setTimeout: vi.fn(async () => {}) }));
afterEach(() => {
  vi.resetAllMocks();
  vi.restoreAllMocks();
});
const basic = {
  requestId: '00000000-0000-4000-8000-000000000001',
  accepted: true,
  message: 'Done',
};

it('implements the released shim policy instead of testing the old Rust source text', () => {
  const policies = {
    managed: 'managed',
    'read-only-inspection': 'inspection',
    'explicit-passthrough': 'passthrough',
    unsupported: 'unsupported',
  };
  for (const entry of shimCommandCases)
    expect(shimPolicy([entry.command])).toBe(policies[entry.policy]);
  for (const alias of shimAliases) expect(shimPolicy(alias.argv)).not.toBe('unsupported');
});

it('keeps argv metadata exact and gives each real registration one matching fresh session identity', () => {
  const argv = ['run', '--config', 'folder with spaces/Caddyfile', '--adapter', 'caddyfile'];
  const registration = buildRegistration(argv);
  expect(registrationSchema.parse(registration)).toEqual(registration);
  expect(registration.registrationId).toBe(registration.entrypointInstance.instanceId);
  expect(registration.ownerProcess.shimSessionNonce).toBe(
    registration.entrypointInstance.shimSessionNonce,
  );
  expect(registration.entrypointInstance.shimSessionNonce).not.toBe(
    buildRegistration(argv).entrypointInstance.shimSessionNonce,
  );
  expect(registration.sourceConfigPath.raw).toBe(resolve('folder with spaces/Caddyfile'));
  expect(registration.shimRun?.rawArguments).toEqual(argv);
  expect(() => buildRegistration(['run', '--config'])).toThrow();
});

it('reconnects the same shim after a daemon restart and unregisters only its latest returned session', async () => {
  const client = new ClientService();
  vi.spyOn(client, 'start').mockResolvedValue();
  const abort = new AbortController();
  let registrations = 0;
  let heartbeats = 0;
  vi.mocked(rpc).mockImplementation((async (_paths: unknown, method: string) => {
    if (method === 'register-entrypoint-request')
      return { ...basic, registrationId: `returned-${++registrations}` };
    if (method === 'heartbeat-entrypoint-request') {
      heartbeats++;
      if (heartbeats === 1) throw new CadderError('daemon-unavailable', 'Offline');
      if (heartbeats === 2) throw new CadderError('stale_session', 'Restarted');
      abort.abort();
    }
    return basic;
  }) as typeof rpc);
  await managedRun(['run'], client, abort.signal);
  expect(client.start).toHaveBeenCalledOnce();
  expect(registrations).toBe(2);
  expect(vi.mocked(rpc).mock.calls.at(-1)?.[1]).toBe('unregister-entrypoint-request');
  expect(vi.mocked(rpc).mock.calls.at(-1)?.[2]).toMatchObject({ registrationId: 'returned-2' });
});

it('never delegates to independent Caddy on failed managed startup or cancellation', async () => {
  const client = new ClientService();
  const start = vi
    .spyOn(client, 'start')
    .mockRejectedValue(new CadderError('daemon-start-failed', 'Failed'));
  await expect(managedRun(['run'], client)).rejects.toMatchObject({ code: 'daemon-start-failed' });
  expect(rpc).not.toHaveBeenCalled();
  start.mockClear();
  await managedRun(['run'], client, AbortSignal.abort());
  expect(start).not.toHaveBeenCalled();
});
