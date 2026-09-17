import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { selectPlatformTarget } from '../lib/platforms.js';
import { commandNames, parseArguments, requiredArgument, runNpm } from './package-utils.js';

async function runInstalledCommand(consumer, command, argumentsValue) {
  return runNpm(['exec', '--offline', '--', command, ...argumentsValue], { cwd: consumer });
}

const argumentsMap = parseArguments(process.argv.slice(2));
const tarballs = resolve(requiredArgument(argumentsMap, 'tarballs'));
const metadata = JSON.parse(await readFile(resolve(tarballs, 'tarballs.json'), 'utf8'));
const target = selectPlatformTarget();
const platformPackage = metadata.packages.find((entry) => entry.name === target.packageName);
const rootPackage = metadata.packages.find((entry) => entry.name === 'cadder');
assert.ok(platformPackage, `Missing ${target.packageName} tarball metadata.`);
assert.ok(rootPackage, 'Missing cadder tarball metadata.');

const workspace = await mkdtemp(resolve(tmpdir(), 'cadder-npm-consumer-'));
try {
  const consumer = resolve(workspace, 'consumer');
  const cache = resolve(workspace, 'cache');
  await mkdir(consumer);
  await mkdir(cache);
  await writeFile(resolve(consumer, 'package.json'), `${JSON.stringify({ private: true }, null, 2)}\n`);
  await runNpm(
    [
      'install',
      '--ignore-scripts',
      '--no-audit',
      '--no-fund',
      '--no-package-lock',
      resolve(tarballs, rootPackage.file),
      resolve(tarballs, platformPackage.file),
    ],
    { cwd: consumer, env: { ...process.env, npm_config_cache: cache } },
  );

  for (const command of commandNames) {
    const version = await runInstalledCommand(consumer, command, ['--version']);
    assert.equal(version.stdout.trim(), `${command} ${metadata.version}`);
    await runInstalledCommand(consumer, command, ['--help']);
  }
  process.stdout.write(`verified cadder@${metadata.version} in an isolated ${target.directory} consumer\n`);
} finally {
  await rm(workspace, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
