import { createServer, type Socket } from 'node:net';
import { chmod, lstat, unlink, writeFile } from 'node:fs/promises';
import { once } from 'node:events';
import { JsonChannel } from '../protocol/channel.ts';
import { authenticateServer } from '../protocol/authentication.ts';
import { dispatchRpc, errorResponseSchema, type RpcHandler } from '../protocol/rpc.ts';
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
  handler: RpcHandler,
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
  let discoveryOwned = false;
  const server = createServer((socket) => {
    if (closing || sockets.size >= 64) {
      socket.destroy();
      return;
    }
    sockets.add(socket);
    socket.once('close', () => sockets.delete(socket));
    const channel = new JsonChannel(socket);
    void (async () => {
      const session = await authenticateServer(channel, secret, paths.instance);
      try {
        const request = await session.receive();
        if (closing) return;
        const response = await dispatchRpc(request, handler);
        try {
          session.send(response);
        } catch (error) {
          if (errorCode(error) !== 'frame-too-large') throw error;
          session.send(
            errorResponseSchema.parse({
              protocolVersion: PROTOCOL_VERSION,
              requestId: response.requestId,
              error: {
                kind: 'frame',
                code: 'frame',
                message: 'Daemon response exceeds the IPC frame size limit.',
                guidance: null,
                retryable: false,
                requestId: response.requestId,
              },
            }),
          );
        }
        await new Promise<void>((resolve) => socket.end(resolve));
      } finally {
        session.close();
      }
    })().catch(() => {
      onDenied();
      channel.close();
    });
  });
  try {
    server.listen(paths.endpoint);
    await once(server, 'listening');
    if (process.platform !== 'win32') {
      await chmod(paths.endpoint, 0o600);
      await protectCreated(paths.endpoint, owner);
    }
    await createProtectedFile(paths.discovery, owner);
    // The lock owner may also replace validated, safe crash residue.
    discoveryOwned = true;
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
  } catch (error) {
    const failures = [error];
    try {
      await close();
    } catch (cleanupError) {
      failures.push(cleanupError);
    }
    if (failures.length > 1)
      throw new AggregateError(failures, 'IPC startup and cleanup failed.', { cause: error });
    throw error;
  }
  function close(): Promise<void> {
    closing ??= closeListener();
    return closing;
  }
  async function closeListener(): Promise<void> {
    const failures: unknown[] = [];
    for (const socket of sockets) {
      try {
        socket.destroy();
      } catch (error) {
        failures.push(error);
      }
    }
    try {
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    } catch (error) {
      if (errorCode(error) !== 'ERR_SERVER_NOT_RUNNING') failures.push(error);
    }
    if (discoveryOwned) {
      try {
        await unlink(paths.discovery);
      } catch (error) {
        if (errorCode(error) !== 'ENOENT') failures.push(error);
      }
    }
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1) throw new AggregateError(failures, 'IPC listener cleanup failed.');
  }
  return { close };
}
