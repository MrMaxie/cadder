import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { platformTargets } from '../lib/platforms.js';
import {
  commandNames,
  documentationFiles,
  ensureNewDirectory,
  npmArchiveName,
  parseArguments,
  parseNpmPackMetadata,
  readJson,
  requiredArgument,
  runNpm,
  sha256,
} from './package-utils.js';

function expectedPackageFiles(target) {
  if (target === undefined) {
    return [
      'CHANGELOG.md',
      'LICENSE',
      'README.md',
      'bin/cadder.js',
      'bin/cadderd.js',
      'bin/caddy.js',
      'cadder.toml.example',
      'lib/launcher.js',
      'lib/platforms.js',
      'package.json',
    ].sort();
  }
  return [
    ...documentationFiles,
    ...commandNames.map((command) => `bin/${command}${target.executableSuffix}`),
    'package.json',
  ].sort();
}

async function packOnce(packageDirectory, destination) {
  const result = await runNpm(['pack', packageDirectory, '--json', '--ignore-scripts', '--pack-destination', destination]);
  return parseNpmPackMetadata(result.stdout);
}

async function verifyAndPack(packageDirectory, output, target, version) {
  const manifest = await readJson(resolve(packageDirectory, 'package.json'));
  assert.equal(manifest.version, version, `${manifest.name} has the wrong version.`);
  const first = await packOnce(packageDirectory, output);
  assert.deepEqual(
    first.files.map((entry) => entry.path).sort(),
    expectedPackageFiles(target),
    `${manifest.name} tarball contains unexpected files.`,
  );

  const archiveName = npmArchiveName(manifest.name, version);
  const archive = resolve(output, archiveName);
  const repeatDirectory = await mkdtemp(resolve(output, '.repeat-'));
  try {
    await packOnce(packageDirectory, repeatDirectory);
    assert.equal(
      await sha256(archive),
      await sha256(resolve(repeatDirectory, archiveName)),
      `${manifest.name} does not pack deterministically.`,
    );
  } finally {
    await rm(repeatDirectory, { recursive: true, force: true });
  }

  return { name: manifest.name, version, file: archiveName, sha256: await sha256(archive) };
}

const argumentsMap = parseArguments(process.argv.slice(2));
const stage = resolve(requiredArgument(argumentsMap, 'stage'));
const output = await ensureNewDirectory(requiredArgument(argumentsMap, 'output'));
const assembly = JSON.parse(await readFile(resolve(stage, 'assembly.json'), 'utf8'));
assert.equal(typeof assembly.version, 'string');
assert.equal(Array.isArray(assembly.packages), true, 'Assembly metadata does not contain package entries.');

const packages = [];
const selectedTargets = platformTargets.filter((target) =>
  assembly.packages.some((packageMetadata) => packageMetadata.directory === target.directory),
);
assert.equal(selectedTargets.length, assembly.packages.length, 'Assembly metadata contains an unknown platform package.');
for (const target of selectedTargets) {
  packages.push(await verifyAndPack(resolve(stage, target.directory), output, target, assembly.version));
}
packages.push(await verifyAndPack(resolve(stage, 'root'), output, undefined, assembly.version));

await writeFile(
  resolve(output, 'tarballs.json'),
  `${JSON.stringify(
    {
      version: assembly.version,
      attestationsVerified: assembly.attestationsVerified,
      approvalOrder: packages.map((entry) => entry.name),
      packages,
    },
    null,
    2,
  )}\n`,
);
process.stdout.write(`packed ${packages.length} deterministic Cadder npm tarballs for ${assembly.version}\n`);
