import { EventEmitter } from 'node:events';
import * as fs from 'node:fs/promises';
import { createServer, type Server, type Socket } from 'node:net';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { serveIpc } from '../src/daemon/ipc-server.ts';
import { resolvePaths } from '../src/daemon/paths.ts';
import * as security from '../src/platform/runtime-security.ts';
import { JsonChannel } from '../src/protocol/channel.ts';
import { authenticateServer } from '../src/protocol/authentication.ts';

vi.mock('node:fs/promises', () => ({
  chmod: vi.fn(),
  lstat: vi.fn(),
  unlink: vi.fn(),
  writeFile: vi.fn(),
}));
vi.mock('node:net', () => ({ createServer: vi.fn() }));
vi.mock('../src/protocol/channel.ts', () => ({ JsonChannel: vi.fn() }));
vi.mock('../src/protocol/authentication.ts', () => ({ authenticateServer: vi.fn() }));
vi.mock('../src/platform/runtime-security.ts', () => ({
  createProtectedFile: vi.fn(),
  protectCreated: vi.fn(),
}));
const paths = resolvePaths({ runtimeDir: '/publication-faults' });
const owner = { id: 'uid:42', uid: 42, elevated: false };
const failure = Object.assign(new Error('publication disk failure'), { code: 'ENOSPC' });
let server: EventEmitter & { listen: ReturnType<typeof vi.fn>; close: ReturnType<typeof vi.fn> };

beforeEach(() => {
  vi.resetAllMocks();
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  server = Object.assign(new EventEmitter(), {
    listen: vi.fn(() => {
      queueMicrotask(() => server.emit('listening'));
    }),
    close: vi.fn((callback: (error?: Error) => void) => callback()),
  });
  vi.mocked(createServer).mockReturnValue(server as unknown as Server);
  vi.mocked(fs.lstat).mockRejectedValue(Object.assign(new Error('missing'), { code: 'ENOENT' }));
  vi.mocked(security.createProtectedFile).mockResolvedValue(true);
  vi.mocked(JsonChannel).mockImplementation(function () {
    return { close: vi.fn() } as never;
  });
});
afterEach(() => vi.restoreAllMocks());

it.each(['sync-bind', 'async-bind', 'chmod', 'protection', 'discovery-create', 'discovery-write'])(
  'closes startup resources after %s',
  async (phase) => {
    if (phase === 'sync-bind')
      server.listen.mockImplementationOnce(() => {
        throw failure;
      });
    if (phase === 'async-bind')
      server.listen.mockImplementationOnce(() => {
        queueMicrotask(() => server.emit('error', failure));
      });
    if (phase === 'chmod') vi.mocked(fs.chmod).mockRejectedValueOnce(failure);
    if (phase === 'protection') vi.mocked(security.protectCreated).mockRejectedValueOnce(failure);
    if (phase === 'discovery-create')
      vi.mocked(security.createProtectedFile).mockRejectedValueOnce(failure);
    if (phase === 'discovery-write') vi.mocked(fs.writeFile).mockRejectedValueOnce(failure);
    const handler = vi.fn();
    await expect(serveIpc(paths, owner, Buffer.alloc(32), handler)).rejects.toBe(failure);
    expect(server.close).toHaveBeenCalledOnce();
    if (phase === 'discovery-write') expect(fs.unlink).toHaveBeenCalledWith(paths.discovery);
    else expect(fs.unlink).not.toHaveBeenCalledWith(paths.discovery);
    expect(handler).not.toHaveBeenCalled();
  },
);

it('preserves unvalidated discovery and foreign endpoint paths', async () => {
  vi.mocked(fs.lstat).mockResolvedValueOnce({ isSocket: () => false, uid: 42 } as never);
  await expect(serveIpc(paths, owner, Buffer.alloc(32), vi.fn())).rejects.toMatchObject({
    code: 'unsafe-endpoint',
  });
  expect(createServer).not.toHaveBeenCalled();
  expect(fs.unlink).not.toHaveBeenCalled();
  vi.mocked(security.createProtectedFile).mockRejectedValueOnce(failure);
  await expect(serveIpc(paths, owner, Buffer.alloc(32), vi.fn())).rejects.toBe(failure);
  expect(fs.unlink).not.toHaveBeenCalledWith(paths.discovery);
});

