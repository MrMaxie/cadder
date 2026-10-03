import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'yaml';

const npmDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const projectDirectory = resolve(npmDirectory, '..');
const workflowPath = resolve(projectDirectory, '.github', 'workflows', 'npm.yml');
const ciWorkflowPath = resolve(projectDirectory, '.github', 'workflows', 'ci.yml');
const releaseWorkflowPath = resolve(projectDirectory, '.github', 'workflows', 'release.yml');
const source = await readFile(workflowPath, 'utf8');
const ciSource = await readFile(ciWorkflowPath, 'utf8');
const releaseSource = await readFile(releaseWorkflowPath, 'utf8');
const stageSource = await readFile(resolve(npmDirectory, 'scripts', 'stage-release.js'), 'utf8');
const assemblySource = await readFile(resolve(npmDirectory, 'scripts', 'assemble-release.js'), 'utf8');
const attestationPolicySource = await readFile(
  resolve(npmDirectory, 'scripts', 'attestation-policy.js'),
  'utf8',
);
const workflow = parse(source);
const releaseWorkflow = parse(releaseSource);

assert.deepEqual(workflow.on.release.types, ['published']);
assert.ok(workflow.on.workflow_dispatch, 'The npm workflow must expose a non-publishing manual dry run.');
assert.equal(workflow.on.workflow_dispatch.inputs.stage.default, false);
assert.equal(workflow.permissions.contents, 'read');
assert.equal(workflow.jobs.stage.environment, 'npm-production');
assert.equal(workflow.jobs.stage.permissions.contents, 'read');
assert.equal(workflow.jobs.stage.permissions['id-token'], 'write');
const stageSetupNode = workflow.jobs.stage.steps.find((step) => step.name === 'Set up Node.js');
assert.equal(stageSetupNode.with['registry-url'], 'https://registry.npmjs.org');
assert.equal(stageSetupNode.with['package-manager-cache'], false);
assert.match(workflow.jobs.stage.if, /github\.event_name == 'release'/);
assert.match(workflow.jobs.stage.if, /inputs\.stage == true/);
assert.equal(source.includes('NPM_TOKEN'), false, 'The npm workflow must not reference NPM_TOKEN.');
assert.equal(source.includes('NODE_AUTH_TOKEN'), false, 'The npm workflow must not reference NODE_AUTH_TOKEN.');
assert.match(source, /node npm\/scripts\/stage-release\.js/);
assert.match(stageSource, /['"]stage['"]\s*,\s*['"]publish['"]/, 'The stage script must use npm stage publish.');
assert.match(stageSource, /['"]publish['"]\s*,\s*archive/, 'The stage script must publish only the verified tarball path.');
assert.match(stageSource, /['"]--provenance['"]/, 'The stage script must request npm provenance.');
assert.equal(source.includes('npm publish '), false, 'The npm workflow must not publish directly.');
assert.match(source, /ref: \$\{\{ env\.RELEASE_TAG \}\}/);
assert.match(source, /git rev-parse HEAD\^\{commit\}/);
assert.match(source, /--source-ref "refs\/tags\/\$RELEASE_TAG"/);
assert.match(source, /--source-digest "\$\{\{ steps\.source\.outputs\.commit \}\}"/);
assert.match(source, /--signer-workflow "\$GITHUB_REPOSITORY\/\.github\/workflows\/release\.yml"/);
assert.match(assemblySource, /attestationVerifyArguments\(path, provenance\)/);
for (const option of ['--repo', '--source-ref', '--source-digest', '--signer-workflow']) {
  assert.ok(attestationPolicySource.includes(`'${option}'`), `Attestation policy must require ${option}.`);
}

for (const jobName of ['prepare', 'verify-native', 'stage']) {
  assert.ok(workflow.jobs[jobName], `Missing ${jobName} job.`);
}

assert.equal(releaseWorkflow.permissions.contents, 'read');
for (const jobName of ['plan', 'build-local-artifacts', 'build-global-artifacts', 'host']) {
  assert.equal(
    releaseWorkflow.jobs[jobName].env?.GH_TOKEN,
    undefined,
    `${jobName} must not receive a GitHub publication token.`,
  );
}
assert.deepEqual(releaseWorkflow.jobs.announce.permissions, {
  attestations: 'write',
  contents: 'write',
  'id-token': 'write',
});
assert.equal(releaseSource.includes('tag-flag'), false);
assert.equal(releaseSource.match(/github\.ref_name/g)?.length, 1);
assert.match(releaseSource, /gh release create "\$RELEASE_TAG"/);

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
