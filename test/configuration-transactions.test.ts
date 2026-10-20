import { expect, it, vi } from 'vitest';
import { composeRoutePlan } from '../src/caddy/composition.ts';
import { configHash, maxCaddyConfigurationBytes } from '../src/caddy/preparation.ts';
import type { CaddyConfig, DesiredState, PortResult } from '../src/contracts/ports.ts';
import {
  ConfigurationTransactions,
  type ConfigurationApplyOutcome,
  type ConfigurationSnapshot,
  type PrepareConfiguration,
} from '../src/daemon/configuration-transactions.ts';
import { protocolErrorSchema, type ProtocolError } from '../src/protocol/errors.ts';
import { fakeRegistration } from './fixtures/rpc-data.ts';

function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function ok<T>(value: T): PortResult<T> {
  return { ok: true, value };
}
const secret = 'private project exception text';
const adapterError: ProtocolError = {
  kind: 'invalidInput',
  code: 'adapter_failed',
  message: secret,
  guidance: secret,
  retryable: false,
  requestId: null,
};
function rejected<T>(): PortResult<T> {
  return { ok: false, error: adapterError };
}
function config(body: string): CaddyConfig {
  return { adaptedConfig: { format: 'json', body }, effectiveConfigHash: configHash(body) };
}
function snapshot(label = 'old', idle = false): ConfigurationSnapshot {
  return {
    config: idle ? null : config(JSON.stringify({ fixture: label })),
    desiredState: {
      projects: [
        {
          projectKey: label,
          sourceWorkingDirectory: '/fixture',
          sourceConfigPath: '/fixture/Caddyfile',
          enabled: true,
          domains: [{ canonicalDomain: 'example.localhost', upstream: null, enabled: true }],
        },
      ],
    },
    registrations: [{ ...fakeRegistration(), registrationId: label }],
  };
}
function fixture(initial = snapshot()) {
  const phases: string[] = [];
  let active = initial.config?.effectiveConfigHash ?? null;
  let durable = structuredClone(initial.desiredState);
  const validate = vi.fn(async (target: CaddyConfig) => {
    phases.push('validate');
    return ok({ effectiveConfigHash: target.effectiveConfigHash, diagnostics: [] });
  });
  const apply = vi.fn(async (target: CaddyConfig | null): Promise<ConfigurationApplyOutcome> => {
    phases.push(`apply:${target?.effectiveConfigHash ?? 'idle'}`);
    active = target?.effectiveConfigHash ?? null;
    return { status: 'applied' };
  });
  const readActive = vi.fn(async (): Promise<PortResult<string | null>> => {
    phases.push('verify');
    return ok(active);
  });
  const persistDesiredState = vi.fn(async (state: DesiredState): Promise<PortResult<void>> => {
    phases.push('persist');
    durable = structuredClone(state);
    return ok(undefined);
  });
  const transactions = new ConfigurationTransactions(
    initial,
    { validate },
    { persistDesiredState },
    { apply, readActive },
  );
  const prepare =
    (target = snapshot('new')): PrepareConfiguration =>
    () => {
      phases.push('prepare');
      return ok(target);
    };
  return {
    transactions,
    phases,
    validate,
    apply,
    readActive,
    persistDesiredState,
    prepare,
    durable: () => durable,
    active: () => active,
  };
}
function errorCode(result: PortResult<ConfigurationSnapshot>) {
  expect(result.ok).toBe(false);
  if (result.ok) throw new Error('Expected failure.');
  expect(protocolErrorSchema.safeParse(result.error).success).toBe(true);
  expect(JSON.stringify(result.error)).not.toContain(secret);
  expect(result.error.message.length).toBeLessThan(200);
  return result.error.code;
}

it('orders preparation, validation, apply, independent verification, persistence and publication', async () => {
  const f = fixture();
  const target = snapshot('new');
  const result = await f.transactions.submit(f.prepare(target));
  expect(result).toEqual(ok(target));
  expect(f.phases).toEqual([
    'prepare',
    'validate',
    `apply:${target.config!.effectiveConfigHash}`,
    'verify',
    'persist',
  ]);
  expect(f.transactions.readCommitted()).toEqual(target);
  expect(f.durable()).toEqual(target.desiredState);
  expect(Object.keys(f.persistDesiredState.mock.calls[0]![0])).toEqual(['projects']);
  expect(JSON.stringify(f.durable())).not.toMatch(/nonce|registrationId|processId|lastHeartbeat/);
});

