import { constants, type BigIntStats } from 'node:fs';
import { lstat, open, realpath, stat } from 'node:fs/promises';
import { extname, posix, win32 } from 'node:path';
import { powershell, psLiteral } from '../platform/powershell.ts';

export function fileIdentity(info: BigIntStats): string {
  if (info.ino === 0n) throw new Error('Native executable file identity is unavailable.');
  return `${info.dev}:${info.ino}`;
}

/** Match Rust absolute paths: Windows drive-rooted paths without a prefix are relative. */
export function absoluteCaddyPath(value: string, platform = process.platform): boolean {
  const path = platform === 'win32' ? win32 : posix;
  return path.isAbsolute(value) && (platform !== 'win32' || path.parse(value).root.length > 1);
}

/** Installed executables need not have runtime owner-only permissions or ACLs. */
export async function nativeExecutable(path: string): Promise<string> {
  if (!absoluteCaddyPath(path) || path.includes('\0'))
    throw new Error('Caddy executable must use an absolute path.');
  if (process.platform === 'win32') {
    if (['.cmd', '.bat', '.ps1', '.js', '.cjs', '.mjs'].includes(extname(path).toLowerCase()))
      throw new Error('Caddy executable must be native, not a command/script wrapper.');
    await assertNotReparsePoint(path);
  }
  const canonical = await realpath(path);
  const file = await open(canonical, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const info = await file.stat({ bigint: true });
    if (!info.isFile()) throw new Error('Caddy executable must be a regular file.');
    fileIdentity(info);
    if (process.platform !== 'win32' && (info.mode & 0o111n) === 0n)
      throw new Error('Caddy image is not executable.');
    const header = Buffer.alloc(4);
    const { bytesRead } = await file.read(header, 0, 4, 0);
    const native =
      process.platform === 'win32'
        ? header[0] === 0x4d && header[1] === 0x5a
        : [
            '7f454c46',
            'feedface',
            'cefaedfe',
            'feedfacf',
            'cffaedfe',
            'cafebabe',
            'bebafeca',
            'cafebabf',
            'bfbafeca',
          ].includes(header.toString('hex'));
    if (bytesRead < 4 || !native)
      throw new Error('Caddy executable must be a native image, not a script wrapper.');
  } finally {
    await file.close();
  }
  return canonical;
}

export async function assertNotReparsePoint(path: string): Promise<void> {
  if ((await lstat(path)).isSymbolicLink())
    throw new Error('Caddy path must not be a Windows reparse point.');
  if (process.platform === 'win32')
    await powershell(`
    if (([IO.File]::GetAttributes(${psLiteral(path)}) -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Caddy path is a reparse point' }
  `);
}

/** Native dev/inode identity (bigint avoids Windows file-index precision loss). */
export async function sameFile(left: string, right: string): Promise<boolean> {
  const [a, b] = await Promise.all([stat(left, { bigint: true }), stat(right, { bigint: true })]);
  return fileIdentity(a) === fileIdentity(b);
}
