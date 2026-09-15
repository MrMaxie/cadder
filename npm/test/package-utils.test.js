import assert from 'node:assert/strict';
import test from 'node:test';

import { parseNpmPackMetadata } from '../scripts/package-utils.js';

const packageMetadata = { name: 'cadder', version: '1.0.0' };

test('reads npm pack metadata from supported npm output shapes', () => {
  assert.deepEqual(parseNpmPackMetadata(JSON.stringify([packageMetadata])), packageMetadata);
  assert.deepEqual(parseNpmPackMetadata(JSON.stringify({ cadder: packageMetadata })), packageMetadata);
});

test('rejects ambiguous npm pack metadata', () => {
  assert.throws(() => parseNpmPackMetadata('{}'), /returned 0 package entries/);
  assert.throws(
    () => parseNpmPackMetadata(JSON.stringify({ cadder: packageMetadata, other: packageMetadata })),
    /returned 2 package entries/,
  );
});
