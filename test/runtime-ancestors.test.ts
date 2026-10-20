import type { Stats } from 'node:fs';
import * as fs from 'node:fs/promises';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  assertProtected,
  createProtectedFile,
  prepareRuntime,
} from '../src/platform/runtime-security.ts';
import { powershell } from '../src/platform/powershell.ts';

// POSIX paths and Unix/Darwin filesystem data are modeled on the Windows runner;
// these cases are not native Unix acceptance.
vi.mock('node:path', async (original) => {
  const path = await original<typeof import('node:path')>();
  return { ...path, ...path.posix };
});
vi.mock('node:fs/promises', async (original) => ({
  ...(await original<typeof import('node:fs/promises')>()),
  lstat: vi.fn(),
  realpath: vi.fn(),
  mkdir: vi.fn(),
  open: vi.fn(),
  unlink: vi.fn(),
  chown: vi.fn(),
}));
vi.mock('../src/platform/powershell.ts', async (original) => ({
  ...(await original<typeof import('../src/platform/powershell.ts')>()),
  powershell: vi.fn(),
}));
const owner = { id: 'uid:42', uid: 42, elevated: false };
function info(uid: number, mode: number, directory = true, link = false): Stats {
  return {
    uid,
    mode,
    isDirectory: () => directory,
    isFile: () => !directory,
    isSymbolicLink: () => link,
  } as Stats;
}
function filesystem(ancestor: string, uid: number, leaf: string) {
  vi.mocked(fs.lstat).mockImplementation(async (path) => {
    if (path === ancestor) return info(uid, 0o777, false, true);
    if (path === leaf) return info(42, 0o600, false);
    // No new owner/mode policy for ordinary real ancestors, even broad modes.
    return info(500, 0o777);
  });
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.spyOn(process, 'platform', 'get').mockReturnValue('darwin');
});
afterEach(() => vi.restoreAllMocks());

it.each(['prepareRuntime', 'assertProtected'] as const)(
  'rejects modeled POSIX trailing-separator leaf links during %s',
  async (operation) => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
    vi.mocked(fs.lstat).mockImplementation(async (path) => {
      if (path === '/runtime/link') return info(42, 0o777, false, true);
      // POSIX lstat with a final slash follows the link to a directory.
      if (path === '/runtime/link/' || path === '/runtime/link//') return info(42, 0o700);
      return info(0, 0o755);
    });
    for (const path of ['/runtime/link/', '/runtime/link//'])
      await expect(
        operation === 'prepareRuntime'
          ? prepareRuntime(path, owner)
          : assertProtected(path, owner, true),
      ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
    expect(fs.lstat).toHaveBeenCalledWith('/runtime/link');
    expect(fs.mkdir).not.toHaveBeenCalled();
    expect(fs.open).not.toHaveBeenCalled();
    expect(fs.chown).not.toHaveBeenCalled();
    expect(fs.unlink).not.toHaveBeenCalled();
  },
);

it.each(['/runtime/real/', '/runtime/real//', '/', '///'])(
  'retains modeled POSIX real directory/root behavior for %s',
  async (path) => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
    vi.mocked(fs.lstat).mockResolvedValue(info(42, 0o700));
    await prepareRuntime(path, owner);
    await assertProtected(path, owner, true);
    const leaf = path.replace(/\/+$/, '') || '/';
    expect(fs.lstat).toHaveBeenLastCalledWith(leaf);
    expect(fs.mkdir).not.toHaveBeenCalled();
    expect(fs.realpath).not.toHaveBeenCalled();
  },
);

