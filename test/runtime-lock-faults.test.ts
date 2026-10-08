import { randomBytes } from 'node:crypto';
import * as fs from 'node:fs/promises';
import { DatabaseSync } from 'node:sqlite';
import { beforeEach, expect, it, vi } from 'vitest';
import { acquireRuntimeLock } from '../src/daemon/runtime-lock.ts';
import { resolvePaths } from '../src/daemon/paths.ts';
import * as security from '../src/platform/runtime-security.ts';

vi.mock('node:crypto', async (original) => ({
  ...(await original<typeof import('node:crypto')>()),
  randomBytes: vi.fn(),
}));
vi.mock('node:fs/promises', () => ({ readFile: vi.fn(), unlink: vi.fn(), writeFile: vi.fn() }));
vi.mock('node:sqlite', () => ({ DatabaseSync: vi.fn() }));
vi.mock('../src/platform/runtime-security.ts', () => ({
  assertProtected: vi.fn(),
  assertRuntimeDescendant: vi.fn(),
  createProtectedFile: vi.fn(),
}));
const paths = resolvePaths({ runtimeDir: '/fault-runtime' });
const owner = { id: 'test-owner', elevated: false };
const database = { exec: vi.fn(), close: vi.fn() };
let secret: Buffer<ArrayBuffer>;
let generated: Buffer<ArrayBuffer>;
const ioFailure = Object.assign(new Error('disk I/O error'), {
  code: 'ERR_SQLITE_ERROR',
  errcode: 10,
});

beforeEach(() => {
  vi.resetAllMocks();
  secret = Buffer.alloc(32, 7);
  generated = Buffer.alloc(32, 9);
  vi.mocked(randomBytes).mockReturnValue(generated as never);
  vi.mocked(DatabaseSync).mockImplementation(function () {
    return database as never;
  });
  vi.mocked(security.createProtectedFile).mockResolvedValue(true);
  vi.mocked(fs.readFile).mockImplementation(async (path) => {
    if (path === paths.metadata) throw Object.assign(new Error('missing'), { code: 'ENOENT' });
    return secret;
  });
});

it.each(['unsafe', 'inspection', 'missing-root', 'missing-intermediate'])(
  'refuses an existing journal before SQLite open: %s',
  async (kind) => {
    const failure = Object.assign(new Error(kind), {
      code:
        (
          {
            'missing-root': 'ENOENT',
            'missing-intermediate': 'ENOENT',
            inspection: 'EACCES',
          } as Record<string, string>
        )[kind] ?? 'unsafe-runtime-permissions',
      path:
        (
          { 'missing-root': paths.directory, 'missing-intermediate': '/missing-parent' } as Record<
            string,
            string
          >
        )[kind] ?? `${paths.lock}-journal`,
    });
    vi.mocked(security.assertRuntimeDescendant).mockRejectedValueOnce(failure);
    await expect(acquireRuntimeLock(paths, owner)).rejects.toBe(failure);
    expect(DatabaseSync).not.toHaveBeenCalled();
    expect(fs.unlink).not.toHaveBeenCalled();
    expect(fs.writeFile).not.toHaveBeenCalled();
  },
);

it('does not treat unlocated ENOENT as journal leaf absence', async () => {
  const failure = Object.assign(new Error('unlocated missing path'), { code: 'ENOENT' });
  vi.mocked(security.assertRuntimeDescendant).mockRejectedValueOnce(failure);
  await expect(acquireRuntimeLock(paths, owner)).rejects.toBe(failure);
  expect(DatabaseSync).not.toHaveBeenCalled();
});

it('allows only known journal leaf absence and checks again while exclusion is held', async () => {
  const journal = `${paths.lock}-journal`;
  vi.mocked(security.assertRuntimeDescendant).mockRejectedValue(
    Object.assign(new Error('absent leaf'), { code: 'ENOENT', path: journal }),
  );
  const lock = await acquireRuntimeLock(paths, owner);
  expect(security.assertRuntimeDescendant).toHaveBeenCalledTimes(2);
  expect(security.assertRuntimeDescendant).toHaveBeenCalledWith(paths.directory, journal, owner);
  expect(vi.mocked(security.assertRuntimeDescendant).mock.invocationCallOrder[0]).toBeLessThan(
    vi.mocked(DatabaseSync).mock.invocationCallOrder[0]!,
  );
  expect(vi.mocked(security.assertRuntimeDescendant).mock.invocationCallOrder[1]).toBeGreaterThan(
    database.exec.mock.invocationCallOrder[1]!,
  );
  expect(vi.mocked(security.assertRuntimeDescendant).mock.invocationCallOrder[1]).toBeLessThan(
    vi.mocked(fs.writeFile).mock.invocationCallOrder[0]!,
  );
  await lock.release();
});

