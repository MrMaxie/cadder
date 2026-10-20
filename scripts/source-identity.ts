import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { readFile, readdir } from 'node:fs/promises';
import { join, relative } from 'node:path';

export async function sourceIdentity(root: string) {
  const files: string[] = [];
  for (const directory of ['src', 'scripts'])
    for (const entry of await readdir(join(root, directory), {
      recursive: true,
      withFileTypes: true,
    }))
      if (entry.isFile() && (/\.tsx?$/.test(entry.name) || entry.name.endsWith('-LICENSE')))
        files.push(join(entry.parentPath, entry.name));
  files.push(
    ...['package.json', 'nub.lock', 'tsconfig.json', 'LICENSE'].map((name) => join(root, name)),
  );
  const hash = createHash('sha256');
  for (const file of files.sort())
    hash.update(relative(root, file).replaceAll('\\', '/') + '\0').update(await readFile(file));
  const execute = promisify(execFile);
  const revision = (await execute('git', ['rev-parse', 'HEAD'], { cwd: root })).stdout.trim();
  const dirty =
    (await execute('git', ['status', '--porcelain'], { cwd: root })).stdout.trim() !== '';
  return { revision, digest: hash.digest('hex'), dirty };
}
