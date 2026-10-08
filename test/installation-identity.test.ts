import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, realpath, rm, symlink, writeFile } from 'node:fs/promises';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, posix, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { afterEach, expect, it, vi } from 'vitest';
import { resolvePaths } from '../src/daemon/paths.ts';
import { resolveInstallationRoot } from '../src/daemon/installation-root.ts';

vi.mock('node:os', async (original) => {
  const os = await original<typeof import('node:os')>();
  return { ...os, homedir: vi.fn(os.homedir) };
});

const roots: string[] = [];
afterEach(async () => {
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
  vi.mocked(homedir).mockReset();
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'cadder-identity-'));
  roots.push(root);
  vi.stubEnv('CADDER_RUNTIME_DIR', undefined);
  vi.stubEnv('CADDER_RUNTIME_PROFILE', 'default');
  return root;
}

it('separates all default runtime artifacts by canonical installation, not version', async () => {
  const root = await fixture();
  const a = join(root, 'A');
  const b = join(root, 'B');
  await mkdir(a);
  await mkdir(b);
  const first = resolvePaths({ installationRoot: a });
  const second = resolvePaths({ installationRoot: b });
  expect(resolvePaths({ installationRoot: a })).toEqual(first);
  for (const key of [
    'directory',
    'instance',
    'endpoint',
    'secret',
    'lock',
    'metadata',
    'discovery',
    'history',
  ] as const)
    expect(first[key]).not.toBe(second[key]);
  expect(first.directory).toMatch(/v2[/\\][a-f0-9]{16}$/);
  expect(first.history).toBe(join(first.directory, 'runtime.sqlite3'));
  expect(second.history).toBe(join(second.directory, 'runtime.sqlite3'));
});

it('keeps the actual platform base and all paths coherent with the full installation digest', async () => {
  const root = await fixture();
  const digest = createHash('sha256')
    .update(await realpath(root))
    .digest('hex')
    .slice(0, 16);
  let base: string;
  if (process.platform === 'win32')
    base = join(
      process.env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local'),
      'Cadder',
      'Cadder',
      'run',
    );
  else if (process.platform === 'darwin')
    base = join(homedir(), 'Library', 'Application Support', 'dev.Cadder.Cadder', 'run');
  else if (process.env.XDG_RUNTIME_DIR) base = join(process.env.XDG_RUNTIME_DIR, 'cadder');
  else
    base = join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local', 'share'), 'cadder', 'run');
  const paths = resolvePaths({ installationRoot: root });
  expect(paths.directory).toBe(join(resolve(base), 'v2', digest));
  expect(paths.instance).toBe(
    createHash('sha256').update(paths.directory).digest('hex').slice(0, 16),
  );
  expect(paths.endpoint).toBe(
    process.platform === 'win32'
      ? `\\\\.\\pipe\\cadder-v2-${paths.instance}`
      : join(paths.directory, 'cadder.sock'),
  );
  for (const [key, filename] of [
    ['secret', 'ipc-secret'],
    ['lock', 'lock.sqlite3'],
    ['metadata', 'cadder.lock.json'],
    ['discovery', 'cadder-ipc.json'],
    ['history', 'runtime.sqlite3'],
  ] as const)
    expect(paths[key]).toBe(join(paths.directory, filename));
});

it('fits a modeled normal macOS default without weakening the portable byte guard', async () => {
  const root = await fixture();
  vi.mocked(homedir).mockReturnValue('/Users/normaluser');
  vi.spyOn(process, 'platform', 'get').mockReturnValue('darwin');
  const digest = createHash('sha256')
    .update(await realpath(root))
    .digest('hex')
    .slice(0, 16);
  const paths = resolvePaths({ installationRoot: root });
  const components = [
    'Library',
    'Application Support',
    'dev.Cadder.Cadder',
    'run',
    'v2',
    digest,
    'cadder.sock',
  ];
  expect(paths.endpoint).toBe(join(resolve('/Users/normaluser'), ...components));
  expect(Buffer.byteLength(paths.endpoint)).toBeLessThanOrEqual(103);
  // POSIX modeling is explicit: Windows native path resolution may add a drive.
  expect(Buffer.byteLength(posix.join('/Users/normaluser', ...components))).toBe(99);
  vi.mocked(homedir).mockReturnValue('/Users/' + 'x'.repeat(40));
  expect(() => resolvePaths({ installationRoot: root })).toThrow('103 UTF-8 bytes');
  vi.mocked(homedir).mockReturnValue('/Users/' + 'é'.repeat(20));
  expect(() => resolvePaths({ installationRoot: root })).toThrow('103 UTF-8 bytes');
});

