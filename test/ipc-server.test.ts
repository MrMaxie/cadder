import { randomBytes, randomUUID } from 'node:crypto';
import { createConnection, createServer } from 'node:net';
import { once } from 'node:events';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { rpc } from '../src/client/connection.ts';
import { resolvePaths } from '../src/daemon/paths.ts';
import { acquireRuntimeLock } from '../src/daemon/runtime-lock.ts';
import { serveIpc } from '../src/daemon/ipc-server.ts';
import { prepareRuntime, runtimeOwner } from '../src/platform/runtime-security.ts';
import { RpcError, toWireError } from '../src/protocol/errors.ts';
import { JsonChannel } from '../src/protocol/channel.ts';
import { authenticateClient, authenticateServer } from '../src/protocol/authentication.ts';
import {
  errorResponseSchema,
  requestSchema,
  responseSchema,
  UNCORRELATED_REQUEST_ID,
  type HandlerResult,
  type RpcRequest,
} from '../src/protocol/rpc.ts';
import { basicResult, fakeStateResult } from './fixtures/rpc-data.ts';
import { MAX_FRAME_BYTES } from '../src/protocol/version.ts';

const id = randomUUID();
const request = { protocolVersion: 3, requestId: id, method: 'query-state-request', params: {} };
const typedError = {
  kind: 'busy' as const,
  code: 'busy',
  message: 'A safe fixture error',
  guidance: null,
  retryable: true,
  requestId: null,
};

it(
  'serves one authenticated correlated outcome, rejects malformed requests before handlers',
  { timeout: 90000 },
  async () => {
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
        if (request.method === 'query-state-request')
          return { ...fakeStateResult(), requestId: randomUUID() };
        if (request.method === 'shutdown-daemon-request') throw new RpcError(typedError);
        if (request.method === 'heartbeat-entrypoint-request')
          return { accepted: false, message: 'Wrong nonce.' };
        if (request.method === 'unregister-entrypoint-request')
          return { arbitrary: true } as unknown as HandlerResult;
        throw new Error('Do not expose internal error text');
      },
      () => {
        denied++;
      },
    );
    async function raw(input: unknown) {
      const channel = new JsonChannel(createConnection(paths.endpoint));
      const session = await authenticateClient(channel, lock.secret, paths.instance);
      try {
        session.send(input);
        session.send(request); // Pipelined input must not produce a second response or handler call.
        const response = await session.receive();
        await expect(session.receive()).rejects.toThrow('closed');
        return response;
      } finally {
        session.close();
        channel.close();
      }
    }
    try {
      const state = await rpc(paths, 'query-state-request');
      expect(state).toMatchObject(fakeStateResult());
      expect(state.requestId).toMatch(/^[a-f0-9-]{36}$/);
      await expect(rpc(paths, 'shutdown-daemon-request')).rejects.toMatchObject({
        code: 'busy',
        message: 'A safe fixture error',
        protocolError: { retryable: true },
      });
      expect(
        await rpc(paths, 'heartbeat-entrypoint-request', {
          registrationId: 'missing',
          shimSessionNonce: 'wrong',
        }),
      ).toMatchObject({ accepted: false });
      for (const method of [
        'unregister-entrypoint-request',
        'set-domain-enabled-request',
      ] as const) {
        const params =
          method === 'unregister-entrypoint-request'
            ? { registrationId: 'id', shimSessionNonce: 'nonce' }
            : { registrationId: 'id', domainKey: 'domain', enabled: true };
        await expect(rpc(paths, method, params)).rejects.toMatchObject({
          code: 'internal',
          message: 'Daemon request failed. See diagnostics.',
        });
      }
      const before = called;
      for (const input of [
        { ...request, method: 'status' },
        { ...request, method: 'shutdown' },
        { ...request, params: { extra: true } },
        { ...request, extra: true },
        { ...request, protocolVersion: 2 },
        { ...request, method: 'heartbeat-entrypoint-request', params: { registrationId: 'id' } },
        {
          ...request,
          method: 'query-logs-request',
          params: { stream: { streamId: 's', domainKey: null, channel: 'c', extra: 1 } },
        },
        { ...request, requestId: 'malformed' },
        null,
      ]) {
        const response = errorResponseSchema.parse(await raw(input));
        expect(response.requestId).toBe(
          input && input.requestId === id ? id : UNCORRELATED_REQUEST_ID,
        );
        expect(response.error.kind).toBe(
          input && ['status', 'shutdown'].includes(input.method)
            ? 'unsupportedOperation'
            : 'payloadDecodeFailed',
        );
        expect(called).toBe(before);
      }
      // Runtime callers cannot bypass request validation even with untyped inputs.
      // @ts-expect-error Unknown fields are also rejected statically.
      await expect(rpc(paths, 'query-state-request', { extra: true })).rejects.toMatchObject({
        code: 'invalid-request',
      });
      expect(called).toBe(before);
      const channel = new JsonChannel(createConnection(paths.endpoint));
      channel.send(request);
      await expect(channel.next()).rejects.toThrow('closed');
      channel.close();
      expect(called).toBe(before);
      expect(denied).toBe(1);
      await writeFile(paths.secret, randomBytes(32));
      await expect(rpc(paths, 'query-state-request')).rejects.toThrow('authentication failed');
      expect(called).toBe(before);
    } finally {
      await server.close();
      await lock.release();
      await rm(root, { recursive: true, force: true });
    }
  },
);

