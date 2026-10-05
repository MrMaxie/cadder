import assert from 'node:assert/strict';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { once } from 'node:events';
import { chmod, copyFile, mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { rpc } from '../../src/client/connection.ts';
import { resolvePaths } from '../../src/daemon/paths.ts';
import { prepareRuntime, runtimeOwner } from '../../src/platform/runtime-security.ts';

if (process.platform === 'win32' || !process.getuid || process.getuid() === 0)
  throw new Error('Run this Unix privilege gate as a non-root user with sudo -n available.');
const daemonEntry = process.argv[2];
const deniedEntry = process.argv[3];
if (!daemonEntry || !deniedEntry)
  throw new Error('Pass packed runtime-child and runtime-denied-client entrypoints.');
// macOS runner tool caches and per-user temp directories are not traversable by nobody.
// Publish only the executable and test bundle in a disposable shared directory.
const inputs = await mkdtemp('/tmp/cadder-privilege-inputs-');
const root = await mkdtemp('/tmp/cadder-privilege-runtime-');
const paths = resolvePaths({ runtimeDir: root });
const environment = { ...process.env };
delete environment.NODE_OPTIONS;
let daemon: ChildProcessWithoutNullStreams | undefined;

try {
  const outsiderNode = join(inputs, 'node');
  const outsiderEntry = join(inputs, 'runtime-denied-client.mjs');
  await copyFile(process.execPath, outsiderNode);
  await copyFile(deniedEntry, outsiderEntry);
  await chmod(outsiderNode, 0o755);
  await chmod(outsiderEntry, 0o644);
  await chmod(inputs, 0o755);
  await chmod(root, 0o755);
  await prepareRuntime(paths.directory, await runtimeOwner());
  daemon = spawn('sudo', ['-n', process.execPath, daemonEntry, root, String(process.getuid())], {
    env: environment,
  });
  let error = '';
  daemon.stderr.on('data', (data: Buffer) => {
    error += data.toString();
  });
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`Elevated daemon startup timeout: ${error}`));
    }, 12000);
    daemon!.stdout.once('data', () => {
      clearTimeout(timer);
      resolve();
    });
    daemon!.once('exit', () => {
      clearTimeout(timer);
      reject(new Error(error));
    });
    daemon!.once('error', reject);
  });
  const status = (await rpc(paths, 'status')) as { processId: number };
  assert.ok(status.processId > 0);
  const discovery = JSON.parse(await readFile(paths.discovery, 'utf8')) as {
    elevated: boolean;
    owner: string;
  };
  assert.equal(discovery.elevated, true);
  assert.equal(discovery.owner, `uid:${process.getuid()}`);
  const outsider = spawn('sudo', ['-n', '-u', 'nobody', outsiderNode, outsiderEntry, root], {
    env: environment,
  });
  let deniedOutput = '';
  let deniedError = '';
  outsider.stdout.on('data', (data: Buffer) => {
    deniedOutput += data.toString();
  });
  outsider.stderr.on('data', (data: Buffer) => {
    deniedError += data.toString();
  });
  const [code] = await once(outsider, 'exit');
  assert.equal(code, 0, deniedError);
  assert.equal(deniedOutput.trim(), 'DENIED');
  await rpc(paths, 'status');
  const exited = once(daemon, 'exit');
  await rpc(paths, 'shutdown');
  await exited;
  daemon = undefined;
  process.stdout.write(`Native privilege gate passed on ${process.platform}/${process.arch}.\n`);
} finally {
  if (daemon?.exitCode === null) {
    const exited = once(daemon, 'exit');
    await rpc(paths, 'shutdown').catch(() => {});
    // The fixture's fixed 30-second lifetime bounds cleanup if authentication failed.
    await exited;
  }
  await rm(root, { recursive: true, force: true });
  await rm(inputs, { recursive: true, force: true });
}
