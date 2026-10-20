import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { startDaemon } from '../src/daemon/start.ts';
import { rpc } from '../src/client/connection.ts';
import { runtimeLogStream } from '../src/storage/schema.ts';

it('runs actual protected product handlers and storage through authenticated IPC, then releases the runtime', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'cadder-product-'));
  let daemon: Awaited<ReturnType<typeof startDaemon>> | undefined;
  try {
    daemon = await startDaemon({ runtimeDir: directory });
    const state = await rpc(daemon.paths, 'query-state-request');
    expect(state.snapshot).toMatchObject({
      registrations: [],
      runtime: { status: 'idle' },
      config: { status: 'idle' },
      storage: { backend: 'sqlite', schemaVersion: 1 },
    });
    const logs = await rpc(daemon.paths, 'query-logs-request', {
      stream: runtimeLogStream,
      limit: 10,
    });
    expect(logs.entries[0]?.rawMessage).toBe('Daemon started.');
    expect((await rpc(daemon.paths, 'shutdown-daemon-request')).accepted).toBe(true);
    await daemon.stop();
    await expect(rpc(daemon.paths, 'query-state-request')).rejects.toMatchObject({
      code: 'daemon-unavailable',
    });
    daemon = await startDaemon({ runtimeDir: directory });
    expect((await rpc(daemon.paths, 'query-state-request')).snapshot?.registrations).toEqual([]);
  } finally {
    await daemon?.stop();
    await rm(directory, { recursive: true, force: true });
  }
}, 60000);