it('trims modeled POSIX separators without erasing unverified dot-dot components', async () => {
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  filesystem('/home/link', 42, '/home/link/../real/');
  await expect(assertProtected('/home/link/../real/', owner, true)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  expect(fs.lstat).toHaveBeenCalledWith('/home/link');
  expect(fs.lstat).not.toHaveBeenCalledWith('/home/real');
});

it.each(['/tmp', '/var', '/etc'])(
  'accepts only the modeled standard Darwin alias %s',
  async (ancestor) => {
    const leaf = `${ancestor}/runtime/secret`;
    filesystem(ancestor, 0, leaf);
    vi.mocked(fs.realpath).mockResolvedValue(`/private${ancestor}`);
    await assertProtected(leaf, owner);
    expect(fs.realpath).toHaveBeenCalledWith(ancestor);
    expect(fs.lstat).toHaveBeenCalledWith(`/private${ancestor}`);
    expect(fs.mkdir).not.toHaveBeenCalled();
    expect(fs.open).not.toHaveBeenCalled();
    expect(powershell).not.toHaveBeenCalled();
  },
);

it.each([
  ['user-owned tmp', 'darwin', '/tmp', 42, '/private/tmp'],
  ['user-owned var', 'darwin', '/var', 42, '/private/var'],
  ['user-owned etc', 'darwin', '/etc', 42, '/private/etc'],
  ['wrong tmp target', 'darwin', '/tmp', 0, '/elsewhere/tmp'],
  ['wrong var target', 'darwin', '/var', 0, '/private/var-extra'],
  ['wrong etc target', 'darwin', '/etc', 0, '/private/tmp'],
  ['nested tmp', 'darwin', '/home/tmp', 0, '/private/tmp'],
  ['nested var', 'darwin', '/home/var', 0, '/private/var'],
  ['nested etc', 'darwin', '/home/etc', 0, '/private/etc'],
  ['different case', 'darwin', '/TMP', 0, '/private/tmp'],
  ['other root-owned link', 'darwin', '/opt', 0, '/private/opt'],
  ['Linux tmp', 'linux', '/tmp', 0, '/private/tmp'],
  ['Linux var', 'linux', '/var', 0, '/private/var'],
  ['Linux etc', 'linux', '/etc', 0, '/private/etc'],
  ['Windows tmp model', 'win32', '/tmp', 0, '/private/tmp'],
] as const)(
  'rejects modeled %s before trust or mutation',
  async (_description, platform, ancestor, uid, target) => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue(platform);
    const leaf = `${ancestor}/missing/secret`;
    filesystem(ancestor, uid, leaf);
    vi.mocked(fs.realpath).mockResolvedValue(target);
    for (const operation of [
      () => assertProtected(leaf, owner),
      () => prepareRuntime(leaf, owner),
      () => createProtectedFile(leaf, owner, 'must-not-be-created'),
    ])
      await expect(operation()).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
    expect(fs.lstat).not.toHaveBeenCalledWith(leaf);
    expect(fs.mkdir).not.toHaveBeenCalled();
    expect(fs.open).not.toHaveBeenCalled();
    expect(fs.unlink).not.toHaveBeenCalled();
    expect(fs.chown).not.toHaveBeenCalled();
    expect(powershell).not.toHaveBeenCalled();
  },
);

