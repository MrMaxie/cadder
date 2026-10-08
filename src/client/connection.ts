import { randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { createConnection } from 'node:net';
import type { RuntimePaths } from '../daemon/paths.ts';
import { assertProtected, runtimeOwner } from '../platform/runtime-security.ts';
import { JsonChannel } from '../protocol/channel.ts';
import { authenticateClient } from '../protocol/authentication.ts';
import {
  requestSchema,
  responseSchema,
  UNCORRELATED_REQUEST_ID,
  type RpcMethod,
  type RpcParams,
  type RpcResult,
} from '../protocol/rpc.ts';
import { PROTOCOL_VERSION } from '../protocol/version.ts';
import { CadderError, RpcError, errorCode } from '../protocol/errors.ts';

export async function rpc<M extends RpcMethod>(
  paths: RuntimePaths,
  method: M,
  ...args: Record<string, never> extends RpcParams<M>
    ? [params?: NoInfer<RpcParams<M>>]
    : [params: NoInfer<RpcParams<M>>]
): Promise<RpcResult<M>> {
  const requestId = randomUUID();
  const request = requestSchema.safeParse({
    protocolVersion: PROTOCOL_VERSION,
    requestId,
    method,
    params: args[0] === undefined ? {} : args[0],
  });
  if (!request.success) throw new CadderError('invalid-request', 'Invalid RPC request.');
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
      session.send(request.data);
      const parsed = responseSchema(method).safeParse(await session.receive());
      if (
        !parsed.success ||
        parsed.data.requestId !== requestId ||
        parsed.data.requestId === UNCORRELATED_REQUEST_ID
      )
        throw new CadderError('invalid-response', 'Daemon response does not match this request.');
      const response = parsed.data;
      if ('error' in response) throw new RpcError(response.error);
      // The selected schema is exactly catalog[method].result.
      return response.result as RpcResult<M>;
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
