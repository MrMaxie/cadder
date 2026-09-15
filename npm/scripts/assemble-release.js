import assert from 'node:assert/strict';
import { chmod, copyFile, cp, mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { platformTargets } from '../lib/platforms.js';
import {
  commandNames,
  documentationFiles,
  ensureNewDirectory,
  listFiles,
  parseArguments,
  readJson,
  requiredArgument,
  run,
  sha256,
} from './package-utils.js';

const npmDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const projectDirectory = resolve(npmDirectory, '..');

async function verifyAttestation(path) {
  await run('gh', ['attestation', 'verify', path, '--repo', 'MrMaxie/Cadder']);
}

async function verifyArchiveChecksum(archive, checksum) {
  const expected = (await readFile(checksum, 'utf8')).trim().split(/\s+/)[0].toLowerCase();
  assert.match(expected, /^[0-9a-f]{64}$/, `Invalid SHA-256 file: ${checksum}`);
  const actual = await sha256(archive);
  assert.equal(actual, expected, `SHA-256 mismatch for ${archive}`);
}

async function extractArchive(archive, destination) {
  if (archive.endsWith('.zip') && process.platform !== 'win32') {
    await run('unzip', ['-q', archive, '-d', destination]);
    return;
  }
  await run('tar', ['-xf', archive, '-C', destination]);
}

async function copyDocumentation(destination) {
  for (const file of documentationFiles) {
    await copyFile(resolve(projectDirectory, file), resolve(destination, file));
  }
}

async function prepareRootPackage(output, expectedVersion) {
  const destination = resolve(output, 'root');
  await mkdir(destination);
  const manifest = await readJson(resolve(npmDirectory, 'package.json'));
  assert.equal(manifest.version, expectedVersion, 'Root npm package version does not match the release version.');
  await copyFile(resolve(npmDirectory, 'package.json'), resolve(destination, 'package.json'));
  await cp(resolve(npmDirectory, 'bin'), resolve(destination, 'bin'), { recursive: true });
  await cp(resolve(npmDirectory, 'lib'), resolve(destination, 'lib'), { recursive: true });
  await copyDocumentation(destination);
  return destination;
}

async function preparePlatformPackage({ assets, output, target, expectedVersion, verifyAttestations }) {
  const archive = resolve(assets, target.archive);
  const checksum = `${archive}.sha256`;
  await verifyArchiveChecksum(archive, checksum);
  if (verifyAttestations) {
    await verifyAttestation(archive);
    await verifyAttestation(checksum);
  }

  const extractionRoot = await mkdtemp(resolve(output, `.extract-${target.directory}-`));
  try {
    await extractArchive(archive, extractionRoot);
    const suffix = target.executableSuffix;
    const expectedArchiveFiles = [
      ...commandNames.map((command) => `${command}${suffix}`),
      ...documentationFiles,
    ].sort();
    assert.deepEqual(await listFiles(extractionRoot), expectedArchiveFiles, `${target.archive} has unexpected contents.`);

    const destination = resolve(output, target.directory);
    const binDirectory = resolve(destination, 'bin');
    await mkdir(binDirectory, { recursive: true });
    const manifestPath = resolve(npmDirectory, 'packages', target.directory, 'package.json');
    const manifest = await readJson(manifestPath);
    assert.equal(manifest.version, expectedVersion, `${manifest.name} version does not match the release version.`);
    await copyFile(manifestPath, resolve(destination, 'package.json'));
    await copyDocumentation(destination);

    const digests = {};
    for (const command of commandNames) {
      const fileName = `${command}${suffix}`;
      const source = resolve(extractionRoot, fileName);
      const copied = resolve(binDirectory, fileName);
      if (target.os[0] !== 'win32') {
        assert.notEqual((await stat(source)).mode & 0o111, 0, `${fileName} is not executable in ${target.archive}.`);
      }
      await copyFile(source, copied);
      if (target.os[0] !== 'win32') await chmod(copied, 0o755);
      const sourceDigest = await sha256(source);
      assert.equal(await sha256(copied), sourceDigest, `${fileName} changed while assembling ${manifest.name}.`);
      digests[command] = sourceDigest;
    }
    return { archive: target.archive, directory: target.directory, name: manifest.name, digests };
  } finally {
    await rm(extractionRoot, { recursive: true, force: true });
  }
}

const argumentsMap = parseArguments(process.argv.slice(2));
const assets = resolve(requiredArgument(argumentsMap, 'assets'));
const output = await ensureNewDirectory(requiredArgument(argumentsMap, 'output'));
const version = requiredArgument(argumentsMap, 'version');
const testMode = argumentsMap.get('test-mode') === true;
if (argumentsMap.has('test-mode') && !testMode) throw new Error('--test-mode does not accept a value.');
const targetDirectory = argumentsMap.get('target');
const selectedTargets =
  typeof targetDirectory === 'string'
    ? platformTargets.filter((target) => target.directory === targetDirectory)
    : platformTargets;
if (typeof targetDirectory !== 'undefined' && selectedTargets.length !== 1) {
  throw new Error(`Unknown npm platform target: ${targetDirectory}`);
}

await prepareRootPackage(output, version);
const packages = [];
for (const target of selectedTargets) {
  packages.push(
    await preparePlatformPackage({
      assets,
      output,
      target,
      expectedVersion: version,
      verifyAttestations: !testMode,
    }),
  );
}

await writeFile(
  resolve(output, 'assembly.json'),
  `${JSON.stringify({ version, repository: 'MrMaxie/Cadder', attestationsVerified: !testMode, packages }, null, 2)}\n`,
);
process.stdout.write(`assembled Cadder npm packages for ${version}${testMode ? ' in explicit test mode' : ''}\n`);
