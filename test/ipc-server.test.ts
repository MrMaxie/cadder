import { randomBytes } from 'node:crypto';
import { createConnection } from 'node:net';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { rpc } from '../src/client/connection.ts';
import { resolvePaths } from '../src/daemon/paths.ts';
import { acquireRuntimeLock } from '../src/daemon/runtime-lock.ts';
import { serveIpc } from '../src/daemon/ipc-server.ts';
import { prepareRuntime, runtimeOwner } from '../src/platform/runtime-security.ts';
import { CadderError } from '../src/protocol/errors.ts';
import { JsonChannel } from '../src/protocol/channel.ts';

it('serves only authenticated RPC and reports typed handler failures', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cadder-ipc-'));
  const owner = await runtimeOwner();
  const paths = resolvePaths({ runtimeDir: root });
  await prepareRuntime(paths.directory, owner);
  const lock = await acquireRuntimeLock(paths, owner);
  let denied = 0;
  let called = 0;
  const server = await serveIpc(
    paths,
    owner,
    lock.secret,
    async (request) => {
      called++;
      if (request.method === 'status') return { online: true };
      if (request.method === 'typed')
        throw new CadderError('fixture-error', 'A safe fixture error');
      throw new Error('Do not expose internal error text');
    },
    () => {
      denied++;
    },
  );
  try {
    expect(await rpc(paths, 'status')).toEqual({ online: true });
    await expect(rpc(paths, 'typed')).rejects.toMatchObject({
      code: 'fixture-error',
      message: 'A safe fixture error',
    });
    await expect(rpc(paths, 'generic')).rejects.toThrow('See diagnostics');
    const calledBefore = called;
    const socket = createConnection(paths.endpoint);
    const channel = new JsonChannel(socket);
    channel.send({ method: 'shutdown' });
    await expect(channel.next()).rejects.toThrow('closed');
    channel.close();
    expect(called).toBe(calledBefore);
    expect(denied).toBe(1);
    await writeFile(paths.secret, randomBytes(32));
    await expect(rpc(paths, 'status')).rejects.toThrow('authentication failed');
    expect(called).toBe(calledBefore);
  } finally {
    await server.close();
    await lock.release();
    await rm(root, { recursive: true, force: true });
  }
});
