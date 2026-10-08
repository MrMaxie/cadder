import {
  copyFile,
  link,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rename,
  rm,
  stat,
  symlink,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { build } from 'esbuild';
import { afterEach, expect, it } from 'vitest';
import { RealCaddyResolver, installationEntries } from '../src/caddy/resolver.ts';
import {
  absoluteCaddyPath,
  fileIdentity,
  nativeExecutable,
  sameFile,
} from '../src/caddy/executable.ts';
import {
  PinnedCaddyImage,
  parseModules,
  parseVersion,
  requiredCaddyModules,
  compatibilityProbeRevision,
} from '../src/caddy/image.ts';
import { configurationLocations } from '../src/caddy/configuration.ts';
import { runOwnedCommand } from '../src/platform/owned-command.ts';
import { resolveInstallationRoot } from '../src/daemon/installation-root.ts';

const cleanup: (() => Promise<unknown>)[] = [];
afterEach(async () => {
  for (const close of cleanup.splice(0).reverse()) await close();
});
const nativeName = (name: string) => (process.platform === 'win32' ? `${name}.exe` : name);
async function setup() {
  const directory = await mkdtemp(join(tmpdir(), 'cadder-caddy-resolution-'));
  cleanup.push(() => rm(directory, { recursive: true, force: true }));
  const root = join(directory, 'project', 'node_modules', 'cadder');
  await mkdir(join(root, 'dist'), { recursive: true });
  await writeFile(
    join(root, 'package.json'),
    JSON.stringify({
      name: 'cadder',
      bin: { caddy: 'dist/caddy.js', cadder: 'dist/cadder.js', cadderd: 'dist/cadderd.js' },
    }),
  );
  const preload = join(directory, 'fixture.cjs');
  await build({
    entryPoints: ['test/fixtures/caddy-preload.ts'],
    bundle: true,
    platform: 'node',
    format: 'cjs',
    outfile: preload,
  });
  const env = {
    ...process.env,
    NODE_OPTIONS: `--require=${JSON.stringify(preload)}`,
    PATH: directory,
  };
  const paths = configurationLocations({
    platform: process.platform,
    installationRoot: root,
    home: directory,
    ...(process.platform === 'win32'
      ? { knownFolders: { roamingAppData: directory, programData: directory } }
      : {}),
  });
  // Source files intentionally distinct even when OS modeled roots share this temp directory.
  const configurationPaths = {
    ...paths,
    user: join(directory, 'user.toml'),
    system: join(directory, 'system.toml'),
  };
  async function fake(name = 'caddy', behavior: object = {}) {
    const path = join(directory, nativeName(name));
    await copyFile(process.execPath, path);
    await writeFile(`${path}.json`, JSON.stringify({ modules: requiredCaddyModules, ...behavior }));
    return path;
  }
  function resolver(extra: ConstructorParameters<typeof RealCaddyResolver>[0] = {}) {
    const value = new RealCaddyResolver({
      environment: env,
      installation: { root, distribution: 'npm' },
      configurationPaths,
      ...extra,
    });
    cleanup.push(() => value.close());
    return value;
  }
  return { directory, root, env, configurationPaths, fake, resolver };
}

it.each([
  ['C:\\tools\\caddy.exe', true],
  ['\\\\server\\share\\caddy.exe', true],
  ['//server/share/caddy.exe', true],
  ['\\caddy.exe', false],
  ['/caddy.exe', false],
  ['C:caddy.exe', false],
  ['caddy.exe', false],
] as const)('modeled Windows qualification %j is %s', (path, expected) => {
  expect(absoluteCaddyPath(path, 'win32')).toBe(expected);
});

it.skipIf(process.platform !== 'win32')(
  'native Windows refuses root-relative executable overrides',
  async () => {
    const s = await setup();
    await expect(s.resolver({ explicitOverride: '\\caddy.exe' }).pin()).rejects.toThrow('absolute');
    await expect(nativeExecutable('\\caddy.exe')).rejects.toThrow('absolute');
  },
);

it('native physical identity detects hardlinks, distinguishes copies and never conflates shared Node', async () => {
  const s = await setup();
  const first = await s.fake();
  const alias = join(s.directory, nativeName('alias'));
  await link(first, alias);
  const second = await s.fake('second');
  expect(fileIdentity(await stat(first, { bigint: true }))).not.toMatch(/:0$/);
  expect(await sameFile(first, alias)).toBe(true);
  expect(await sameFile(first, second)).toBe(false);
  expect(await sameFile(first, process.execPath)).toBe(false);
});

it('pins compatibility/image evidence and closes explicitly', async () => {
  const s = await setup();
  const path = await s.fake();
  const resolver = s.resolver();
  const image = await resolver.pin();
  expect(image.path).toBe(await realpath(path));
  expect(image.source).toBe('path');
  expect(image.sha256).toMatch(/^[a-f0-9]{64}$/);
  expect(image.evidence).toEqual({
    version: '2.11.4',
    modules: [...requiredCaddyModules].sort(),
    probeRevision: compatibilityProbeRevision,
  });
  expect(await resolver.pin()).toBe(image);
  await resolver.close();
  await expect(image.verify()).rejects.toThrow('closed');
  await expect(resolver.pin()).rejects.toThrow('closed');
});

it.each(['override', 'portable', 'user', 'system'] as const)(
  'resolves precedence source %s',
  async (source) => {
    const s = await setup();
    const selected = await s.fake('selected');
    await s.fake();
    const lower = '[caddy]\nreal_path = "relative-must-not-win"';
    for (const kind of ['portable', 'user', 'system'] as const)
      await writeFile(s.configurationPaths[kind], lower);
    if (source !== 'override') {
      for (const higher of ['portable', 'user', 'system'] as const) {
        if (higher === source) break;
        await writeFile(s.configurationPaths[higher], '[caddy]');
      }
      await writeFile(
        s.configurationPaths[source],
        `[caddy]\nreal_path = ${JSON.stringify(selected)}`,
      );
    }
    const image = await s
      .resolver(source === 'override' ? { explicitOverride: selected } : {})
      .pin();
    expect(image.source).toBe(source);
    expect(image.path).toBe(await realpath(selected));
  },
);

it('portable single command preserves PATH order and ignores relative entries/project/legacy selectors', async () => {
  const s = await setup();
  const chosen = await s.fake('caddy-real');
  await s.fake();
  await writeFile(s.configurationPaths.portable, '[caddy]\nreal_command = "caddy-real"');
  await writeFile(join(s.directory, 'cadder.toml'), '[caddy]\nreal_path = "untrusted-project"');
  s.env.PATH = ['.', 'relative', s.directory, s.root].join(delimiter);
  const resolver = s.resolver({
    environment: {
      ...s.env,
      CADDER_CADDY_REAL_COMMAND: 'wrong',
      CADDER_CADDY__REAL_PATH: 'wrong',
      CADDER_CADDY_SHIM_PATH: chosen,
    },
  });
  expect((await resolver.pin()).path).toBe(await realpath(chosen));
});

it.skipIf(process.platform !== 'win32')(
  'native Windows PATH preserves quoted absolute directories containing separators',
  async () => {
    const s = await setup();
    const original = await s.fake();
    const directory = join(s.directory, 'quoted; directory');
    await mkdir(directory);
    const path = join(directory, nativeName('caddy'));
    await rename(original, path);
    await rename(`${original}.json`, `${path}.json`);
    const image = await s
      .resolver({ environment: { ...s.env, PATH: `.;"${directory}";relative` } })
      .pin();
    expect(image.path).toBe(await realpath(path));
  },
);

async function waitForProbe(marker: string): Promise<void> {
  for (let attempt = 0; attempt < 300; attempt++) {
    try {
      if ((await readFile(marker, 'utf8')).includes('"version"')) return;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    }
    await delay(20);
  }
  throw new Error('Owned version probe did not become ready.');
}

it('caller cancellation during the first probe cannot poison concurrent or later pin callers', async () => {
  const s = await setup();
  const marker = join(s.directory, 'probe-commands');
  const releaseVersion = join(s.directory, 'release-version');
  await s.fake('caddy', { marker, releaseVersion });
  const resolver = s.resolver();
  const controller = new AbortController();
  const cancelled = resolver.pin(controller.signal).catch((error: unknown) => error);
  const otherController = new AbortController();
  const concurrent = resolver.pin(otherController.signal).catch((error: unknown) => error);
  await waitForProbe(marker);
  controller.abort();
  expect(await cancelled).toMatchObject({ code: 'abort' });
  await writeFile(releaseVersion, 'go');
  const image = await resolver.pin();
  expect(await concurrent).toBe(image);
  expect(image.evidence.version).toBe('2.11.4');
  expect((await readFile(marker, 'utf8')).trim().split('\n')).toHaveLength(2);
});

it('resolver close cancels its owned pending probe and rejects future pin callers', async () => {
  const s = await setup();
  const marker = join(s.directory, 'probe-commands');
  await s.fake('caddy', { marker, releaseVersion: join(s.directory, 'never-released') });
  const resolver = s.resolver();
  const pending = resolver.pin().catch((error: unknown) => error);
  await waitForProbe(marker);
  await resolver.close();
  expect(await pending).toMatchObject({ code: 'abort' });
  await expect(resolver.pin()).rejects.toThrow('closed');
});

it.each(['caddy --version', '../caddy', '', 'caddy; evil', 'C:caddy', 'caddy\\relative'])(
  'denies arbitrary configured commands %j',
  async (command) => {
    const s = await setup();
    await s.fake();
    await writeFile(
      s.configurationPaths.user,
      `[caddy]\nreal_command = ${JSON.stringify(command)}`,
    );
    await expect(s.resolver().pin()).rejects.toThrow('single program');
  },
);

it('invalid selected native image fails instead of trying a lower source', async () => {
  const s = await setup();
  await s.fake();
  await writeFile(s.configurationPaths.portable, '[caddy]\nreal_path = "relative"');
  await expect(s.resolver().pin()).rejects.toThrow('absolute');
});

it.each(['.cmd', '.ps1', '.js', ''])(
  'rejects npm script wrapper %j before executing any help probe',
  async (suffix) => {
    const s = await setup();
    const path = join(s.directory, `wrapper${suffix}`);
    await writeFile(path, '#!/usr/bin/env node\nthrow new Error("recursive shim")', {
      mode: 0o755,
    });
    await expect(s.resolver({ explicitOverride: path }).pin()).rejects.toThrow(/native|wrapper/);
  },
);

it('rejects hardlinked npm bin identities and skips aliased entries on PATH', async () => {
  const s = await setup();
  const real = await s.fake('upstream');
  const shim = join(s.root, 'dist', 'caddy.js');
  await copyFile(process.execPath, shim);
  const candidate = join(s.directory, nativeName('caddy'));
  await link(shim, candidate);
  await expect(s.resolver({ explicitOverride: candidate }).pin()).rejects.toThrow(
    'Cadder distribution',
  );
  const upstreamDir = join(s.directory, 'upstream');
  await mkdir(upstreamDir);
  const upstream = join(upstreamDir, nativeName('caddy'));
  await rename(real, upstream);
  await rename(`${real}.json`, `${upstream}.json`);
  expect(
    (
      await s
        .resolver({ environment: { ...s.env, PATH: [s.directory, upstreamDir].join(delimiter) } })
        .pin()
    ).path,
  ).toBe(await realpath(upstream));
});

it('SEA root denies known executable and sibling hardlink aliases', async () => {
  const s = await setup();
  const sea = join(s.root, nativeName('cadderd'));
  await copyFile(process.execPath, sea);
  const candidate = join(s.directory, nativeName('upstream'));
  await link(sea, candidate);
  await expect(
    s
      .resolver({
        explicitOverride: candidate,
        installation: { root: s.root, distribution: 'sea', executable: sea },
      })
      .pin(),
  ).rejects.toThrow('Cadder distribution');
  expect(
    await installationEntries({ root: s.root, distribution: 'sea', executable: sea }),
  ).toContain(join(s.root, nativeName('caddy')));
});

it('realistic physical npm local/global and SEA anchors do not use cwd/shared Node', async () => {
  const s = await setup();
  const entry = join(s.root, 'dist', 'cadderd.js');
  await writeFile(entry, '// entry');
  const moduleUrl = pathToFileURL(entry).href;
  expect(resolveInstallationRoot({ moduleUrl })).toBe(await realpath(s.root));
  expect(resolveInstallationRoot({ sea: true, executable: entry })).toBe(
    await realpath(join(s.root, 'dist')),
  );
  const entries = await installationEntries({ root: s.root, distribution: 'npm' });
  expect(entries).toContain(join(s.root, 'dist', 'caddy.js'));
  expect(entries).toContain(join(s.directory, 'project', 'node_modules', '.bin', 'caddy.cmd'));
  expect(entries).not.toContain(process.execPath);
});

it.skipIf(process.platform !== 'win32')(
  'native Scoop descriptor selects only its checked native target and denies Cadder alias targets',
  async () => {
    const s = await setup();
    const wrapper = await s.fake();
    const target = await s.fake('scoop-target');
    await writeFile(wrapper.replace(/\.exe$/, '.shim'), `path = "${target}"\nargs = "--ignored"`);
    expect((await s.resolver().pin()).path).toBe(await realpath(target));
    const sea = join(s.root, nativeName('cadderd'));
    await copyFile(process.execPath, sea);
    await writeFile(wrapper.replace(/\.exe$/, '.shim'), `path = "${sea}"`);
    await expect(
      s.resolver({ installation: { root: s.root, distribution: 'sea', executable: sea } }).pin(),
    ).rejects.toThrow('safe absolute PATH');
  },
);

it.skipIf(process.platform === 'win32')(
  'native Unix symlinks follow identity but scripts/non-executable files fail',
  async () => {
    const s = await setup();
    const first = await s.fake();
    const alias = join(s.directory, 'alias');
    await symlink(first, alias);
    expect(await sameFile(first, alias)).toBe(true);
    expect(await nativeExecutable(alias)).toBe(await realpath(first));
    const noExec = join(s.directory, 'not-executable');
    await writeFile(noExec, Buffer.from('7f454c46', 'hex'), { mode: 0o600 });
    await expect(nativeExecutable(noExec)).rejects.toThrow('not executable');
  },
);

it.skipIf(process.platform !== 'win32')(
  'native Windows final symlink/reparse executable is denied',
  async (context) => {
    const s = await setup();
    const path = await s.fake();
    const alias = join(s.directory, nativeName('symlink-alias'));
    try {
      await symlink(path, alias, 'file');
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'EPERM') {
        context.skip();
        return;
      }
      throw error;
    }
    expect(await sameFile(path, alias)).toBe(true);
    await expect(nativeExecutable(alias)).rejects.toThrow('reparse');
  },
);

