import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { rpc } from '../../src/client/connection.ts';
import { resolvePaths } from '../../src/daemon/paths.ts';
import { prepareRuntime, runtimeOwner } from '../../src/platform/runtime-security.ts';

if (process.platform !== 'win32') throw new Error('This interactive gate requires Windows.');
const [action, runtimeDir] = process.argv.slice(2);
if (!runtimeDir)
  throw new Error('Pass prepare, elevated-status or shutdown and a test runtime directory.');
const paths = resolvePaths({ runtimeDir });
const owner = await runtimeOwner();

if (action === 'prepare' || action === 'elevated-status') {
  assert.equal(owner.elevated, false, 'Run the client from a non-elevated shell.');
}
if (action === 'prepare') {
  await prepareRuntime(paths.directory, owner);
  process.stdout.write('PREPARED\n');
} else if (action === 'elevated-status') {
  const discovery = JSON.parse(await readFile(paths.discovery, 'utf8')) as {
    elevated: boolean;
    owner: string;
  };
  assert.equal(discovery.elevated, true, 'The fixture daemon must be elevated.');
  assert.equal(discovery.owner, owner.id, 'The daemon must have the same owner SID.');
  const result = (await rpc(paths, 'status')) as { processId: number };
  assert.ok(result.processId > 0);
  process.stdout.write('SAME_OWNER_ELEVATED_CONTACT_PASSED\n');
} else if (action === 'shutdown') {
  assert.deepEqual(await rpc(paths, 'shutdown'), { stopped: true });
  process.stdout.write('STOPPED\n');
} else {
  throw new Error('Expected prepare, elevated-status or shutdown.');
}
