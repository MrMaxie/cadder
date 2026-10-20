import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtemp, rm, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { packNode, npmCli } from './pack-node.ts';
import { fileURLToPath } from 'node:url';

const execute = promisify(execFile);
const packed = await packNode();
const consumer = await mkdtemp(join(tmpdir(), 'cadder-node-consumer-'));
const environment = { ...process.env, CADDER_RUNTIME_DIR: join(consumer, 'runtime') };
let command: string | undefined;
try {
  await execute(
    process.execPath,
    [
      await npmCli(),
      'install',
      '--prefix',
      consumer,
      '--ignore-scripts',
      '--no-audit',
      '--no-fund',
      '--package-lock=false',
      packed.archive,
    ],
    { timeout: 120000, maxBuffer: 1024 * 1024 },
  );
  command = join(consumer, 'node_modules/cadder/lib/entrypoints/cadder.js');
  if (process.argv[2])
    await execute(
      process.execPath,
      [
        fileURLToPath(new URL('./verify-product-journey.ts', import.meta.url)),
        join(consumer, 'node_modules/cadder'),
        process.argv[2],
        process.execPath,
      ],
      { timeout: 900000, maxBuffer: 1024 * 1024 },
    ).then((result) => process.stdout.write(result.stdout));
  const run = (args: string[]) =>
    execute(process.execPath, [command!, ...args], {
      cwd: consumer,
      env: environment,
      timeout: 45000,
      maxBuffer: 1024 * 1024,
    });
  assert.match((await run(['--version'])).stdout, /^2\.0\.0-rc\.1\s*$/);
  assert.match((await run([])).stderr, /Usage: cadder/);
  await assert.rejects(
    run(['status']),
    (error: unknown) => (error as { code?: unknown }).code === 3,
  );
  const prefix = join(consumer, 'global');
  await execute(
    process.execPath,
    [
      await npmCli(),
      'install',
      '--global',
      '--prefix',
      prefix,
      '--ignore-scripts',
      '--no-audit',
      '--no-fund',
      packed.archive,
    ],
    { timeout: 120000, maxBuffer: 1024 * 1024 },
  );
  const wrapperDirectory = process.platform === 'win32' ? prefix : join(prefix, 'bin');
  for (const name of ['cadder', 'cadderd', 'caddy']) {
    const arguments_ = [name === 'caddy' ? '--cadder-shim-info' : '--version'];
    for (const directory of [join(consumer, 'node_modules/.bin'), wrapperDirectory]) {
      const wrapper = join(directory, process.platform === 'win32' ? `${name}.cmd` : name);
      const result =
        process.platform === 'win32'
          ? await execute(
              join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/cmd.exe'),
              ['/d', '/s', '/c', `""${wrapper}" ${arguments_[0]}"`],
              { env: environment, windowsVerbatimArguments: true },
            )
          : await execute(wrapper, arguments_, { env: environment });
      assert.match(result.stdout, /2\.0\.0-rc\.1/);
    }
  }
  const globalPackage = join(
    prefix,
    process.platform === 'win32' ? 'node_modules/cadder' : 'lib/node_modules/cadder',
  );
  const installed = JSON.parse(await readFile(join(globalPackage, 'package.json'), 'utf8')) as {
    scripts?: unknown;
    optionalDependencies?: unknown;
  };
  assert.equal(installed.scripts, undefined);
  assert.equal(installed.optionalDependencies, undefined);
  assert.match((await run(['daemon', 'start'])).stdout, /running/);
  assert.match((await run(['status'])).stdout, /cadderd {2}running/);
  assert.match((await run(['projects', 'list'])).stdout, /No projects registered/);
  assert.match((await run(['logs', 'runtime', '--limit', '1'])).stdout, /Daemon started/);
  await assert.rejects(
    run(['logs', 'runtime', '--tail']),
    (error: unknown) => (error as { code?: unknown }).code === 2,
  );
  assert.match((await run(['daemon', 'stop'])).stdout, /stopped/);
  await assert.rejects(
    run(['status']),
    (error: unknown) => (error as { code?: unknown }).code === 3,
  );
  process.stdout.write(
    JSON.stringify({
      archive: packed.archive,
      result: 'PASS',
      scope:
        'compiled npm local lifecycle, local/global three-bin wrappers, actual SQLite worker and CLI, no TS loader',
    }) + '\n',
  );
} catch (error) {
  const failure = error as { message?: string; stdout?: string; stderr?: string };
  process.stderr.write(
    [failure.message, failure.stdout, failure.stderr].filter(Boolean).join('\n') + '\n',
  );
  throw error;
} finally {
  if (command)
    await execute(process.execPath, [command, 'daemon', 'stop'], {
      env: environment,
      timeout: 20000,
    }).catch(() => {});
  await rm(consumer, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }).catch(
    (error: unknown) => {
      process.stderr.write(
        `Consumer cleanup failed: ${error instanceof Error ? error.message : 'unknown failure'}\n`,
      );
      process.exitCode = 1;
    },
  );
}