it('canonicalizes physical installation aliases including native junctions or symlinks', async () => {
  const root = await fixture();
  const installation = join(root, 'package');
  await mkdir(join(installation, 'sub'), { recursive: true });
  const alias = join(root, 'alias');
  await symlink(installation, alias, process.platform === 'win32' ? 'junction' : 'dir');
  const paths = resolvePaths({ installationRoot: installation });
  for (const spelling of [alias, join(installation, 'sub', '..'), `${installation}/`])
    expect(resolvePaths({ installationRoot: spelling })).toEqual(paths);
  if (process.platform === 'win32')
    expect(resolvePaths({ installationRoot: installation.toUpperCase() })).toEqual(paths);
});

it('anchors npm to the nearest physical cadder package, never shared Node, argv or cwd', async () => {
  const root = await fixture();
  await writeFile(join(root, 'package.json'), JSON.stringify({ name: 'cadder' }));
  const packageRoot = join(root, 'nested');
  const module = join(packageRoot, 'dist', 'daemon', 'paths.js');
  await mkdir(dirname(module), { recursive: true });
  await writeFile(join(packageRoot, 'package.json'), JSON.stringify({ name: 'cadder' }));
  await writeFile(join(packageRoot, 'dist', 'package.json'), JSON.stringify({ name: 'other' }));
  await writeFile(module, '');
  const alias = join(root, 'alias');
  await symlink(packageRoot, alias, process.platform === 'win32' ? 'junction' : 'dir');
  const input = { sea: false, executable: process.execPath };
  expect(resolveInstallationRoot({ ...input, moduleUrl: pathToFileURL(module).href })).toBe(
    await realpath(packageRoot),
  );
  expect(
    resolveInstallationRoot({
      ...input,
      moduleUrl: pathToFileURL(join(alias, 'dist', 'daemon', 'paths.js')).href,
    }),
  ).toBe(await realpath(packageRoot));
  expect(await realpath(packageRoot)).not.toBe(dirname(process.execPath));
  expect(resolveInstallationRoot()).toBe(await realpath(resolve('.')));
});

it('makes all three SEA siblings agree and follows a real executable alias', async () => {
  const root = await fixture();
  const installation = join(root, 'sea');
  await mkdir(installation);
  const paths = resolvePaths({ installationRoot: installation });
  for (const name of ['cadder', 'cadderd', 'caddy']) {
    const executable = join(installation, name);
    await writeFile(executable, '');
    const installationRoot = resolveInstallationRoot({ sea: true, executable });
    expect(installationRoot).toBe(await realpath(installation));
    expect(resolvePaths({ installationRoot })).toEqual(paths);
  }
  const alias = join(root, 'alias');
  await symlink(installation, alias, process.platform === 'win32' ? 'junction' : 'dir');
  expect(resolveInstallationRoot({ sea: true, executable: join(alias, 'caddy') })).toBe(
    await realpath(installation),
  );
});

it('fails closed on missing roots, non-directories, missing executables and npm anchors', async () => {
  const root = await fixture();
  const module = join(root, 'module.js');
  await writeFile(module, '');
  for (const input of [
    { installationRoot: join(root, 'missing') },
    { installationRoot: module },
    { sea: true, executable: join(root, 'missing') },
    { sea: false, moduleUrl: pathToFileURL(module).href },
    { sea: false, moduleUrl: pathToFileURL(join(root, 'missing')).href },
    { sea: false, moduleUrl: 'https://invalid.example/module.js' },
  ])
    expect(() => resolveInstallationRoot(input)).toThrow('Cannot resolve');
  await writeFile(join(root, 'package.json'), '{');
  expect(() =>
    resolveInstallationRoot({ sea: false, moduleUrl: pathToFileURL(module).href }),
  ).toThrow('Cannot resolve');
  expect(() => resolvePaths({ installationRoot: join(root, 'missing') })).toThrow('Cannot resolve');
});

