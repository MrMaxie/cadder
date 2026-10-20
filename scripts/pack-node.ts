import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { access, chmod, copyFile, mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { sourceIdentity } from './source-identity.ts';
import { VERSION } from '../src/protocol/version.ts';

const execute = promisify(execFile);
export const repositoryRoot = fileURLToPath(new URL('../', import.meta.url));
export async function npmCli(): Promise<string> {
  for (const candidate of [
    join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'),
    join(dirname(process.execPath), '../lib/node_modules/npm/bin/npm-cli.js'),
  ]) {
    try {
      await access(candidate);
      return candidate;
    } catch {
      /* Try the other standard Node layout. */
    }
  }
  throw new Error('The Node installation must include npm to pack consumer artifacts.');
}

/** Compile and pack the actual consumer package; all outputs stay outside checkout. */
export async function packNode(): Promise<{ stage: string; archive: string }> {
  const temporary = await mkdtemp(join(tmpdir(), 'cadder-node-pack-'));
  const stage = join(temporary, 'package');
  await mkdir(stage);
  const manifest = JSON.parse(await readFile(join(repositoryRoot, 'package.json'), 'utf8')) as {
    name: string;
    version: string;
    description: string;
    engines: Record<string, string>;
    dependencies: Record<string, string>;
  };
  const config = join(temporary, 'tsconfig.json');
  if (manifest.version !== VERSION)
    throw new Error('Package and protocol release versions must match.');
  await writeFile(
    config,
    JSON.stringify({
      extends: join(repositoryRoot, 'tsconfig.json'),
      include: ['src/**/*.ts', 'src/**/*.tsx'].map((path) =>
        join(repositoryRoot, path).replaceAll('\\', '/'),
      ),
      compilerOptions: {
        noEmit: false,
        rootDir: join(repositoryRoot, 'src'),
        outDir: join(stage, 'lib'),
        typeRoots: [join(repositoryRoot, 'node_modules/@types')],
        declaration: false,
        sourceMap: false,
      },
    }),
  );
  await execute(
    process.execPath,
    [join(repositoryRoot, 'node_modules/typescript/bin/tsc'), '--project', config],
    { cwd: repositoryRoot },
  );
  await writeFile(
    join(stage, 'package.json'),
    JSON.stringify(
      {
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        engines: manifest.engines,
        dependencies: manifest.dependencies,
        type: 'module',
        license: 'Apache-2.0',
        repository: { type: 'git', url: 'git+https://github.com/MrMaxie/cadder.git' },
        cadderBuild: await sourceIdentity(repositoryRoot),
        bin: {
          cadder: 'lib/entrypoints/cadder.js',
          cadderd: 'lib/entrypoints/cadderd.js',
          caddy: 'lib/entrypoints/caddy.js',
        },
        files: ['lib', 'LICENSE', 'README.md', 'cadder.toml.example'],
      },
      null,
      2,
    ) + '\n',
  );
  await copyFile(join(repositoryRoot, 'LICENSE'), join(stage, 'LICENSE'));
  await copyFile(join(repositoryRoot, 'docs/NODE-CANDIDATE.md'), join(stage, 'README.md'));
  await copyFile(join(repositoryRoot, 'cadder.toml.example'), join(stage, 'cadder.toml.example'));
  for (const entry of ['cadder', 'cadderd', 'caddy'])
    await chmod(join(stage, 'lib/entrypoints', `${entry}.js`), 0o755);
  const packed = await execute(
    process.execPath,
    [await npmCli(), 'pack', stage, '--pack-destination', temporary, '--json', '--ignore-scripts'],
    { cwd: temporary, maxBuffer: 1024 * 1024 },
  );
  type PackResult = { filename: string; files: { path: string }[] };
  const report = JSON.parse(packed.stdout) as PackResult[] | Record<string, PackResult>;
  const result = Array.isArray(report) ? report[0] : report[manifest.name];
  if (
    !result ||
    result.files.some(
      ({ path }) =>
        !['package.json', 'README.md', 'LICENSE', 'cadder.toml.example'].includes(path) &&
        (!path.startsWith('lib/') || !path.endsWith('.js')),
    )
  )
    throw new Error('Unexpected npm package contents.');
  return { stage, archive: join(temporary, result.filename) };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  process.stdout.write(JSON.stringify(await packNode()) + '\n');
