import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'yaml';

const npmDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const projectDirectory = resolve(npmDirectory, '..');
const workflowPath = resolve(projectDirectory, '.github', 'workflows', 'npm.yml');
const ciWorkflowPath = resolve(projectDirectory, '.github', 'workflows', 'ci.yml');
const source = await readFile(workflowPath, 'utf8');
const ciSource = await readFile(ciWorkflowPath, 'utf8');
const stageSource = await readFile(resolve(npmDirectory, 'scripts', 'stage-release.js'), 'utf8');
const workflow = parse(source);

assert.deepEqual(workflow.on.release.types, ['published']);
assert.ok(workflow.on.workflow_dispatch, 'The npm workflow must expose a non-publishing manual dry run.');
assert.equal(workflow.on.workflow_dispatch.inputs.stage.default, false);
assert.equal(workflow.permissions.contents, 'read');
assert.equal(workflow.jobs.stage.environment, 'npm-production');
assert.equal(workflow.jobs.stage.permissions.contents, 'read');
assert.equal(workflow.jobs.stage.permissions['id-token'], 'write');
assert.match(workflow.jobs.stage.if, /github\.event_name == 'release'/);
assert.match(workflow.jobs.stage.if, /inputs\.stage == true/);
assert.equal(source.includes('NPM_TOKEN'), false, 'The npm workflow must not reference NPM_TOKEN.');
assert.equal(source.includes('NODE_AUTH_TOKEN'), false, 'The npm workflow must not reference NODE_AUTH_TOKEN.');
assert.match(source, /node npm\/scripts\/stage-release\.js/);
assert.match(stageSource, /['"]stage['"]\s*,\s*['"]publish['"]/, 'The stage script must use npm stage publish.');
assert.match(stageSource, /['"]publish['"]\s*,\s*archive/, 'The stage script must publish only the verified tarball path.');
assert.match(stageSource, /['"]--provenance['"]/, 'The stage script must request npm provenance.');
assert.equal(source.includes('npm publish '), false, 'The npm workflow must not publish directly.');

for (const jobName of ['prepare', 'verify-native', 'stage']) {
  assert.ok(workflow.jobs[jobName], `Missing ${jobName} job.`);
}

const setupNodeAction = 'actions/setup-node@2028fbc5c25fe9cf00d9f06a71cc4710d4507903';
const nubInstallCommand = 'npm install --global @nubjs/nub@0.7.5';

for (const [workflowName, workflowSource, expectedSetupCount, expectedNubInstallCount] of [
  ['CI', ciSource, 1, 1],
  ['npm distribution', source, 3, 1],
]) {
  assert.equal(
    workflowSource.includes('nubjs/setup-nub@'),
    false,
    `${workflowName} must not depend on an action rejected by the repository allowlist.`,
  );
  assert.equal(
    workflowSource.split(setupNodeAction).length - 1,
    expectedSetupCount,
    `${workflowName} must pin the expected Node setup action.`,
  );
  assert.equal(
    workflowSource.split(nubInstallCommand).length - 1,
    expectedNubInstallCount,
    `${workflowName} must install the pinned Nub CLI only where it is used.`,
  );
}

for (const match of source.matchAll(/uses:\s+([^\s#]+)/g)) {
  assert.match(match[1], /^[^@]+@[0-9a-f]{40}$/, `Action is not pinned to a full commit SHA: ${match[1]}`);
}

process.stdout.write('verified npm workflow structure and stage-only permissions\n');
