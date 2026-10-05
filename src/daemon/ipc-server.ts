import { createServer, type Socket } from 'node:net';
import { chmod, lstat, unlink, writeFile } from 'node:fs/promises';
import { once } from 'node:events';
import { JsonChannel } from '../protocol/channel.ts';
import { authenticateServer } from '../protocol/authentication.ts';
import { requestSchema, type RpcRequest } from '../protocol/rpc.ts';
import { CadderError, errorCode } from '../protocol/errors.ts';
import { PROTOCOL_VERSION, SECURITY_POLICY_VERSION, VERSION } from '../protocol/version.ts';
import {
  createProtectedFile,
  protectCreated,
  type RuntimeOwner,
} from '../platform/runtime-security.ts';
import type { RuntimePaths } from './paths.ts';

export async function serveIpc(
  paths: RuntimePaths,
  owner: RuntimeOwner,
  secret: Buffer,
  handler: (request: RpcRequest) => Promise<unknown>,
  onDenied: () => void = () => {},
): Promise<{ close(): Promise<void> }> {
  // Caller must hold the SQLite runtime lock before removing crash residue.
  if (process.platform !== 'win32') {
    try {
      const info = await lstat(paths.endpoint);
      if (!info.isSocket() || info.uid !== owner.uid)
        throw new CadderError('unsafe-endpoint', 'Refusing to replace a non-owned socket path.');
      await unlink(paths.endpoint);
    } catch (error) {
      if (errorCode(error) !== 'ENOENT') throw error;
    }
  }
  const sockets = new Set<Socket>();
  let closing: Promise<void> | undefined;
  let published = false;
  const server = createServer((socket) => {
    if (sockets.size >= 64) {
      socket.destroy();
      return;
    }
    sockets.add(socket);
    socket.once('close', () => sockets.delete(socket));
    const channel = new JsonChannel(socket);
    void (async () => {
      const session = await authenticateServer(channel, secret, paths.instance);
      try {
        const request = requestSchema.parse(await session.receive());
        let response;
        try {
          response = { result: await handler(request) };
        } catch (error) {
          response = {
            error: {
              code: errorCode(error) ?? 'request-failed',
              message:
                error instanceof CadderError
                  ? error.message
                  : 'Daemon request failed. See diagnostics.',
            },
          };
        }
        session.send({
          protocolVersion: PROTOCOL_VERSION,
          requestId: request.requestId,
          ...response,
        });
        await new Promise<void>((resolve) => socket.end(resolve));
      } finally {
        session.close();
      }
    })().catch(() => {
      onDenied();
      channel.close();
    });
  });
  server.listen(paths.endpoint);
  await once(server, 'listening');
  try {
    if (process.platform !== 'win32') {
      await chmod(paths.endpoint, 0o600);
      await protectCreated(paths.endpoint, owner);
    }
    await createProtectedFile(paths.discovery, owner);
    await writeFile(
      paths.discovery,
      JSON.stringify({
        version: VERSION,
        protocolVersion: PROTOCOL_VERSION,
        securityPolicyVersion: SECURITY_POLICY_VERSION,
        runtimeDir: paths.directory,
        profile: paths.profile,
        instance: paths.instance,
        endpoint: paths.endpoint,
        processId: process.pid,
        owner: owner.id,
        elevated: owner.elevated,
      }),
    );
    published = true;
  } catch (error) {
    await close();
    throw error;
  }
  function close(): Promise<void> {
    closing ??= closeListener();
    return closing;
  }
  async function closeListener(): Promise<void> {
    for (const socket of sockets) socket.destroy();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
    if (published) {
      await unlink(paths.discovery).catch((error: unknown) => {
        if (errorCode(error) !== 'ENOENT') throw error;
      });
    }
  }
  return { close };
}