it.each(['callback', 'throw'])(
  'unlinks validated discovery even if listener close fails: %s',
  async (phase) => {
    const listener = await serveIpc(paths, owner, Buffer.alloc(32), vi.fn());
    if (phase === 'callback')
      server.close.mockImplementationOnce((callback: (error: Error) => void) => callback(failure));
    else
      server.close.mockImplementationOnce(() => {
        throw failure;
      });
    const closing = listener.close();
    expect(listener.close()).toBe(closing);
    await expect(closing).rejects.toBe(failure);
    await expect(listener.close()).rejects.toBe(failure);
    expect(fs.unlink).toHaveBeenCalledWith(paths.discovery);
  },
);

it('retains publication failure and every cleanup failure', async () => {
  vi.mocked(fs.writeFile).mockRejectedValueOnce(failure);
  server.close.mockImplementationOnce((callback: (error: Error) => void) =>
    callback(new Error('close')),
  );
  vi.mocked(fs.unlink).mockRejectedValueOnce(new Error('unlink'));
  const error = await serveIpc(paths, owner, Buffer.alloc(32), vi.fn()).catch(
    (error: unknown) => error,
  );
  expect((error as AggregateError).errors).toHaveLength(2);
  expect(error).toMatchObject({ cause: failure });
  expect(fs.unlink).toHaveBeenCalledWith(paths.discovery);
});

it('revokes queued requests and rejects new admission after startup failure', async () => {
  let deliver: (request: unknown) => void = () => {};
  const session = {
    receive: vi.fn(
      () =>
        new Promise((resolve) => {
          deliver = resolve;
        }),
    ),
    close: vi.fn(),
  };
  vi.mocked(authenticateServer).mockResolvedValue(session as never);
  const socket = Object.assign(new EventEmitter(), { destroy: vi.fn() });
  vi.mocked(fs.writeFile).mockImplementationOnce(async () => {
    const accept = vi.mocked(createServer).mock.calls[0]![0] as (socket: Socket) => void;
    accept(socket as unknown as Socket);
    await Promise.resolve();
    throw failure;
  });
  const handler = vi.fn();
  await expect(serveIpc(paths, owner, Buffer.alloc(32), handler)).rejects.toBe(failure);
  expect(session.receive).toHaveBeenCalledOnce();
  deliver({
    protocolVersion: 3,
    requestId: '00000000-0000-4000-8000-000000000001',
    method: 'query-state-request',
    params: {},
  });
  await Promise.resolve();
  expect(handler).not.toHaveBeenCalled();
  expect(session.close).toHaveBeenCalledOnce();
  expect(socket.destroy).toHaveBeenCalledOnce();
  const lateSocket = Object.assign(new EventEmitter(), { destroy: vi.fn() });
  const accept = vi.mocked(createServer).mock.calls[0]![0] as (socket: Socket) => void;
  accept(lateSocket as unknown as Socket);
  expect(lateSocket.destroy).toHaveBeenCalledOnce();
  expect(authenticateServer).toHaveBeenCalledOnce();
});

it('attempts every socket, listener and publication finalizer when a socket destroy throws', async () => {
  vi.mocked(authenticateServer).mockReturnValue(new Promise(() => {}));
  const listener = await serveIpc(paths, owner, Buffer.alloc(32), vi.fn());
  const accept = vi.mocked(createServer).mock.calls[0]![0] as (socket: Socket) => void;
  const first = Object.assign(new EventEmitter(), {
    destroy: vi.fn(() => {
      throw failure;
    }),
  });
  const second = Object.assign(new EventEmitter(), { destroy: vi.fn() });
  accept(first as unknown as Socket);
  accept(second as unknown as Socket);
  await expect(listener.close()).rejects.toBe(failure);
  expect(second.destroy).toHaveBeenCalledOnce();
  expect(server.close).toHaveBeenCalledOnce();
  expect(fs.unlink).toHaveBeenCalledWith(paths.discovery);
});

it('cleans validated safe discovery residue after a partial replacement fails', async () => {
  vi.mocked(security.createProtectedFile).mockResolvedValueOnce(false);
  vi.mocked(fs.writeFile).mockRejectedValueOnce(failure);
  await expect(serveIpc(paths, owner, Buffer.alloc(32), vi.fn())).rejects.toBe(failure);
  expect(fs.unlink).toHaveBeenCalledWith(paths.discovery);
});

it('reports discovery unlink failure consistently without repeating finalizers', async () => {
  const listener = await serveIpc(paths, owner, Buffer.alloc(32), vi.fn());
  vi.mocked(fs.unlink).mockRejectedValueOnce(failure);
  await expect(listener.close()).rejects.toBe(failure);
  await expect(listener.close()).rejects.toBe(failure);
  expect(server.close).toHaveBeenCalledOnce();
  expect(fs.unlink).toHaveBeenCalledOnce();
});
