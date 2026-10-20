import { Worker } from 'node:worker_threads';
import { getAsset, isSea } from 'node:sea';
import { join } from 'node:path';
import type { DesiredState, PortResult, RuntimeOwner, StoragePort } from '../contracts/ports.ts';
import {
  assertProtected,
  assertRuntimeDescendant,
  createProtectedFile,
} from '../platform/runtime-security.ts';
import { errorCode, RpcError } from '../protocol/errors.ts';
import type { LogInput, StoredLog } from './schema.ts';
import { desiredStateSchema } from './schema.ts';

function failure(): PortResult<never> {
  return {
    ok: false,
    error: {
      kind: 'storage',
      code: 'storage_unavailable',
      message: 'Application storage is unavailable.',
      guidance: null,
      retryable: false,
      requestId: null,
    },
  };
}
type Reply = { id: number; ok: boolean; value?: unknown };

export class ApplicationStorage implements StoragePort {
  #nextId = 1;
  #pending = new Map<
    number,
    { resolve(value: unknown): void; reject(error: unknown): void; timer: NodeJS.Timeout }
  >();
  #closed = false;
  #closing: Promise<void> | undefined;
  readonly #exit: Promise<unknown>;
  private constructor(private readonly worker: Worker) {
    this.#exit = new Promise((resolve) => worker.once('exit', resolve));
    worker.on('message', (reply: Reply) => {
      const pending = this.#pending.get(reply.id);
      if (!pending) return;
      this.#pending.delete(reply.id);
      clearTimeout(pending.timer);
      if (reply.ok) pending.resolve(reply.value);
      else pending.reject(this.#error());
    });
    worker.on('error', () => this.#fail());
    worker.on('exit', () => this.#fail());
  }

  static async open(directory: string, owner: RuntimeOwner): Promise<ApplicationStorage> {
    await assertProtected(directory, owner, true);
    const path = join(directory, 'runtime.sqlite3');
    await createProtectedFile(path, owner);
    for (const suffix of ['-journal', '-wal', '-shm']) {
      try {
        await assertRuntimeDescendant(directory, path + suffix, owner);
        if (suffix !== '-journal') throw new Error('Unsupported application journal mode.');
      } catch (error) {
        if (errorCode(error) !== 'ENOENT') throw error;
      }
    }
    const url = isSea()
      ? new URL(
          'data:text/javascript,' + encodeURIComponent(getAsset('cadder-storage-worker', 'utf8')),
        )
      : new URL(import.meta.url.endsWith('.ts') ? './worker.ts' : './worker.js', import.meta.url);
    const worker = new Worker(url, { workerData: { path } });
    const storage = new ApplicationStorage(worker);
    try {
      await storage.#wait(0);
      await assertProtected(path, owner);
      return storage;
    } catch (error) {
      await worker.terminate();
      throw error;
    }
  }

  async loadDesiredState(): Promise<PortResult<DesiredState>> {
    try {
      return { ok: true, value: desiredStateSchema.parse(await this.#job('load')) };
    } catch {
      return failure();
    }
  }
  async persistDesiredState(state: DesiredState): Promise<PortResult<void>> {
    try {
      await this.#job('persist', desiredStateSchema.parse(state));
      return { ok: true, value: undefined };
    } catch {
      return failure();
    }
  }
  async append(entries: readonly LogInput[]): Promise<void> {
    await this.#job('append', entries);
  }
  async queryLogs(stream: LogInput['stream'], limit: number): Promise<StoredLog[]> {
    return (await this.#job('logs', stream, limit)) as StoredLog[];
  }
  close(): Promise<void> {
    this.#closing ??= (async () => {
      if (this.#closed) {
        await this.#exit;
        return;
      }
      const closing = this.#job('close');
      this.#closed = true;
      try {
        await closing;
        await this.#exit;
      } catch (error) {
        await this.worker.terminate();
        throw error;
      }
    })();
    return this.#closing;
  }
  #job(method: string, value?: unknown, limit?: number): Promise<unknown> {
    if (this.#closed || this.#pending.size >= 64) return Promise.reject(this.#error());
    const id = this.#nextId++;
    const result = this.#wait(id);
    try {
      this.worker.postMessage({ id, method, value, limit });
    } catch {
      this.#fail();
      void this.worker.terminate();
    }
    return result;
  }
  #wait(id: number): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#fail();
        void this.worker.terminate();
      }, 5000);
      this.#pending.set(id, { resolve, reject, timer });
    });
  }
  #error(): RpcError {
    const result = failure();
    if (result.ok) throw new Error('Unreachable.');
    return new RpcError(result.error);
  }
  #fail(): void {
    this.#closed = true;
    for (const pending of this.#pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(this.#error());
    }
    this.#pending.clear();
  }
}
