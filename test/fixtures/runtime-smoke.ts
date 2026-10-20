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

async function start(recovered = false): Promise<ChildProcessWithoutNullStreams> {
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
    let stdout = '';
    processHandle.stdout.on('data', (data: Buffer) => {
      stdout += data.toString();
      if (!stdout.includes('\n')) return;
      clearTimeout(timer);
      if (stdout.trim() !== `READY recovered=${recovered}`)
        reject(new Error(`Unexpected fixture readiness: ${stdout}`));
      else resolve();
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
  assert.equal((await rpc(paths, 'query-state-request')).snapshot?.runtime.processId, child.pid);
  await assert.rejects(() => start(), /already locked/);
  let exited = once(child, 'exit');
  child.kill('SIGKILL');
  await exited;
  child = await start(true);
  assert.equal((await rpc(paths, 'query-state-request')).snapshot?.runtime.processId, child.pid);
  exited = once(child, 'exit');
  assert.equal((await rpc(paths, 'shutdown-daemon-request')).accepted, true);
  await exited;
  child = undefined;
  await assert.rejects(
    () => rpc(paths, 'query-state-request'),
    /Start it with cadder daemon start/,
  );
  process.stdout.write(`Native runtime gate passed on ${process.platform}/${process.arch}.\n`);
} finally {
  if (child?.exitCode === null) {
    const exited = once(child, 'exit');
    child.kill('SIGKILL');
    await exited;
  }
  await rm(root, { recursive: true, force: true });
}
