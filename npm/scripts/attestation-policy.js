import assert from 'node:assert/strict';

export const releaseRepository = 'MrMaxie/Cadder';
export const releaseSignerWorkflow = 'MrMaxie/Cadder/.github/workflows/release.yml';

export function attestationVerifyArguments(path, { sourceRef, sourceDigest, signerWorkflow }) {
  assert.match(sourceRef, /^refs\/tags\/v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/);
  assert.match(sourceDigest, /^[0-9a-f]{40}$/);
  assert.equal(signerWorkflow, releaseSignerWorkflow);
  return [
    'attestation',
    'verify',
    path,
    '--repo',
    releaseRepository,
    '--source-ref',
    sourceRef,
    '--source-digest',
    sourceDigest,
    '--signer-workflow',
    signerWorkflow,
  ];
}