it('serializes competing preparations, consumes route plans inside the queue and reads the newest commit', async () => {
  const f = fixture();
  const entered = deferred();
  const release = deferred();
  f.persistDesiredState.mockImplementationOnce(async () => {
    f.phases.push('persist-pending');
    entered.resolve();
    await release.promise;
    return ok(undefined);
  });
  const first = f.transactions.submit(f.prepare(snapshot('first')));
  const secondPrepare = vi.fn((committed: ConfigurationSnapshot) => {
    f.phases.push('compose');
    expect(committed).toEqual(snapshot('first'));
    const plan = composeRoutePlan([
      {
        registration: committed.registrations[0]!,
        config: config(
          JSON.stringify({
            apps: {
              http: {
                servers: {
                  project: {
                    routes: [
                      {
                        match: [{ host: ['example.localhost'] }],
                        handle: [{ handler: 'static_response', body: 'fixture' }],
                      },
                    ],
                  },
                },
              },
            },
          }),
        ),
      },
    ]);
    expect(plan.ok).toBe(true);
    if (!plan.ok) throw new Error('Expected route plan.');
    expect(plan.value.kind).toBe('caddy-route-plan');
    expect(plan.value.tlsSubjects).toEqual(['example.localhost']);
    // This test candidate is independent: a route plan is not a runnable configuration.
    return ok(snapshot('second'));
  });
  const second = f.transactions.submit(secondPrepare);
  await entered.promise;
  expect(secondPrepare).not.toHaveBeenCalled();
  expect(f.transactions.readCommitted()).toEqual(snapshot());
  expect(f.validate).toHaveBeenCalledTimes(1);
  release.resolve();
  expect((await first).ok).toBe(true);
  expect((await second).ok).toBe(true);
  expect(f.transactions.readCommitted()).toEqual(snapshot('second'));
  expect(f.phases.indexOf('compose')).toBeGreaterThan(f.phases.indexOf('persist-pending'));
});

it.each(['prepare', 'validate', 'apply', 'verify', 'persist'] as const)(
  'keeps pending %s state invisible and the next preparation blocked',
  async (phase) => {
    const f = fixture();
    const entered = deferred();
    const release = deferred();
    const wait = async () => {
      entered.resolve();
      await release.promise;
    };
    let prepare = f.prepare();
    if (phase === 'prepare')
      prepare = async () => {
        await wait();
        return ok(snapshot('new'));
      };
    if (phase === 'validate')
      f.validate.mockImplementationOnce(async (target) => {
        await wait();
        return ok({ effectiveConfigHash: target.effectiveConfigHash, diagnostics: [] });
      });
    if (phase === 'apply') {
      const apply = f.apply.getMockImplementation()!;
      f.apply.mockImplementationOnce(async (target) => {
        await wait();
        return apply(target);
      });
    }
    if (phase === 'verify')
      f.readActive.mockImplementationOnce(async () => {
        await wait();
        return ok(snapshot('new').config!.effectiveConfigHash);
      });
    if (phase === 'persist')
      f.persistDesiredState.mockImplementationOnce(async () => {
        await wait();
        return ok(undefined);
      });
    const first = f.transactions.submit(prepare);
    const next = vi.fn(f.prepare(snapshot('later')));
    const second = f.transactions.submit(next);
    await entered.promise;
    expect(f.transactions.readCommitted()).toEqual(snapshot());
    expect(next).not.toHaveBeenCalled();
    release.resolve();
    expect((await first).ok).toBe(true);
    expect((await second).ok).toBe(true);
  },
);