it.each(['result', 'error', 'escaped-result', 'escaped-error'] as const)(
  'replaces an oversized valid %s with one bounded correlated frame error',
  { timeout: 45000 },
  async (scenario) => {
    const root = await mkdtemp(join(tmpdir(), 'cadder-oversized-'));
    const owner = await runtimeOwner();
    const paths = resolvePaths({ runtimeDir: root });
    await prepareRuntime(paths.directory, owner);
    const lock = await acquireRuntimeLock(paths, owner);
    const escaped = scenario.startsWith('escaped-');
    const largeMessage = escaped
      ? '"'.repeat(Math.floor(MAX_FRAME_BYTES / 3))
      : 'x'.repeat(MAX_FRAME_BYTES + 1);
    const method =
      scenario === 'escaped-result' ? 'shutdown-daemon-request' : 'query-state-request';
    const oversizedResult =
      scenario === 'escaped-result'
        ? { ...basicResult, message: largeMessage }
        : { ...fakeStateResult(), message: largeMessage };
    const oversizedError = new RpcError({ ...typedError, message: largeMessage });
    const isError = scenario.endsWith('error');
    const originalResponse = {
      protocolVersion: 3,
      requestId: id,
      ...(isError
        ? { error: { ...oversizedError.protocolError, requestId: id } }
        : { result: { ...oversizedResult, requestId: id } }),
    };
    expect(responseSchema(method).safeParse(originalResponse).success).toBe(true);
    const payload = JSON.stringify(originalResponse);
    if (escaped) expect(Buffer.byteLength(payload)).toBeLessThan(MAX_FRAME_BYTES);
    else expect(Buffer.byteLength(payload)).toBeGreaterThan(MAX_FRAME_BYTES);
    expect(
      Buffer.byteLength(JSON.stringify({ sequence: 0, payload, mac: '0'.repeat(64) }) + '\n'),
    ).toBeGreaterThan(MAX_FRAME_BYTES);
    let called = 0;
    let denied = 0;
    let requestId = '';
    const server = await serveIpc(
      paths,
      owner,
      lock.secret,
      async (request) => {
        called++;
        requestId = request.requestId;
        if (isError) throw oversizedError;
        return oversizedResult;
      },
      () => {
        denied++;
      },
    );
    try {
      const failure = await rpc(paths, method).catch((error: unknown) => error);
      expect(failure).toBeInstanceOf(RpcError);
      expect(failure).toMatchObject({
        code: 'frame',
        protocolError: { kind: 'frame', requestId, retryable: false },
      });
      expect(called).toBe(1);
      const channel = new JsonChannel(createConnection(paths.endpoint));
      const session = await authenticateClient(channel, lock.secret, paths.instance);
      try {
        session.send({ ...request, method });
        const response = errorResponseSchema.parse(await session.receive());
        expect(response.requestId).toBe(id);
        expect(response.error).toEqual({
          kind: 'frame',
          code: 'frame',
          message: 'Daemon response exceeds the IPC frame size limit.',
          guidance: null,
          retryable: false,
          requestId: id,
        });
        expect(Buffer.byteLength(JSON.stringify(response))).toBeLessThan(MAX_FRAME_BYTES);
        expect(JSON.stringify(response)).not.toContain(largeMessage);
        expect(JSON.stringify(response)).not.toContain(lock.secret.toString('hex'));
        await expect(session.receive()).rejects.toThrow('closed');
        expect(called).toBe(2);
        expect(denied).toBe(0);
      } finally {
        session.close();
        channel.close();
      }
    } finally {
      await server.close();
      await lock.release();
      await rm(root, { recursive: true, force: true });
    }
  },
);

