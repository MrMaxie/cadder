import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'yaml';

const npmDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const projectDirectory = resolve(npmDirectory, '..');
const workflowPath = resolve(projectDirectory, '.github', 'workflows', 'npm.yml');
const source = await readFile(workflowPath, 'utf8');
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

for (const match of source.matchAll(/uses:\s+([^\s#]+)/g)) {
  assert.match(match[1], /^[^@]+@[0-9a-f]{40}$/, `Action is not pinned to a full commit SHA: ${match[1]}`);
}

process.stdout.write('verified npm workflow structure and stage-only permissions\n');