it('realistic global npm prefix inventories are physical, not shared Node', async () => {
  const s = await setup();
  const globalRoot = join(s.directory, 'prefix', 'lib', 'node_modules', 'cadder');
  await mkdir(globalRoot, { recursive: true });
  await writeFile(
    join(globalRoot, 'package.json'),
    JSON.stringify({ name: 'cadder', bin: 'dist/caddy.js' }),
  );
  const entries = await installationEntries({ root: globalRoot, distribution: 'npm' });
  expect(entries).toContain(join(s.directory, 'prefix', 'bin', 'caddy'));
  expect(entries).toContain(join(globalRoot, 'dist', 'caddy.js'));
});

it('missing PATH and only relative PATH never select cwd', async () => {
  const s = await setup();
  await s.fake();
  const environment = { ...s.env };
  for (const key of Object.keys(environment))
    if (key.toLowerCase() === 'path') delete (environment as NodeJS.ProcessEnv)[key];
  await expect(s.resolver({ environment }).pin()).rejects.toThrow('PATH is not set');
  await expect(
    s.resolver({ environment: { ...environment, PATH: '.;relative' } }).pin(),
  ).rejects.toThrow('safe absolute PATH');
});

it('incompatible real native probes and missing modules fail without usable evidence', async () => {
  const s = await setup();
  const path = await s.fake('caddy', { version: 'v3.0.0' });
  await expect(s.resolver().pin()).rejects.toThrow('Unsupported Caddy');
  await writeFile(`${path}.json`, JSON.stringify({ modules: [] }));
  await expect(s.resolver().pin()).rejects.toThrow('missing required modules');
  expect(requiredCaddyModules).toHaveLength(19);
});

