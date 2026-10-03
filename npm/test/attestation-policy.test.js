import assert from 'node:assert/strict';
import test from 'node:test';

import {
  attestationVerifyArguments,
  releaseRepository,
  releaseSignerWorkflow,
} from '../scripts/attestation-policy.js';

const provenance = {
  sourceRef: 'refs/tags/v1.0.1',
  sourceDigest: 'a'.repeat(40),
  signerWorkflow: releaseSignerWorkflow,
};

test('binds verification to the exact release identity', () => {
  assert.deepEqual(attestationVerifyArguments('artifact.zip', provenance), [
    'attestation',
    'verify',
    'artifact.zip',
    '--repo',
    releaseRepository,
    '--source-ref',
    provenance.sourceRef,
    '--source-digest',
    provenance.sourceDigest,
    '--signer-workflow',
    releaseSignerWorkflow,
  ]);
});

test('rejects a non-release ref, invalid commit, or another signer workflow', () => {
  assert.throws(() =>
    attestationVerifyArguments('artifact.zip', {
      ...provenance,
      sourceRef: 'refs/heads/master',
    }),
  );
  assert.throws(() =>
    attestationVerifyArguments('artifact.zip', {
      ...provenance,
      sourceDigest: 'not-a-commit',
    }),
  );
  assert.throws(() =>
    attestationVerifyArguments('artifact.zip', {
      ...provenance,
      signerWorkflow: 'MrMaxie/Cadder/.github/workflows/other.yml',
    }),
  );
});
