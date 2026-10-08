import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  configurationLocations,
  defaultConfigurationPaths,
  parseConfiguration,
  readConfiguration,
  selectConfiguration,
  type ConfigurationPaths,
} from '../src/caddy/configuration.ts';
import { configurationCases, caddyResolutionPrecedence } from './compatibility/catalog.ts';

const directories: string[] = [];
afterEach(async () => {
  await Promise.all(
    directories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});
async function paths(): Promise<ConfigurationPaths> {
  const directory = await mkdtemp(join(tmpdir(), 'cadder-caddy-config-'));
  directories.push(directory);
  return Object.fromEntries(
    ['portable', 'user', 'system'].map((source) => [source, join(directory, `${source}.toml`)]),
  ) as unknown as ConfigurationPaths;
}

describe('released configuration contract', () => {
  it.each(configurationCases)('$result', ({ toml, result }) => {
    if (result.startsWith('reject:')) expect(() => parseConfiguration(toml)).toThrow();
    else {
      const selected = parseConfiguration(toml)!;
      expect(`${selected.kind}:${selected.value}`).toBe(result);
    }
  });
  it.each(['', '[caddy]', '# empty'])('defaults/empty selection: %j', (toml) => {
    expect(parseConfiguration(toml)).toBeUndefined();
  });
  it.each(['caddy = 1', '[caddy]\nreal_path = 42', 'unknown = true', '[caddy]\nreal_command = []'])(
    'strict types and unknown top-level fields: %j',
    (toml) => expect(() => parseConfiguration(toml)).toThrow(),
  );
  it('does not reinterpret an empty configured string as an absent selector', () => {
    expect(parseConfiguration('[caddy]\nreal_command = ""')).toEqual({
      kind: 'command',
      value: '',
    });
  });
  it('preserves all five source priorities', async () => {
    expect(caddyResolutionPrecedence).toHaveLength(5);
    const input = await paths();
    for (const source of ['portable', 'user', 'system'] as const)
      await writeFile(input[source], `[caddy]\nreal_command = "${source}"`);
    expect((await selectConfiguration(input, process.execPath)).source).toBe('override');
    for (const source of ['portable', 'user', 'system'] as const) {
      expect(await selectConfiguration(input)).toEqual({
        selection: { kind: 'command', value: source },
        source,
      });
      await writeFile(input[source], '[caddy]');
    }
    expect((await selectConfiguration(input)).source).toBe('path');
  });
  it.each(['[caddy', '[caddy]\nunknown = true', '[caddy]\nreal_path = "x"\nreal_command = "x"'])(
    'invalid higher priority never falls back: %j',
    async (toml) => {
      const input = await paths();
      await writeFile(input.user, toml);
      await writeFile(input.system, '[caddy]\nreal_command = "caddy"');
      await expect(selectConfiguration(input)).rejects.toThrow();
    },
  );
  it('rejects relative override without inspecting lower configuration', async () => {
    await expect(selectConfiguration(await paths(), 'caddy')).rejects.toThrow('absolute');
  });
  it('invalid UTF-8 configuration fails rather than replacing bytes', async () => {
    const input = await paths();
    await writeFile(input.portable, Buffer.from([0xff]));
    await expect(selectConfiguration(input)).rejects.toThrow();
  });
  it('missing files fall through but directories and relative sources fail', async () => {
    const input = await paths();
    expect((await selectConfiguration(input)).source).toBe('path');
    await mkdir(input.portable);
    await expect(selectConfiguration(input)).rejects.toThrow('regular file');
    await expect(readConfiguration('cadder.toml')).rejects.toThrow('absolute');
  });
});

describe('OS-modeled locations (not native filesystem proof)', () => {
  it('Linux preserves absolute XDG and default locations, never relative XDG', () => {
    const input = {
      platform: 'linux' as const,
      installationRoot: '/opt/cadder',
      home: '/home/user',
    };
    expect(configurationLocations(input)).toEqual({
      portable: '/opt/cadder/cadder.toml',
      user: '/home/user/.config/cadder/cadder.toml',
      system: '/etc/cadder/cadder.toml',
    });
    expect(configurationLocations({ ...input, xdgConfigHome: '/config' }).user).toBe(
      '/config/cadder/cadder.toml',
    );
    expect(configurationLocations({ ...input, xdgConfigHome: 'project' }).user).toBe(
      '/home/user/.config/cadder/cadder.toml',
    );
  });
  it('macOS preserves the released ProjectDirs spelling', () => {
    expect(
      configurationLocations({
        platform: 'darwin',
        installationRoot: '/opt/cadder',
        home: '/Users/test',
      }).user,
    ).toBe('/Users/test/Library/Application Support/dev.Cadder.Cadder/cadder.toml');
  });
  it('Windows requires known folders rather than ambient APPDATA/PROGRAMDATA', () => {
    const input = {
      platform: 'win32' as const,
      installationRoot: 'C:\\apps\\cadder',
      home: 'C:\\Users\\test',
    };
    expect(() => configurationLocations(input)).toThrow('known folders');
    expect(
      configurationLocations({
        ...input,
        knownFolders: { roamingAppData: 'C:\\roaming', programData: 'C:\\policy' },
      }),
    ).toEqual({
      portable: 'C:\\apps\\cadder\\cadder.toml',
      user: 'C:\\roaming\\Cadder\\Cadder\\config\\cadder.toml',
      system: 'C:\\policy\\Cadder\\cadder.toml',
    });
  });
  it('relative installation/home anchors fail', () => {
    expect(() =>
      configurationLocations({ platform: 'linux', installationRoot: '.', home: '/home/test' }),
    ).toThrow('absolute');
  });
});

it.skipIf(process.platform !== 'win32')(
  'native Windows defaults ignore spoofed PROGRAMDATA',
  async () => {
    const before = await defaultConfigurationPaths();
    const previous = process.env.PROGRAMDATA;
    try {
      process.env.PROGRAMDATA = 'C:\\untrusted-policy';
      expect((await defaultConfigurationPaths()).system).toBe(before.system);
    } finally {
      if (previous === undefined) delete process.env.PROGRAMDATA;
      else process.env.PROGRAMDATA = previous;
    }
  },
);
