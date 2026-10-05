import { randomBytes } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import { DatabaseSync } from 'node:sqlite';
import type { RuntimePaths } from './paths.ts';
import type { RuntimeOwner } from '../platform/runtime-security.ts';
import { assertProtected, createProtectedFile } from '../platform/runtime-security.ts';
import { CadderError, errorCode } from '../protocol/errors.ts';
import { PROTOCOL_VERSION, SECURITY_POLICY_VERSION, VERSION } from '../protocol/version.ts';

export interface RuntimeLock {
  recovered: boolean;
  secret: Buffer;
  release(): Promise<void>;
}

export async function acquireRuntimeLock(
  paths: RuntimePaths,
  owner: RuntimeOwner,
): Promise<RuntimeLock> {
  await assertProtected(paths.directory, owner, true);
  await createProtectedFile(paths.lock, owner);
  const database = new DatabaseSync(paths.lock, { timeout: 0 });
  try {
    database.exec('PRAGMA journal_mode=DELETE; BEGIN EXCLUSIVE;');
  } catch {
    database.close();
    throw new CadderError(
      'runtime-already-running',
      'Runtime is already locked. Use cadder daemon status or cadder daemon shutdown.',
    );
  }
  try {
    let recovered = false;
    try {
      await readFile(paths.metadata);
      recovered = true;
    } catch (error) {
      if (errorCode(error) !== 'ENOENT') throw error;
    }
    await createProtectedFile(paths.secret, owner, randomBytes(32));
    const secret = await readFile(paths.secret);
    if (secret.length !== 32)
      throw new CadderError('invalid-runtime-secret', 'Runtime secret must contain 32 bytes.');
    await createProtectedFile(paths.metadata, owner);
    await writeFile(
      paths.metadata,
      JSON.stringify({
        version: VERSION,
        protocolVersion: PROTOCOL_VERSION,
        securityPolicyVersion: SECURITY_POLICY_VERSION,
        processId: process.pid,
        acquiredAt: new Date().toISOString(),
        owner: owner.id,
        elevated: owner.elevated,
        runtimeDir: paths.directory,
        profile: paths.profile,
        endpoint: paths.endpoint,
        executable: process.execPath,
      }),
    );
    let released = false;
    return {
      recovered,
      secret,
      async release() {
        if (released) return;
        released = true;
        try {
          await unlink(paths.metadata).catch((error: unknown) => {
            if (errorCode(error) !== 'ENOENT') throw error;
          });
        } finally {
          database.exec('ROLLBACK;');
          database.close();
          secret.fill(0);
        }
      },
    };
  } catch (error) {
    database.close();
    throw error;
  }
}
