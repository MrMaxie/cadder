import assert from 'node:assert/strict';
import { rpc } from '../../src/client/connection.ts';
import { resolvePaths } from '../../src/daemon/paths.ts';
import { errorCode } from '../../src/protocol/errors.ts';

const runtimeDir = process.argv[2];
if (!runtimeDir) throw new Error('Pass the owner runtime directory.');
try {
  await rpc(resolvePaths({ runtimeDir }), 'query-state-request');
  throw new Error('An unauthorized account contacted the runtime.');
} catch (error) {
  assert.ok(
    ['unsafe-runtime-permissions', 'EACCES', 'EPERM'].includes(errorCode(error) ?? ''),
    'The account must be denied by the runtime owner boundary.',
  );
  process.stdout.write('DENIED\n');
}
