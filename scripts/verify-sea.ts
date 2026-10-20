import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createHash } from 'node:crypto';
import { lstat, mkdir, mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const execute = promisify(execFile);
let stage = resolve(process.argv[2] ?? '');
assert.ok(process.argv[2], 'Pass the extracted SEA artifact directory.');
const runtime = await mkdtemp(join(tmpdir(), 'cadder-sea-consumer-'));
if (/\.(?:zip|tar\.gz)$/.test(stage)) {
  const unpacked = join(runtime, 'unpacked');
  await mkdir(unpacked);
  await execute(
    process.platform === 'win32'
      ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe')
      : 'tar',
    ['-xf', stage, '-C', unpacked],
  );
  stage = join(unpacked, 'cadder');
}
await mkdir(join(runtime, 'empty-path'));
const environment = Object.fromEntries(
  Object.entries(process.env).filter(([key]) => key.toLowerCase() !== 'path'),
);
environment.PATH =
  process.platform === 'win32'
    ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32')
    : join(runtime, 'empty-path');
environment.CADDER_RUNTIME_DIR = join(runtime, 'runtime');
const binary = (name: string): string =>
  join(stage, process.platform === 'win32' ? `${name}.exe` : name);
const run = (args: string[]) =>
  execute(binary('cadder'), args, {
    env: environment,
    cwd: runtime,
    timeout: 45000,
    maxBuffer: 1024 * 1024,
  });
try {
  const identity = JSON.parse(await readFile(join(stage, 'BUILD.json'), 'utf8')) as {
    version: string;
    target: string;
  };
  assert.equal(identity.target, `${process.platform}-${process.arch}`);
  const allowed = new Set(
    ['cadder', 'cadderd', 'caddy']
      .map((name) => (process.platform === 'win32' ? `${name}.exe` : name))
      .concat(['LICENSE', 'NODE-LICENSE', 'THIRD-PARTY-NOTICES', 'BUILD.json']),
  );
  assert.deepEqual((await readdir(stage)).sort(), [...allowed, 'SHA256SUMS'].sort());
  const checksums = (await readFile(join(stage, 'SHA256SUMS'), 'utf8')).trim().split('\n');
  assert.equal(checksums.length, allowed.size);
  for (const line of checksums) {
    const hash = line.slice(0, 64);
    const name = line.slice(66);
    assert.match(hash, /^[a-f0-9]{64}$/);
    assert.ok(allowed.delete(name));
    const path = join(stage, name);
    const info = await lstat(path);
    assert.ok(info.isFile() && !info.isSymbolicLink());
    assert.equal(
      createHash('sha256')
        .update(await readFile(path))
        .digest('hex'),
      hash,
    );
  }
  assert.equal((await run(['--version'])).stdout.trim(), identity.version);
  assert.equal(
    (await execute(binary('cadderd'), ['--version'], { env: environment })).stdout.trim(),
    identity.version,
  );
  assert.equal(
    JSON.parse(
      (await execute(binary('caddy'), ['--cadder-shim-info'], { env: environment })).stdout,
    ).version,
    identity.version,
  );
  await assert.rejects(
    run(['status']),
    (error: unknown) => (error as { code?: unknown }).code === 3,
  );
  assert.match((await run(['daemon', 'start'])).stdout, /running/);
  assert.match((await run(['status'])).stdout, /cadderd {2}running/);
  assert.match((await run(['logs', 'runtime', '--limit', '1'])).stdout, /Daemon started/);
  assert.match((await run(['daemon', 'restart'])).stdout, /running/);
  assert.match((await run(['daemon', 'stop'])).stdout, /stopped/);
  process.stdout.write(
    JSON.stringify({
      stage,
      result: 'PASS',
      scope:
        'native SEA entries, authenticated daemon, SQLite worker and restart without Node/npm on PATH',
    }) + '\n',
  );
} finally {
  await run(['daemon', 'stop']);
  await rm(runtime, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
