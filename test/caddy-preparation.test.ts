import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';
import { dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { join, normalize } from 'node:path';
import { build } from 'esbuild';
import { afterEach, expect, it, vi } from 'vitest';
import { CaddyConfigAdapter, type CaddyAdaptInput } from '../src/caddy/adapter.ts';
import { CaddyConfigValidator } from '../src/caddy/validation.ts';
import {
  configHash,
  maxCaddyConfigurationBytes,
  caddyConfigurationTimeoutMs,
  configurationCommandOptions,
  preparationFailure,
} from '../src/caddy/preparation.ts';
import { RealCaddyResolver } from '../src/caddy/resolver.ts';
import { requiredCaddyModules } from '../src/caddy/image.ts';
import type { CaddyConfig, CaddyPort } from '../src/contracts/ports.ts';
import { caddyfileExamples } from './compatibility/catalog.ts';

const cleanups: (() => Promise<unknown>)[] = [];
afterEach(async () => {
  for (const close of cleanups.splice(0).reverse()) await close();
});
const candidate = (body = '{}'): CaddyConfig => ({
  effectiveConfigHash: configHash(body),
  adaptedConfig: { format: 'json', body },
});
async function setup() {
  const directory = await mkdtemp(join(tmpdir(), 'cadder-preparation-test-'));
  cleanups.push(() => rm(directory, { recursive: true, force: true }));
  const root = join(directory, 'node_modules', 'cadder');
  await mkdir(root, { recursive: true });
  await writeFile(
    join(root, 'package.json'),
    JSON.stringify({ name: 'cadder', bin: { caddy: 'dist/caddy.js' } }),
  );
  const imagePath = join(directory, process.platform === 'win32' ? 'real-caddy.exe' : 'real-caddy');
  await copyFile(process.execPath, imagePath);
  const preload = join(directory, 'preload.cjs');
  await build({
    entryPoints: ['test/fixtures/caddy-preparation-preload.ts'],
    bundle: true,
    format: 'cjs',
    platform: 'node',
    outfile: preload,
  });
  const env = { ...process.env, NODE_OPTIONS: `--require=${JSON.stringify(preload)}` };
  await writeFile(join(directory, 'Caddyfile'), caddyfileExamples[0]!.source);
  const log = join(directory, 'commands.jsonl');
  const settings = { modules: requiredCaddyModules, log };
  async function behavior(value: object) {
    await writeFile(`${imagePath}.json`, JSON.stringify({ ...settings, ...value }));
  }
  await behavior({});
  const resolver = new RealCaddyResolver({
    explicitOverride: imagePath,
    environment: env,
    installation: { root, distribution: 'npm' },
    configurationPaths: {
      portable: join(root, 'cadder.toml'),
      user: join(directory, 'user.toml'),
      system: join(directory, 'system.toml'),
    },
  });
  cleanups.push(() => resolver.close());
  const input: CaddyAdaptInput = {
    sourceWorkingDirectory: { raw: directory, canonical: null },
    sourceConfigPath: { raw: 'Caddyfile', canonical: null },
    shimRun: null,
  };
  async function records() {
    return (await readFile(log, 'utf8'))
      .trim()
      .split('\n')
      .map(
        (line) =>
          JSON.parse(line) as {
            command: string;
            args: string[];
            cwd: string;
            image: string;
            path: string;
            body?: string;
            sourceBody?: string;
            pid: number;
          },
      );
  }
  return {
    directory,
    imagePath,
    env,
    input,
    resolver,
    behavior,
    records,
    adapter: new CaddyConfigAdapter(resolver, { env }),
    validator: new CaddyConfigValidator(resolver, { env }),
  };
}

it('adapts with raw relative path/source cwd and retained caddyfile default into complete JSON/hash', async () => {
  const s = await setup();
  const result = await s.adapter.adapt(s.input);
  expect(result).toEqual({ ok: true, value: candidate('{"apps":{"http":{"servers":{}}}}\n') });
  const [record] = await s.records();
  expect(record!.args).toEqual(['adapt', '--config', 'Caddyfile', '--adapter', 'caddyfile']);
  expect(normalize(record!.cwd)).toBe(normalize(s.directory));
  expect(normalize(record!.image)).toBe(normalize(s.imagePath));
  expect(record!.sourceBody).toBe(caddyfileExamples[0]!.source);
});

it('prefers canonical config/cwd and preserves adapter metadata as direct argv', async () => {
  const s = await setup();
  const cwd = join(s.directory, 'source project');
  await mkdir(cwd);
  const path = join(cwd, 'config space;literal.json');
  await writeFile(path, '{}');
  const input = {
    ...s.input,
    sourceWorkingDirectory: { raw: 'unusable-relative', canonical: cwd },
    sourceConfigPath: { raw: 'wrong', canonical: path },
    shimRun: {
      adapter: 'custom adapter',
      rawArguments: ['--real-caddy', 'wrong'],
      commandLine: 'ignored',
    },
  };
  expect((await s.adapter.adapt(input)).ok).toBe(true);
  const [record] = await s.records();
  expect(record!.args).toEqual(['adapt', '--config', path, '--adapter', 'custom adapter']);
  expect(normalize(record!.cwd)).toBe(normalize(cwd));
});

it.each(['malformed', 'invalid-utf8', 'invalid-stderr', 'exit'])(
  'adapt refuses %s output without a partial candidate',
  async (mode) => {
    const s = await setup();
    await s.behavior({ adapt: { mode, body: 'x'.repeat(20000) } });
    const result = await s.adapter.adapt(s.input);
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error.message.length).toBeLessThanOrEqual(4096);
    expect(result).not.toHaveProperty('value');
  },
);

