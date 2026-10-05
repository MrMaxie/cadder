import { randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { createConnection } from 'node:net';
import type { RuntimePaths } from '../daemon/paths.ts';
import { assertProtected, runtimeOwner } from '../platform/runtime-security.ts';
import { JsonChannel } from '../protocol/channel.ts';
import { authenticateClient } from '../protocol/authentication.ts';
import { responseSchema } from '../protocol/rpc.ts';
import { PROTOCOL_VERSION } from '../protocol/version.ts';
import { CadderError, errorCode } from '../protocol/errors.ts';

export async function rpc(paths: RuntimePaths, method: string, params?: unknown): Promise<unknown> {
  const owner = await runtimeOwner(process.getuid?.());
  let secret: Buffer;
  try {
    await assertProtected(paths.directory, owner, true);
    await assertProtected(paths.secret, owner);
    secret = await readFile(paths.secret);
  } catch (error) {
    if (errorCode(error) === 'ENOENT') throw unavailable();
    throw error;
  }
  if (secret.length !== 32)
    throw new CadderError('invalid-runtime-secret', 'Invalid runtime secret.');
  const socket = createConnection(paths.endpoint);
  const channel = new JsonChannel(socket);
  try {
    const session = await authenticateClient(channel, secret, paths.instance);
    try {
      const requestId = randomUUID();
      session.send({ protocolVersion: PROTOCOL_VERSION, requestId, method, params });
      const response = responseSchema.parse(await session.receive());
      if (response.requestId !== requestId)
        throw new CadderError('invalid-response', 'Daemon response does not match this request.');
      if (response.error) throw new CadderError(response.error.code, response.error.message);
      return response.result;
    } finally {
      session.close();
    }
  } catch (error) {
    if (['ENOENT', 'ECONNREFUSED'].includes(errorCode(error) ?? '')) throw unavailable();
    throw error;
  } finally {
    secret.fill(0);
    channel.close();
  }
}

function unavailable(): CadderError {
  return new CadderError(
    'daemon-unavailable',
    'cadderd is not running. Start it with cadder daemon start.',
  );
}