it.each([
  {
    ...snapshot('bad'),
    config: { adaptedConfig: { format: 'json', body: '{' }, effectiveConfigHash: configHash('{') },
  },
  { ...snapshot('bad'), config: { ...config('{}'), effectiveConfigHash: 'wrong' } },
  {
    ...snapshot('bad'),
    config: { ...config('{}'), adaptedConfig: { format: 'yaml', body: '{}' } },
  },
  { ...snapshot('bad'), desiredState: { projects: [], nonce: 'forbidden' } },
  { ...snapshot('bad'), registrations: [{}] },
  {
    ...snapshot('bad'),
    config: {
      effectiveConfigHash: 'wrong',
      adaptedConfig: { format: 'json', body: ' '.repeat(maxCaddyConfigurationBytes + 1) },
    },
  },
])('rejects malformed/unstable/hash-invalid candidates before adapters', async (invalid) => {
  const f = fixture();
  expect(errorCode(await f.transactions.submit(() => ok(invalid as ConfigurationSnapshot)))).toBe(
    'config_candidate_invalid',
  );
  expect(f.validate).not.toHaveBeenCalled();
  expect(f.apply).not.toHaveBeenCalled();
  expect(f.transactions.readCommitted()).toEqual(snapshot());
  expect(f.transactions.readFence()).toBeNull();
  expect((await f.transactions.submit(f.prepare())).ok).toBe(true);
});

it('validates the supplied initial shape without inventing startup verification or storage work', () => {
  expect(() => fixture({ ...snapshot(), config: config('{') })).toThrow(
    'Invalid committed configuration snapshot.',
  );
  const f = fixture(snapshot('idle', true));
  expect(f.phases).toEqual([]);
  expect(f.transactions.readFence()).toBeNull();
});

it.each(['rejected', 'throw'] as const)(
  'retains old state after preparation %s and continues the tail',
  async (failure) => {
    const f = fixture();
    expect(
      errorCode(
        await f.transactions.submit(() => {
          if (failure === 'throw') throw new Error(secret);
          return rejected();
        }),
      ),
    ).toBe('config_prepare_failed');
    expect(f.apply).not.toHaveBeenCalled();
    expect(f.transactions.readFence()).toBeNull();
    expect(f.transactions.readCommitted()).toEqual(snapshot());
    expect((await f.transactions.submit(f.prepare())).ok).toBe(true);
  },
);

it.each(['rejected', 'throw', 'hash'] as const)(
  'retains old state after validator %s without fencing',
  async (failure) => {
    const f = fixture();
    f.validate.mockImplementationOnce(async () => {
      if (failure === 'throw') throw new Error(secret);
      if (failure === 'hash') return ok({ effectiveConfigHash: 'wrong', diagnostics: [] });
      return rejected();
    });
    expect(errorCode(await f.transactions.submit(f.prepare()))).toBe(
      failure === 'hash' ? 'config_validation_mismatch' : 'config_validation_failed',
    );
    expect(f.apply).not.toHaveBeenCalled();
    expect(f.transactions.readFence()).toBeNull();
    expect(f.transactions.readCommitted()).toEqual(snapshot());
    expect((await f.transactions.submit(f.prepare())).ok).toBe(true);
  },
);

it('skips validation only for a real idle target, and still applies, observes and persists', async () => {
  const f = fixture();
  const idle = snapshot('idle', true);
  expect(await f.transactions.submit(f.prepare(idle))).toEqual(ok(idle));
  expect(f.validate).not.toHaveBeenCalled();
  expect(f.apply).toHaveBeenCalledWith(null);
  expect(f.phases).toEqual(['prepare', 'apply:idle', 'verify', 'persist']);
});

