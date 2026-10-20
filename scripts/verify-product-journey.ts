import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { get } from 'node:http';
import { get as getTls } from 'node:https';
import { setTimeout as delay } from 'node:timers/promises';
import { DatabaseSync } from 'node:sqlite';

// Run on an isolated native machine with free loopback ports 80 and 443.
const [directory, upstream, node] = process.argv.slice(2);
assert.ok(
  directory && upstream,
  'Pass the product directory, separate real Caddy, and Node for npm only.',
);
const root = await mkdtemp(join(tmpdir(), 'cadder-product-journey-'));
const stage = resolve(directory);
const binary = (name: string): string =>
  join(stage, process.platform === 'win32' ? `${name}.exe` : name);
const command = (name: string, args: string[]) =>
  node
    ? { file: resolve(node), args: [join(stage, `lib/entrypoints/${name}.js`), ...args] }
    : { file: binary(name), args };
await mkdir(join(root, 'empty-path'));
const environment = Object.fromEntries(
  Object.entries(process.env).filter(([key]) => key.toLowerCase() !== 'path'),
);
environment.PATH = node
  ? dirname(resolve(node))
  : process.platform === 'win32'
    ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32')
    : join(root, 'empty-path');
if (node && process.platform === 'win32')
  environment.PATH += ';' + join(process.env.SystemRoot ?? 'C:\\Windows', 'System32');
