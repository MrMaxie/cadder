import assert from 'node:assert/strict';
import test from 'node:test';

import { archiveContentRoot, parseNpmPackMetadata } from '../scripts/package-utils.js';

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

test('accepts flat and cargo-dist wrapped archive contents', () => {
  const expectedFiles = ['LICENSE', 'cadder'];
  assert.equal(archiveContentRoot('cadder-linux.tar.xz', expectedFiles, expectedFiles), '');
  assert.equal(
    archiveContentRoot('cadder-linux.tar.xz', ['cadder-linux/LICENSE', 'cadder-linux/cadder'], expectedFiles),
    'cadder-linux',
  );
});

test('rejects unexpected archive wrappers and files', () => {
  const expectedFiles = ['LICENSE', 'cadder'];
  assert.equal(archiveContentRoot('cadder-linux.tar.xz', ['other/LICENSE', 'other/cadder'], expectedFiles), null);
  assert.equal(
    archiveContentRoot(
      'cadder-linux.tar.xz',
      ['cadder-linux/LICENSE', 'cadder-linux/cadder', 'cadder-linux/extra'],
      expectedFiles,
    ),
    null,
  );
});
