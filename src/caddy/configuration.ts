import { constants } from 'node:fs';
import { lstat, open, realpath } from 'node:fs/promises';
import { homedir } from 'node:os';
import { posix, win32 } from 'node:path';
import { parse } from 'smol-toml';
import { z } from 'zod';
import { resolveInstallationRoot } from '../daemon/installation-root.ts';
import { powershell } from '../platform/powershell.ts';
import { absoluteCaddyPath } from './executable.ts';
import { assertNotReparsePoint } from './executable.ts';

export type CaddySelection = Readonly<{ kind: 'command' | 'path'; value: string }>;
export type ConfigurationPaths = Readonly<{ portable: string; user: string; system: string }>;

const selectors = z.strictObject({
  real_command: z.string().optional(),
  real_path: z.string().optional(),
});
const configuration = z.strictObject({ caddy: selectors.optional() });

/** Parse the released selectors only; source safety and executable checks are separate. */
export function parseConfiguration(contents: string): CaddySelection | undefined {
  const { caddy } = configuration.parse(parse(contents));
  if (!caddy) return undefined;
  if (caddy.real_command !== undefined && caddy.real_path !== undefined)
    throw new Error('caddy.real_command and caddy.real_path cannot both be configured.');
  if (caddy.real_command !== undefined) return { kind: 'command', value: caddy.real_command };
  if (caddy.real_path !== undefined) return { kind: 'path', value: caddy.real_path };
  return undefined;
}

/** Only absence falls through. Broken links, unsafe files and invalid TOML fail closed. */
export async function readConfiguration(path: string): Promise<CaddySelection | undefined> {
  if (!absoluteCaddyPath(path)) throw new Error('Cadder configuration must use an absolute path.');
  try {
    await lstat(path);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined;
    throw error;
  }
  if (process.platform === 'win32') await assertNotReparsePoint(path);
  const canonical = await realpath(path);
  const file = await open(canonical, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    if (!(await file.stat()).isFile())
      throw new Error('Cadder configuration must be a regular file.');
    return parseConfiguration(
      new TextDecoder('utf-8', { fatal: true }).decode(await file.readFile()),
    );
  } finally {
    await file.close();
  }
}

export interface ConfigurationLocationInput {
  platform: NodeJS.Platform;
  installationRoot: string;
  home: string;
  xdgConfigHome?: string | undefined;
  knownFolders?: Readonly<{ roamingAppData: string; programData: string }> | undefined;
}

/** Pure OS location calculation; modeled locations are not native platform evidence. */
export function configurationLocations(input: ConfigurationLocationInput): ConfigurationPaths {
  const path = input.platform === 'win32' ? win32 : posix;
  if (
    !absoluteCaddyPath(input.installationRoot, input.platform) ||
    !absoluteCaddyPath(input.home, input.platform)
  )
    throw new Error('Configuration roots must be absolute.');
  const portable = path.join(input.installationRoot, 'cadder.toml');
  if (input.platform === 'win32') {
    const folders = input.knownFolders;
    if (
      !folders ||
      !absoluteCaddyPath(folders.roamingAppData, 'win32') ||
      !absoluteCaddyPath(folders.programData, 'win32')
    )
      throw new Error('Windows configuration requires OS known folders.');
    return {
      portable,
      user: win32.join(folders.roamingAppData, 'Cadder', 'Cadder', 'config', 'cadder.toml'),
      system: win32.join(folders.programData, 'Cadder', 'cadder.toml'),
    };
  }
  if (input.platform === 'darwin')
    return {
      portable,
      user: posix.join(
        input.home,
        'Library',
        'Application Support',
        'dev.Cadder.Cadder',
        'cadder.toml',
      ),
      system: '/Library/Application Support/Cadder/cadder.toml',
    };
  return {
    portable,
    user: posix.join(
      input.xdgConfigHome && posix.isAbsolute(input.xdgConfigHome)
        ? input.xdgConfigHome
        : posix.join(input.home, '.config'),
      'cadder',
      'cadder.toml',
    ),
    system: '/etc/cadder/cadder.toml',
  };
}

/** Physical npm package / SEA directory, never cwd, argv wrappers or shared Node. */
export async function defaultConfigurationPaths(): Promise<ConfigurationPaths> {
  const knownFolders =
    process.platform === 'win32'
      ? (JSON.parse(
          await powershell(`
            @{ roamingAppData = [Environment]::GetFolderPath('ApplicationData');
               programData = [Environment]::GetFolderPath('CommonApplicationData') } | ConvertTo-Json -Compress
          `),
        ) as { roamingAppData: string; programData: string })
      : undefined;
  return configurationLocations({
    platform: process.platform,
    installationRoot: resolveInstallationRoot(),
    home: homedir(),
    xdgConfigHome: process.env.XDG_CONFIG_HOME,
    knownFolders,
  });
}

export type SelectionSource = 'override' | 'portable' | 'user' | 'system' | 'path';
export type SelectedConfiguration = Readonly<{
  selection: CaddySelection;
  source: SelectionSource;
}>;

/** Daemon-owned inputs only. No project, shim argument or environment selector is accepted. */
export async function selectConfiguration(
  paths: ConfigurationPaths,
  explicitOverride?: string,
): Promise<SelectedConfiguration> {
  if (explicitOverride !== undefined) {
    if (!absoluteCaddyPath(explicitOverride))
      throw new Error('Real-Caddy override must be absolute.');
    return { selection: { kind: 'path', value: explicitOverride }, source: 'override' };
  }
  for (const source of ['portable', 'user', 'system'] as const) {
    const selection = await readConfiguration(paths[source]);
    if (selection) return { selection, source };
  }
  return { selection: { kind: 'command', value: 'caddy' }, source: 'path' };
}
