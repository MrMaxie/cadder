import assert from 'node:assert/strict';
import { chmod, mkdtemp, mkdir, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, relative } from 'node:path';
import test from 'node:test';

import { removeLauncherFromPath } from '../lib/launcher.js';

test('removes only the PATH entry whose caddy command targets the npm launcher', async () => {
  const workspace = await mkdtemp(join(tmpdir(), 'cadder-path-test-'));
  try {
    const packageRoot = join(workspace, 'node_modules', 'cadder');
    const launcherPath = join(packageRoot, 'bin', 'caddy.js');
    const ownBin = join(workspace, 'node_modules', '.bin');
    const upstreamBin = join(workspace, 'upstream');
    await mkdir(dirname(launcherPath), { recursive: true });
    await mkdir(ownBin, { recursive: true });
    await mkdir(upstreamBin, { recursive: true });
    await writeFile(launcherPath, '#!/usr/bin/env node\n');

    if (process.platform === 'win32') {
      const relativeLauncher = relative(ownBin, launcherPath);
      await writeFile(join(ownBin, 'caddy.cmd'), `@node "%~dp0\\${relativeLauncher}" %*\n`);
      await writeFile(join(upstreamBin, 'caddy.exe'), 'not a real executable\n');
    } else {
      await chmod(launcherPath, 0o755);
      await symlink(launcherPath, join(ownBin, 'caddy'));
      await writeFile(join(upstreamBin, 'caddy'), '#!/bin/sh\nexit 0\n');
      await chmod(join(upstreamBin, 'caddy'), 0o755);
    }

    const separator = process.platform === 'win32' ? ';' : ':';
    const filtered = await removeLauncherFromPath({
      pathValue: [upstreamBin, ownBin].join(separator),
      launcherPath,
      platform: process.platform,
      pathDelimiter: separator,
      pathExt: '.COM;.EXE;.BAT;.CMD',
    });

    assert.deepEqual(filtered.split(separator), [upstreamBin]);
  } finally {
    await rm(workspace, { recursive: true, force: true });
  }
});

test('keeps a PATH entry whose caddy command is not the npm launcher', async () => {
  const workspace = await mkdtemp(join(tmpdir(), 'cadder-path-test-'));
  try {
    const launcherPath = join(workspace, 'package', 'bin', 'caddy.js');
    const upstreamBin = join(workspace, 'upstream');
    await mkdir(dirname(launcherPath), { recursive: true });
    await mkdir(upstreamBin, { recursive: true });
    await writeFile(launcherPath, '#!/usr/bin/env node\n');
    const upstream = join(upstreamBin, process.platform === 'win32' ? 'caddy.exe' : 'caddy');
    await writeFile(upstream, process.platform === 'win32' ? 'native fixture\n' : '#!/bin/sh\nexit 0\n');
    if (process.platform !== 'win32') await chmod(upstream, 0o755);

    const filtered = await removeLauncherFromPath({
      pathValue: upstreamBin,
      launcherPath,
      platform: process.platform,
      pathDelimiter: process.platform === 'win32' ? ';' : ':',
      pathExt: '.COM;.EXE;.BAT;.CMD',
    });

    assert.equal(filtered, upstreamBin);
  } finally {
    await rm(workspace, { recursive: true, force: true });
  }
});