it('isolates caller, adapter, initial, read and result data with private deeply frozen copies', async () => {
  const initial = snapshot();
  const target = snapshot('new');
  const original = structuredClone(target);
  const f = fixture(initial);
  const entered = deferred();
  const release = deferred();
  initial.registrations[0]!.registrationId = 'caller changed initial';
  const attempt = (value: object, key: string) => {
    expect(Object.isFrozen(value)).toBe(true);
    expect(() => {
      Reflect.set(value, key, 'tampered');
    }).not.toThrow();
    expect(Reflect.set(value, key, 'tampered')).toBe(false);
  };
  f.validate.mockImplementationOnce(async (config) => {
    attempt(config.adaptedConfig, 'body');
    entered.resolve();
    await release.promise;
    return ok({ effectiveConfigHash: config.effectiveConfigHash, diagnostics: [] });
  });
  f.apply.mockImplementationOnce(async (config) => {
    attempt(config!.adaptedConfig, 'body');
    return { status: 'applied' };
  });
  f.readActive.mockResolvedValueOnce(ok(target.config!.effectiveConfigHash));
  f.persistDesiredState.mockImplementationOnce(async (state) => {
    attempt(state.projects[0]!.domains[0]!, 'canonicalDomain');
    return ok(undefined);
  });
  const pending = f.transactions.submit((committed) => {
    attempt(committed.registrations[0]!.entrypointInstance, 'shimSessionNonce');
    return ok(target);
  });
  await entered.promise;
  target.registrations[0]!.registrationId = 'caller changed pending';
  Reflect.set(target.config!.adaptedConfig, 'body', '{}');
  expect(f.transactions.readCommitted()).toEqual(snapshot());
  release.resolve();
  const result = await pending;
  expect(result).toEqual(ok(original));
  if (!result.ok) throw new Error('Expected commit.');
  attempt(result.value.registrations[0]!.ownerProcess, 'processId');
  attempt(f.transactions.readCommitted().registrations[0]!, 'registrationId');
  expect(f.transactions.readCommitted()).toEqual(original);
});

it('closes admissions and drains outstanding work without starting later side effects early', async () => {
  const f = fixture();
  const entered = deferred();
  const release = deferred();
  const pending = f.transactions.submit(async () => {
    entered.resolve();
    await release.promise;
    return ok(snapshot('new'));
  });
  const alreadyQueued = f.transactions.submit(f.prepare(snapshot('last')));
  await entered.promise;
  let drained = false;
  const drain = f.transactions.close().then(() => {
    drained = true;
  });
  const refused = vi.fn(f.prepare());
  expect(errorCode(await f.transactions.submit(refused))).toBe('config_transactions_closed');
  expect(drained).toBe(false);
  expect(refused).not.toHaveBeenCalled();
  expect(f.apply).not.toHaveBeenCalled();
  release.resolve();
  await pending;
  await alreadyQueued;
  await drain;
  expect(drained).toBe(true);
  expect(f.transactions.readCommitted()).toEqual(snapshot('last'));
  await f.transactions.close();
  expect(errorCode(await f.transactions.reconcile())).toBe('config_transactions_closed');
});

it('continues the queue after explicit definite rejection without observation, persistence or fencing', async () => {
  const f = fixture();
  f.apply.mockResolvedValueOnce({ status: 'definitely-rejected' });
  expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_apply_rejected');
  expect(f.transactions.readCommitted()).toEqual(snapshot());
  expect(f.transactions.readFence()).toBeNull();
  expect(f.readActive).not.toHaveBeenCalled();
  expect(f.persistDesiredState).not.toHaveBeenCalled();
  expect((await f.transactions.submit(f.prepare(snapshot('later')))).ok).toBe(true);
});

it.each(['ambiguous', 'throw', 'invalid', 'receipt', 'extra'] as const)(
  'fences %s apply results without inferring rejection from an error kind',
  async (failure) => {
    const f = fixture();
    f.apply.mockImplementationOnce(async () => {
      if (failure === 'throw') throw { ...adapterError, kind: 'invalidInput' };
      if (failure === 'invalid') return { status: 'other' } as unknown as ConfigurationApplyOutcome;
      if (failure === 'receipt')
        return {
          effectiveConfigHash: snapshot('new').config!.effectiveConfigHash,
        } as unknown as ConfigurationApplyOutcome;
      if (failure === 'extra')
        return { status: 'applied', error: adapterError } as ConfigurationApplyOutcome;
      return { status: 'ambiguous' };
    });
    expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_apply_uncertain');
    expect(f.transactions.readCommitted()).toEqual(snapshot());
    expect(f.transactions.readFence()?.code).toBe('config_apply_uncertain');
    expect(f.readActive).not.toHaveBeenCalled();
    expect(f.persistDesiredState).not.toHaveBeenCalled();
    const later = vi.fn(f.prepare());
    expect(errorCode(await f.transactions.submit(later))).toBe('config_mutations_fenced');
    expect(later).not.toHaveBeenCalled();
  },
);