it.each(['version', 'list-modules'])(
  'probe %s failure is sticky and never tries another image',
  async (fail) => {
    const s = await setup();
    const marker = join(s.directory, 'commands');
    const path = await s.fake('caddy', { fail, marker });
    const resolver = s.resolver();
    await expect(resolver.pin()).rejects.toThrow('probe failed');
    await writeFile(`${path}.json`, JSON.stringify({ modules: requiredCaddyModules, marker }));
    await expect(resolver.pin()).rejects.toThrow('probe failed');
    const commands = (await readFile(marker, 'utf8')).trim().split('\n');
    expect(commands).toHaveLength(fail === 'version' ? 1 : 2);
  },
);

it.each(['v2.11.3', 'v3.0.0', 'v2.11.4-rc.1', 'garbage', ''])(
  'denies incompatible version %j',
  (version) => {
    expect(() => parseVersion(Buffer.from(version))).toThrow();
  },
);
it.each(['v2.11.4 h1:sum', '2.12.0-rc.1', 'v2.11.4+custom'])(
  'accepts retained compatible version %j',
  (version) => {
    expect(parseVersion(Buffer.from(version))).toBe(version.split(' ')[0]!.replace(/^v/, ''));
  },
);
it.each(['[]', '{}', 'not JSON', '[{"module_name":7}]'])(
  'denies missing/malformed module inventory %j',
  (body) => {
    expect(() => parseModules(Buffer.from(body))).toThrow();
  },
);

