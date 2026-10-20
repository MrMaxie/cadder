import { lstat, readFile, realpath } from 'node:fs/promises';
import { dirname, extname, join, parse, resolve, delimiter } from 'node:path';
import { isSea } from 'node:sea';
import { resolveInstallationRoot } from '../daemon/installation-root.ts';
import { OwnedCommandError } from '../platform/owned-command.ts';
import {
  defaultConfigurationPaths,
  selectConfiguration,
  type ConfigurationPaths,
  type SelectionSource,
} from './configuration.ts';
import { absoluteCaddyPath, nativeExecutable, sameFile } from './executable.ts';
import { PinnedCaddyImage } from './image.ts';

export type CaddyInstallation = Readonly<{
  root: string;
  distribution: 'npm' | 'sea';
  executable?: string;
}>;
export interface CaddyResolverOptions {
  explicitOverride?: string;
  configurationPaths?: ConfigurationPaths;
  installation?: CaddyInstallation;
  /** Daemon startup environment snapshot, not project/shim selectors. */
  environment?: NodeJS.ProcessEnv;
}

async function exists(path: string): Promise<boolean> {
  try {
    await lstat(path);
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false;
    throw error;
  }
}

export async function installationEntries(
  installation: CaddyInstallation,
): Promise<readonly string[]> {
  if (!absoluteCaddyPath(installation.root))
    throw new Error('Cadder installation root must be absolute.');
  const root = await realpath(installation.root);
  const entries: string[] = [];
  const names = ['caddy', 'cadder', 'cadderd', 'cadder-caddy'];
  if (installation.distribution === 'sea') {
    if (!installation.executable || !absoluteCaddyPath(installation.executable))
      throw new Error('SEA installation requires its native executable.');
    entries.push(installation.executable);
    for (const name of names)
      entries.push(join(root, process.platform === 'win32' ? `${name}.exe` : name));
  } else {
    let manifest: { name?: unknown; bin?: unknown };
    try {
      manifest = JSON.parse(await readFile(join(root, 'package.json'), 'utf8')) as typeof manifest;
    } catch (error) {
      throw new Error('Cannot read the physical Cadder npm manifest.', { cause: error });
    }
    if (manifest.name !== 'cadder')
      throw new Error('Portable npm anchor is not the physical Cadder package.');
    const bin = manifest.bin;
    if (typeof bin === 'string') entries.push(resolve(root, bin));
    else if (bin !== undefined) {
      if (!bin || typeof bin !== 'object' || Array.isArray(bin))
        throw new Error('Invalid Cadder npm bin inventory.');
      for (const target of Object.values(bin)) {
        if (typeof target !== 'string') throw new Error('Invalid Cadder npm bin target.');
        entries.push(resolve(root, target));
      }
    }
    // Local npm .bin and Windows/global npm wrappers sit outside the physical package.
    let directory = root;
    while (dirname(directory) !== directory) {
      if (parse(directory).base === 'node_modules') {
        const prefix = dirname(directory);
        const wrappers = [join(directory, '.bin'), prefix, resolve(prefix, '..', 'bin')];
        for (const wrapper of wrappers)
          for (const name of names)
            for (const suffix of ['', '.cmd', '.ps1', '.exe'])
              entries.push(join(wrapper, `${name}${suffix}`));
        break;
      }
      directory = dirname(directory);
    }
  }
  return entries;
}

async function rejectInstallationIdentity(
  candidate: string,
  entries: readonly string[],
): Promise<void> {
  for (const entry of entries) {
    if ((await exists(entry)) && (await sameFile(candidate, entry)))
      throw new Error('Selected Caddy aliases a Cadder distribution entry.');
  }
}

function assertCommand(command: string): void {
  if (!command || /[\s\\/\0:]/.test(command) || ['.', '..'].includes(command))
    throw new Error('Real-Caddy command must be a single program name without arguments.');
}

