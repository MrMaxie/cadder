import * as fs from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createConnection, createServer } from 'node:net';
import { once } from 'node:events';
import { DatabaseSync } from 'node:sqlite';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { startLocalRuntime } from '../src/daemon/local-runtime.ts';
import { resolvePaths } from '../src/daemon/paths.ts';
import { acquireRuntimeLock } from '../src/daemon/runtime-lock.ts';
import { prepareRuntime, runtimeOwner } from '../src/platform/runtime-security.ts';
import { basicResult } from './fixtures/rpc-data.ts';

vi.mock('node:fs/promises', async (original) => {
  const actual = await original<typeof import('node:fs/promises')>();
  return {
    ...actual,
    readFile: vi.fn(actual.readFile),
    writeFile: vi.fn(actual.writeFile),
    unlink: vi.fn(actual.unlink),
  };
});
vi.mock('node:net', async (original) => {
  const actual = await original<typeof import('node:net')>();
  return { ...actual, createServer: vi.fn(actual.createServer) };
});
vi.mock('node:sqlite', async (original) => {
  const actual = await original<typeof import('node:sqlite')>();
  return {
    ...actual,
    DatabaseSync: vi.fn(function (...args: ConstructorParameters<typeof actual.DatabaseSync>) {
      return new actual.DatabaseSync(...args);
    }),
  };
});
const realFs = await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises');
const realNet = await vi.importActual<typeof import('node:net')>('node:net');
const realSqlite = await vi.importActual<typeof import('node:sqlite')>('node:sqlite');
let root: string;
let options: { runtimeDir: string; runtimeOwner?: number };
let paths: ReturnType<typeof resolvePaths>;
const failure = Object.assign(new Error('simulated storage exhaustion'), { code: 'ENOSPC' });

beforeEach(async () => {
  vi.mocked(fs.readFile).mockImplementation(realFs.readFile);
  vi.mocked(fs.writeFile).mockImplementation(realFs.writeFile);
  vi.mocked(fs.unlink).mockImplementation(realFs.unlink);
  vi.mocked(createServer).mockImplementation(realNet.createServer);
  vi.mocked(DatabaseSync).mockImplementation(function (...args) {
    return new realSqlite.DatabaseSync(...args);
  });
  root = await fs.mkdtemp(join(tmpdir(), 'cadder-phase-fault-'));
  options = { runtimeDir: root, ...(process.getuid ? { runtimeOwner: process.getuid() } : {}) };
  paths = resolvePaths(options);
});
afterEach(async () => {
  vi.restoreAllMocks();
  await realFs.rm(root, { recursive: true, force: true });
});

async function assertStopped(): Promise<void> {
  const socket = createConnection(paths.endpoint);
  const result = once(socket, 'connect').then(
    () => true,
    () => false,
  );
  expect(await result).toBe(false);
  socket.destroy();
}

it.each(['metadata', 'discovery'] as const)(
  'cleans real partial %s after ENOSPC and permits restart',
  async (artifact) => {
    let liveSecret: Buffer | undefined;
    vi.mocked(fs.readFile).mockImplementation(async (path, options) => {
      const result = await realFs.readFile(path, options);
      if (path === paths.secret && Buffer.isBuffer(result)) liveSecret = result;
      return result;
    });
    vi.mocked(fs.writeFile).mockImplementation(async (path, data, options) => {
      if (path !== paths[artifact]) return realFs.writeFile(path, data, options);
      vi.mocked(fs.writeFile).mockImplementation(realFs.writeFile);
      await realFs.writeFile(path, String(data).slice(0, 8), options);
      throw failure;
    });
    const handler = vi.fn(async () => basicResult);
    await expect(startLocalRuntime(options, handler)).rejects.toBe(failure);
    await assertStopped();
    expect(liveSecret?.every((byte) => byte === 0)).toBe(true);
    await expect(fs.lstat(paths.metadata)).rejects.toMatchObject({ code: 'ENOENT' });
    await expect(fs.lstat(paths.discovery)).rejects.toMatchObject({ code: 'ENOENT' });
    expect(handler).not.toHaveBeenCalled();
    const runtime = await startLocalRuntime(options, handler);
    await runtime.stop();
  },
);

it.each(['discovery-unlink', 'metadata-unlink', 'rollback', 'database-close', 'listener-close'])(
  'finalizes native resources and permits reacquisition despite %s error',
  async (phase) => {
    let secret: Buffer | undefined;
    vi.mocked(fs.readFile).mockImplementation(async (path, options) => {
      const result = await realFs.readFile(path, options);
      if (path === paths.secret && Buffer.isBuffer(result)) secret = result;
      return result;
    });
    if (phase === 'rollback' || phase === 'database-close') {
      vi.mocked(DatabaseSync).mockImplementationOnce(function (...args) {
        const database = new realSqlite.DatabaseSync(...args);
        const exec = database.exec.bind(database);
        const close = database.close.bind(database);
        vi.spyOn(database, 'exec').mockImplementation((sql) => {
          if (phase === 'rollback' && sql === 'ROLLBACK;') throw failure;
          return exec(sql);
        });
        vi.spyOn(database, 'close').mockImplementation(() => {
          close();
          if (phase === 'database-close') throw failure;
        });
        return database;
      });
    }
    if (phase === 'listener-close') {
      vi.mocked(createServer).mockImplementationOnce((...args) => {
        const server = realNet.createServer(...args);
        const close = server.close.bind(server);
        vi.spyOn(server, 'close').mockImplementation((callback) =>
          close(() => callback?.(failure)),
        );
        return server;
      });
    }
    const runtime = await startLocalRuntime(options, async () => basicResult);
    if (phase.endsWith('unlink')) {
      const target = phase === 'discovery-unlink' ? paths.discovery : paths.metadata;
      vi.mocked(fs.unlink).mockImplementation(async (path) => {
        if (path === target) {
          vi.mocked(fs.unlink).mockImplementation(realFs.unlink);
          throw failure;
        }
        return realFs.unlink(path);
      });
    }
    const stopping = runtime.stop();
    expect(runtime.stop()).toBe(stopping);
    await expect(stopping).rejects.toBe(failure);
    await expect(runtime.stop()).rejects.toBe(failure);
    await assertStopped();
    expect(secret?.every((byte) => byte === 0)).toBe(true);
    const owner = await runtimeOwner(options.runtimeOwner);
    const lock = await acquireRuntimeLock(paths, owner);
    secret = lock.secret;
    await lock.release();
    expect(secret.every((byte) => byte === 0)).toBe(true);
    const restarted = await startLocalRuntime(options, async () => basicResult);
    await restarted.stop();
  },
);

it(
  'refuses an occupied listener without closing the unrelated server and releases its exclusion',
  { skip: process.platform !== 'win32' },
  async () => {
    const owner = await runtimeOwner(options.runtimeOwner);
    await prepareRuntime(paths.directory, owner);
    const foreign = realNet.createServer();
    foreign.listen(paths.endpoint);
    await once(foreign, 'listening');
    try {
      await expect(startLocalRuntime(options, async () => basicResult)).rejects.toMatchObject({
        code: 'EADDRINUSE',
      });
      expect(foreign.listening).toBe(true);
      await expect(fs.lstat(paths.discovery)).rejects.toMatchObject({ code: 'ENOENT' });
      const lock = await acquireRuntimeLock(paths, owner);
      await lock.release();
    } finally {
      await new Promise<void>((resolve) => foreign.close(() => resolve()));
    }
  },
);