it('rejects unsafe source metadata before spawning', async () => {
  const s = await setup();
  for (const input of [
    { ...s.input, sourceWorkingDirectory: { raw: '.', canonical: null } },
    { ...s.input, sourceConfigPath: { raw: '', canonical: null } },
    { ...s.input, sourceConfigPath: { raw: 'good', canonical: 'relative' } },
    { ...s.input, sourceConfigPath: { raw: 'bad\0path', canonical: null } },
  ])
    expect((await s.adapter.adapt(input)).ok).toBe(false);
  await expect(readFile(join(s.directory, 'commands.jsonl'))).rejects.toMatchObject({
    code: 'ENOENT',
  });
});

it('validates the complete JSON carrier through the existing validate-only port and removes staging', async () => {
  const s = await setup();
  const validator: Pick<CaddyPort, 'validate'> = s.validator;
  const config = candidate('{"apps":{}}\n');
  expect(await validator.validate(config)).toEqual({
    ok: true,
    value: { effectiveConfigHash: config.effectiveConfigHash, diagnostics: [] },
  });
  expect(validator).not.toHaveProperty('apply');
  const [record] = await s.records();
  expect(record!.args).toEqual(['validate', '--config', record!.path]);
  expect(record!.body).toBe(config.adaptedConfig.body);
  expect(record!.path).not.toContain(process.cwd());
  await expect(readFile(record!.path)).rejects.toMatchObject({ code: 'ENOENT' });
});

it('validation snapshots the original complete body/hash across asynchronous pinning', async () => {
  const s = await setup();
  const config = {
    effectiveConfigHash: configHash('{}'),
    adaptedConfig: { format: 'json' as const, body: '{}' },
  };
  const command = s.validator.validate(config);
  config.adaptedConfig.body = '{"changed":true}';
  config.effectiveConfigHash = configHash(config.adaptedConfig.body);
  expect(await command).toEqual({
    ok: true,
    value: { effectiveConfigHash: configHash('{}'), diagnostics: [] },
  });
  const [record] = await s.records();
  expect(record!.body).toBe('{}');
});