it.each(['hash', 'null', 'rejected', 'throw', 'invalid'] as const)(
  'fences authoritative verification %s failures even after an applied outcome',
  async (failure) => {
    const f = fixture();
    f.readActive.mockImplementationOnce(async () => {
      if (failure === 'throw') throw new Error(secret);
      if (failure === 'rejected') return rejected();
      if (failure === 'invalid') return ok(123) as unknown as PortResult<string | null>;
      return ok(failure === 'null' ? null : 'wrong');
    });
    expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_verification_failed');
    expect(f.transactions.readFence()?.code).toBe('config_verification_failed');
    expect(f.transactions.readCommitted()).toEqual(snapshot());
    expect(f.persistDesiredState).not.toHaveBeenCalled();
  },
);

it.each([false, true])(
  'requires independent verification for idle=%s, never receipt-only success',
  async (idle) => {
    const f = fixture(snapshot('old', !idle));
    const target = snapshot('new', idle);
    f.readActive.mockResolvedValueOnce(ok(idle ? snapshot().config!.effectiveConfigHash : null));
    expect(errorCode(await f.transactions.submit(f.prepare(target)))).toBe(
      'config_verification_failed',
    );
    expect(f.persistDesiredState).not.toHaveBeenCalled();
  },
);

it.each([false, true])(
  'restores and verifies atomic persistence failures, including first apply idle=%s',
  async (idle) => {
    for (const failure of ['rejected', 'throw', 'invalid'] as const) {
      const initial = snapshot('old', idle);
      const f = fixture(initial);
      f.persistDesiredState.mockImplementationOnce(async () => {
        f.phases.push('persist-failed');
        if (failure === 'throw') throw new Error(secret);
        if (failure === 'invalid') return null as unknown as PortResult<void>;
        return rejected();
      });
      expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_persist_failed');
      expect(f.apply.mock.calls.map(([config]) => config)).toEqual([
        snapshot('new').config,
        initial.config,
      ]);
      expect(f.readActive).toHaveBeenCalledTimes(2);
      expect(f.phases).toEqual([
        'prepare',
        'validate',
        `apply:${snapshot('new').config!.effectiveConfigHash}`,
        'verify',
        'persist-failed',
        `apply:${initial.config?.effectiveConfigHash ?? 'idle'}`,
        'verify',
      ]);
      expect(f.active()).toBe(initial.config?.effectiveConfigHash ?? null);
      expect(f.durable()).toEqual(initial.desiredState);
      expect(f.transactions.readCommitted()).toEqual(initial);
      expect(f.transactions.readFence()).toBeNull();
      expect((await f.transactions.submit(f.prepare(snapshot('later')))).ok).toBe(true);
    }
  },
);

const recoveryFailures = [
  'definitely-rejected',
  'ambiguous',
  'throw',
  'invalid',
  'hash',
  'read-rejected',
  'read-throw',
  'read-invalid',
] as const;
function failRestore(f: ReturnType<typeof fixture>, failure: (typeof recoveryFailures)[number]) {
  if (['definitely-rejected', 'ambiguous', 'throw', 'invalid'].includes(failure)) {
    f.apply.mockImplementationOnce(async () => {
      if (failure === 'throw') throw new Error(secret);
      if (failure === 'invalid') return {} as ConfigurationApplyOutcome;
      return { status: failure as 'definitely-rejected' | 'ambiguous' };
    });
  } else {
    f.readActive.mockImplementationOnce(async () => {
      if (failure === 'read-throw') throw new Error(secret);
      if (failure === 'read-rejected') return rejected();
      if (failure === 'read-invalid') return undefined as unknown as PortResult<string | null>;
      return ok('wrong');
    });
  }
}

