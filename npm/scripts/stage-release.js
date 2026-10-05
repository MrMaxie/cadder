import assert from 'node:assert/strict';
import { appendFile, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { platformTargets } from '../lib/platforms.js';
import { parseArguments, requiredArgument, runNpm, sha256 } from './package-utils.js';

const argumentsMap = parseArguments(process.argv.slice(2));
const tarballs = resolve(requiredArgument(argumentsMap, 'tarballs'));
const metadata = JSON.parse(await readFile(resolve(tarballs, 'tarballs.json'), 'utf8'));
assert.equal(metadata.attestationsVerified, true, 'Refusing to stage packages assembled without verified attestations.');
assert.equal(metadata.packages.length, 5, 'Expected four platform packages and one root package.');
const expectedOrder = [...platformTargets.map((target) => target.packageName), 'cadder'];
assert.deepEqual(metadata.approvalOrder, expectedOrder, 'Package approval order must be platform-first and root-last.');
assert.deepEqual(
  metadata.packages.map((packageMetadata) => packageMetadata.name),
  expectedOrder,
  'Tarball order must match the required approval order.',
);

const summary = ['## Cadder npm staging', '', `Version: ${metadata.version}`, '', 'Required 2FA approval order:', ''];
for (const [index, packageMetadata] of metadata.packages.entries()) {
  const archive = resolve(tarballs, packageMetadata.file);
  assert.equal(await sha256(archive), packageMetadata.sha256, `${packageMetadata.file} digest changed before staging.`);
  const result = await runNpm([
    'stage',
    'publish',
    archive,
    '--access',
    'public',
    '--provenance',
  ]);
  process.stdout.write(result.stdout);
  process.stderr.write(result.stderr);
  summary.push(`${index + 1}. ${packageMetadata.name}@${packageMetadata.version}`);
}

summary.push('', 'Review every staged tarball before approval. Approve all platform packages before cadder.');
if (process.env.GITHUB_STEP_SUMMARY) {
  await appendFile(process.env.GITHUB_STEP_SUMMARY, `${summary.join('\n')}\n`);
}
process.stdout.write(`staged ${metadata.packages.length} Cadder npm packages for maintainer review\n`);