environment.CADDER_RUNTIME_DIR = join(root, 'runtime');
const execute = promisify(execFile);
const run = (args: string[]) => {
  const entry = command('cadder', args);
  return execute(entry.file, entry.args, {
    env: environment,
    cwd: root,
    timeout: 45000,
    maxBuffer: 1024 * 1024,
  });
};
const children: ReturnType<typeof spawn>[] = [];
let daemonErrors = '';
function child(name: string, args: string[], cwd: string) {
  const entry = command(name, args);
  const process_ = spawn(entry.file, entry.args, {
    env: environment,
    cwd,
    windowsHide: true,
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  process_.stderr!.on('data', (chunk: Buffer) => {
    daemonErrors = (daemonErrors + `${name} (${cwd}): ` + chunk.toString()).slice(-8192);
  });
  children.push(process_);
  return process_;
}
async function until(check: () => Promise<void>, timeout = 120000): Promise<void> {
  const deadline = Date.now() + timeout;
  let last: unknown;
  while (Date.now() < deadline) {
    try {
      await check();
      return;
    } catch (error) {
      last = error;
      await delay(200);
    }
  }
  throw new Error(`Product scenario deadline exceeded: ${String(last)}\n${daemonErrors}`);
}
async function request(host: string, tls = false): Promise<{ status: number; body: string }> {
  const ca = tls
    ? await readFile(
        join(environment.CADDER_RUNTIME_DIR!, 'v2/caddy/data/caddy/pki/authorities/local/root.crt'),
      )
    : undefined;
  return new Promise((resolveResponse, reject) => {
    const request_ = (tls ? getTls : get)(
      {
        hostname: '127.0.0.1',
        port: tls ? 443 : 80,
        path: '/',
        headers: { host },
        ...(tls ? { servername: host, ca } : {}),
      },
      (response) => {
        let body = '';
        response.setEncoding('utf8');
        response.on('data', (chunk: string) => {
          body = (body + chunk).slice(0, 4096);
        });
        response.on('end', () => resolveResponse({ status: response.statusCode ?? 0, body }));
      },
    );
    request_.on('error', reject);
    request_.setTimeout(5000, () => request_.destroy(new Error('HTTP deadline exceeded.')));
  });
}
async function project(name: string, body: string) {
  const path = join(root, name);
  await mkdir(path);
  const config = join(path, 'Caddyfile');
  await writeFile(config, body);
  return { path, config, shim: child('caddy', ['run', '--config', config], path) };
}
try {
  const daemon = child('cadderd', ['--real-caddy', resolve(upstream)], root);
  await until(async () => {
    assert.equal(daemon.exitCode, null);
    assert.match((await run(['status'])).stdout, /cadderd {2}running/);
  });
  const alpha = await project('alpha', 'alpha.localhost {\n respond "alpha"\n}\n');
  await until(async () => assert.equal((await request('alpha.localhost')).body, 'alpha'));
  assert.equal((await request('alpha.localhost', true)).body, 'alpha');
  const beta = await project('beta', 'beta.localhost {\n respond "beta"\n}\n');
  await until(async () => assert.equal((await request('beta.localhost')).body, 'beta'));
  assert.equal((await request('alpha.localhost')).body, 'alpha');
  assert.equal((await request('beta.localhost', true)).body, 'beta');
  assert.notEqual((await request('unknown.localhost')).body, 'alpha');
  await run(['domains', 'disable', 'alpha.localhost']);
  assert.notEqual((await request('alpha.localhost')).body, 'alpha');
  await run(['domains', 'enable', 'alpha.localhost']);
  assert.equal((await request('alpha.localhost')).body, 'alpha');
  await run(['projects', 'disable', beta.config]);
  assert.notEqual((await request('beta.localhost')).body, 'beta');
  await run(['projects', 'enable', beta.config]);
  assert.equal((await request('beta.localhost')).body, 'beta');
  const database = new DatabaseSync(join(environment.CADDER_RUNTIME_DIR!, 'v2/runtime.sqlite3'), {
    timeout: 1000,
  });
  try {
    database.exec(
      "CREATE TRIGGER gate_reject_desired BEFORE UPDATE ON desired BEGIN SELECT RAISE(FAIL, 'acceptance persistence refusal'); END",
    );
    await assert.rejects(run(['projects', 'disable', beta.config]));
    assert.equal((await request('beta.localhost')).body, 'beta');
  } finally {
    database.exec('DROP TRIGGER IF EXISTS gate_reject_desired');
    database.close();
  }
  await run(['projects', 'enable', beta.config]);
  assert.equal((await request('beta.localhost')).body, 'beta');
  const conflict = await project('conflict', 'alpha.localhost {\n respond "conflict"\n}\n');
  await until(async () => assert.equal(conflict.shim.exitCode, 1));
  assert.equal((await request('alpha.localhost')).body, 'alpha');
  await writeFile(alpha.config, 'not a valid { Caddyfile');
  await assert.rejects(run(['projects', 'enable', alpha.config]));
  assert.equal((await request('alpha.localhost')).body, 'alpha');
  await writeFile(alpha.config, 'alpha.localhost {\n respond "alpha"\n}\n');
  await delay(5500);
  assert.match((await run(['domains', 'list'])).stdout, /alpha.localhost/);
  alpha.shim.kill();
  await until(async () => assert.notEqual((await request('alpha.localhost')).body, 'alpha'), 25000);
  assert.equal((await request('beta.localhost')).body, 'beta');
  const admin = /Admin\s+https:\/\/127\.0\.0\.1:(\d+)/.exec((await run(['diagnostics'])).stdout);
  assert.ok(admin);
  await assert.rejects(
    new Promise((resolveResponse, reject) => {
      const attempt = getTls(
        {
          hostname: '127.0.0.1',
          port: Number(admin[1]),
          path: '/config/',
          rejectUnauthorized: false,
        },
        (response) => {
          response.resume();
          if ((response.statusCode ?? 0) >= 400) reject(new Error('Unauthorized client refused.'));
          else resolveResponse(response.statusCode);
        },
      );
      attempt.on('error', reject);
      attempt.setTimeout(5000, () =>
        attempt.destroy(new Error('Administration deadline exceeded.')),
      );
    }),
  );
  await run(['daemon', 'stop']);
  await assert.rejects(request('beta.localhost'));
  beta.shim.kill();
  process.stdout.write(
    JSON.stringify({
      result: 'PASS',
      platform: process.platform,
      distribution: node ? 'npm' : 'sea',
      checks: [
        'actual daemon/shim/operator',
        'multi-project HTTP/verified HTTPS',
        'domain/project toggles',
        'conflict and malformed rejection with last-known-good',
        'post-apply SQLite failure restores last-known-good',
        'heartbeat and crashed-session expiry',
        'unauthorized admin denial',
        'owned shutdown',
      ],
      nodeOnProductPath: Boolean(node),
    }) + '\n',
  );
} catch (error) {
  for (const args of [['status'], ['projects', 'list'], ['logs', 'runtime', '--limit', '20']]) {
    try {
      process.stderr.write((await run(args)).stdout);
    } catch {
      /* The original failure remains authoritative. */
    }
  }
  process.stderr.write(
    JSON.stringify(
      children.map((entry) => ({
        pid: entry.pid,
        exitCode: entry.exitCode,
        signal: entry.signalCode,
      })),
    ) +
      '\n' +
      daemonErrors +
      '\n',
  );
  throw error;
} finally {
  await run(['daemon', 'stop']).catch(() => {});
  for (const process_ of children) if (process_.exitCode === null) process_.kill();
  await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