it('refuses an unsafe generated journal and finalizes exclusion before publication', async () => {
  const failure = new Error('unsafe generated journal');
  vi.mocked(security.assertRuntimeDescendant)
    .mockResolvedValueOnce()
    .mockRejectedValueOnce(failure);
  await expect(acquireRuntimeLock(paths, owner)).rejects.toBe(failure);
  expect(database.exec).toHaveBeenCalledWith('ROLLBACK;');
  expect(database.close).toHaveBeenCalledOnce();
  expect(security.createProtectedFile).toHaveBeenCalledTimes(1);
  expect(fs.unlink).not.toHaveBeenCalled();
  expect(fs.writeFile).not.toHaveBeenCalled();
});

it('propagates SQLite open failure without pretending contention or deleting foreign artifacts', async () => {
  vi.mocked(DatabaseSync).mockImplementationOnce(function () {
    throw ioFailure;
  });
  await expect(acquireRuntimeLock(paths, owner)).rejects.toBe(ioFailure);
  expect(fs.unlink).not.toHaveBeenCalled();
});

it.each(['PRAGMA', 'BEGIN'])(
  'preserves storage errors during %s and closes the connection',
  async (phase) => {
    database.exec.mockImplementation((sql: string) => {
      if (sql.includes(phase)) throw ioFailure;
    });
    await expect(acquireRuntimeLock(paths, owner)).rejects.toBe(ioFailure);
    expect(database.close).toHaveBeenCalledOnce();
    expect(fs.unlink).not.toHaveBeenCalled();
  },
);

it.each([10, 13, 778])(
  'does not misclassify SQLite I/O or full-disk errors: %s',
  async (errcode) => {
    const failure = Object.assign(new Error('storage failure'), {
      code: 'ERR_SQLITE_ERROR',
      errcode,
    });
    database.exec.mockImplementationOnce(() => {
      throw failure;
    });
    await expect(acquireRuntimeLock(paths, owner)).rejects.toBe(failure);
    expect(database.close).toHaveBeenCalledOnce();
  },
);

it.each(['directory', 'lock-create', 'metadata-read', 'secret-create'])(
  'does not publish or delete unvalidated paths after %s fails',
  async (phase) => {
    const failure = new Error(phase);
    if (phase === 'directory') vi.mocked(security.assertProtected).mockRejectedValueOnce(failure);
    if (phase === 'lock-create')
      vi.mocked(security.createProtectedFile).mockRejectedValueOnce(failure);
    if (phase === 'metadata-read') vi.mocked(fs.readFile).mockRejectedValueOnce(failure);
    if (phase === 'secret-create')
      vi.mocked(security.createProtectedFile).mockImplementation(async (path) => {
        if (path === paths.secret) throw failure;
        return true;
      });
    await expect(acquireRuntimeLock(paths, owner)).rejects.toBe(failure);
    expect(fs.unlink).not.toHaveBeenCalled();
    expect(fs.writeFile).not.toHaveBeenCalled();
    if (phase === 'directory' || phase === 'lock-create')
      expect(DatabaseSync).not.toHaveBeenCalled();
    else expect(database.close).toHaveBeenCalledOnce();
    if (phase === 'secret-create') expect(generated.every((byte) => byte === 0)).toBe(true);
  },
);

it.each([5, 6, 261])(
  'classifies only SQLite busy/locked codes as contention: %s',
  async (errcode) => {
    database.exec.mockImplementationOnce(() => {
      throw Object.assign(new Error('busy'), { code: 'ERR_SQLITE_ERROR', errcode });
    });
    await expect(acquireRuntimeLock(paths, owner)).rejects.toMatchObject({
      code: 'runtime-already-running',
    });
    expect(database.close).toHaveBeenCalledOnce();
  },
);

