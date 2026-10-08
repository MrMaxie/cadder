import { z } from 'zod';
import { assertCandidate } from '../caddy/preparation.ts';
import type {
  CaddyConfig,
  CaddyPort,
  DesiredState,
  PortResult,
  StoragePort,
} from '../contracts/ports.ts';
import { registrationSchema, type EntrypointRegistration } from '../protocol/dto.ts';
import { RpcError, type ProtocolError } from '../protocol/errors.ts';

export type ConfigurationSnapshot = Readonly<{
  config: CaddyConfig | null;
  desiredState: DesiredState;
  registrations: readonly EntrypointRegistration[];
}>;

/** Rejection guarantees no transition; throws and unrecognized results are uncertain. */
export type ConfigurationApplyOutcome = Readonly<{
  status: 'applied' | 'definitely-rejected' | 'ambiguous';
}>;

/** Required owned-runtime backend; null means a real transition to idle, not a stub. */
export interface ConfigurationRuntime {
  apply(config: CaddyConfig | null): Promise<ConfigurationApplyOutcome>;
  /**
   * Independent protected-runtime observation, never a cached apply receipt.
   * Null is positively observed idle; undeterminable state must return ok:false, not null.
   */
  readActive(): Promise<PortResult<string | null>>;
}

export type PrepareConfiguration = (
  committed: ConfigurationSnapshot,
) => PortResult<ConfigurationSnapshot> | Promise<PortResult<ConfigurationSnapshot>>;

const applyOutcomeSchema = z.strictObject({
  status: z.enum(['applied', 'definitely-rejected', 'ambiguous']),
});

const configurationSnapshotSchema = z.strictObject({
  config: z
    .strictObject({
      effectiveConfigHash: z.string(),
      adaptedConfig: z.strictObject({ format: z.literal('json'), body: z.string() }),
    })
    .nullable(),
  desiredState: z.strictObject({
    projects: z.array(
      z.strictObject({
        projectKey: z.string(),
        sourceWorkingDirectory: z.string(),
        sourceConfigPath: z.string(),
        enabled: z.boolean(),
        domains: z.array(
          z.strictObject({
            canonicalDomain: z.string(),
            upstream: z.string().nullable(),
            enabled: z.boolean(),
          }),
        ),
      }),
    ),
  }),
  registrations: z.array(registrationSchema),
});

function freeze<T>(value: T): T {
  if (value !== null && typeof value === 'object') {
    for (const child of Object.values(value)) freeze(child);
    Object.freeze(value);
  }
  return value;
}

function copySnapshot(value: ConfigurationSnapshot): ConfigurationSnapshot {
  const copy = configurationSnapshotSchema.parse(structuredClone(value));
  if (copy.config !== null) assertCandidate(copy.config);
  return freeze(copy);
}

function diagnostic(kind: ProtocolError['kind'], code: string, message: string): ProtocolError {
  return { kind, code, message, guidance: null, retryable: false, requestId: null };
}

function failure(kind: ProtocolError['kind'], code: string, message: string): PortResult<never> {
  return { ok: false, error: diagnostic(kind, code, message) };
}

/** One queue owns preparation through publication; initial state is already verified/durable. */
export class ConfigurationTransactions {
  #committed: ConfigurationSnapshot;
  #fence: ProtocolError | null = null;
  #closed = false;
  #tail: Promise<void> = Promise.resolve();
  readonly #validator: Pick<CaddyPort, 'validate'>;
  readonly #storage: Pick<StoragePort, 'persistDesiredState'>;
  readonly #runtime: ConfigurationRuntime;

  constructor(
    initial: ConfigurationSnapshot,
    validator: Pick<CaddyPort, 'validate'>,
    storage: Pick<StoragePort, 'persistDesiredState'>,
    runtime: ConfigurationRuntime,
  ) {
    try {
      this.#committed = copySnapshot(initial);
    } catch {
      throw new RpcError(
        diagnostic(
          'configuration',
          'config_candidate_invalid',
          'Invalid committed configuration snapshot.',
        ),
      );
    }
    this.#validator = validator;
    this.#storage = storage;
    this.#runtime = runtime;
  }

  readCommitted(): ConfigurationSnapshot {
    return freeze(structuredClone(this.#committed));
  }

  readFence(): ProtocolError | null {
    return this.#fence === null ? null : { ...this.#fence };
  }

  submit(prepare: PrepareConfiguration): Promise<PortResult<ConfigurationSnapshot>> {
    return this.#enqueue(async () => {
      if (this.#fence !== null)
        return failure(
          'caddyRuntime',
          'config_mutations_fenced',
          'Runtime reconciliation is required before another change.',
        );
      return this.#change(prepare);
    });
  }

  /** Queued proof of last-known-good only; abandoned candidates are never committed. */
  reconcile(): Promise<PortResult<ConfigurationSnapshot>> {
    return this.#enqueue(async () => {
      // Prepare the isolated return value before any restoration side effects.
      const success = { ok: true as const, value: this.readCommitted() };
      const observed = await this.#observe();
      if (
        (!observed.ok || observed.value !== this.#targetHash()) &&
        !(await this.#restoreCommitted())
      )
        return this.#fenced(
          'config_reconciliation_failed',
          'Cannot reconcile the last committed Caddy configuration.',
        );
      this.#fence = null;
      return success;
    });
  }

