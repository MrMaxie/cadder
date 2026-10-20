import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';

// Pinned from caddyserver/caddy v2.11.4's published SHA-512 checksums.
const targets: Record<string, [string, string]> = {
  'win32-x64': [
    'windows_amd64.zip',
    'cd5ccfd86a4b40732cf715890d0dca5bf3f63adefec5a7914de85adf240c60ce7e5d2791631b88ef9758e46b23bb1730e020b9c5d696889740b284ffd4788e35',
  ],
  'linux-x64': [
    'linux_amd64.tar.gz',
    '8220d1f013b6f27510247b2360c9e0ca9f018feebd82515f07635318b34ff9777ccc8fd0b6e6f2486ce3a33fe389fbb7db12d05baa474f4587509fb4f5ebf1c9',
  ],
  'darwin-x64': [
    'mac_amd64.tar.gz',
    'e04eb10f9ce7e2e079bc9bff1bd5d3a3164888d1edbb1a49e5d15be4eab691b57e89ed36bb29c65ba43f1ba8d9279e0967b1003991c13fe4cb78384c3caf25de',
  ],
  'darwin-arm64': [
    'mac_arm64.tar.gz',
    '3190ae0df98b59ab4b6021556fa35adc3c526a4f3e138776b0eaec8a037cc26121cbbb1ad53453f565551b47d37d5ba4755e2c2c3652256737fe2ce9e53c8ec0',
  ],
};
const target = targets[`${process.platform}-${process.arch}`];
assert.ok(target, 'Unsupported native acceptance target.');
const root = await mkdtemp(join(tmpdir(), 'cadder-upstream-gate-'));
const filename = `caddy_2.11.4_${target[0]}`;
const response = await fetch(
  `https://github.com/caddyserver/caddy/releases/download/v2.11.4/${filename}`,
  { signal: AbortSignal.timeout(120000) },
);
assert.ok(response.ok, `Upstream acceptance download failed: ${response.status}`);
const bytes = Buffer.from(await response.arrayBuffer());
assert.equal(createHash('sha512').update(bytes).digest('hex'), target[1]);
const archive = join(root, filename);
await writeFile(archive, bytes);
await promisify(execFile)(
  process.platform === 'win32'
    ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe')
    : 'tar',
  ['-xf', archive, '-C', root],
);
process.stdout.write(join(root, process.platform === 'win32' ? 'caddy.exe' : 'caddy') + '\n');
