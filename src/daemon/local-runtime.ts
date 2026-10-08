import { resolvePaths, type RuntimePaths } from './paths.ts';
import { acquireRuntimeLock } from './runtime-lock.ts';
import { serveIpc } from './ipc-server.ts';
import { prepareRuntime, runtimeOwner } from '../platform/runtime-security.ts';
import type { RpcHandler } from '../protocol/rpc.ts';
import { CadderError } from '../protocol/errors.ts';

export interface LocalRuntime {
  paths: RuntimePaths;
  recovered: boolean;
  stop(): Promise<void>;
}

export async function startLocalRuntime(
  options: {
    runtimeDir?: string;
    profile?: string;
    runtimeOwner?: number;
    installationRoot?: string;
  },
  handler: RpcHandler,
  onDenied?: () => void,
): Promise<LocalRuntime> {
  if (process.getuid?.() === 0 && !options.runtimeDir) {
    throw new CadderError(
      'runtime-owner-required',
      'Root must specify --runtime-owner and --runtime-dir.',
    );
  }
  const owner = await runtimeOwner(options.runtimeOwner);
  const paths = resolvePaths(options);
  await prepareRuntime(paths.directory, owner);
  const lock = await acquireRuntimeLock(paths, owner);
  try {
    const listener = await serveIpc(paths, owner, lock.secret, handler, onDenied);
    let stopping: Promise<void> | undefined;
    return {
      paths,
      recovered: lock.recovered,
      stop() {
        stopping ??= (async () => {
          const failures: unknown[] = [];
          for (const finalize of [() => listener.close(), () => lock.release()]) {
            try {
              await finalize();
            } catch (error) {
              failures.push(error);
            }
          }
          if (failures.length === 1) throw failures[0];
          if (failures.length > 1)
            throw new AggregateError(failures, 'Local runtime shutdown failed.');
        })();
        return stopping;
      },
    };
  } catch (error) {
    const failures = [error];
    try {
      await lock.release();
    } catch (cleanupError) {
      failures.push(cleanupError);
    }
    if (failures.length > 1)
      throw new AggregateError(failures, 'Local runtime startup and cleanup failed.', {
        cause: error,
      });
    throw error;
  }
}
