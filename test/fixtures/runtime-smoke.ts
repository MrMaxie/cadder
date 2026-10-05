import assert from 'node:assert/strict';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { resolvePaths } from '../../src/daemon/paths.ts';
import { rpc } from '../../src/client/connection.ts';

// Native-platform gate for the packed runtime fixture, not a product release.
const entry = process.argv[2];
if (!entry) throw new Error('Pass the absolute path to packed runtime-child.mjs.');
const root = await mkdtemp(join(tmpdir(), 'cadder-native-'));
const paths = resolvePaths({ runtimeDir: root });
const environment = { ...process.env };
delete environment.NODE_OPTIONS;
let child: ChildProcessWithoutNullStreams | undefined;

async function start(): Promise<ChildProcessWithoutNullStreams> {
  const processHandle = spawn(
    process.execPath,
    [entry!, root, ...(process.getuid ? [String(process.getuid())] : [])],
    { windowsHide: true, env: environment },
  );
  let stderr = '';
  processHandle.stderr.on('data', (data: Buffer) => {
    stderr += data.toString();
  });
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      processHandle.kill();
      reject(new Error(`Startup timeout: ${stderr}`));
    }, 12000);
    processHandle.stdout.once('data', () => {
      clearTimeout(timer);
      resolve();
    });
    processHandle.once('exit', () => {
      clearTimeout(timer);
      reject(new Error(stderr));
    });
    processHandle.once('error', reject);
  });
  return processHandle;
}

try {
  child = await start();
  assert.deepEqual(await rpc(paths, 'status'), { processId: child.pid, recovered: false });
  await assert.rejects(start, /already locked/);
  let exited = once(child, 'exit');
  child.kill('SIGKILL');
  await exited;
  child = await start();
  assert.deepEqual(await rpc(paths, 'status'), { processId: child.pid, recovered: true });
  exited = once(child, 'exit');
  await rpc(paths, 'shutdown');
  await exited;
  child = undefined;
  await assert.rejects(() => rpc(paths, 'status'), /Start it with cadder daemon start/);
  process.stdout.write(`Native runtime gate passed on ${process.platform}/${process.arch}.\n`);
} finally {
  if (child?.exitCode === null) {
    const exited = once(child, 'exit');
    child.kill('SIGKILL');
    await exited;
  }
  await rm(root, { recursive: true, force: true });
}
