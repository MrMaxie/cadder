import { randomBytes } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import { DatabaseSync } from 'node:sqlite';
import type { RuntimePaths } from './paths.ts';
import type { RuntimeOwner } from '../platform/runtime-security.ts';
import { normalizeRuntimeCreationOwner } from '../platform/runtime-creation-owner.ts';
import {
  assertProtected,
  assertRuntimeDescendant,
  createProtectedFile,
} from '../platform/runtime-security.ts';
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
  const journal = `${paths.lock}-journal`;
  async function assertJournal(): Promise<void> {
    try {
      await assertRuntimeDescendant(paths.directory, journal, owner);
    } catch (error) {
      // Only this known leaf may be absent; root/chain/inspection failures stay fatal.
      if (
        errorCode(error) !== 'ENOENT' ||
        !(error && typeof error === 'object' && 'path' in error && error.path === journal)
      )
        throw error;
    }
  }
  await assertJournal();
  await normalizeRuntimeCreationOwner(owner);
  const database = new DatabaseSync(paths.lock, { timeout: 0 });
  let transaction = false;
  let metadataOwned = false;
  let secretCreated = false;
  let secret: Buffer | undefined;

  async function finalize(failedStart: boolean): Promise<unknown[]> {
    const failures: unknown[] = [];
    for (const path of [
      ...(metadataOwned ? [paths.metadata] : []),
      ...(failedStart && secretCreated ? [paths.secret] : []),
    ]) {
      try {
        await unlink(path);
      } catch (error) {
        if (errorCode(error) !== 'ENOENT') failures.push(error);
      }
    }
    try {
      if (transaction) database.exec('ROLLBACK;');
    } catch (error) {
      failures.push(error);
    }
    try {
      database.close();
    } catch (error) {
      failures.push(error);
    }
    secret?.fill(0);
    return failures;
  }

  try {
    try {
      database.exec('PRAGMA journal_mode=DELETE;');
      database.exec('BEGIN EXCLUSIVE;');
      transaction = true;
    } catch (error) {
      const sqliteCode =
        error &&
        typeof error === 'object' &&
        'errcode' in error &&
        typeof error.errcode === 'number'
          ? error.errcode & 0xff
          : undefined;
      if (sqliteCode === 5 || sqliteCode === 6) {
        throw new CadderError(
          'runtime-already-running',
          'Runtime is already locked. Use cadder daemon status or cadder daemon shutdown.',
        );
      }
      throw error;
    }
    // SQLite may have created the DELETE journal; exclusion remains held until release.
    await assertJournal();
    let recovered = false;
    try {
      await readFile(paths.metadata);
      recovered = true;
    } catch (error) {
      if (errorCode(error) !== 'ENOENT') throw error;
    }
    const generated = randomBytes(32);
    try {
      secretCreated = await createProtectedFile(paths.secret, owner, generated);
    } finally {
      generated.fill(0);
    }
    secret = await readFile(paths.secret);
    if (secret.length !== 32)
      throw new CadderError('invalid-runtime-secret', 'Runtime secret must contain 32 bytes.');
    await createProtectedFile(paths.metadata, owner);
    // Safe stale metadata becomes ours only after lock acquisition and path validation.
    metadataOwned = true;
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
    let releasing: Promise<void> | undefined;
    return {
      recovered,
      secret,
      release() {
        releasing ??= (async () => {
          const failures = await finalize(false);
          if (failures.length === 1) throw failures[0];
          if (failures.length > 1)
            throw new AggregateError(failures, 'Runtime lock cleanup failed.');
        })();
        return releasing;
      },
    };
  } catch (error) {
    const failures = await finalize(true);
    if (failures.length === 0) throw error;
    throw new AggregateError([error, ...failures], 'Runtime lock acquisition and cleanup failed.', {
      cause: error,
    });
  }
}