it.each(['invalid-utf8', 'invalid-stderr', 'exit'])(
  'validation refuses %s and removes owned staging',
  async (mode) => {
    const s = await setup();
    await s.behavior({ validate: { mode, body: 'rejected '.repeat(5000) } });
    const result = await s.validator.validate(candidate());
    expect(result.ok).toBe(false);
    expect(result).not.toHaveProperty('value');
    if (!result.ok) expect(result.error.message.length).toBeLessThanOrEqual(4096);
    const [record] = await s.records();
    await expect(readFile(record!.path)).rejects.toMatchObject({ code: 'ENOENT' });
  },
);

it.each([
  ['adapt', 'stdout', false],
  ['adapt', 'stderr', false],
  ['validate', 'stdout', false],
  ['validate', 'stderr', false],
  ['adapt', 'stdout', true],
  ['adapt', 'stderr', true],
  ['validate', 'stdout', true],
  ['validate', 'stderr', true],
] as const)(
  '%s %s enforces exact 32-MiB stream bound (overflow=%s)',
  async (operation, stream, overflow) => {
    const s = await setup();
    await s.behavior({
      [operation]: { mode: stream, bytes: maxCaddyConfigurationBytes + Number(overflow) },
    });
    const result =
      operation === 'adapt'
        ? await s.adapter.adapt(s.input)
        : await s.validator.validate(candidate());
    expect(result.ok).toBe(!overflow);
    if (overflow)
      expect(result).toMatchObject({ ok: false, error: { code: 'caddy_command_overflow' } });
    if (operation === 'validate') {
      const [record] = await s.records();
      await expect(readFile(record!.path)).rejects.toMatchObject({ code: 'ENOENT' });
    }
  },
  30000,
);

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
async function until<T>(action: () => Promise<T>): Promise<T> {
  const deadline = Date.now() + 8000;
  for (;;) {
    try {
      return await action();
    } catch (error) {
      if (Date.now() >= deadline) throw error;
      await delay(20);
    }
  }
}

it.each(['adapt', 'validate'] as const)(
  '%s timeout cleans inherited-pipe descendants and unrelated child survives',
  async (operation) => {
    const s = await setup();
    await s.resolver.pin();
    const marker = join(s.directory, 'descendant.pid');
    await s.behavior({ [operation]: { mode: 'tree', marker } });
    const unrelated = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {
      stdio: 'ignore',
    });
    let descendant: number | undefined;
    try {
      const result =
        operation === 'adapt'
          ? await new CaddyConfigAdapter(s.resolver, { env: s.env, timeoutMs: 5000 }).adapt(s.input)
          : await new CaddyConfigValidator(s.resolver, { env: s.env, timeoutMs: 5000 }).validate(
              candidate(),
            );
      descendant = Number(await readFile(marker, 'utf8'));
      expect(result).toMatchObject({ ok: false, error: { code: 'caddy_command_timeout' } });
      expect(alive(descendant)).toBe(false);
      expect(alive(unrelated.pid!)).toBe(true);
      if (operation === 'validate') {
        const [record] = await s.records();
        await expect(readFile(record!.path)).rejects.toMatchObject({ code: 'ENOENT' });
      }
    } finally {
      if (descendant && alive(descendant)) process.kill(descendant, 'SIGKILL');
      unrelated.kill('SIGKILL');
      await new Promise<void>((resolve) => {
        if (unrelated.exitCode !== null) resolve();
        else unrelated.once('exit', () => resolve());
      });
    }
  },
  30000,
);

it.each(['adapt', 'validate'] as const)(
  '%s abort kills ready descendants and closes inherited pipes',
  async (operation) => {
    const s = await setup();
    await s.resolver.pin();
    const marker = join(s.directory, 'descendant.pid');
    await s.behavior({ [operation]: { mode: 'tree', marker } });
    const controller = new AbortController();
    const command =
      operation === 'adapt'
        ? s.adapter.adapt(s.input, controller.signal)
        : s.validator.validate(candidate(), controller.signal);
    const descendant = await until(async () => Number(await readFile(marker, 'utf8')));
    expect(alive(descendant)).toBe(true);
    controller.abort();
    expect(await command).toMatchObject({ ok: false, error: { code: 'caddy_command_abort' } });
    expect(alive(descendant)).toBe(false);
    if (operation === 'validate') {
      const [record] = await s.records();
      await expect(readFile(record!.path)).rejects.toMatchObject({ code: 'ENOENT' });
    }
  },
  30000,
);

