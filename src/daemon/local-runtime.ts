import { resolvePaths, type RuntimePaths } from './paths.ts';
import { acquireRuntimeLock } from './runtime-lock.ts';
import { serveIpc } from './ipc-server.ts';
import { prepareRuntime, runtimeOwner } from '../platform/runtime-security.ts';
import type { RpcRequest } from '../protocol/rpc.ts';
import { CadderError } from '../protocol/errors.ts';

export interface LocalRuntime {
  paths: RuntimePaths;
  recovered: boolean;
  stop(): Promise<void>;
}

export async function startLocalRuntime(
  options: { runtimeDir?: string; profile?: string; runtimeOwner?: number },
  handler: (request: RpcRequest) => Promise<unknown>,
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
          try {
            await listener.close();
          } finally {
            await lock.release();
          }
        })();
        return stopping;
      },
    };
  } catch (error) {
    await lock.release();
    throw error;
  }
}
