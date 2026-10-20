import { spawn, type ChildProcess } from 'node:child_process';
import { access } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isSea } from 'node:sea';

/** Resolve only this installation's entry; never consult PATH for cadderd. */
export async function daemonCommand(): Promise<{ file: string; args: string[] }> {
  if (isSea()) {
    const file = join(
      dirname(process.execPath),
      process.platform === 'win32' ? 'cadderd.exe' : 'cadderd',
    );
    await access(file);
    return { file, args: [] };
  }
  const source = import.meta.url.endsWith('.ts');
  const path = fileURLToPath(
    new URL(source ? '../entrypoints/cadderd.ts' : '../entrypoints/cadderd.js', import.meta.url),
  );
  await access(path);
  return {
    file: process.execPath,
    args: [...(source ? ['--import', import.meta.resolve('tsx')] : []), path],
  };
}

export async function launchDaemon(args: readonly string[] = []): Promise<ChildProcess> {
  const command = await daemonCommand();
  return new Promise<ChildProcess>((resolve, reject) => {
    const child = spawn(command.file, [...command.args, ...args], {
      detached: true,
      windowsHide: true,
      stdio: 'ignore',
      shell: false,
    });
    child.once('error', reject);
    child.once('spawn', () => {
      child.unref();
      resolve(child);
    });
  });
}
