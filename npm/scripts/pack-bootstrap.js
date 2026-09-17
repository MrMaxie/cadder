import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { platformTargets } from '../lib/platforms.js';
import {
  ensureNewDirectory,
  npmArchiveName,
  parseArguments,
  parseNpmPackMetadata,
  readJson,
  requiredArgument,
  runNpm,
  sha256,
} from './package-utils.js';

const bootstrapVersion = '0.0.0-bootstrap.0';

async function packOnce(packageDirectory, destination) {
  const result = await runNpm(['pack', packageDirectory, '--json', '--ignore-scripts', '--pack-destination', destination]);
  const metadata = parseNpmPackMetadata(result.stdout);
  assert.deepEqual(
    metadata.files.map((entry) => entry.path).sort(),
    ['README.md', 'package.json'],
    `${metadata.name} bootstrap tarball must be non-executable.`,
  );
  return metadata;
}

const argumentsMap = parseArguments(process.argv.slice(2));
const source = resolve(requiredArgument(argumentsMap, 'source'));
const output = await ensureNewDirectory(requiredArgument(argumentsMap, 'output'));
const directories = [...platformTargets.map((target) => target.directory), 'root'];
const packages = [];

for (const directory of directories) {
  const packageDirectory = resolve(source, directory);
  const manifest = await readJson(resolve(packageDirectory, 'package.json'));
  assert.equal(manifest.version, bootstrapVersion);
  const metadata = await packOnce(packageDirectory, output);
  const archiveName = npmArchiveName(manifest.name, bootstrapVersion);
  const repeatDirectory = await mkdtemp(resolve(output, '.repeat-'));
  try {
    await packOnce(packageDirectory, repeatDirectory);
    assert.equal(await sha256(resolve(output, archiveName)), await sha256(resolve(repeatDirectory, archiveName)));
  } finally {
    await rm(repeatDirectory, { recursive: true, force: true });
  }
  packages.push({ name: manifest.name, version: manifest.version, file: archiveName, tag: 'bootstrap' });
}

await writeFile(
  resolve(output, 'bootstrap-tarballs.json'),
  `${JSON.stringify({ version: bootstrapVersion, tag: 'bootstrap', packages }, null, 2)}\n`,
);
process.stdout.write(`packed ${packages.length} non-executable bootstrap tarballs with the bootstrap tag\n`);
