import assert from 'node:assert/strict';
import test from 'node:test';

import { selectPlatformTarget } from '../lib/platforms.js';

test('selects every supported platform package', () => {
  assert.equal(selectPlatformTarget({ platform: 'win32', arch: 'x64' }).directory, 'win32-x64');
  assert.equal(selectPlatformTarget({ platform: 'linux', arch: 'x64', libc: 'glibc' }).directory, 'linux-x64-gnu');
  assert.equal(selectPlatformTarget({ platform: 'darwin', arch: 'x64' }).directory, 'darwin-x64');
  assert.equal(selectPlatformTarget({ platform: 'darwin', arch: 'arm64' }).directory, 'darwin-arm64');
});

test('rejects unsupported operating systems, architectures, and C libraries', () => {
  assert.throws(
    () => selectPlatformTarget({ platform: 'linux', arch: 'x64', libc: 'musl' }),
    /linux\/x64\/musl/,
  );
  assert.throws(() => selectPlatformTarget({ platform: 'win32', arch: 'arm64' }), /win32\/arm64/);
  assert.throws(() => selectPlatformTarget({ platform: 'freebsd', arch: 'x64' }), /freebsd\/x64/);
});