it('pin replacement and hardlink content mutation are detected, with no reselection', async () => {
  const s = await setup();
  const path = await s.fake();
  const resolver = s.resolver();
  const image = await resolver.pin();
  const alias = join(s.directory, nativeName('alias'));
  await link(path, alias);
  await writeFile(alias, Buffer.from('mutated'));
  await expect(image.verify()).rejects.toThrow('digest changed');
  await expect(image.run(['version'], { env: s.env })).rejects.toThrow('digest changed');
  expect(await resolver.pin()).toBe(image);
  const replacement = await s.fake('replacement');
  await rename(path, join(s.directory, nativeName('old')));
  await rename(replacement, path);
  await expect(image.verify()).rejects.toThrow('identity changed');
});

it.skipIf(process.platform !== 'win32')(
  'mutation during wrapper startup is rejected before native execution',
  async () => {
    const s = await setup();
    const path = await s.fake();
    const image = await PinnedCaddyImage.open(path, 'override');
    cleanup.push(() => image.close());
    const marker = join(s.directory, 'must-not-execute');
    const command = runOwnedCommand(path, ['marker', marker], {
      env: s.env,
      beforeSpawn: () => image.verify(),
      afterSpawn: () => image.verify(),
    });
    const mutation = new Promise<void>((resolve, reject) =>
      setTimeout(() => {
        void writeFile(path, Buffer.from('mutation during compilation')).then(
          () => resolve(),
          reject,
        );
      }, 100),
    );
    await mutation;
    await expect(command).rejects.toThrow('digest changed');
    await expect(readFile(marker)).rejects.toMatchObject({ code: 'ENOENT' });
  },
);

it.skipIf(process.platform !== 'win32')(
  'post-native-create pin mismatch kills the suspended owned Job before any execution',
  async () => {
    const s = await setup();
    const path = await s.fake();
    const image = await PinnedCaddyImage.open(path, 'override');
    cleanup.push(() => image.close());
    const marker = join(s.directory, 'must-not-execute');
    await expect(
      runOwnedCommand(path, ['marker', marker], {
        env: s.env,
        beforeSpawn: () => image.verify(),
        afterSpawn: async () => {
          throw new Error('post-native-create mismatch');
        },
      }),
    ).rejects.toThrow('post-native-create mismatch');
    await expect(readFile(marker)).rejects.toMatchObject({ code: 'ENOENT' });
  },
);