async function resolveNative(
  path: string,
  entries: readonly string[],
  scoop: boolean,
): Promise<string> {
  let canonical = await nativeExecutable(path);
  await rejectInstallationIdentity(canonical, entries);
  if (scoop && process.platform === 'win32') {
    const descriptor = canonical.slice(0, canonical.length - extname(canonical).length) + '.shim';
    if (await exists(descriptor)) {
      const contents = await readFile(descriptor, 'utf8');
      const target = contents
        .split(/\r?\n/)
        .map((line) => /^\s*path\s*=\s*"([^"\0]+)"\s*$/.exec(line)?.[1])
        .find((value) => value !== undefined);
      if (!target) throw new Error('Invalid native Scoop target descriptor.');
      canonical = await nativeExecutable(
        absoluteCaddyPath(target) ? target : resolve(dirname(descriptor), target),
      );
      await rejectInstallationIdentity(canonical, entries);
    }
  }
  return canonical;
}

export class RealCaddyResolver {
  private readonly options: CaddyResolverOptions;
  private readonly environment: NodeJS.ProcessEnv;
  private pinned: Promise<PinnedCaddyImage> | undefined;
  private readonly lifetime = new AbortController();
  private closed = false;

  constructor(options: CaddyResolverOptions = {}) {
    this.options = { ...options };
    this.environment = { ...(options.environment ?? process.env) };
  }

  /** Evidence failures stay cached; caller cancellation stops only that caller's wait. */
  async pin(signal?: AbortSignal): Promise<PinnedCaddyImage> {
    if (this.closed) throw new Error('Caddy resolver is closed.');
    if (signal?.aborted) throw new OwnedCommandError('abort', 'Owned command cancelled.');
    const pinned = (this.pinned ??= this.capture());
    if (!signal) return await pinned;
    return await new Promise<PinnedCaddyImage>((resolve, reject) => {
      const abort = () => reject(new OwnedCommandError('abort', 'Owned command cancelled.'));
      signal.addEventListener('abort', abort, { once: true });
      if (signal.aborted) abort();
      void pinned.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort));
    });
  }

  private async capture(): Promise<PinnedCaddyImage> {
    const sea = isSea();
    const installation = this.options.installation ?? {
      root: resolveInstallationRoot(),
      distribution: sea ? 'sea' : 'npm',
      ...(sea ? { executable: process.execPath } : {}),
    };
    const paths = this.options.configurationPaths ?? {
      ...(await defaultConfigurationPaths()),
      portable: join(await realpath(installation.root), 'cadder.toml'),
    };
    const entries = await installationEntries(installation);
    const selected = await selectConfiguration(paths, this.options.explicitOverride);
    let path: string;
    if (selected.selection.kind === 'path')
      path = await resolveNative(selected.selection.value, entries, false);
    else path = await this.onPath(selected.selection.value, entries);
    const image = await PinnedCaddyImage.open(path, selected.source);
    try {
      await image.probe({ env: this.environment, signal: this.lifetime.signal });
      return image;
    } catch (error) {
      await image.close();
      throw error;
    }
  }

  private async onPath(command: string, entries: readonly string[]): Promise<string> {
    assertCommand(command);
    const key = Object.keys(this.environment).find((name) =>
      process.platform === 'win32' ? name.toLowerCase() === 'path' : name === 'PATH',
    );
    const value = key === undefined ? undefined : this.environment[key];
    if (value === undefined) throw new Error('PATH is not set.');
    for (const directory of pathDirectories(value)) {
      if (!absoluteCaddyPath(directory)) continue;
      const candidate = join(
        directory,
        process.platform === 'win32' && !extname(command) ? `${command}.exe` : command,
      );
      if (!(await exists(candidate))) continue;
      try {
        return await resolveNative(candidate, entries, true);
      } catch {
        /* Unsafe PATH candidates cannot select an image; retain PATH order among safe files. */
      }
    }
    throw new Error('Trusted native Caddy command was not found on safe absolute PATH.');
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.lifetime.abort();
    if (this.pinned) {
      const image = await this.pinned.catch(() => undefined);
      await image?.close();
    }
  }
}

/** Windows split_paths semantics: quotes protect separators, not relative entries. */
function pathDirectories(value: string): string[] {
  if (process.platform !== 'win32') return value.split(delimiter);
  const directories = [''];
  let quoted = false;
  for (const character of value) {
    if (character === '"') quoted = !quoted;
    else if (character === ';' && !quoted) directories.push('');
    else directories[directories.length - 1] += character;
  }
  return directories;
}

export type { SelectionSource };
