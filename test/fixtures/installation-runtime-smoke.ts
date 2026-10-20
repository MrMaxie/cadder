import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { startLocalRuntime, type LocalRuntime } from '../../src/daemon/local-runtime.ts';
import { rpc } from '../../src/client/connection.ts';
import { resolvePaths } from '../../src/daemon/paths.ts';
import { fakeStateResult } from './rpc-data.ts';

// Test-only distribution-root inputs; no product packaging or storage worker is implied.
export async function verifyInstallationRuntimes(): Promise<void> {
  // Keep the synthetic macOS home short and exclusively owned; never write to
  // the real user's home. /tmp's OS alias does not prove runtime ancestor safety.
  const root = await mkdtemp(
    process.platform === 'darwin' ? '/tmp/c-' : join(tmpdir(), 'cadder-two-'),
  );
  const keys = [
    'CADDER_RUNTIME_DIR',
    'CADDER_RUNTIME_PROFILE',
    'LOCALAPPDATA',
    'XDG_RUNTIME_DIR',
    'HOME',
  ] as const;
  const environment = Object.fromEntries(keys.map((key) => [key, process.env[key]]));
  const runtimes: LocalRuntime[] = [];
  try {
    delete process.env.CADDER_RUNTIME_DIR;
    process.env.CADDER_RUNTIME_PROFILE = 'default';
    process.env.LOCALAPPDATA = root;
    process.env.XDG_RUNTIME_DIR = root;
    if (process.platform === 'darwin') process.env.HOME = root;
    const a = join(root, 'A');
    const b = join(root, 'B');
    await mkdir(a);
    await mkdir(b);
    const firstOptions = { installationRoot: a, profile: 'default' };
    const secondOptions = { installationRoot: b, profile: 'default' };
    const first = await startLocalRuntime(firstOptions, async () => fakeStateResult(101));
    runtimes.push(first);
    const second = await startLocalRuntime(secondOptions, async () => fakeStateResult(202));
    runtimes.push(second);
    assert.deepEqual(first.paths, resolvePaths(firstOptions));
    assert.deepEqual(second.paths, resolvePaths(secondOptions));
    for (const key of [
      'directory',
      'instance',
      'endpoint',
      'secret',
      'lock',
      'metadata',
      'discovery',
      'history',
    ] as const)
      assert.notEqual(first.paths[key], second.paths[key]);
    assert.notDeepEqual(await readFile(first.paths.secret), await readFile(second.paths.secret));
    for (const runtime of runtimes) {
      for (const path of [runtime.paths.metadata, runtime.paths.discovery]) {
        const diagnostic = JSON.parse(await readFile(path, 'utf8')) as {
          runtimeDir: string;
          endpoint: string;
        };
        assert.equal(diagnostic.runtimeDir, runtime.paths.directory);
        assert.equal(diagnostic.endpoint, runtime.paths.endpoint);
      }
    }
    assert.equal((await rpc(first.paths, 'query-state-request')).snapshot?.runtime.processId, 101);
    assert.equal((await rpc(second.paths, 'query-state-request')).snapshot?.runtime.processId, 202);
    await assert.rejects(
      startLocalRuntime(firstOptions, async () => fakeStateResult()),
      /already locked/,
    );
    await assert.rejects(
      startLocalRuntime(secondOptions, async () => fakeStateResult()),
      /already locked/,
    );
    await first.stop();
    await assert.rejects(
      rpc(first.paths, 'query-state-request'),
      /Start it with cadder daemon start/,
    );
    assert.equal((await rpc(second.paths, 'query-state-request')).snapshot?.runtime.processId, 202);
    await assert.rejects(
      startLocalRuntime(secondOptions, async () => fakeStateResult()),
      /already locked/,
    );
    const restarted = await startLocalRuntime(firstOptions, async () => fakeStateResult(303));
    runtimes.push(restarted);
    assert.equal(
      (await rpc(restarted.paths, 'query-state-request')).snapshot?.runtime.processId,
      303,
    );
    assert.equal((await rpc(second.paths, 'query-state-request')).snapshot?.runtime.processId, 202);
  } finally {
    try {
      const stopping = runtimes.map((runtime) => runtime.stop());
      // Wait for every finalizer before cleanup, then propagate any rejection.
      await Promise.allSettled(stopping);
      await Promise.all(stopping);
    } finally {
      for (const key of keys) {
        if (environment[key] === undefined) delete process.env[key];
        else process.env[key] = environment[key];
      }
      await rm(root, { recursive: true, force: true });
    }
  }
}
