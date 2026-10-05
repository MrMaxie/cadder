import { mkdtemp, readFile, rm, unlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { startLocalRuntime } from '../src/daemon/local-runtime.ts';
import { rpc } from '../src/client/connection.ts';
import { resolvePaths } from '../src/daemon/paths.ts';
import { prepareRuntime, runtimeOwner } from '../src/platform/runtime-security.ts';

it('owns the full bind/lock lifetime and releases it idempotently', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cadder-local-'));
  const options = {
    runtimeDir: root,
    ...(process.getuid ? { runtimeOwner: process.getuid() } : {}),
  };
  try {
    const runtime = await startLocalRuntime(options, async () => ({ online: true }));
    try {
      expect(await rpc(runtime.paths, 'status')).toEqual({ online: true });
      await expect(startLocalRuntime(options, async () => null)).rejects.toThrow('already locked');
    } finally {
      await Promise.all([runtime.stop(), runtime.stop()]);
    }
    const restarted = await startLocalRuntime(options, async () => null);
    expect(restarted.recovered).toBe(false);
    await restarted.stop();
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

it('closes the listener and releases the lock when protected discovery publication fails', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cadder-local-failure-'));
  const options = {
    runtimeDir: root,
    ...(process.getuid ? { runtimeOwner: process.getuid() } : {}),
  };
  const paths = resolvePaths(options);
  const owner = await runtimeOwner(options.runtimeOwner);
  try {
    await prepareRuntime(paths.directory, owner);
    await writeFile(paths.discovery, 'unsafe-existing-metadata', { mode: 0o644 });
    await expect(startLocalRuntime(options, async () => null)).rejects.toMatchObject({
      code: 'unsafe-runtime-permissions',
    });
    expect(await readFile(paths.discovery, 'utf8')).toBe('unsafe-existing-metadata');
    await unlink(paths.discovery);
    const runtime = await startLocalRuntime(options, async () => null);
    await runtime.stop();
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