it.each(recoveryFailures)(
  'fences rollback %s, reports storage failure and never publishes the abandoned candidate',
  async (failure) => {
    const f = fixture();
    const initialApply = f.apply.getMockImplementation()!;
    const initialRead = f.readActive.getMockImplementation()!;
    f.apply.mockImplementationOnce(initialApply);
    f.readActive.mockImplementationOnce(initialRead);
    failRestore(f, failure);
    f.persistDesiredState.mockResolvedValueOnce(rejected());
    expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_persist_failed');
    expect(f.transactions.readFence()?.code).toBe('config_rollback_failed');
    expect(f.transactions.readCommitted()).toEqual(snapshot());
    expect(f.durable()).toEqual(snapshot().desiredState);
    const refused = vi.fn(f.prepare(snapshot('later')));
    expect(errorCode(await f.transactions.submit(refused))).toBe('config_mutations_fenced');
    expect(refused).not.toHaveBeenCalled();
  },
);

it('holds the queue through unfinished rollback; no timer or later preparation can overlap it', async () => {
  const f = fixture(snapshot('idle', true));
  const entered = deferred();
  const release = deferred();
  const initialApply = f.apply.getMockImplementation()!;
  f.apply.mockImplementationOnce(initialApply);
  f.apply.mockImplementationOnce(async (target) => {
    expect(target).toBeNull();
    entered.resolve();
    await release.promise;
    return initialApply(target);
  });
  f.persistDesiredState.mockResolvedValueOnce(rejected());
  const failed = f.transactions.submit(f.prepare());
  const nextPrepare = vi.fn(f.prepare(snapshot('later')));
  const next = f.transactions.submit(nextPrepare);
  await entered.promise;
  expect(nextPrepare).not.toHaveBeenCalled();
  expect(f.transactions.readCommitted()).toEqual(snapshot('idle', true));
  release.resolve();
  expect(errorCode(await failed)).toBe('config_persist_failed');
  expect((await next).ok).toBe(true);
});

it('queues fence refusal, reconciliation and recovery, restoring old state rather than committing an observed abandoned candidate', async () => {
  const f = fixture();
  const entered = deferred();
  const release = deferred();
  const actualApply = f.apply.getMockImplementation()!;
  f.apply.mockImplementationOnce(async (target) => {
    await actualApply(target);
    entered.resolve();
    await release.promise;
    return { status: 'ambiguous' };
  });
  const uncertain = f.transactions.submit(f.prepare());
  const refusedPrepare = vi.fn(f.prepare(snapshot('refused')));
  const refused = f.transactions.submit(refusedPrepare);
  const reconciliation = f.transactions.reconcile();
  const recoveredPrepare = vi.fn((committed: ConfigurationSnapshot) => {
    expect(committed).toEqual(snapshot());
    return ok(snapshot('recovered'));
  });
  const recovered = f.transactions.submit(recoveredPrepare);
  await entered.promise;
  expect(f.readActive).not.toHaveBeenCalled();
  expect(refusedPrepare).not.toHaveBeenCalled();
  expect(recoveredPrepare).not.toHaveBeenCalled();
  expect(f.transactions.readCommitted()).toEqual(snapshot());
  release.resolve();
  expect(errorCode(await uncertain)).toBe('config_apply_uncertain');
  expect(errorCode(await refused)).toBe('config_mutations_fenced');
  expect(refusedPrepare).not.toHaveBeenCalled();
  expect(await reconciliation).toEqual(ok(snapshot()));
  expect((await recovered).ok).toBe(true);
  expect(f.apply.mock.calls.map(([target]) => target)).toEqual([
    snapshot('new').config,
    snapshot().config,
    snapshot('recovered').config,
  ]);
  expect(f.persistDesiredState).toHaveBeenCalledTimes(1);
  expect(f.transactions.readCommitted()).toEqual(snapshot('recovered'));
  expect(f.transactions.readFence()).toBeNull();
});

it.each([false, true])(
  'clears a fence on independent proof of unchanged committed state idle=%s without apply or persist',
  async (idle) => {
    const initial = snapshot('old', idle);
    const f = fixture(initial);
    f.apply.mockResolvedValueOnce({ status: 'ambiguous' });
    await f.transactions.submit(f.prepare());
    const fence = f.transactions.readFence()!;
    fence.message = secret;
    expect(f.transactions.readFence()?.message).not.toBe(secret);
    expect(await f.transactions.reconcile()).toEqual(ok(initial));
    expect(f.apply).toHaveBeenCalledTimes(1);
    expect(f.readActive).toHaveBeenCalledTimes(1);
    expect(f.persistDesiredState).not.toHaveBeenCalled();
    expect(f.transactions.readFence()).toBeNull();
    expect((await f.transactions.submit(f.prepare())).ok).toBe(true);
  },
);