it('exited validation parent leaves no orphan, inherited pipes or staging', async () => {
  const s = await setup();
  const marker = join(s.directory, 'orphan.pid');
  await s.behavior({ validate: { mode: 'orphan', marker } });
  expect((await s.validator.validate(candidate())).ok).toBe(true);
  expect(alive(Number(await readFile(marker, 'utf8')))).toBe(false);
  const [record] = await s.records();
  await expect(readFile(record!.path)).rejects.toMatchObject({ code: 'ENOENT' });
});

it('cancellation before initial pin creates no native command or staging', async () => {
  const s = await setup();
  const signal = AbortSignal.abort();
  expect(await s.adapter.adapt(s.input, signal)).toMatchObject({
    ok: false,
    error: { code: 'caddy_command_abort' },
  });
  expect(await s.validator.validate(candidate(), signal)).toMatchObject({
    ok: false,
    error: { code: 'caddy_command_abort' },
  });
  await expect(readFile(join(s.directory, 'commands.jsonl'))).rejects.toMatchObject({
    code: 'ENOENT',
  });
});

it.each(['adapt', 'validate'] as const)(
  '%s native spawn failure refuses partial results and removes staging',
  async (operation) => {
    const s = await setup();
    const image = await s.resolver.pin();
    const original = image.verify.bind(image);
    let count = 0;
    const before = (await readdir(tmpdir())).filter((name) =>
      name.startsWith('cadder-caddy-validation-'),
    );
    const spy = vi.spyOn(image, 'verify').mockImplementation(async () => {
      count++;
      await original();
      if (count === 2) await rename(s.imagePath, join(s.directory, 'moved-before-create'));
    });
    try {
      const result =
        operation === 'adapt'
          ? await s.adapter.adapt(s.input)
          : await s.validator.validate(candidate());
      expect(result).toMatchObject({ ok: false, error: { code: 'caddy_command_spawn' } });
      expect(
        (await readdir(tmpdir())).filter((name) => name.startsWith('cadder-caddy-validation-')),
      ).toEqual(before);
      await expect(readFile(join(s.directory, 'commands.jsonl'))).rejects.toMatchObject({
        code: 'ENOENT',
      });
    } finally {
      spy.mockRestore();
    }
  },
);

it('preparation limits cannot be raised above the retained command policy', () => {
  expect(
    configurationCommandOptions({ timeoutMs: 60000, maxStreamBytes: 64 * 1024 * 1024 }),
  ).toMatchObject({ timeoutMs: 30000, maxStreamBytes: 32 * 1024 * 1024 });
  expect(
    preparationFailure(new AggregateError([], 'unbounded private internals'), 'validate'),
  ).toMatchObject({
    code: 'caddy_cleanup_failed',
    message: 'Caddy command or staging cleanup failed.',
  });
});

it('pin mutation prevents adaptation/validation without reselection or leftover staging', async () => {
  const s = await setup();
  await s.resolver.pin();
  await writeFile(s.imagePath, Buffer.from('mutated image'));
  expect((await s.adapter.adapt(s.input)).ok).toBe(false);
  const before = (await readdir(tmpdir())).filter((name) =>
    name.startsWith('cadder-caddy-validation-'),
  );
  expect((await s.validator.validate(candidate())).ok).toBe(false);
  expect(
    (await readdir(tmpdir())).filter((name) => name.startsWith('cadder-caddy-validation-')),
  ).toEqual(before);
  await expect(readFile(join(s.directory, 'commands.jsonl'))).rejects.toMatchObject({
    code: 'ENOENT',
  });
});