  /** Stops admissions and waits for owned queue work, without claiming resource teardown. */
  async close(): Promise<void> {
    this.#closed = true;
    await this.#tail;
  }

  #enqueue(
    operation: () => Promise<PortResult<ConfigurationSnapshot>>,
  ): Promise<PortResult<ConfigurationSnapshot>> {
    if (this.#closed)
      return Promise.resolve(
        failure(
          'shuttingDown',
          'config_transactions_closed',
          'Configuration transactions are closed.',
        ),
      );
    const result = this.#tail
      .then(operation)
      .catch(() =>
        this.#fenced('config_transaction_failed', 'Configuration transaction failed unexpectedly.'),
      );
    this.#tail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  #fenced(code: string, message: string): PortResult<ConfigurationSnapshot> {
    this.#fence = diagnostic('caddyRuntime', code, message);
    return { ok: false, error: { ...this.#fence } };
  }

  async #change(prepare: PrepareConfiguration): Promise<PortResult<ConfigurationSnapshot>> {
    let prepared: PortResult<ConfigurationSnapshot>;
    try {
      prepared = await prepare(this.readCommitted());
      if (prepared.ok !== true)
        return failure(
          'configuration',
          'config_prepare_failed',
          'Configuration preparation was rejected.',
        );
    } catch {
      return failure(
        'configuration',
        'config_prepare_failed',
        'Cannot prepare the configuration change.',
      );
    }
    let candidate: ConfigurationSnapshot;
    let success: PortResult<ConfigurationSnapshot>;
    try {
      candidate = copySnapshot(prepared.value);
      // Build the isolated result before side effects: publication itself cannot fail.
      success = { ok: true, value: freeze(structuredClone(candidate)) };
    } catch {
      return failure(
        'configuration',
        'config_candidate_invalid',
        'Invalid configuration transaction candidate.',
      );
    }
    if (candidate.config !== null) {
      try {
        const validation = await this.#validator.validate(
          freeze(structuredClone(candidate.config)),
        );
        if (validation.ok !== true)
          return failure(
            'configuration',
            'config_validation_failed',
            'Caddy configuration validation failed.',
          );
        if (validation.value.effectiveConfigHash !== candidate.config.effectiveConfigHash)
          return failure(
            'configuration',
            'config_validation_mismatch',
            'Caddy validation did not identify the complete candidate.',
          );
      } catch {
        return failure(
          'configuration',
          'config_validation_failed',
          'Caddy configuration validation failed.',
        );
      }
    }
    const outcome = await this.#apply(candidate.config);
    if (outcome === 'definitely-rejected')
      return failure(
        'caddyRuntime',
        'config_apply_rejected',
        'Caddy definitely rejected the configuration change.',
      );
    if (outcome !== 'applied')
      return this.#fenced('config_apply_uncertain', 'Caddy application outcome is uncertain.');
    const observed = await this.#observe();
    if (!observed.ok || observed.value !== (candidate.config?.effectiveConfigHash ?? null))
      return this.#fenced(
        'config_verification_failed',
        'Cannot verify the active Caddy configuration.',
      );
    let persisted = false;
    try {
      const result = await this.#storage.persistDesiredState(
        freeze(structuredClone(candidate.desiredState)),
      );
      persisted = result.ok === true;
    } catch {
      // STO-002 dependency: a thrown/rejected persist leaves previous durable intent intact.
    }
    if (!persisted) {
      if (!(await this.#restoreCommitted()))
        this.#fenced(
          'config_rollback_failed',
          'Cannot restore and verify the last committed Caddy configuration.',
        );
      return failure(
        'storage',
        'config_persist_failed',
        'Cannot persist the configuration change.',
      );
    }
    this.#committed = candidate;
    return success;
  }

  #targetHash(): string | null {
    return this.#committed.config?.effectiveConfigHash ?? null;
  }

  async #apply(config: CaddyConfig | null): Promise<ConfigurationApplyOutcome['status']> {
    try {
      const outcome = await this.#runtime.apply(freeze(structuredClone(config)));
      return applyOutcomeSchema.parse(outcome).status;
    } catch {
      return 'ambiguous';
    }
  }

  async #observe(): Promise<PortResult<string | null>> {
    try {
      const observed = await this.#runtime.readActive();
      if (observed.ok === true && (observed.value === null || typeof observed.value === 'string'))
        return { ok: true, value: observed.value };
    } catch {
      // Arbitrary backend exception text cannot cross the ProtocolError boundary.
    }
    return failure(
      'caddyRuntime',
      'config_verification_failed',
      'Cannot observe the active Caddy configuration.',
    );
  }

  async #restoreCommitted(): Promise<boolean> {
    if ((await this.#apply(this.#committed.config)) !== 'applied') return false;
    const observed = await this.#observe();
    return observed.ok && observed.value === this.#targetHash();
  }
}