it(
  'client rejects authenticated malformed peers, non-exclusive outcomes and correlation failures',
  { timeout: 90000 },
  async () => {
    const root = await mkdtemp(join(tmpdir(), 'cadder-peer-'));
    const owner = await runtimeOwner();
    const paths = resolvePaths({ runtimeDir: root });
    await prepareRuntime(paths.directory, owner);
    const lock = await acquireRuntimeLock(paths, owner);
    const mutations: Array<(request: RpcRequest) => unknown> = [
      (request) => ({ protocolVersion: 3, requestId: request.requestId }),
      (request) => ({
        protocolVersion: 3,
        requestId: request.requestId,
        result: { ...fakeStateResult(), requestId: request.requestId },
        error: { ...typedError, requestId: request.requestId },
      }),
      (request) => ({
        protocolVersion: 3,
        requestId: randomUUID(),
        result: { ...fakeStateResult(), requestId: request.requestId },
      }),
      () => {
        const requestId = randomUUID();
        return { protocolVersion: 3, requestId, result: { ...fakeStateResult(), requestId } };
      },
      (request) => ({
        protocolVersion: 3,
        requestId: request.requestId,
        result: { ...fakeStateResult(), requestId: randomUUID() },
      }),
      (request) => ({
        protocolVersion: 3,
        requestId: request.requestId,
        error: { ...typedError, requestId: randomUUID() },
      }),
      (request) => ({
        protocolVersion: 3,
        requestId: request.requestId,
        result: { ...basicResult, requestId: request.requestId },
      }),
      (request) => ({
        protocolVersion: 3,
        requestId: request.requestId,
        result: { ...fakeStateResult(), requestId: request.requestId, recovered: true },
      }),
      (request) => ({
        protocolVersion: 3,
        requestId: request.requestId,
        error: { code: 'busy', message: 'Incomplete error' },
      }),
      (request) => ({
        protocolVersion: 2,
        requestId: request.requestId,
        result: { ...fakeStateResult(), requestId: request.requestId },
      }),
      (request) => ({
        protocolVersion: 3,
        requestId: request.requestId,
        result: fakeStateResult(),
      }),
      (request) => ({
        protocolVersion: 3,
        requestId: request.requestId,
        error: { ...typedError, requestId: request.requestId, extra: true },
      }),
      () => ({
        protocolVersion: 3,
        requestId: UNCORRELATED_REQUEST_ID,
        error: toWireError(undefined, UNCORRELATED_REQUEST_ID),
      }),
      () => null,
    ];
    let index = 0;
    const failures: unknown[] = [];
    const server = createServer((socket) => {
      const channel = new JsonChannel(socket);
      void (async () => {
        const session = await authenticateServer(channel, lock.secret, paths.instance);
        try {
          const incoming: RpcRequest = requestSchema.parse(await session.receive());
          session.send(mutations[index++]!(incoming));
          await new Promise<void>((resolve) => socket.end(resolve));
        } finally {
          session.close();
          channel.close();
        }
      })().catch((error: unknown) => {
        failures.push(error);
        channel.close();
      });
    });
    server.listen(paths.endpoint);
    await once(server, 'listening');
    try {
      for (let attempt = 0; attempt < mutations.length; attempt++) {
        await expect(rpc(paths, 'query-state-request')).rejects.toMatchObject({
          code: 'invalid-response',
        });
      }
      expect(index).toBe(mutations.length);
      expect(failures).toEqual([]);
    } finally {
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await lock.release();
      await rm(root, { recursive: true, force: true });
    }
  },
);
