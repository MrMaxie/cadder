import * as fs from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join, sep } from 'node:path';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { acquireRuntimeLock } from '../src/daemon/runtime-lock.ts';
import { resolvePaths } from '../src/daemon/paths.ts';
import * as shell from '../src/platform/powershell.ts';
import * as security from '../src/platform/runtime-security.ts';

vi.mock('node:fs/promises', async (original) => {
  const actual = await original<typeof import('node:fs/promises')>();
  return { ...actual, lstat: vi.fn(actual.lstat) };
});

const nativeFs = await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises');
let fixture: string | undefined;
let root: string;
let owner: security.RuntimeOwner;

beforeEach(async (context) => {
  vi.mocked(fs.lstat).mockReset().mockImplementation(nativeFs.lstat);
  owner = await security.runtimeOwner(process.getuid?.());
  if (process.platform === 'win32' && owner.elevated) context.skip();
  fixture = await fs.mkdtemp(join(tmpdir(), 'cadder-descendant-'));
  root = join(fixture, 'protected');
  await security.prepareRuntime(root, owner);
});
afterEach(async () => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  if (fixture) await fs.rm(fixture, { recursive: true, force: true });
  fixture = undefined;
});

async function snapshot(path: string): Promise<string> {
  const acl =
    process.platform === 'win32'
      ? await shell.powershell(`(Get-Acl -LiteralPath ${shell.psLiteral(path)}).Sddl`)
      : JSON.stringify(await fs.lstat(path));
  return createHash('sha256').update(acl).digest('hex');
}
async function descendants(): Promise<{ directory: string; file: string }> {
  const directory = join(root, 'native', 'nested');
  await fs.mkdir(join(root, 'native'), { mode: 0o700 });
  await fs.mkdir(directory, { mode: 0o700 });
  const file = join(directory, 'leaf');
  await fs.writeFile(file, 'unchanged content', { mode: 0o600 });
  return { directory, file };
}

