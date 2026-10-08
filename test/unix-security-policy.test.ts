import { afterEach, expect, it, vi } from 'vitest';
import type { Stats } from 'node:fs';
import { lstat, chown } from 'node:fs/promises';
import { assertProtected, protectCreated, runtimeOwner } from '../src/platform/runtime-security.ts';

vi.mock('node:fs/promises', async (original) => ({
  ...(await original<typeof import('node:fs/promises')>()),
  lstat: vi.fn(),
  chown: vi.fn(),
}));
afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetAllMocks();
});

function unix(uid: number) {
  vi.stubGlobal('process', { ...process, platform: 'linux', getuid: () => uid });
}
function info(uid: number, mode: number, directory = false, link = false): Stats {
  return {
    uid,
    mode,
    isDirectory: () => directory,
    isFile: () => !directory,
    isSymbolicLink: () => link,
  } as Stats;
}

it('requires an explicit root owner and rejects invalid owner UIDs', async () => {
  unix(0);
  await expect(runtimeOwner()).rejects.toMatchObject({ code: 'runtime-owner-required' });
  expect(await runtimeOwner(1234)).toEqual({ id: 'uid:1234', uid: 1234, elevated: true });
  await expect(runtimeOwner(-1)).rejects.toMatchObject({ code: 'invalid-runtime-owner' });
  unix(1234);
  await expect(runtimeOwner(1235)).rejects.toMatchObject({ code: 'invalid-runtime-owner' });
  await expect(runtimeOwner(1.5)).rejects.toMatchObject({ code: 'invalid-runtime-owner' });
  expect(await runtimeOwner()).toEqual({ id: 'uid:1234', uid: 1234, elevated: false });
});

function leafInfo(value: Stats) {
  vi.mocked(lstat).mockImplementation(async (path) =>
    path === 'fixture' ? value : info(0, 0o777, true),
  );
}

it('fails closed on broad permissions, another owner and symlinks', async () => {
  unix(1234);
  const owner = { id: 'uid:1234', uid: 1234, elevated: false };
  leafInfo(info(1234, 0o700, true));
  await assertProtected('fixture', owner, true);
  leafInfo(info(1234, 0o600));
  await assertProtected('fixture', owner);
  for (const value of [info(1234, 0o644), info(1235, 0o600), info(1234, 0o600, false, true)]) {
    leafInfo(value);
    await expect(assertProtected('fixture', owner)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
    expect(lstat).toHaveBeenLastCalledWith('fixture');
  }
});

it('sets the explicit user ownership for newly created root-owned paths', async () => {
  unix(0);
  const owner = { id: 'uid:1234', uid: 1234, elevated: true };
  await protectCreated('fixture', owner);
  expect(chown).toHaveBeenCalledWith('fixture', 1234, 1234);
  vi.mocked(chown).mockClear();
  unix(1234);
  await protectCreated('fixture', owner);
  expect(chown).not.toHaveBeenCalled();
});
