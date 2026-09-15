import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { platformTargets } from '../lib/platforms.js';

const npmDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repository = {
  type: 'git',
  url: 'git+https://github.com/MrMaxie/cadder.git',
};
const lifecycleScripts = new Set([
  'preinstall',
  'install',
  'postinstall',
  'prepare',
  'prepack',
  'postpack',
  'prepublish',
  'prepublishOnly',
  'publish',
  'postpublish',
]);
const packageFiles = ['bin', 'README.md', 'CHANGELOG.md', 'LICENSE', 'cadder.toml.example'];

async function readJson(path) {
  return JSON.parse(await readFile(path, 'utf8'));
}

function checkIdentity(manifest, expectedName, expectedVersion) {
  assert.equal(manifest.name, expectedName);
  assert.equal(manifest.version, expectedVersion);
  assert.equal(manifest.license, 'Apache-2.0');
  assert.deepEqual(manifest.repository, repository);
  assert.deepEqual(manifest.publishConfig, { access: 'public', provenance: true });
  for (const script of Object.keys(manifest.scripts ?? {})) {
    assert.equal(lifecycleScripts.has(script), false, `${manifest.name} must not define the ${script} lifecycle script`);
  }
}

const rootManifest = await readJson(resolve(npmDirectory, 'package.json'));
assert.match(rootManifest.version, /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/, 'Root package version must be SemVer.');
checkIdentity(rootManifest, 'cadder', rootManifest.version);
assert.equal(rootManifest.packageManager, 'nub@0.7.5');
assert.deepEqual(rootManifest.workspaces, ['packages/*']);
assert.deepEqual(rootManifest.bin, {
  cadder: 'bin/cadder.js',
  cadderd: 'bin/cadderd.js',
  caddy: 'bin/caddy.js',
});
assert.deepEqual(rootManifest.files, ['bin', 'lib', 'README.md', 'CHANGELOG.md', 'LICENSE', 'cadder.toml.example']);
assert.deepEqual(
  rootManifest.optionalDependencies,
  Object.fromEntries(platformTargets.map((target) => [target.packageName, rootManifest.version]).sort()),
);

for (const target of platformTargets) {
  const manifest = await readJson(resolve(npmDirectory, 'packages', target.directory, 'package.json'));
  checkIdentity(manifest, target.packageName, rootManifest.version);
  assert.deepEqual(manifest.os, target.os);
  assert.deepEqual(manifest.cpu, target.cpu);
  assert.deepEqual(manifest.libc, target.libc);
  assert.deepEqual(manifest.files, packageFiles);
  assert.equal(manifest.bin, undefined);
  assert.equal(manifest.dependencies, undefined);
  assert.equal(manifest.optionalDependencies, undefined);
}

for (const bunFile of ['bun.lock', 'bun.lockb', 'bunfig.toml']) {
  await assert.rejects(access(resolve(npmDirectory, bunFile)), undefined, `${bunFile} must not exist`);
}

process.stdout.write('verified Cadder npm package manifests\n');