it.skipIf(process.platform !== 'win32')(
  'actual suspended native creation mismatch rejects validation and cleans staging before execution',
  async () => {
    const s = await setup();
    const image = await s.resolver.pin();
    const original = image.verify.bind(image);
    let count = 0;
    let replaced = false;
    const replacement = join(s.directory, 'replacement.exe');
    await copyFile(process.execPath, replacement);
    const before = (await readdir(tmpdir())).filter((name) =>
      name.startsWith('cadder-caddy-validation-'),
    );
    const spy = vi.spyOn(image, 'verify').mockImplementation(async () => {
      count++;
      if (count === 3) {
        await rename(s.imagePath, join(s.directory, 'old.exe'));
        await rename(replacement, s.imagePath);
        replaced = true;
      }
      await original();
    });
    try {
      expect((await s.validator.validate(candidate())).ok).toBe(false);
      expect(count).toBe(3);
      expect(replaced).toBe(true);
      await expect(readFile(join(s.directory, 'commands.jsonl'))).rejects.toMatchObject({
        code: 'ENOENT',
      });
      expect(
        (await readdir(tmpdir())).filter((name) => name.startsWith('cadder-caddy-validation-')),
      ).toEqual(before);
    } finally {
      spy.mockRestore();
    }
  },
);

it.skipIf(process.platform !== 'win32')(
  'native staging cleanup failure cannot return validated success',
  async () => {
    const s = await setup();
    await s.resolver.pin();
    const release = join(s.directory, 'release');
    await s.behavior({ validate: { mode: 'wait', release } });
    const command = s.validator.validate(candidate());
    const [record] = await until(() => s.records());
    cleanups.push(() => rm(dirname(record!.path), { recursive: true, force: true }));
    const script = `$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$stream=[IO.File]::Open($env:CADDER_TEST_LOCK_FILE,'Open','Read','None');[Console]::Out.WriteLine('locked');[Console]::Out.Flush();[Console]::In.ReadLine() | Out-Null;$stream.Dispose()`;
    const locker = spawn(
      join(
        process.env.SystemRoot ?? 'C:\\Windows',
        'System32',
        'WindowsPowerShell',
        'v1.0',
        'powershell.exe',
      ),
      [
        '-NoProfile',
        '-NonInteractive',
        '-EncodedCommand',
        Buffer.from(script, 'utf16le').toString('base64'),
      ],
      { env: { ...process.env, CADDER_TEST_LOCK_FILE: record!.path }, stdio: 'pipe' },
    );
    try {
      await new Promise<void>((resolve, reject) => {
        locker.once('error', reject);
        locker.stdout.once('data', () => resolve());
      });
      await writeFile(release, 'go');
      expect(await command).toMatchObject({ ok: false, error: { code: 'caddy_cleanup_failed' } });
    } finally {
      locker.stdin.end();
      await new Promise<void>((resolve) => {
        if (locker.exitCode !== null) resolve();
        else locker.once('exit', () => resolve());
      });
    }
    expect(await readFile(record!.path, 'utf8')).toBe('{}');
  },
  30000,
);

it('invalid JSON/hash/encoding/oversize carriers never reach Caddy', async () => {
  const s = await setup();
  for (const config of [
    candidate('{'),
    { ...candidate(), effectiveConfigHash: 'wrong' },
    candidate('"\ud800"'),
    candidate(' '.repeat(maxCaddyConfigurationBytes + 1)),
    {
      ...candidate(),
      adaptedConfig: { format: 'caddyfile', body: '{}' },
    } as unknown as CaddyConfig,
  ])
    expect(await s.validator.validate(config)).toMatchObject({
      ok: false,
      error: { code: 'caddy_candidate_invalid' },
    });
  await expect(readFile(join(s.directory, 'commands.jsonl'))).rejects.toMatchObject({
    code: 'ENOENT',
  });
  expect(caddyConfigurationTimeoutMs).toBe(30000);
  expect(maxCaddyConfigurationBytes).toBe(32 * 1024 * 1024);
});
