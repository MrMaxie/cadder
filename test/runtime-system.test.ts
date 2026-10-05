import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';
import { expect, it } from 'vitest';
import { resolvePaths } from '../src/daemon/paths.ts';
import { rpc } from '../src/client/connection.ts';

async function start(root: string, entry: string): Promise<ChildProcessWithoutNullStreams> {
  const environment = { ...process.env };
  delete environment.NODE_OPTIONS;
  const child = spawn(
    process.execPath,
    [entry, root, ...(process.getuid ? [String(process.getuid())] : [])],
    { windowsHide: true, env: environment },
  );
  let error = '';
  child.stderr.on('data', (data: Buffer) => {
    error += data.toString();
  });
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`Fixture startup timed out: ${error}`));
    }, 12000);
    child.stdout.once('data', () => {
      clearTimeout(timer);
      resolve();
    });
    child.once('exit', () => {
      clearTimeout(timer);
      reject(new Error(error));
    });
    child.once('error', reject);
  });
  return child;
}

it(
  'system gate: real child process lock, authenticated local transport and crash recovery',
  { timeout: 30000 },
  async () => {
    const root = await mkdtemp(join(tmpdir(), 'cadder-system-'));
    const paths = resolvePaths({ runtimeDir: root });
    let child: ChildProcessWithoutNullStreams | undefined;
    try {
      const entry = join(root, 'runtime-child.mjs');
      await build({
        entryPoints: [fileURLToPath(new URL('./fixtures/runtime-child.ts', import.meta.url))],
        outfile: entry,
        bundle: true,
        platform: 'node',
        format: 'esm',
        target: 'node24.18',
      });
      child = await start(root, entry);
      expect(await rpc(paths, 'status')).toMatchObject({ processId: child.pid, recovered: false });
      await expect(start(root, entry)).rejects.toThrow('already locked');
      const crashed = once(child, 'exit');
      child.kill('SIGKILL');
      await crashed;
      child = await start(root, entry);
      expect(await rpc(paths, 'status')).toMatchObject({ processId: child.pid, recovered: true });
      const exited = once(child, 'exit');
      expect(await rpc(paths, 'shutdown')).toEqual({ stopped: true });
      await exited;
      child = undefined;
      await expect(rpc(paths, 'status')).rejects.toThrow('Start it with cadder daemon start');
    } finally {
      if (child?.exitCode === null) {
        const exited = once(child, 'exit');
        child.kill('SIGKILL');
        await exited;
      }
      await rm(root, { recursive: true, force: true });
    }
  },
);