it('never treats unavailable readback as idle proof after an ambiguous first apply', async () => {
  const initial = snapshot('idle', true);
  const f = fixture(initial);
  const actualApply = f.apply.getMockImplementation()!;
  f.apply.mockImplementationOnce(async (target) => {
    await actualApply(target);
    return { status: 'ambiguous' };
  });
  expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_apply_uncertain');
  expect(f.active()).toBe(snapshot('new').config!.effectiveConfigHash);
  // The backend contract reports unknown/unavailable state as failure, never ok(null).
  f.readActive.mockResolvedValueOnce(rejected()).mockResolvedValueOnce(rejected());
  expect(errorCode(await f.transactions.reconcile())).toBe('config_reconciliation_failed');
  expect(f.transactions.readFence()).not.toBeNull();
  expect(f.transactions.readCommitted()).toEqual(initial);
  expect(f.apply).toHaveBeenCalledTimes(2);
  const refused = vi.fn(f.prepare(snapshot('refused')));
  expect(errorCode(await f.transactions.submit(refused))).toBe('config_mutations_fenced');
  expect(refused).not.toHaveBeenCalled();
  expect(await f.transactions.reconcile()).toEqual(ok(initial));
  expect(f.apply.mock.calls.map(([target]) => target)).toEqual([snapshot('new').config, null]);
  expect(f.readActive).toHaveBeenCalledTimes(3);
  expect(f.active()).toBeNull();
  expect(f.transactions.readFence()).toBeNull();
  expect(f.persistDesiredState).not.toHaveBeenCalled();
  expect(f.transactions.readCommitted()).toEqual(initial);
});

it.each(['rejected', 'throw', 'invalid'] as const)(
  'keeps reconciliation observation %s fenced when restoration cannot be independently verified',
  async (failure) => {
    const f = fixture();
    f.apply.mockResolvedValueOnce({ status: 'ambiguous' });
    await f.transactions.submit(f.prepare());
    const unavailableRead = async (): Promise<PortResult<string | null>> => {
      if (failure === 'throw') throw new Error(secret);
      if (failure === 'invalid') return {} as PortResult<string | null>;
      return rejected();
    };
    f.readActive.mockImplementationOnce(unavailableRead).mockImplementationOnce(unavailableRead);
    expect(errorCode(await f.transactions.reconcile())).toBe('config_reconciliation_failed');
    expect(f.apply).toHaveBeenCalledTimes(2);
    expect(f.transactions.readFence()).not.toBeNull();
    expect(f.transactions.readCommitted()).toEqual(snapshot());
    expect((await f.transactions.reconcile()).ok).toBe(true);
  },
);

it.each(['rejected', 'throw', 'invalid'] as const)(
  'restores committed state after initial observation %s only with independent final proof',
  async (failure) => {
    const f = fixture();
    f.apply.mockResolvedValueOnce({ status: 'ambiguous' });
    await f.transactions.submit(f.prepare());
    f.readActive.mockImplementationOnce(async () => {
      if (failure === 'throw') throw new Error(secret);
      if (failure === 'invalid') return {} as PortResult<string | null>;
      return rejected();
    });
    expect(await f.transactions.reconcile()).toEqual(ok(snapshot()));
    expect(f.apply.mock.calls.map(([target]) => target)).toEqual([
      snapshot('new').config,
      snapshot().config,
    ]);
    expect(f.readActive).toHaveBeenCalledTimes(2);
    expect(f.transactions.readFence()).toBeNull();
    expect(f.transactions.readCommitted()).toEqual(snapshot());
    expect(f.persistDesiredState).not.toHaveBeenCalled();
  },
);