it.each(['missing-target', 'inaccessible-target', 'linked-target', 'file-target'])(
  'rejects an unverifiable modeled OS alias: %s',
  async (phase) => {
    filesystem('/tmp', 0, '/tmp/runtime/secret');
    vi.mocked(fs.realpath).mockResolvedValue('/private/tmp');
    if (phase === 'missing-target')
      vi.mocked(fs.realpath).mockRejectedValue(
        Object.assign(new Error('missing'), { code: 'ENOENT' }),
      );
    else {
      const inspect = vi.mocked(fs.lstat).getMockImplementation()!;
      vi.mocked(fs.lstat).mockImplementation(async (path) => {
        if (path !== '/private/tmp') return inspect(path);
        if (phase === 'inaccessible-target')
          throw Object.assign(new Error('denied'), { code: 'EACCES' });
        return info(0, 0o777, false, phase === 'linked-target');
      });
    }
    await expect(assertProtected('/tmp/runtime/secret', owner)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
  },
);

it('does not introduce owner/mode restrictions on ordinary modeled real ancestors', async () => {
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  filesystem('/unused-link', 42, '/ordinary/runtime/secret');
  await assertProtected('/ordinary/runtime/secret', owner);
  expect(fs.realpath).not.toHaveBeenCalled();
});

it('checks the linked prefix before dot-dot or deeper missing paths in modeled Unix', async () => {
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  filesystem('/home/link', 42, '/home/link/../secret');
  await expect(assertProtected('/home/link/../secret', owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  expect(fs.lstat).not.toHaveBeenCalledWith('/home/link/..');
  expect(fs.lstat).not.toHaveBeenCalledWith('/home/link/../secret');
});

it('checks modeled missing/dot-dot paths instead of stopping before an existing link', async () => {
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  const leaf = '/home/missing/../link/secret';
  filesystem('/home/link', 42, leaf);
  const inspect = vi.mocked(fs.lstat).getMockImplementation()!;
  vi.mocked(fs.lstat).mockImplementation(async (path) => {
    if (path === '/home/missing') throw Object.assign(new Error('missing'), { code: 'ENOENT' });
    return inspect(path);
  });
  for (const operation of [
    () => prepareRuntime(leaf, owner),
    () => createProtectedFile(leaf, owner),
  ])
    await expect(operation()).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
  expect(fs.lstat).toHaveBeenCalledWith('/home/link');
  expect(fs.mkdir).not.toHaveBeenCalled();
  expect(fs.open).not.toHaveBeenCalled();
});

it.each(['/tmp', '/var', '/etc'])(
  'retains modeled Darwin %s dot/dot-dot semantics without rewriting the caller path',
  async (alias) => {
    const leaf = `${alias}/nested/./../runtime/secret`;
    filesystem(alias, 0, leaf);
    vi.mocked(fs.realpath).mockResolvedValue(`/private${alias}`);
    await assertProtected(leaf, owner);
    expect(fs.lstat).toHaveBeenCalledWith(`/private${alias}/runtime`);
    expect(fs.lstat).toHaveBeenLastCalledWith(leaf);
  },
);

it('checks the fixed Darwin physical cursor after missing/dot-dot rather than normalizing to root', async () => {
  const leaf = '/tmp/missing/../../hidden/secret';
  filesystem('/tmp', 0, leaf);
  vi.mocked(fs.realpath).mockResolvedValue('/private/tmp');
  const inspect = vi.mocked(fs.lstat).getMockImplementation()!;
  vi.mocked(fs.lstat).mockImplementation(async (path) => {
    if (path === '/private/tmp/missing')
      throw Object.assign(new Error('missing'), { code: 'ENOENT' });
    if (path === '/private/hidden') return info(42, 0o777, false, true);
    return inspect(path);
  });
  for (const operation of [
    () => assertProtected(leaf, owner),
    () => prepareRuntime(leaf, owner),
    () => createProtectedFile(leaf, owner),
  ])
    await expect(operation()).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
  expect(fs.lstat).not.toHaveBeenCalledWith('/hidden');
  expect(fs.lstat).not.toHaveBeenCalledWith(leaf);
  expect(fs.mkdir).not.toHaveBeenCalled();
  expect(fs.open).not.toHaveBeenCalled();
});

it('does not extend Darwin recognition to a root-owned nested alias below a permitted alias', async () => {
  const leaf = '/tmp/nested/tmp/secret';
  filesystem('/tmp', 0, leaf);
  vi.mocked(fs.realpath).mockResolvedValue('/private/tmp');
  const inspect = vi.mocked(fs.lstat).getMockImplementation()!;
  vi.mocked(fs.lstat).mockImplementation(async (path) =>
    path === '/private/tmp/nested/tmp' ? info(0, 0o777, false, true) : inspect(path),
  );
  await expect(assertProtected(leaf, owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  expect(fs.realpath).toHaveBeenCalledOnce();
  expect(fs.realpath).toHaveBeenCalledWith('/tmp');
});

it('retains unexpected filesystem failures without attempting creation', async () => {
  filesystem('/unused-link', 42, '/ordinary/runtime/secret');
  const failure = Object.assign(new Error('I/O failure'), { code: 'EIO' });
  vi.mocked(fs.lstat).mockRejectedValueOnce(failure);
  await expect(prepareRuntime('/ordinary/runtime', owner)).rejects.toBe(failure);
  vi.mocked(fs.lstat).mockRejectedValueOnce(failure);
  await expect(createProtectedFile('/ordinary/runtime/secret', owner)).rejects.toBe(failure);
  expect(fs.mkdir).not.toHaveBeenCalled();
  expect(fs.open).not.toHaveBeenCalled();
});
