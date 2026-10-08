import { readFile } from 'node:fs/promises';
import type { CaddyConfig, PortResult } from '../../src/contracts/ports.ts';
import type { ConfigurationApplyOutcome } from '../../src/daemon/configuration-transactions.ts';
import {
  OwnedCommandError,
  type OwnedCommandOptions,
  type OwnedCommandOutput,
} from '../../src/platform/owned-command.ts';
import type { PinnedCaddyImage } from '../../src/caddy/image.ts';
import type { RealCaddyResolver } from '../../src/caddy/resolver.ts';
import { normalizeCaddyConfig } from '../../src/caddy/admin-policy.ts';

const empty = (): OwnedCommandOutput => ({
  stdout: Buffer.alloc(0),
  stderr: Buffer.alloc(0),
  exitCode: 0,
});
export const backend = {
  active: null as string | null,
  runCalls: [] as { args: readonly string[]; options: OwnedCommandOptions; body: string }[],
  reads: 0,
  stops: 0,
  loads: 0,
  loadStatus: 'applied' as ConfigurationApplyOutcome['status'],
  uncertainLoadChanges: false,
  startup: 'ready' as 'ready' | 'silent' | 'mismatch' | 'exit' | 'launch-failed',
  launchGate: undefined as Promise<void> | undefined,
  stopMode: 'graceful' as 'graceful' | 'ignored' | 'hanging',
  cleanupFault: false,
  validationFails: false,
  blockRead: false,
  settle: undefined as (() => void) | undefined,
  reset(): void {
    this.active = null;
    this.runCalls = [];
    this.reads = 0;
    this.stops = 0;
    this.loads = 0;
    this.loadStatus = 'applied';
    this.uncertainLoadChanges = false;
    this.startup = 'ready';
    this.launchGate = undefined;
    this.stopMode = 'graceful';
    this.cleanupFault = false;
    this.validationFails = false;
    this.blockRead = false;
    this.settle = undefined;
  },
  async readActive(signal?: AbortSignal): Promise<PortResult<string>> {
    this.reads++;
    if (this.blockRead && this.runCalls.length > 0 && !signal?.aborted) await cancelled(signal);
    if (!signal?.aborted && this.active !== null) return { ok: true, value: this.active };
    return {
      ok: false,
      error: {
        kind: 'caddyRuntime',
        code: 'fixture_unavailable',
        message: 'Fixture unavailable.',
        guidance: null,
        retryable: false,
        requestId: null,
      },
    };
  },
  async apply(config: CaddyConfig, signal?: AbortSignal): Promise<ConfigurationApplyOutcome> {
    this.loads++;
    if (this.blockRead && !signal?.aborted) await cancelled(signal);
    if (signal?.aborted) return { status: 'ambiguous' };
    if (this.loadStatus === 'applied' || this.uncertainLoadChanges)
      this.active = config.effectiveConfigHash;
    return { status: this.loadStatus };
  },
  async stop(signal?: AbortSignal): Promise<ConfigurationApplyOutcome> {
    this.stops++;
    if (this.stopMode === 'hanging' && !signal?.aborted) await cancelled(signal);
    if (this.stopMode === 'graceful') this.settle?.();
    return { status: this.stopMode === 'graceful' ? 'applied' : 'ambiguous' };
  },
};
function cancelled(signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) =>
    signal?.addEventListener('abort', () => resolve(), { once: true }),
  );
}

export function fixtureResolver(): RealCaddyResolver {
  const image = {
    run(args: readonly string[], options: OwnedCommandOptions = {}): Promise<OwnedCommandOutput> {
      if (args[0] === 'validate')
        return Promise.resolve({ ...empty(), exitCode: backend.validationFails ? 1 : 0 });
      return new Promise((resolve, reject) => {
        let done = false;
        const finish = (error?: unknown): void => {
          if (done) return;
          done = true;
          options.signal?.removeEventListener('abort', abort);
          backend.active = null;
          if (error) reject(error);
          else resolve(empty());
        };
        const abort = (): void =>
          finish(
            new OwnedCommandError(
              backend.cleanupFault ? 'cleanup' : 'abort',
              'Fixture settlement.',
            ),
          );
        backend.settle = () => finish();
        options.signal?.addEventListener('abort', abort, { once: true });
        if (options.signal?.aborted) {
          abort();
          return;
        }
        void readFile(args[2]!, 'utf8')
          .then(async (body) => {
            if (done) return;
            backend.runCalls.push({ args: [...args], options, body });
            await backend.launchGate;
            if (done) return;
            if (backend.startup === 'launch-failed') {
              finish(new OwnedCommandError('spawn', 'Fixture launch failed.'));
              return;
            }
            if (backend.startup === 'exit') {
              finish();
              return;
            }
            options.onStarted?.();
            if (backend.startup === 'ready')
              backend.active = normalizeCaddyConfig(body).effectiveConfigHash;
            else if (backend.startup === 'mismatch') backend.active = 'independent-mismatch';
          }, finish)
          .catch(finish);
      });
    },
  } as PinnedCaddyImage;
  return { pin: async () => image } as unknown as RealCaddyResolver;
}