it.each(['metadata-create', 'metadata-write', 'secret-read', 'invalid-secret'])(
  'finalizes owned resources after %s',
  async (phase) => {
    const failure = new Error(phase);
    if (phase === 'metadata-create')
      vi.mocked(security.createProtectedFile).mockImplementation(async (path) => {
        if (path === paths.metadata) throw failure;
        return true;
      });
    if (phase === 'metadata-write') vi.mocked(fs.writeFile).mockRejectedValueOnce(failure);
    if (phase === 'secret-read')
      vi.mocked(fs.readFile)
        .mockRejectedValueOnce(Object.assign(new Error('missing'), { code: 'ENOENT' }))
        .mockRejectedValueOnce(failure);
    if (phase === 'invalid-secret') secret = Buffer.alloc(5, 7);
    await expect(acquireRuntimeLock(paths, owner)).rejects.toThrow();
    expect(database.close).toHaveBeenCalledOnce();
    expect(database.exec).toHaveBeenCalledWith('ROLLBACK;');
    expect(generated.every((byte) => byte === 0)).toBe(true);
    if (phase !== 'secret-read') expect(secret.every((byte) => byte === 0)).toBe(true);
    expect(fs.unlink).toHaveBeenCalledWith(paths.secret);
    if (phase === 'metadata-write') expect(fs.unlink).toHaveBeenCalledWith(paths.metadata);
    else expect(fs.unlink).not.toHaveBeenCalledWith(paths.metadata);
  },
);

it('preserves pre-existing secret and unvalidated metadata on acquisition failure', async () => {
  vi.mocked(security.createProtectedFile).mockImplementation(async (path) => {
    if (path === paths.metadata) throw new Error('unsafe metadata');
    return false;
  });
  await expect(acquireRuntimeLock(paths, owner)).rejects.toThrow('unsafe metadata');
  expect(fs.unlink).not.toHaveBeenCalled();
  expect(secret.every((byte) => byte === 0)).toBe(true);
  expect(generated.every((byte) => byte === 0)).toBe(true);
});

it.each(['unlink', 'rollback', 'close'])(
  'attempts every finalizer and shares release failure: %s',
  async (phase) => {
    const lock = await acquireRuntimeLock(paths, owner);
    const failure = new Error(phase);
    if (phase === 'unlink') vi.mocked(fs.unlink).mockRejectedValueOnce(failure);
    if (phase === 'rollback')
      database.exec.mockImplementation((sql: string) => {
        if (sql === 'ROLLBACK;') throw failure;
      });
    if (phase === 'close')
      database.close.mockImplementationOnce(() => {
        throw failure;
      });
    const first = lock.release();
    expect(lock.release()).toBe(first);
    await expect(first).rejects.toBe(failure);
    await expect(lock.release()).rejects.toBe(failure);
    expect(database.exec).toHaveBeenCalledWith('ROLLBACK;');
    expect(database.close).toHaveBeenCalledOnce();
    expect(secret.every((byte) => byte === 0)).toBe(true);
  },
);

it('cleans validated stale diagnostic metadata but preserves an existing secret after write failure', async () => {
  vi.mocked(fs.readFile).mockResolvedValueOnce(Buffer.from('stale')).mockResolvedValueOnce(secret);
  vi.mocked(security.createProtectedFile).mockResolvedValue(false);
  vi.mocked(fs.writeFile).mockRejectedValueOnce(ioFailure);
  await expect(acquireRuntimeLock(paths, owner)).rejects.toBe(ioFailure);
  expect(fs.unlink).toHaveBeenCalledWith(paths.metadata);
  expect(fs.unlink).not.toHaveBeenCalledWith(paths.secret);
  expect(secret.every((byte) => byte === 0)).toBe(true);
});

it('retains all release failures and zeroes the secret even when no finalizer succeeds', async () => {
  const lock = await acquireRuntimeLock(paths, owner);
  vi.mocked(fs.unlink).mockRejectedValueOnce(new Error('unlink'));
  database.exec.mockImplementation((sql: string) => {
    if (sql === 'ROLLBACK;') throw new Error('rollback');
  });
  database.close.mockImplementationOnce(() => {
    throw new Error('close');
  });
  const error = await lock.release().catch((error: unknown) => error);
  expect((error as AggregateError).errors).toHaveLength(3);
  await expect(lock.release()).rejects.toBe(error);
  expect(database.close).toHaveBeenCalledOnce();
  expect(secret.every((byte) => byte === 0)).toBe(true);
});

it('retains acquisition failure alongside rollback/close/unlink failures', async () => {
  vi.mocked(fs.writeFile).mockRejectedValueOnce(ioFailure);
  vi.mocked(fs.unlink).mockRejectedValue(new Error('unlink'));
  database.exec.mockImplementation((sql: string) => {
    if (sql === 'ROLLBACK;') throw new Error('rollback');
  });
  database.close.mockImplementationOnce(() => {
    throw new Error('close');
  });
  const error = await acquireRuntimeLock(paths, owner).catch((error: unknown) => error);
  expect(error).toMatchObject({ cause: ioFailure });
  expect((error as AggregateError).errors).toHaveLength(5);
  expect(database.close).toHaveBeenCalledOnce();
  expect(secret.every((byte) => byte === 0)).toBe(true);
});
