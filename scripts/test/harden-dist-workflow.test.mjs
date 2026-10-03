import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import test from 'node:test';

import { validateHardenedWorkflow } from '../harden-dist-workflow.mjs';

const workflowPath = resolve('.github', 'workflows', 'release.yml');

test('accepts the committed hardened release workflow', async () => {
  validateHardenedWorkflow(await readFile(workflowPath, 'utf8'));
});

test('rejects repository write permission at the workflow root', async () => {
  const source = (await readFile(workflowPath, 'utf8')).replace(
    'permissions:\n  "contents": "read"',
    'permissions:\n  "contents": "write"',
  );
  assert.throws(() => validateHardenedWorkflow(source));
});

test('rejects publication tokens outside the announce job', async () => {
  const source = `${await readFile(workflowPath, 'utf8')}\nGH_TOKEN: \${{ secrets.GITHUB_TOKEN }}\n`;
  assert.throws(() => validateHardenedWorkflow(source));
});

test('rejects direct release-tag interpolation in shell source', async () => {
  const source = (await readFile(workflowPath, 'utf8')).replace(
    'gh release create "$RELEASE_TAG"',
    'gh release create "${{ github.ref_name }}"',
  );
  assert.throws(() => validateHardenedWorkflow(source));
});

test('rejects missing release-tag validation', async () => {
  const source = (await readFile(workflowPath, 'utf8')).replace(
    'if [[ ! "$RELEASE_TAG" =~ ^v',
    'if [[ "$RELEASE_TAG" =~ ^v',
  );
  assert.throws(() => validateHardenedWorkflow(source));
});

test('rejects cargo-dist commands without the reviewed workflow override', async () => {
  const source = (await readFile(workflowPath, 'utf8')).replace(
    'dist plan --allow-dirty',
    'dist plan',
  );
  assert.throws(() => validateHardenedWorkflow(source));
});
