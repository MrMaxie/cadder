import { build } from 'esbuild';
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { promisify } from 'node:util';
import { copyFile, mkdir, mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { sourceIdentity } from './source-identity.ts';
import { VERSION } from '../src/protocol/version.ts';

const execute = promisify(execFile);
// Verified against https://nodejs.org/en/download/archive/v26.12.0 .
export const seaBuilderVersion = 'v26.12.0';
const builder = process.argv[2] ? resolve(process.argv[2]) : process.execPath;
if ((await execute(builder, ['--version'])).stdout.trim() !== seaBuilderVersion)
  throw new Error(
    `SEA requires the verified ${seaBuilderVersion} builder. Pass its absolute executable path.`,
  );
const stage = await mkdtemp(join(tmpdir(), 'cadder-sea-'));
const payload = join(stage, 'cadder');
await mkdir(payload);
const root = fileURLToPath(new URL('../', import.meta.url));
const manifest = JSON.parse(await readFile(join(root, 'package.json'), 'utf8')) as {
  version: string;
};
if (manifest.version !== VERSION)
  throw new Error('Package and protocol release versions must match.');
const banner =
  "import { createRequire as cadderCreateRequire } from 'node:module'; const require = cadderCreateRequire(process.execPath);";
// Ink guards this optional developer tool behind an installed-peer check.
const common = {
  bundle: true,
  platform: 'node' as const,
  format: 'esm' as const,
  target: 'node26',
  absWorkingDir: root,
  metafile: true as const,
  external: ['./devtools.js'],
  banner: { js: banner },
  define: { 'process.env.NODE_ENV': '"production"' },
};
const worker = join(stage, 'storage-worker.mjs');
const workerBuild = await build({
  ...common,
  entryPoints: [join(root, 'src/storage/worker.ts')],
  outfile: worker,
});
const inputs = new Set(Object.keys(workerBuild.metafile.inputs));
const artifacts: string[] = [];
for (const entry of ['cadder', 'cadderd', 'caddy']) {
  const bundle = join(stage, `${entry}.mjs`);
  const bundled = await build({
    ...common,
    entryPoints: [join(root, `src/entrypoints/${entry}.ts`)],
    outfile: bundle,
  });
  for (const input of Object.keys(bundled.metafile.inputs)) inputs.add(input);
  const output = join(payload, process.platform === 'win32' ? `${entry}.exe` : entry);
  const config = join(stage, `${entry}-sea.json`);
  await writeFile(
    config,
    JSON.stringify({
      main: bundle,
      mainFormat: 'module',
      output,
      disableExperimentalSEAWarning: true,
      useCodeCache: false,
      useSnapshot: false,
      execArgvExtension: 'none',
      assets: { 'cadder-storage-worker': worker },
    }),
  );
  await execute(builder, ['--build-sea', config], { timeout: 120000, maxBuffer: 1024 * 1024 });
  if (process.platform === 'darwin') await execute('/usr/bin/codesign', ['--sign', '-', output]);
  artifacts.push(output);
}
await copyFile(join(root, 'LICENSE'), join(payload, 'LICENSE'));
await copyFile(join(root, 'docs/NODE-CANDIDATE.md'), join(payload, 'README.md'));
await copyFile(join(root, 'cadder.toml.example'), join(payload, 'cadder.toml.example'));
let nodeLicense: string | undefined;
for (const candidate of ['LICENSE', '../LICENSE', '../share/doc/node/LICENSE']) {
  try {
    nodeLicense = await readFile(join(dirname(builder), candidate), 'utf8');
    break;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
  }
}
if (!nodeLicense) throw new Error('The pinned Node builder must include its distribution LICENSE.');
await writeFile(join(payload, 'NODE-LICENSE'), nodeLicense);
const packages = new Map<string, string>();
for (const input of inputs) {
  if (!input.replaceAll('\\', '/').includes('node_modules/')) continue;
  let directory = dirname(resolve(root, input));
  while (directory !== dirname(directory)) {
    try {
      const metadata = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8')) as {
        name?: string;
        version?: string;
      };
      if (metadata.name && metadata.version) {
        packages.set(`${metadata.name}@${metadata.version}`, directory);
        break;
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    }
    directory = dirname(directory);
  }
}
const notices: string[] = [];
for (const [name, directory] of [...packages].sort(([left], [right]) =>
  left.localeCompare(right),
)) {
  const licenses = (await readdir(directory, { withFileTypes: true })).filter(
    (entry) => entry.isFile() && /^(licen[cs]e|copying|notice)([.-]|$)/i.test(entry.name),
  );
  if (name === 'yoga-layout@3.2.1' && !licenses.length) {
    // The published package omits its upstream MIT text; retain the exact release license.
    notices.push(
      `${name} - https://github.com/facebook/yoga/blob/v3.2.1/LICENSE\n\n${await readFile(join(root, 'scripts/licenses/yoga-layout-3.2.1-LICENSE'), 'utf8')}`,
    );
    continue;
  }
  if (!licenses.length) throw new Error(`Missing bundled dependency license: ${name}`);
  for (const license of licenses)
    notices.push(
      `${name} - ${license.name}\n\n${await readFile(join(directory, license.name), 'utf8')}`,
    );
}
await writeFile(join(payload, 'THIRD-PARTY-NOTICES'), notices.join('\n\n') + '\n');
await writeFile(
  join(payload, 'BUILD.json'),
  JSON.stringify(
    {
      version: manifest.version,
      builder: seaBuilderVersion,
      target: `${process.platform}-${process.arch}`,
      source: await sourceIdentity(root),
    },
    null,
    2,
  ) + '\n',
);
const checksums: string[] = [];
for (const path of [
  ...artifacts,
  ...[
    'LICENSE',
    'NODE-LICENSE',
    'THIRD-PARTY-NOTICES',
    'BUILD.json',
    'README.md',
    'cadder.toml.example',
  ].map((name) => join(payload, name)),
])
  checksums.push(
    `${createHash('sha256')
      .update(await readFile(path))
      .digest('hex')}  ${path.slice(payload.length + 1)}`,
  );
await writeFile(join(payload, 'SHA256SUMS'), checksums.join('\n') + '\n');
const archive = join(
  stage,
  `cadder-${manifest.version}-${process.platform}-${process.arch}${process.platform === 'win32' ? '.zip' : '.tar.gz'}`,
);
await execute(
  process.platform === 'win32'
    ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe')
    : 'tar',
  [process.platform === 'win32' ? '-acf' : '-czf', archive, '-C', stage, 'cadder'],
);
process.stdout.write(
  JSON.stringify({
    stage: payload,
    archive,
    version: manifest.version,
    builder: seaBuilderVersion,
    target: `${process.platform}-${process.arch}`,
    artifacts,
  }) + '\n',
);