it('preserves option/env overrides and explicit profile behavior without requiring an anchor', async () => {
  const root = await fixture();
  vi.stubEnv('CADDER_RUNTIME_DIR', join(root, 'env'));
  vi.stubEnv('CADDER_RUNTIME_PROFILE', 'dev');
  const missing = join(root, 'missing');
  const env = resolvePaths({ installationRoot: missing });
  expect(env.directory).toBe(join(root, 'env', 'v2'));
  expect(env.profile).toBe('default');
  const explicit = resolvePaths({
    runtimeDir: join(root, 'option'),
    installationRoot: missing,
    profile: 'dev',
  });
  expect(explicit.directory).toBe(join(root, 'option', 'v2'));
  expect(explicit.profile).toBe('dev');
});

it('preserves existing empty override cwd resolution without installation anchoring', async () => {
  const root = await fixture();
  const missing = join(root, 'missing');
  expect(resolvePaths({ runtimeDir: '', installationRoot: missing }).directory).toBe(
    join(resolve(''), 'v2'),
  );
  vi.stubEnv('CADDER_RUNTIME_DIR', '');
  expect(resolvePaths({ installationRoot: missing }).directory).toBe(join(resolve(''), 'v2'));
});

it('keeps dev namespaces beneath each installation and validates dev ids', async () => {
  const root = await fixture();
  const a = join(root, 'A');
  const b = join(root, 'B');
  await mkdir(a);
  await mkdir(b);
  vi.stubEnv('CADDER_DEV_ID', 'test');
  const first = resolvePaths({ installationRoot: a, profile: 'dev' });
  const second = resolvePaths({ installationRoot: b, profile: 'dev' });
  expect(first.directory).toMatch(/v2[/\\][a-f0-9]{16}[/\\]profiles[/\\]dev[/\\]test$/);
  expect(first.endpoint).not.toBe(second.endpoint);
  vi.stubEnv('CADDER_DEV_ID', '..');
  expect(() => resolvePaths({ installationRoot: a, profile: 'dev' })).toThrow(
    'Invalid CADDER_DEV_ID',
  );
});

it.each(['linux', 'darwin'] as const)(
  'bounds %s sockets at 103 UTF-8 bytes, not characters',
  (platform) => {
    vi.spyOn(process, 'platform', 'get').mockReturnValue(platform);
    const prefix = resolve('/r');
    const suffixBytes =
      Buffer.byteLength(join(prefix, 'v2', 'cadder.sock')) - Buffer.byteLength(prefix);
    const runtimeDir = prefix + 'x'.repeat(103 - suffixBytes - Buffer.byteLength(prefix));
    expect(Buffer.byteLength(resolvePaths({ runtimeDir }).endpoint)).toBe(103);
    expect(() => resolvePaths({ runtimeDir: runtimeDir + 'x' })).toThrow('103 UTF-8 bytes');
    const multibyte = runtimeDir.slice(0, -1) + 'é';
    expect(() => resolvePaths({ runtimeDir: multibyte })).toThrow('103 UTF-8 bytes');
  },
);

it('checks representative Unix default bases and fails closed on long configured bases', async () => {
  const root = await fixture();
  vi.spyOn(process, 'platform', 'get').mockReturnValue('linux');
  vi.stubEnv('XDG_RUNTIME_DIR', '/run/user/1000');
  expect(Buffer.byteLength(resolvePaths({ installationRoot: root }).endpoint)).toBeLessThanOrEqual(
    103,
  );
  vi.stubEnv('XDG_RUNTIME_DIR', '');
  vi.stubEnv('XDG_DATA_HOME', '/home/user/.local/share');
  expect(Buffer.byteLength(resolvePaths({ installationRoot: root }).endpoint)).toBeLessThanOrEqual(
    103,
  );
  vi.stubEnv('XDG_DATA_HOME', '/home/' + 'x'.repeat(80));
  expect(() => resolvePaths({ installationRoot: root })).toThrow(
    'Select a shorter CADDER_RUNTIME_DIR',
  );
});