it.each(recoveryFailures)(
  'keeps reconciliation restoration %s fenced and permits only a subsequent verified recovery',
  async (failure) => {
    const f = fixture(snapshot('idle', true));
    const actualApply = f.apply.getMockImplementation()!;
    f.apply.mockImplementationOnce(async (target) => {
      await actualApply(target);
      return { status: 'ambiguous' };
    });
    await f.transactions.submit(f.prepare());
    const actualRead = f.readActive.getMockImplementation()!;
    f.readActive.mockImplementationOnce(actualRead);
    failRestore(f, failure);
    expect(errorCode(await f.transactions.reconcile())).toBe('config_reconciliation_failed');
    expect(f.transactions.readFence()).not.toBeNull();
    expect(f.transactions.readCommitted()).toEqual(snapshot('idle', true));
    expect(f.persistDesiredState).not.toHaveBeenCalled();
    expect((await f.transactions.reconcile()).ok).toBe(true);
    expect(f.transactions.readFence()).toBeNull();
    expect((await f.transactions.submit(f.prepare())).ok).toBe(true);
  },
);

it('never clears the fence until reconciliation readback settles and proves the committed target', async () => {
  const f = fixture();
  const entered = deferred();
  const release = deferred();
  const actualApply = f.apply.getMockImplementation()!;
  f.apply.mockImplementationOnce(async (target) => {
    await actualApply(target);
    return { status: 'ambiguous' };
  });
  await f.transactions.submit(f.prepare());
  const actualRead = f.readActive.getMockImplementation()!;
  f.readActive.mockImplementationOnce(actualRead);
  f.readActive.mockImplementationOnce(async () => {
    entered.resolve();
    await release.promise;
    return actualRead();
  });
  const reconcile = f.transactions.reconcile();
  const nextPrepare = vi.fn(f.prepare(snapshot('later')));
  const next = f.transactions.submit(nextPrepare);
  await entered.promise;
  expect(f.transactions.readFence()).not.toBeNull();
  expect(nextPrepare).not.toHaveBeenCalled();
  expect(f.transactions.readCommitted()).toEqual(snapshot());
  release.resolve();
  expect(await reconcile).toEqual(ok(snapshot()));
  expect((await next).ok).toBe(true);
});

it('redacts invalid callback/validator carriers without poisoning the tail or unnecessarily fencing', async () => {
  const f = fixture();
  expect(
    errorCode(
      await f.transactions.submit(() => null as unknown as PortResult<ConfigurationSnapshot>),
    ),
  ).toBe('config_prepare_failed');
  f.validate.mockResolvedValueOnce({ ok: true } as never);
  expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_validation_failed');
  expect(f.transactions.readFence()).toBeNull();
  expect((await f.transactions.submit(f.prepare())).ok).toBe(true);
});

it('treats attempted callback/validator mutation as bounded pre-apply failure, not runtime uncertainty', async () => {
  const f = fixture();
  expect(
    errorCode(
      await f.transactions.submit((committed) => {
        committed.registrations[0]!.registrationId = secret;
        return ok(snapshot('new'));
      }),
    ),
  ).toBe('config_prepare_failed');
  f.validate.mockImplementationOnce(async (target) => {
    Object.defineProperty(target.adaptedConfig, 'body', { value: '{}' });
    return ok({ effectiveConfigHash: target.effectiveConfigHash, diagnostics: [] });
  });
  expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_validation_failed');
  expect(f.transactions.readFence()).toBeNull();
  expect(f.apply).not.toHaveBeenCalled();
  expect((await f.transactions.submit(f.prepare())).ok).toBe(true);
});

it('settles even unexpected queue errors; close drains failures and leaves admissions closed', async () => {
  const f = fixture();
  vi.spyOn(f.transactions, 'readCommitted').mockImplementationOnce(() => {
    throw new Error(secret);
  });
  expect(errorCode(await f.transactions.reconcile())).toBe('config_transaction_failed');
  expect(f.transactions.readFence()).not.toBeNull();
  expect((await f.transactions.reconcile()).ok).toBe(true);
  const failed = f.transactions.submit(() => {
    throw new Error(secret);
  });
  await f.transactions.close();
  expect(errorCode(await failed)).toBe('config_prepare_failed');
  expect(errorCode(await f.transactions.submit(f.prepare()))).toBe('config_transactions_closed');
});
