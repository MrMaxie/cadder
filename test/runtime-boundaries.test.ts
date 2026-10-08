import {
  chmod,
  lstat,
  mkdtemp,
  mkdir,
  readFile,
  readlink,
  readdir,
  rm,
  symlink,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, sep } from 'node:path';
import { createConnection } from 'node:net';
import { once } from 'node:events';
import { afterEach, expect, it, vi } from 'vitest';
import { resolvePaths } from '../src/daemon/paths.ts';
import { startLocalRuntime } from '../src/daemon/local-runtime.ts';
import {
  assertProtected,
  createProtectedFile,
  prepareRuntime,
  runtimeOwner,
} from '../src/platform/runtime-security.ts';
import { powershell, psLiteral } from '../src/platform/powershell.ts';
import { basicResult } from './fixtures/rpc-data.ts';

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
const artifacts = ['directory', 'secret', 'lock', 'metadata', 'discovery'] as const;
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'cadder-boundary-'));
  roots.push(root);
  const options = {
    runtimeDir: root,
    ...(process.getuid ? { runtimeOwner: process.getuid() } : {}),
  };
  const paths = resolvePaths(options);
  const owner = await runtimeOwner(options.runtimeOwner);
  return { root, options, paths, owner };
}
async function snapshot(path: string) {
  if (process.platform === 'win32')
    return powershell(`(Get-Acl -LiteralPath ${psLiteral(path)}).Sddl`);
  const info = await lstat(path);
  return JSON.stringify({ uid: info.uid, gid: info.gid, mode: info.mode });
}

