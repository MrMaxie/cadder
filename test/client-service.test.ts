import { afterEach, expect, it, vi } from 'vitest';
import { ClientService } from '../src/client/service.ts';
import { daemonCommand, launchDaemon } from '../src/client/launch.ts';
import { rpc } from '../src/client/connection.ts';
import { CadderError, RpcError } from '../src/protocol/errors.ts';
import { fakeSnapshot } from './fixtures/rpc-data.ts';
import { runtimeLogStream } from '../src/storage/schema.ts';
import type { ChildProcess } from 'node:child_process';

vi.mock('../src/client/connection.ts', () => ({ rpc: vi.fn() }));
vi.mock('../src/client/launch.ts', async (original) => ({
  ...(await original<typeof import('../src/client/launch.ts')>()),
  launchDaemon: vi.fn(),
}));
afterEach(() => {
  vi.resetAllMocks();
  vi.restoreAllMocks();
});
const basic = {
  requestId: '00000000-0000-4000-8000-000000000001',
  accepted: true,
  message: 'Done',
};
const ready = { ...basic, snapshot: fakeSnapshot() };
const offline = () => new CadderError('daemon-unavailable', 'Offline');

it('shares the closed RPC operations and retains typed offline/rejection failures', async () => {
  const client = new ClientService();
  vi.mocked(rpc).mockResolvedValue(ready);
  expect((await client.queryState()).ok).toBe(true);
  await client.queryLogs({ stream: runtimeLogStream, limit: 5 });
  await client.setProjectEnabled({ registrationId: 'project', enabled: false });
  await client.setDomainEnabled({
    registrationId: 'project',
    domainKey: 'app.localhost',
    enabled: false,
  });
  await client.shutdown();
  expect(vi.mocked(rpc).mock.calls.map((call) => call[1])).toEqual([
    'query-state-request',
    'query-logs-request',
    'set-entrypoint-enabled-request',
    'set-domain-enabled-request',
    'shutdown-daemon-request',
  ]);
  const conflict = new RpcError({
    kind: 'conflict',
    code: 'conflict',
    message: 'Conflict',
    guidance: null,
    retryable: false,
    requestId: null,
  });
  vi.mocked(rpc).mockRejectedValueOnce(conflict);
  expect(await client.setProjectEnabled({ registrationId: 'project', enabled: true })).toEqual({
    ok: false,
    error: conflict.protocolError,
  });
  vi.mocked(rpc).mockRejectedValueOnce(offline());
  expect(await client.queryState()).toHaveProperty('error.code', 'daemon_unavailable');
});

it('attaches first and launches only the packaged daemon when genuinely missing', async () => {
  const client = new ClientService();
  vi.mocked(rpc).mockResolvedValue(ready);
  await client.start();
  expect(launchDaemon).not.toHaveBeenCalled();
  vi.mocked(rpc).mockRejectedValueOnce(offline());
  await client.start();
  expect(launchDaemon).toHaveBeenCalledOnce();
  const command = await daemonCommand();
  expect(command.file).toBe(process.execPath);
  expect(command.args.at(-1)).toMatch(/entrypoints[/\\]cadderd\.ts$/);
  vi.mocked(rpc).mockRejectedValueOnce(new CadderError('authentication-failed', 'Wrong peer'));
  await expect(client.start()).rejects.toMatchObject({ code: 'authentication-failed' });
  expect(launchDaemon).toHaveBeenCalledOnce();
});

it('waits through owned shutdown before restart instead of treating closing as stopped', async () => {
  const client = new ClientService();
  vi.mocked(rpc)
    .mockResolvedValueOnce(basic)
    .mockRejectedValueOnce(new CadderError('daemon_shutting_down', 'Closing'))
    .mockRejectedValueOnce(offline())
    .mockRejectedValueOnce(offline())
    .mockRejectedValueOnce(offline())
    .mockRejectedValueOnce(offline())
    .mockResolvedValue(ready);
  await client.restart();
  const methods = vi.mocked(rpc).mock.calls.map((call) => call[1]);
  expect(methods[0]).toBe('shutdown-daemon-request');
  expect(methods.filter((method) => method === 'query-state-request')).toHaveLength(6);
  expect(launchDaemon).toHaveBeenCalledOnce();
  vi.mocked(rpc).mockRejectedValueOnce(offline());
  await client.stop();
});

it('waits for startup ACL publication without accepting an unsafe connection or masking later failures', async () => {
  const client = new ClientService();
  const kill = vi.fn();
  vi.mocked(launchDaemon).mockResolvedValue({ exitCode: null, kill } as unknown as ChildProcess);
  vi.mocked(rpc)
    .mockRejectedValueOnce(offline())
    .mockRejectedValueOnce(
      new CadderError('unsafe-runtime-permissions', 'ACL is not published yet'),
    )
    .mockResolvedValueOnce(ready);
  vi.spyOn(Date, 'now').mockReturnValue(31000).mockReturnValueOnce(0);
  await client.start();
  expect(kill).not.toHaveBeenCalled();
  vi.mocked(rpc)
    .mockRejectedValueOnce(offline())
    .mockRejectedValueOnce(new CadderError('authentication-failed', 'Wrong peer'));
  await expect(client.start()).rejects.toMatchObject({ code: 'authentication-failed' });
  expect(kill).toHaveBeenCalledOnce();
  vi.mocked(rpc).mockRejectedValueOnce(
    new CadderError('unsafe-runtime-permissions', 'Existing unsafe path'),
  );
  await expect(client.start()).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
  expect(launchDaemon).toHaveBeenCalledTimes(2);
});
