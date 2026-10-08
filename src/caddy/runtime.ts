import { randomUUID } from 'node:crypto';
import { unlink } from 'node:fs/promises';
import { join } from 'node:path';
import type { CaddyConfig, PortResult, RuntimeOwner } from '../contracts/ports.ts';
import type {
  ConfigurationApplyOutcome,
  ConfigurationRuntime,
} from '../daemon/configuration-transactions.ts';
import { OwnedCommandError } from '../platform/owned-command.ts';
import {
  assertProtected,
  assertRuntimeDescendant,
  createProtectedFile,
} from '../platform/runtime-security.ts';
import { CaddyAdminClient } from './admin-client.ts';
import type { SecureAdminPolicy } from './admin-policy.ts';
import type { RealCaddyResolver } from './resolver.ts';
import { CaddyConfigValidator } from './validation.ts';

export type CaddyRuntimeOptions = Readonly<{
  startupMs?: number;
  stopMs?: number;
  pollMs?: number;
}>;
type Execution = {
  abort: AbortController;
  settled: Promise<void>;
  completed: boolean;
  started: boolean;
  clean: boolean;
  staged: string;
};
const rejected: ConfigurationApplyOutcome = Object.freeze({ status: 'definitely-rejected' });
const ambiguous: ConfigurationApplyOutcome = Object.freeze({ status: 'ambiguous' });
const applied: ConfigurationApplyOutcome = Object.freeze({ status: 'applied' });

function bound(value: number | undefined, fallback: number): number {
  const result = value ?? fallback;
  if (!Number.isInteger(result) || result < 1 || result > 30_000)
    throw new Error('Invalid Caddy lifecycle bounds.');
  return result;
}
function unavailable(): PortResult<never> {
  return {
    ok: false,
    error: {
      kind: 'caddyRuntime',
      code: 'caddy_runtime_unavailable',
      message: 'Cannot prove the owned Caddy runtime state.',
      guidance: null,
      retryable: false,
      requestId: null,
    },
  };
}

/** One caller-owned resolver, one prepared identity, one owned execution. No automatic restart. */
export class CaddyRuntime implements ConfigurationRuntime {
  readonly #policy: SecureAdminPolicy;
  readonly #owner: RuntimeOwner;
  readonly #admin: CaddyAdminClient;
  readonly #validator: CaddyConfigValidator;
  readonly #startupMs: number;
  readonly #stopMs: number;
  readonly #pollMs: number;
  #execution: Execution | undefined;
  #uncertain = false;
  #closed = false;
  #tail: Promise<void> = Promise.resolve();
  #operation: AbortController | undefined;
  #closing: Promise<void> | undefined;

