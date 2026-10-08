import type { Stats } from 'node:fs';
import * as fs from 'node:fs/promises';
import { parse } from 'node:path';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  assertProtected,
  createProtectedFile,
  prepareRuntime,
} from '../src/platform/runtime-security.ts';
import { powershell } from '../src/platform/powershell.ts';

// Drive and UNC parsing uses Windows path primitives with a modeled filesystem;
// no UNC server or share is contacted by these cases.
vi.mock('node:path', async (original) => {
  const path = await original<typeof import('node:path')>();
  return { ...path, ...path.win32 };
});
vi.mock('node:fs/promises', async (original) => ({
  ...(await original<typeof import('node:fs/promises')>()),
  lstat: vi.fn(),
  realpath: vi.fn(),
  mkdir: vi.fn(),
  open: vi.fn(),
}));
vi.mock('../src/platform/powershell.ts', async (original) => ({
  ...(await original<typeof import('../src/platform/powershell.ts')>()),
  powershell: vi.fn(),
}));
const owner = { id: 'S-1-5-21-fixture', elevated: false };
function info(directory = true, link = false): Stats {
  return {
    isSymbolicLink: () => link,
    isDirectory: () => directory,
    isFile: () => !directory,
  } as Stats;
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.spyOn(process, 'platform', 'get').mockReturnValue('win32');
  vi.mocked(powershell).mockResolvedValue('');
});
afterEach(() => vi.restoreAllMocks());

it.each([
  'C:\\',
  'C:\\\\\\',
  'C:/',
  'C:////',
  '\\\\server\\share',
  '\\\\server\\share\\',
  '\\\\server\\share\\\\\\',
])('preserves modeled Windows drive/UNC root validation for %s', async (path) => {
  vi.mocked(fs.lstat).mockResolvedValue(info());
  await prepareRuntime(path, owner);
  await assertProtected(path, owner, true);
  expect(fs.lstat).toHaveBeenLastCalledWith(parse(path).root);
  expect(fs.mkdir).not.toHaveBeenCalled();
});

it('trims only modeled Windows real leaf separators while keeping caller ACL spelling', async () => {
  const path = 'C:/runtime/real\\//';
  vi.mocked(fs.lstat).mockResolvedValue(info());
  await prepareRuntime(path, owner);
  await assertProtected(path, owner, true);
  expect(fs.lstat).toHaveBeenLastCalledWith('C:/runtime/real');
  expect(powershell).toHaveBeenLastCalledWith(expect.stringContaining(path));
  expect(fs.mkdir).not.toHaveBeenCalled();
});

it.each(['C:\\', '\\\\server\\share\\'])(
  'checks modeled Windows %s linked/dot-dot and missing/dot-dot ancestors',
  async (root) => {
    const linked = `${root}link`;
    vi.mocked(fs.lstat).mockImplementation(async (path) => {
      if (path === `${root}missing`) throw Object.assign(new Error('missing'), { code: 'ENOENT' });
      return info(true, path === linked);
    });
    for (const path of [`${root}link\\..\\safe\\secret`, `${root}missing\\..\\link\\secret`]) {
      for (const operation of [
        () => assertProtected(path, owner),
        () => prepareRuntime(path, owner),
        () => createProtectedFile(path, owner),
      ])
        await expect(operation()).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
    }
    expect(fs.lstat).toHaveBeenCalledWith(root);
    expect(fs.lstat).toHaveBeenCalledWith(linked);
    expect(fs.mkdir).not.toHaveBeenCalled();
    expect(fs.open).not.toHaveBeenCalled();
    expect(powershell).not.toHaveBeenCalled();
  },
);

it.each(['C:\\', '\\\\server\\share\\'])(
  'retains modeled Windows %s real dot/dot-dot semantics and leaf spelling',
  async (root) => {
    const leaf = `${root}real\\nested\\.\\..\\v2\\secret`;
    vi.mocked(fs.lstat).mockImplementation(async (path) => info(path !== leaf));
    await assertProtected(leaf, owner);
    expect(fs.lstat).toHaveBeenCalledWith(root);
    expect(fs.lstat).toHaveBeenCalledWith(`${root}real\\v2`);
    expect(fs.lstat).toHaveBeenLastCalledWith(leaf);
  },
);

it('uses native Windows separator rules for mixed-separator input without erasing a link', async () => {
  const leaf = 'C:/runtime/missing/../link/secret';
  vi.mocked(fs.lstat).mockImplementation(async (path) => {
    if (path === 'C:\\runtime\\missing')
      throw Object.assign(new Error('missing'), { code: 'ENOENT' });
    return info(true, path === 'C:\\runtime\\link');
  });
  await expect(assertProtected(leaf, owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  expect(fs.lstat).toHaveBeenCalledWith('C:\\runtime\\link');
  expect(fs.lstat).not.toHaveBeenCalledWith(leaf);
});