it.each(['prepareRuntime', 'assertProtected'] as const)(
  'denies a native trailing-separator leaf link during %s without modifying the target',
  async (operation) => {
    const { root, owner } = await fixture();
    const target = join(root, 'target');
    await prepareRuntime(target, owner);
    const sentinel = join(target, 'sentinel');
    await createProtectedFile(sentinel, owner, 'leave-target-unchanged');
    const before = await snapshot(target);
    const alias = join(root, 'alias');
    await symlink(target, alias, process.platform === 'win32' ? 'junction' : 'dir');
    const link = await readlink(alias);
    const suffixes = process.platform === 'win32' ? ['\\', '/', '\\//'] : ['/', '//'];
    for (const suffix of suffixes) {
      const path = alias + suffix;
      await expect(
        operation === 'prepareRuntime'
          ? prepareRuntime(path, owner)
          : assertProtected(path, owner, true),
      ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
    }
    expect(await readdir(target)).toEqual(['sentinel']);
    expect(await readFile(sentinel, 'utf8')).toBe('leave-target-unchanged');
    expect(await snapshot(target)).toBe(before);
    expect(await readlink(alias)).toBe(link);
  },
);

it('retains ordinary native real-directory trailing-separator behavior', async () => {
  const { root, owner } = await fixture();
  const directory = join(root, 'real');
  await prepareRuntime(directory + sep + sep, owner);
  await assertProtected(directory + sep, owner, true);
  await createProtectedFile(join(directory, 'sentinel'), owner, 'ordinary-real-path');
  expect(await readFile(join(directory, 'sentinel'), 'utf8')).toBe('ordinary-real-path');
});

it('rejects a native ancestor junction before trusting a protected leaf or starting a listener', async () => {
  const { root, owner } = await fixture();
  const target = join(root, 'target');
  const physical = resolvePaths({ runtimeDir: target });
  await prepareRuntime(physical.directory, owner);
  for (const artifact of artifacts.filter((key) => key !== 'directory'))
    await createProtectedFile(
      physical[artifact],
      owner,
      artifact === 'secret' ? 'x'.repeat(32) : '',
    );
  const sentinel = join(physical.directory, 'sentinel');
  await createProtectedFile(sentinel, owner, 'leave-target-unchanged');
  const contents = await Promise.all(
    artifacts.filter((key) => key !== 'directory').map((key) => readFile(physical[key])),
  );
  const acls = await Promise.all(artifacts.map((key) => snapshot(physical[key])));
  const alias = join(root, 'alias');
  await symlink(target, alias, process.platform === 'win32' ? 'junction' : 'dir');
  const paths = resolvePaths({ runtimeDir: alias });
  const link = await readlink(alias);
  for (const artifact of artifacts)
    await expect(
      assertProtected(paths[artifact], owner, artifact === 'directory'),
    ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
  const handler = vi.fn(async () => basicResult);
  let started: Awaited<ReturnType<typeof startLocalRuntime>> | undefined;
  try {
    await expect(
      startLocalRuntime(
        { runtimeDir: alias, ...(process.getuid ? { runtimeOwner: process.getuid() } : {}) },
        handler,
      ).then((runtime) => {
        started = runtime;
        return runtime;
      }),
    ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
  } finally {
    await started?.stop();
  }
  const socket = createConnection(paths.endpoint);
  try {
    await expect(once(socket, 'connect')).rejects.toBeDefined();
  } finally {
    socket.destroy();
  }
  expect(handler).not.toHaveBeenCalled();
  expect(await readlink(alias)).toBe(link);
  expect(await readFile(sentinel, 'utf8')).toBe('leave-target-unchanged');
  expect(
    await Promise.all(
      artifacts.filter((key) => key !== 'directory').map((key) => readFile(physical[key])),
    ),
  ).toEqual(contents);
  expect(await Promise.all(artifacts.map((key) => snapshot(physical[key])))).toEqual(acls);
});

it.each(['prepareRuntime', 'createProtectedFile'] as const)(
  'rejects native linked ancestors before %s creates a missing path',
  async (operation) => {
    const { root, owner } = await fixture();
    const target = join(root, 'target');
    await prepareRuntime(target, owner);
    const sentinel = join(target, 'sentinel');
    await createProtectedFile(sentinel, owner, 'leave-target-unchanged');
    const before = await snapshot(target);
    const alias = join(root, 'alias');
    await symlink(target, alias, process.platform === 'win32' ? 'junction' : 'dir');
    const link = await readlink(alias);
    const denied =
      operation === 'prepareRuntime'
        ? prepareRuntime(join(alias, 'missing', 'v2'), owner)
        : createProtectedFile(join(alias, 'new-secret'), owner, 'must-not-be-created');
    await expect(denied).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
    await expect(
      startLocalRuntime(
        {
          runtimeDir: join(alias, 'missing'),
          ...(process.getuid ? { runtimeOwner: process.getuid() } : {}),
        },
        async () => basicResult,
      ),
    ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
    expect(await readdir(target)).toEqual(['sentinel']);
    expect(await readFile(sentinel, 'utf8')).toBe('leave-target-unchanged');
    expect(await snapshot(target)).toBe(before);
    expect(await readlink(alias)).toBe(link);
  },
);

it.each(['prepareRuntime', 'createProtectedFile'] as const)(
  'checks native missing/dot-dot/link paths before %s mutates a target',
  async (operation) => {
    const { root, owner } = await fixture();
    const target = join(root, 'target');
    await prepareRuntime(target, owner);
    const sentinel = join(target, 'sentinel');
    await createProtectedFile(sentinel, owner, 'leave-target-unchanged');
    const before = await snapshot(target);
    const alias = join(root, 'alias');
    await symlink(target, alias, process.platform === 'win32' ? 'junction' : 'dir');
    const link = await readlink(alias);
    const path = `${root}${sep}missing${sep}..${sep}alias${sep}escaped`;
    const denied =
      operation === 'prepareRuntime'
        ? prepareRuntime(path, owner)
        : createProtectedFile(path, owner, 'must-not-be-created');
    await expect(denied).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
    expect(await readdir(target)).toEqual(['sentinel']);
    expect(await readFile(sentinel, 'utf8')).toBe('leave-target-unchanged');
    expect(await snapshot(target)).toBe(before);
    expect(await readlink(alias)).toBe(link);
  },
);

it('retains ordinary native real-directory dot and dot-dot semantics', async () => {
  const { root, owner } = await fixture();
  await prepareRuntime(join(root, 'real', 'nested'), owner);
  const directory = `${root}${sep}real${sep}nested${sep}.${sep}..${sep}v2`;
  await prepareRuntime(directory, owner);
  await assertProtected(directory, owner, true);
  await createProtectedFile(`${directory}${sep}secret`, owner, 'ordinary-real-path');
  expect(await readFile(join(root, 'real', 'v2', 'secret'), 'utf8')).toBe('ordinary-real-path');
});

it('does not erase a native linked ancestor with lexical dot-dot normalization', async () => {
  const { root, owner } = await fixture();
  const target = join(root, 'target');
  await mkdir(join(target, 'nested'), { recursive: true });
  await prepareRuntime(join(target, 'v2'), owner);
  const alias = join(root, 'alias');
  await symlink(join(target, 'nested'), alias, process.platform === 'win32' ? 'junction' : 'dir');
  await expect(assertProtected(`${alias}${sep}..${sep}v2`, owner, true)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
});

it.each(artifacts)(
  'denies a native %s link/reparse substitution without touching the target',
  async (artifact) => {
    const { root, options, paths, owner } = await fixture();
    const target = join(root, 'target');
    const sentinel = join(target, 'sentinel');
    await mkdir(target, { mode: 0o700 });
    await writeFile(sentinel, 'leave-target-unchanged', { mode: 0o600 });
    if (artifact !== 'directory') await prepareRuntime(paths.directory, owner);
    // Junction creation on Windows needs no symlink privilege; file artifacts are
    // deliberately replaced by directory reparse points, not claimed file symlinks.
    await symlink(target, paths[artifact], process.platform === 'win32' ? 'junction' : 'dir');
    const before = await snapshot(target);
    const link = await readlink(paths[artifact]);
    await expect(
      assertProtected(paths[artifact], owner, artifact === 'directory'),
    ).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
    await expect(startLocalRuntime(options, async () => basicResult)).rejects.toMatchObject({
      code: artifact === 'metadata' ? 'EISDIR' : 'unsafe-runtime-permissions',
    });
    if (artifact !== 'discovery')
      await expect(lstat(paths.discovery)).rejects.toMatchObject({ code: 'ENOENT' });
    expect(await readlink(paths[artifact])).toBe(link);
    expect(await readFile(sentinel, 'utf8')).toBe('leave-target-unchanged');
    expect(await snapshot(target)).toBe(before);
  },
);

it.each(artifacts)(
  'denies unsafe native %s permissions/ACLs without repairing existing content',
  async (artifact) => {
    const { options, paths, owner } = await fixture();
    await prepareRuntime(paths.directory, owner);
    const path = paths[artifact];
    const sentinel = artifact === 'directory' ? join(path, 'sentinel') : path;
    const content = artifact === 'secret' ? 'x'.repeat(32) : 'untouched-existing-content';
    if (artifact === 'directory') await createProtectedFile(sentinel, owner, content);
    else await createProtectedFile(path, owner, content);
    if (process.platform === 'win32') {
      await powershell(`
      $sections=[System.Security.AccessControl.AccessControlSections]::Owner -bor [System.Security.AccessControl.AccessControlSections]::Access;
      $acl=[System.IO.${artifact === 'directory' ? 'Directory' : 'File'}]::GetAccessControl(${psLiteral(path)},$sections);
      $sid=New-Object System.Security.Principal.SecurityIdentifier('S-1-1-0');
      $acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($sid,'Read','Allow')));
      [System.IO.${artifact === 'directory' ? 'Directory' : 'File'}]::SetAccessControl(${psLiteral(path)},$acl);
    `);
    } else await chmod(path, artifact === 'directory' ? 0o755 : 0o644);
    const before = await snapshot(path);
    await expect(startLocalRuntime(options, async () => basicResult)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
    expect(await readFile(sentinel, 'utf8')).toBe(content);
    expect(await snapshot(path)).toBe(before);
  },
);

it.each(artifacts)(
  'denies a native %s owner mismatch without changing its owner or content',
  async (artifact) => {
    const { paths, owner } = await fixture();
    await prepareRuntime(paths.directory, owner);
    const path = paths[artifact];
    const sentinel = artifact === 'directory' ? join(path, 'sentinel') : path;
    await createProtectedFile(sentinel, owner, 'untouched-owner-mismatch');
    const before = await snapshot(path);
    // Read the real ACL/stat against a deliberately different expected owner.
    // This does not create an account or transfer ownership to another host user.
    const other =
      process.platform === 'win32'
        ? { id: 'S-1-1-0', elevated: false }
        : { id: `uid:${owner.uid! + 1}`, uid: owner.uid! + 1, elevated: false };
    await expect(assertProtected(path, other, artifact === 'directory')).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
    expect(await snapshot(path)).toBe(before);
    expect(await readFile(sentinel, 'utf8')).toBe('untouched-owner-mismatch');
  },
);

it.each(artifacts)(
  'denies inherited native Windows %s ACLs without rewriting them',
  {
    skip: process.platform !== 'win32',
  },
  async (artifact) => {
    const { options, paths, owner } = await fixture();
    await prepareRuntime(paths.directory, owner);
    const path = paths[artifact];
    const sentinel = artifact === 'directory' ? join(path, 'sentinel') : path;
    await createProtectedFile(sentinel, owner, 'untouched-inherited-acl');
    await powershell(`
    $sections=[System.Security.AccessControl.AccessControlSections]::Owner -bor [System.Security.AccessControl.AccessControlSections]::Access;
    $acl=[System.IO.${artifact === 'directory' ? 'Directory' : 'File'}]::GetAccessControl(${psLiteral(path)},$sections);
    $acl.SetAccessRuleProtection($false,$true);
    [System.IO.${artifact === 'directory' ? 'Directory' : 'File'}]::SetAccessControl(${psLiteral(path)},$acl);
  `);
    const before = await snapshot(path);
    await expect(startLocalRuntime(options, async () => basicResult)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
    expect(await snapshot(path)).toBe(before);
    expect(await readFile(sentinel, 'utf8')).toBe('untouched-inherited-acl');
  },
);
