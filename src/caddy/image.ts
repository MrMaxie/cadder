import { createHash } from 'node:crypto';
import { constants, type BigIntStats } from 'node:fs';
import { open, realpath, type FileHandle } from 'node:fs/promises';
import {
  runOwnedCommand,
  type OwnedCommandOptions,
  type OwnedCommandOutput,
} from '../platform/owned-command.ts';
import { fileIdentity, nativeExecutable } from './executable.ts';
import type { SelectionSource } from './configuration.ts';

export const minimumCaddyVersion = '2.11.4';
export const compatibilityProbeRevision = 'cadder-v1-probe-1';
export const requiredCaddyModules = Object.freeze([
  'http',
  'http.encoders.gzip',
  'http.encoders.zstd',
  'http.handlers.encode',
  'http.handlers.file_server',
  'http.handlers.headers',
  'http.handlers.reverse_proxy',
  'http.handlers.rewrite',
  'http.handlers.static_response',
  'http.handlers.subroute',
  'http.matchers.header',
  'http.matchers.host',
  'http.matchers.method',
  'http.matchers.path',
  'http.matchers.query',
  'http.reverse_proxy.transport.http',
  'pki',
  'tls',
  'tls.issuance.internal',
]);
export type CompatibilityEvidence = Readonly<{
  version: string;
  modules: readonly string[];
  probeRevision: string;
}>;

export function parseVersion(output: Buffer): string {
  const token = new TextDecoder('utf-8', { fatal: true })
    .decode(output)
    .trim()
    .split(/\s+/)[0]
    ?.replace(/^v/, '');
  const match =
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*)?$/.exec(
      token ?? '',
    );
  if (!match) throw new Error('Cannot parse Caddy semantic version.');
  const major = BigInt(match[1]!);
  const minor = BigInt(match[2]!);
  const patch = BigInt(match[3]!);
  // Baseline semver >=2.11.4, major<3: prerelease of the minimum is below it.
  if (
    [major, minor, patch].some((part) => part > 0xffffffffffffffffn) ||
    major !== 2n ||
    minor < 11n ||
    (minor === 11n && (patch < 4n || (patch === 4n && match[4] !== undefined)))
  )
    throw new Error(`Unsupported Caddy version; requires >=${minimumCaddyVersion}, major<3.`);
  return token!;
}

export function parseModules(output: Buffer): readonly string[] {
  const value: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(output));
  if (
    !Array.isArray(value) ||
    value.some(
      (item: unknown) =>
        !item ||
        typeof item !== 'object' ||
        typeof (item as { module_name?: unknown }).module_name !== 'string',
    )
  )
    throw new Error('Invalid Caddy module inventory.');
  const modules = [
    ...new Set(
      (value as { module_name: string }[])
        .map((item) => item.module_name)
        .filter((name) => name.trim()),
    ),
  ].sort();
  const missing = requiredCaddyModules.filter((name) => !modules.includes(name));
  if (missing.length) throw new Error(`Caddy is missing required modules: ${missing.join(', ')}.`);
  return Object.freeze(modules);
}

function assertImage(info: BigIntStats): void {
  if (
    !info.isFile() ||
    info.nlink === 0n ||
    (process.platform !== 'win32' && (info.mode & 0o111n) === 0n)
  )
    throw new Error('Pinned Caddy image is not an executable regular file.');
}
async function digest(file: FileHandle): Promise<string> {
  const before = await file.stat({ bigint: true });
  assertImage(before);
  const hash = createHash('sha256');
  const buffer = Buffer.alloc(64 * 1024);
  let position = 0;
  while (BigInt(position) < before.size) {
    const count = Number(before.size - BigInt(position));
    const { bytesRead } = await file.read(buffer, 0, Math.min(buffer.length, count), position);
    if (!bytesRead) throw new Error('Pinned Caddy image shrank while hashing.');
    hash.update(buffer.subarray(0, bytesRead));
    position += bytesRead;
  }
  const after = await file.stat({ bigint: true });
  if (
    before.size !== after.size ||
    before.ctimeNs !== after.ctimeNs ||
    before.mtimeNs !== after.mtimeNs
  )
    throw new Error('Pinned Caddy image changed while hashing.');
  return hash.digest('hex');
}

/** Open anchor + immutable native identity/content evidence; explicitly close after owned work. */
export class PinnedCaddyImage {
  private closed = false;
  private compatibility: CompatibilityEvidence | undefined;
  private active = 0;
  private constructor(
    readonly path: string,
    readonly source: SelectionSource,
    readonly identity: string,
    readonly sha256: string,
    private readonly anchor: FileHandle,
  ) {}

  static async open(path: string, source: SelectionSource): Promise<PinnedCaddyImage> {
    const canonical = await nativeExecutable(path);
    const anchor = await open(canonical, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    try {
      const identity = fileIdentity(await anchor.stat({ bigint: true }));
      const image = new PinnedCaddyImage(canonical, source, identity, await digest(anchor), anchor);
      await image.verify();
      return image;
    } catch (error) {
      await anchor.close();
      throw error;
    }
  }

  get evidence(): CompatibilityEvidence {
    if (!this.compatibility || this.closed)
      throw new Error('Caddy compatibility evidence is unavailable.');
    return this.compatibility;
  }

  async verify(): Promise<void> {
    if (this.closed) throw new Error('Pinned Caddy image is closed.');
    if ((await realpath(this.path)) !== this.path)
      throw new Error('Pinned Caddy canonical path changed.');
    const current = await open(this.path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    try {
      const info = await current.stat({ bigint: true });
      assertImage(info);
      if (fileIdentity(info) !== this.identity)
        throw new Error('Pinned Caddy file identity changed.');
      if ((await digest(current)) !== this.sha256)
        throw new Error('Pinned Caddy content digest changed.');
    } finally {
      await current.close();
    }
  }

  /** Stage 2 reuses this method; adapt/validate must specify 32 MiB per stream. */
  async run(
    args: readonly string[],
    options: Omit<OwnedCommandOptions, 'beforeSpawn' | 'afterSpawn'> = {},
  ): Promise<OwnedCommandOutput> {
    void this.evidence;
    return await this.execute(args, options);
  }

  private async execute(
    args: readonly string[],
    options: Omit<OwnedCommandOptions, 'beforeSpawn' | 'afterSpawn'>,
  ): Promise<OwnedCommandOutput> {
    await this.verify();
    this.active++;
    try {
      return await runOwnedCommand(this.path, args, {
        ...options,
        beforeSpawn: () => this.verify(),
        afterSpawn: () => this.verify(),
      });
    } finally {
      this.active--;
    }
  }

  async probe(options: Pick<OwnedCommandOptions, 'env' | 'signal'> = {}): Promise<void> {
    if (this.compatibility) {
      await this.verify();
      return;
    }
    const version = await this.execute(['version'], options);
    if (version.exitCode !== 0) throw new Error('Caddy version probe failed.');
    const parsedVersion = parseVersion(version.stdout);
    const modules = await this.execute(['list-modules', '--json'], options);
    if (modules.exitCode !== 0) throw new Error('Caddy modules probe failed.');
    this.compatibility = Object.freeze({
      version: parsedVersion,
      modules: parseModules(modules.stdout),
      probeRevision: compatibilityProbeRevision,
    });
    await this.verify();
  }

  async close(): Promise<void> {
    if (this.closed) return;
    if (this.active) throw new Error('Cannot close pinned Caddy while an owned command is active.');
    this.closed = true;
    await this.anchor.close();
  }
}