  constructor(
    policy: SecureAdminPolicy,
    owner: RuntimeOwner,
    private readonly resolver: RealCaddyResolver,
    options: CaddyRuntimeOptions = {},
  ) {
    // Copy all startup primitives; no project environment or mutable policy is retained.
    this.#policy = Object.freeze({
      config: Object.freeze({
        adaptedConfig: Object.freeze({ ...policy.config.adaptedConfig }),
        effectiveConfigHash: policy.config.effectiveConfigHash,
      }),
      paths: Object.freeze({ ...policy.paths }),
      environment: Object.freeze({ ...policy.environment }),
      client: Object.freeze({ ...policy.client }),
    });
    this.#owner = Object.freeze({ ...owner });
    this.#startupMs = bound(options.startupMs, 30_000);
    this.#stopMs = bound(options.stopMs, 5000);
    this.#pollMs = bound(options.pollMs, 50);
    this.#admin = new CaddyAdminClient(this.#policy);
    this.#validator = new CaddyConfigValidator(resolver, { env: this.#policy.environment });
  }

  apply(config: CaddyConfig | null): Promise<ConfigurationApplyOutcome> {
    if (this.#closed) return Promise.resolve(rejected);
    let candidate: CaddyConfig | null = null;
    if (config !== null) {
      try {
        // Snapshot before entering the queue, and refuse noncanonical/security-changing input.
        candidate = Object.freeze({
          adaptedConfig: Object.freeze({ ...config.adaptedConfig }),
          effectiveConfigHash: config.effectiveConfigHash,
        });
        if (!this.#admin.accepts(candidate)) return Promise.resolve(rejected);
      } catch {
        return Promise.resolve(rejected);
      }
    }
    return this.#enqueue(async (signal) => {
      if (candidate === null) return await this.#stop(signal);
      if (this.#uncertain) return ambiguous;
      if (this.#execution?.completed) {
        if (!(await this.#release())) return ambiguous;
      }
      if (this.#execution) return await this.#admin.apply(candidate, signal);
      return await this.#start(candidate, signal);
    }, rejected);
  }

  readActive(): Promise<PortResult<string | null>> {
    return this.#enqueue(async (signal) => {
      if (this.#uncertain || this.#execution?.completed) return unavailable();
      if (!this.#execution) return { ok: true, value: null };
      const observed = await this.#admin.readActive(signal);
      // An exit during observation cannot be cached as successful active state.
      return this.#execution.completed ? unavailable() : observed;
    }, unavailable());
  }

  /** Refuse admission immediately, cancel pending admin/start work, then drain native work/staging. */
  close(): Promise<void> {
    if (this.#closing) return this.#closing;
    this.#closed = true;
    this.#operation?.abort();
    if (this.#execution && !this.#execution.started) this.#execution.abort.abort();
    this.#closing = (async () => {
      await this.#tail;
      try {
        const result = await this.#stop(new AbortController().signal);
        if (result.status !== 'applied')
          throw new Error('Owned Caddy teardown could not be proved.');
      } finally {
        this.#admin.close();
      }
    })();
    return this.#closing;
  }

  #enqueue<T>(operation: (signal: AbortSignal) => Promise<T>, refused: T): Promise<T> {
    if (this.#closed) return Promise.resolve(refused);
    const result = this.#tail.then(async () => {
      if (this.#closed) return refused;
      const abort = new AbortController();
      this.#operation = abort;
      try {
        return await operation(abort.signal);
      } finally {
        this.#operation = undefined;
      }
    });
    this.#tail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  async #roots(signal: AbortSignal): Promise<void> {
    const paths = this.#policy.paths;
    for (const name of ['data', 'config', 'scratch', 'home'] as const) {
      if (paths[name] !== join(paths.base, name)) throw new Error('Invalid runtime roots.');
    }
    if (
      paths.defaultStorage !== join(paths.data, 'caddy') ||
      paths.autosaveDirectory !== join(paths.config, 'caddy')
    )
      throw new Error('Invalid runtime roots.');
    for (const path of [paths.base, paths.data, paths.config, paths.scratch, paths.home]) {
      if (signal.aborted) return;
      await assertProtected(path, this.#owner, true);
    }
    if (signal.aborted) return;
    await assertRuntimeDescendant(paths.data, paths.defaultStorage, this.#owner, true);
    if (signal.aborted) return;
    await assertRuntimeDescendant(paths.config, paths.autosaveDirectory, this.#owner, true);
  }

  async #start(candidate: CaddyConfig, outer: AbortSignal): Promise<ConfigurationApplyOutcome> {
    const abort = new AbortController();
    const cancel = (): void => abort.abort();
    outer.addEventListener('abort', cancel, { once: true });
    if (outer.aborted) cancel();
    const timer = setTimeout(cancel, this.#startupMs);
    let staged: string | undefined;
    try {
      await this.#roots(abort.signal);
      if (abort.signal.aborted) return rejected;
      // Persisted TLS identity authenticates a peer, not ownership of this execution.
      // Refuse an existing endpoint rather than adopting or stopping its process.
      if ((await this.#admin.readActive(abort.signal)).ok || abort.signal.aborted) return rejected;
      const validation = await this.#validator.validate(candidate, abort.signal);
      if (!validation.ok || abort.signal.aborted) return rejected;
      const image = await this.resolver.pin(abort.signal);
      if (abort.signal.aborted) return rejected;
      const path = join(this.#policy.paths.scratch, `caddy-${randomUUID()}.json`);
      if (!(await createProtectedFile(path, this.#owner, candidate.adaptedConfig.body)))
        return rejected;
      staged = path;
      await assertProtected(path, this.#owner);
      if (abort.signal.aborted) return rejected;
      const execution: Execution = {
        abort: new AbortController(),
        completed: false,
        started: false,
        clean: false,
        staged: path,
        settled: Promise.resolve(),
      };
      this.#execution = execution;
      staged = undefined; // The execution now owns staging until positive settlement.
      let notifyStarted!: () => void;
      const ready = new Promise<void>((resolve) => {
        notifyStarted = resolve;
      });
      execution.settled = image
        .run(['run', '--config', path], {
          longLived: true,
          signal: execution.abort.signal,
          env: this.#policy.environment,
          cwd: this.#policy.paths.scratch,
          onStarted: () => {
            execution.started = true;
            notifyStarted();
          },
        })
        .then(
          () => {
            execution.clean = true;
          },
          (error: unknown) => {
            execution.clean = error instanceof OwnedCommandError && error.code === 'abort';
          },
        )
        .then(() => {
          execution.completed = true;
        });
      void execution.settled.then(cancel); // Early exit aborts a pending startup readback.
      await this.#waitForStart(execution, ready, abort.signal);
      while (execution.started && !abort.signal.aborted && !execution.completed) {
        const observed = await this.#admin.readActive(abort.signal);
        if (observed.ok) {
          if (
            observed.value === candidate.effectiveConfigHash &&
            !execution.completed &&
            !abort.signal.aborted
          )
            return applied;
          break; // Authenticated mismatch is a failed startup, not a retry receipt.
        }
        await this.#pause(abort.signal);
      }
      execution.abort.abort();
      await execution.settled;
      await this.#release();
      return ambiguous;
    } catch {
      if (this.#execution) {
        this.#execution.abort.abort();
        await this.#execution.settled;
        await this.#release();
        return ambiguous;
      }
      return rejected;
    } finally {
      clearTimeout(timer);
      outer.removeEventListener('abort', cancel);
      if (staged !== undefined) {
        try {
          await assertProtected(staged, this.#owner);
          await unlink(staged);
        } catch {
          this.#uncertain = true;
        }
      }
    }
  }

  #waitForStart(execution: Execution, ready: Promise<void>, signal: AbortSignal): Promise<void> {
    if (signal.aborted || execution.completed) return Promise.resolve();
    return new Promise((resolve) => {
      const finish = (): void => {
        signal.removeEventListener('abort', finish);
        resolve();
      };
      signal.addEventListener('abort', finish, { once: true });
      void ready.then(finish);
      void execution.settled.then(finish);
    });
  }

  #pause(signal: AbortSignal): Promise<void> {
    if (signal.aborted) return Promise.resolve();
    return new Promise((resolve) => {
      const finish = (): void => {
        clearTimeout(timer);
        signal.removeEventListener('abort', finish);
        resolve();
      };
      const timer = setTimeout(finish, this.#pollMs);
      signal.addEventListener('abort', finish, { once: true });
    });
  }

  async #stop(signal: AbortSignal): Promise<ConfigurationApplyOutcome> {
    const execution = this.#execution;
    if (!execution) return this.#uncertain ? ambiguous : applied;
    if (execution.started && !execution.completed && !signal.aborted) {
      const grace = new AbortController();
      const cancel = (): void => grace.abort();
      signal.addEventListener('abort', cancel, { once: true });
      const timer = setTimeout(cancel, this.#stopMs);
      try {
        await this.#admin.stop(grace.signal);
        while (!execution.completed && !grace.signal.aborted) await this.#pause(grace.signal);
      } finally {
        clearTimeout(timer);
        signal.removeEventListener('abort', cancel);
      }
    }
    execution.abort.abort();
    await execution.settled;
    return (await this.#release()) ? applied : ambiguous;
  }

  async #release(): Promise<boolean> {
    const execution = this.#execution;
    if (!execution?.completed || !execution.clean) {
      this.#uncertain = true;
      return false;
    }
    try {
      await assertProtected(execution.staged, this.#owner);
      await unlink(execution.staged);
      this.#execution = undefined;
      this.#uncertain = false;
      return true;
    } catch {
      this.#uncertain = true;
      return false;
    }
  }
}
