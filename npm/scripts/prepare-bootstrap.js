import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { platformTargets } from '../lib/platforms.js';
import { ensureNewDirectory, parseArguments, readJson, requiredArgument } from './package-utils.js';

const npmDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const bootstrapVersion = '0.0.0-bootstrap.0';

async function preparePackage(sourceManifestPath, destination) {
  const source = await readJson(sourceManifestPath);
  const manifest = {
    name: source.name,
    version: bootstrapVersion,
    description: `Non-executable package-name bootstrap for ${source.name}.`,
    license: source.license,
    repository: source.repository,
    files: ['README.md'],
    publishConfig: { access: 'public' },
  };
  assert.deepEqual(manifest.publishConfig, { access: 'public' });
  await mkdir(destination, { recursive: true });
  await writeFile(resolve(destination, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  await writeFile(
    resolve(destination, 'README.md'),
    `# ${source.name}\n\nThis bootstrap package reserves the name for Cadder trusted publishing. It contains no executable application.\n`,
  );
}

const argumentsMap = parseArguments(process.argv.slice(2));
const output = await ensureNewDirectory(requiredArgument(argumentsMap, 'output'));
await preparePackage(resolve(npmDirectory, 'package.json'), resolve(output, 'root'));
for (const target of platformTargets) {
  await preparePackage(
    resolve(npmDirectory, 'packages', target.directory, 'package.json'),
    resolve(output, target.directory),
  );
}
process.stdout.write(`prepared five non-executable Cadder bootstrap packages at ${bootstrapVersion}\n`);
