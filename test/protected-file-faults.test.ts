import { constants } from 'node:fs';
import * as fs from 'node:fs/promises';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createProtectedFile, prepareRuntime } from '../src/platform/runtime-security.ts';
import { powershell } from '../src/platform/powershell.ts';

vi.mock('node:fs/promises', () => ({
  open: vi.fn(),
  lstat: vi.fn(),
  mkdir: vi.fn(),
  chown: vi.fn(),
  unlink: vi.fn(),
}));
vi.mock('../src/platform/powershell.ts', async (original) => ({
  ...(await original<typeof import('../src/platform/powershell.ts')>()),
  powershell: vi.fn(),
}));
const owner = { id: 'uid:42', uid: 42, elevated: false };
const failure = Object.assign(new Error('disk failure'), { code: 'ENOSPC' });
const handle = { writeFile: vi.fn(), sync: vi.fn(), close: vi.fn() };
const uidDescriptor = Object.getOwnPropertyDescriptor(process, 'getuid');

beforeEach(() => {
  vi.resetAllMocks();
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  Object.defineProperty(process, 'getuid', { configurable: true, value: () => 0 });
  vi.mocked(fs.open).mockResolvedValue(handle as never);
  vi.mocked(fs.lstat).mockImplementation(async (path) => {
    const target = ['secret', 'metadata', 'foreign', 'new', '/runtime/v2'].includes(String(path));
    return {
      isSymbolicLink: () => false,
      isFile: () => target && path !== '/runtime/v2',
      isDirectory: () => !target || path === '/runtime/v2',
      uid: target ? 42 : 0,
      mode: target ? 0o600 : 0o777,
    } as never;
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  if (uidDescriptor) Object.defineProperty(process, 'getuid', uidDescriptor);
  else Reflect.deleteProperty(process, 'getuid');
});

it.each(['writeFile', 'sync', 'close'] as const)(
  'removes a new partial file after %s fails',
  async (phase) => {
    handle[phase].mockRejectedValueOnce(failure);
    await expect(createProtectedFile('secret', owner, 'private')).rejects.toBe(failure);
    expect(handle.close).toHaveBeenCalledOnce();
    expect(fs.unlink).toHaveBeenCalledWith('secret');
  },
);

it('removes only its new file when protection or final validation fails', async () => {
  vi.mocked(fs.chown).mockRejectedValueOnce(failure);
  await expect(createProtectedFile('secret', owner)).rejects.toBe(failure);
  expect(fs.unlink).toHaveBeenCalledWith('secret');
  vi.mocked(fs.unlink).mockClear();
  for (const phase of Object.values(handle)) phase.mockClear();
  const inspect = vi.mocked(fs.lstat).getMockImplementation()!;
  vi.mocked(fs.lstat).mockImplementation(async (path, options) => {
    if (path === 'metadata') throw failure;
    return inspect(path, options);
  });
  await expect(createProtectedFile('metadata', owner)).rejects.toBe(failure);
  expect(fs.lstat).toHaveBeenCalledWith('metadata');
  expect(handle.writeFile).toHaveBeenCalledWith('');
  expect(handle.sync).toHaveBeenCalledOnce();
  expect(handle.close).toHaveBeenCalledOnce();
  expect(fs.unlink).toHaveBeenCalledWith('metadata');
});

it('retains the primary failure while attempting close and unlink', async () => {
  handle.writeFile.mockRejectedValueOnce(failure);
  handle.close.mockRejectedValueOnce(new Error('close failed'));
  vi.mocked(fs.unlink).mockRejectedValueOnce(new Error('unlink failed'));
  const error = await createProtectedFile('secret', owner).catch((error: unknown) => error);
  expect(error).toMatchObject({ cause: failure });
  expect((error as AggregateError).errors).toHaveLength(3);
  expect(fs.unlink).toHaveBeenCalledWith('secret');
});

it('does not remove or repair unsafe existing files or failed exclusive opens', async () => {
  vi.mocked(fs.open).mockRejectedValueOnce(Object.assign(new Error('exists'), { code: 'EEXIST' }));
  const inspect = vi.mocked(fs.lstat).getMockImplementation()!;
  vi.mocked(fs.lstat).mockImplementation(async (path, options) => {
    if (path === 'foreign') return { isSymbolicLink: () => true } as never;
    return inspect(path, options);
  });
  await expect(createProtectedFile('foreign', owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  vi.mocked(fs.open).mockRejectedValueOnce(failure);
  await expect(createProtectedFile('new', owner)).rejects.toBe(failure);
  expect(fs.unlink).not.toHaveBeenCalled();
  expect(fs.chown).not.toHaveBeenCalled();
  expect(fs.open).toHaveBeenCalledWith(
    'new',
    constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY,
    0o600,
  );
});

it('removes its new Windows file after ACL protection fails', async () => {
  vi.spyOn(process, 'platform', 'get').mockReturnValue('win32');
  vi.mocked(powershell).mockRejectedValueOnce(failure);
  await expect(
    createProtectedFile('secret', { id: 'S-1-5-21-test', elevated: false }),
  ).rejects.toBe(failure);
  expect(handle.close).toHaveBeenCalledOnce();
  expect(fs.unlink).toHaveBeenCalledWith('secret');
});

it('rejects an unsafe existing directory without repairing or deleting it', async () => {
  await expect(prepareRuntime('/runtime/v2', owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  expect(fs.mkdir).not.toHaveBeenCalled();
  expect(fs.chown).not.toHaveBeenCalled();
  expect(fs.unlink).not.toHaveBeenCalled();
});

it.each(['mkdir-parent', 'mkdir-leaf', 'protection', 'validation'])(
  'fails directory preparation without repairing existing paths: %s',
  async (phase) => {
    const inspect = vi.mocked(fs.lstat).getMockImplementation()!;
    let targetChecks = 0;
    vi.mocked(fs.lstat).mockImplementation(async (path, options) => {
      if (path !== '/runtime/v2') return inspect(path, options);
      targetChecks += 1;
      if (targetChecks === 1) throw Object.assign(new Error('missing'), { code: 'ENOENT' });
      throw failure;
    });
    if (phase === 'mkdir-parent') vi.mocked(fs.mkdir).mockRejectedValueOnce(failure);
    if (phase === 'mkdir-leaf')
      vi.mocked(fs.mkdir).mockResolvedValueOnce(undefined).mockRejectedValueOnce(failure);
    if (phase === 'protection') vi.mocked(fs.chown).mockRejectedValueOnce(failure);
    await expect(prepareRuntime('/runtime/v2', owner)).rejects.toBe(failure);
    expect(fs.mkdir).toHaveBeenCalledWith('/runtime', { mode: 0o700, recursive: true });
    expect(fs.mkdir).toHaveBeenCalledTimes(phase === 'mkdir-parent' ? 1 : 2);
    if (phase !== 'mkdir-parent')
      expect(fs.mkdir).toHaveBeenCalledWith('/runtime/v2', { mode: 0o700 });
    if (phase === 'protection' || phase === 'validation')
      expect(fs.chown).toHaveBeenCalledWith('/runtime/v2', 42, 42);
    else expect(fs.chown).not.toHaveBeenCalled();
    expect(targetChecks).toBe(phase === 'validation' ? 2 : 1);
    expect(fs.unlink).not.toHaveBeenCalled();
  },
);