it('accepts native nested owner-only storage without modifying ACLs or content', async () => {
  const { directory, file } = await descendants();
  const paths = [root, join(root, 'native'), directory, file];
  const before = await Promise.all(paths.map(snapshot));
  await security.assertRuntimeDescendant(root, directory, owner, true);
  await security.assertRuntimeDescendant(root, file, owner);
  expect(await Promise.all(paths.map(snapshot))).toEqual(before);
  expect(await fs.readFile(file, 'utf8')).toBe('unchanged content');
  if (process.platform === 'win32') {
    expect(owner.elevated).toBe(false);
    await expect(security.assertProtected(file, owner)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
  }
});

it('accepts a real SQLite DELETE journal while preserving strict primary database checks', async () => {
  const paths = resolvePaths({ runtimeDir: root });
  await security.prepareRuntime(paths.directory, owner);
  const lock = await acquireRuntimeLock(paths, owner);
  try {
    const journal = `${paths.lock}-journal`;
    await security.assertProtected(paths.lock, owner);
    await security.assertRuntimeDescendant(paths.directory, journal, owner);
    if (process.platform === 'win32')
      await expect(security.assertProtected(journal, owner)).rejects.toMatchObject({
        code: 'unsafe-runtime-permissions',
      });
  } finally {
    await lock.release();
  }
});

it('rejects escapes, equal roots and lexical dot aliases before inspecting their target', async () => {
  const { file } = await descendants();
  const inspect = vi.spyOn(fs, 'lstat').mockClear();
  for (const path of [
    root,
    join(fixture!, 'outside'),
    `${root}-sibling${sep}leaf`,
    'relative-leaf',
    `${root}${sep}native${sep}..${sep}native${sep}nested${sep}leaf`,
    `${root}${sep}.${sep}native${sep}nested${sep}leaf`,
  ]) {
    await expect(security.assertRuntimeDescendant(root, path, owner)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
  }
  expect(inspect).not.toHaveBeenCalled();
  await security.assertRuntimeDescendant(root, file, owner);
});

it.skipIf(process.platform !== 'win32')(
  'does not relax an inherited root even when its descendants are owner-only',
  async () => {
    const { file } = await descendants();
    await expect(security.assertRuntimeDescendant(fixture!, file, owner)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
  },
);

it('refuses in-root junctions/links, including final separators and linked directory chains', async () => {
  const { directory, file } = await descendants();
  const alias = join(root, 'alias');
  await fs.symlink(directory, alias, process.platform === 'win32' ? 'junction' : 'dir');
  const before = await snapshot(directory);
  for (const path of [alias, alias + sep, alias + sep + sep])
    await expect(security.assertRuntimeDescendant(root, path, owner, true)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
  await expect(
    security.assertRuntimeDescendant(root, join(alias, 'leaf'), owner),
  ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
  expect(await snapshot(directory)).toBe(before);
  expect(await fs.readFile(file, 'utf8')).toBe('unchanged content');
});

it('refuses wrong kinds, missing root/chain/leaf and inspection faults without mutation', async () => {
  const { directory, file } = await descendants();
  for (const [path, isDirectory] of [
    [directory, false],
    [file, true],
    [file + sep, false],
    [join(file, 'child'), false],
  ] as const)
    await expect(
      security.assertRuntimeDescendant(root, path, owner, isDirectory),
    ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
  for (const missing of [join(root, 'missing'), join(root, 'missing', 'leaf')])
    await expect(security.assertRuntimeDescendant(root, missing, owner)).rejects.toMatchObject({
      code: 'ENOENT',
      path: join(root, 'missing'),
    });
  const missingRoot = join(fixture!, 'missing-root');
  await expect(
    security.assertRuntimeDescendant(missingRoot, join(missingRoot, 'leaf'), owner),
  ).rejects.toMatchObject({ code: 'ENOENT', path: missingRoot });
  const original = nativeFs.lstat;
  const failure = Object.assign(new Error('inspection failed'), { code: 'EACCES' });
  vi.spyOn(fs, 'lstat').mockImplementation((async (path: unknown) => {
    if (path === file) throw failure;
    return original(path as string);
  }) as typeof fs.lstat);
  await expect(security.assertRuntimeDescendant(root, file, owner)).rejects.toBe(failure);
});

it('refuses a root junction before accepting any descendant', async () => {
  const { file } = await descendants();
  const alias = join(fixture!, 'root-alias');
  await fs.symlink(root, alias, process.platform === 'win32' ? 'junction' : 'dir');
  await expect(
    security.assertRuntimeDescendant(alias + sep, join(alias, 'native', 'nested', 'leaf'), owner),
  ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
  expect(await fs.readFile(file, 'utf8')).toBe('unchanged content');
});

async function fixtureAcl(path: string, directory: boolean, extra: string): Promise<void> {
  await shell.powershell(`
    $sid = New-Object Security.Principal.SecurityIdentifier(${shell.psLiteral(owner.id)});
    $foreign = New-Object Security.Principal.SecurityIdentifier('S-1-1-0');
    $acl = New-Object Security.AccessControl.${directory ? 'DirectorySecurity' : 'FileSecurity'};
    $acl.SetAccessRuleProtection($true,$false);
    ${extra}
    [IO.${directory ? 'Directory' : 'File'}]::SetAccessControl(${shell.psLiteral(path)},$acl);
  `);
}

it('refuses an unsafe root without repairing it', async () => {
  const { file } = await descendants();
  if (process.platform === 'win32')
    await fixtureAcl(
      root,
      true,
      "$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))); $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($foreign,'Read','Allow')));",
    );
  else await fs.chmod(root, 0o755);
  const before = await snapshot(root);
  await expect(security.assertRuntimeDescendant(root, file, owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  expect(await snapshot(root)).toBe(before);
});

it.skipIf(process.platform !== 'win32')(
  'refuses foreign, denied, insufficient and InheritOnly-only full access on native Windows fixtures',
  async () => {
    const rules = [
      "$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl','Allow'))); $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($foreign,'Read','Allow')));",
      "$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'Read','Allow')));",
      "$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl','Allow'))); $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'Write','Deny')));",
      "$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'Read','Allow'))); $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl','ContainerInherit,ObjectInherit','InheritOnly','Allow')));",
    ];
    for (let index = 0; index < rules.length; index++) {
      const directory = join(root, `unsafe-${index}`);
      await fs.mkdir(directory);
      await fixtureAcl(directory, true, rules[index]!);
      const before = await snapshot(directory);
      await expect(
        security.assertRuntimeDescendant(root, directory, owner, true),
      ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
      // Reject the unsafe chain before inspecting even an absent target beneath it.
      await expect(
        security.assertRuntimeDescendant(root, join(directory, 'absent-leaf'), owner),
      ).rejects.toMatchObject({ code: 'unsafe-runtime-permissions' });
      expect(await snapshot(directory)).toBe(before);
    }
  },
);

it.skipIf(process.platform !== 'win32')(
  'models a different Windows owner despite owner-only FullControl without assigning a native owner',
  async () => {
    const { file } = await descendants();
    const before = await snapshot(file);
    const original = shell.powershell;
    vi.spyOn(shell, 'powershell').mockImplementation(async (source) => {
      // Change only the in-memory descriptor returned for this leaf, never the actual ACL.
      const get = `$acl = Get-Acl -LiteralPath ${shell.psLiteral(file)};`;
      return original(
        source.replace(
          get,
          `${get} $acl.SetOwner((New-Object Security.Principal.SecurityIdentifier('S-1-5-18')));`,
        ),
      );
    });
    await expect(security.assertRuntimeDescendant(root, file, owner)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
    vi.restoreAllMocks();
    expect(await snapshot(file)).toBe(before);
  },
);

it('models Unix exact uid/mode checks for roots, directories and files without chown', async () => {
  const { directory, file } = await descendants();
  const original = nativeFs.lstat;
  const uid = 101;
  const unixOwner = { id: `uid:${uid}`, uid, elevated: false };
  vi.stubGlobal('process', Object.create(process, { platform: { value: 'linux' } }));
  let unsafe: { path: string; uid?: number; mode?: number } | undefined;
  vi.spyOn(fs, 'lstat').mockImplementation((async (path: unknown) => {
    const info = await original(path as string);
    const normalMode = info.isDirectory() ? 0o700 : 0o600;
    const requestedMode = unsafe && unsafe.path === path ? unsafe.mode : undefined;
    return Object.create(info, {
      uid: { value: unsafe && unsafe.path === path && unsafe.uid !== undefined ? unsafe.uid : uid },
      mode: { value: (info.mode & ~0o777) | (requestedMode ?? normalMode) },
    });
  }) as typeof fs.lstat);
  await security.assertRuntimeDescendant(root, file, unixOwner);
  for (const path of [root, directory, file]) {
    unsafe = { path, uid: uid + 1 };
    await expect(security.assertRuntimeDescendant(root, file, unixOwner)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
    unsafe = { path, mode: path === file ? 0o640 : 0o750 };
    await expect(security.assertRuntimeDescendant(root, file, unixOwner)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
  }
});

it('refuses a pre-existing unsafe SQLite journal without consuming or deleting it', async () => {
  const paths = resolvePaths({ runtimeDir: root });
  await security.prepareRuntime(paths.directory, owner);
  await security.createProtectedFile(paths.lock, owner);
  const journal = `${paths.lock}-journal`;
  await fs.writeFile(journal, 'untrusted journal sentinel', { mode: 0o600 });
  if (process.platform === 'win32')
    await fixtureAcl(
      journal,
      false,
      "$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl','Allow'))); $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($foreign,'Read','Allow')));",
    );
  else await fs.chmod(journal, 0o644);
  const before = await snapshot(journal);
  await expect(acquireRuntimeLock(paths, owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  expect(await snapshot(journal)).toBe(before);
  expect(await fs.readFile(journal, 'utf8')).toBe('untrusted journal sentinel');
  await expect(fs.lstat(paths.metadata)).rejects.toMatchObject({ code: 'ENOENT' });
});
