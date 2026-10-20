import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { resolvePaths } from '../src/daemon/paths.ts';
import { acquireRuntimeLock } from '../src/daemon/runtime-lock.ts';
import {
  createProtectedFile,
  prepareRuntime,
  runtimeOwner,
} from '../src/platform/runtime-security.ts';

it('uses actual SQLite exclusion rather than diagnostic PID or expiry', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cadder-lock-'));
  const paths = resolvePaths({ runtimeDir: root });
  const owner = await runtimeOwner();
  try {
    await prepareRuntime(paths.directory, owner);
    const first = await acquireRuntimeLock(paths, owner);
    try {
      expect(first.recovered).toBe(false);
      expect(first.secret.length).toBe(32);
      await writeFile(paths.metadata, JSON.stringify({ processId: -1, acquiredAt: '1970-01-01' }));
      await expect(acquireRuntimeLock(paths, owner)).rejects.toThrow('already locked');
    } finally {
      await first.release();
      await first.release();
    }
    await createProtectedFile(paths.metadata, owner, 'stale metadata');
    const second = await acquireRuntimeLock(paths, owner);
    try {
      expect(second.recovered).toBe(true);
      expect(JSON.parse(await readFile(paths.metadata, 'utf8')).processId).toBe(process.pid);
    } finally {
      await second.release();
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
