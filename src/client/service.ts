import { setTimeout as delay } from 'node:timers/promises';
import type { ClientServicePort, PortResult } from '../contracts/ports.ts';
import type { RpcParams } from '../protocol/rpc.ts';
import type { StateSnapshot } from '../protocol/dto.ts';
import { CadderError, RpcError, errorCode } from '../protocol/errors.ts';
import { resolvePaths, type RuntimePaths } from '../daemon/paths.ts';
import { rpc } from './connection.ts';
import { launchDaemon } from './launch.ts';

export class ClientService implements ClientServicePort {
  constructor(readonly paths: RuntimePaths = resolvePaths()) {}
  queryState() {
    return this.#result(() => rpc(this.paths, 'query-state-request'));
  }
  queryLogs(params: RpcParams<'query-logs-request'>) {
    return this.#result(() => rpc(this.paths, 'query-logs-request', params));
  }
  setProjectEnabled(params: RpcParams<'set-entrypoint-enabled-request'>) {
    return this.#result(() => rpc(this.paths, 'set-entrypoint-enabled-request', params));
  }
  setDomainEnabled(params: RpcParams<'set-domain-enabled-request'>) {
    return this.#result(() => rpc(this.paths, 'set-domain-enabled-request', params));
  }
  shutdown() {
    return this.#result(() => rpc(this.paths, 'shutdown-daemon-request'));
  }

  async snapshot(): Promise<StateSnapshot> {
    const response = await rpc(this.paths, 'query-state-request');
    if (!response.accepted || !response.snapshot)
      throw new CadderError('daemon-unavailable', 'Daemon state is unavailable.');
    return response.snapshot;
  }
  async start(args: readonly string[] = []): Promise<void> {
    try {
      await this.snapshot();
      return;
    } catch (error) {
      if (errorCode(error) !== 'daemon-unavailable') throw error;
    }
    const child = await launchDaemon(args);
    let ready = false;
    let permissionsFailure: unknown;
    try {
      // Cold Windows ACL preparation can exceed thirty seconds in an isolated VM.
      const deadline = Date.now() + 60000;
      while (Date.now() < deadline) {
        try {
          await this.snapshot();
          ready = true;
          return;
        } catch (error) {
          // New Windows paths exist briefly before their owner-only ACL is published.
          if (errorCode(error) === 'unsafe-runtime-permissions') permissionsFailure = error;
          else if (errorCode(error) !== 'daemon-unavailable') throw error;
        }
        await delay(100);
      }
      if (permissionsFailure) throw permissionsFailure;
      throw new CadderError(
        'daemon-start-failed',
        'cadderd did not become ready. Run cadderd in the foreground for diagnostics.',
      );
    } finally {
      if (!ready && child.exitCode === null) child.kill();
    }
  }
  async stop(): Promise<void> {
    try {
      await rpc(this.paths, 'shutdown-daemon-request');
    } catch (error) {
      if (errorCode(error) === 'daemon-unavailable') return;
      throw error;
    }
    const deadline = Date.now() + 15000;
    let unavailable = 0;
    while (Date.now() < deadline) {
      try {
        await this.snapshot();
        unavailable = 0;
      } catch (error) {
        if (errorCode(error) === 'daemon-unavailable') {
          if (++unavailable === 3) return;
        } else if (errorCode(error) === 'daemon_shutting_down') unavailable = 0;
        else throw error;
      }
      await delay(100);
    }
    throw new CadderError(
      'daemon-stop-failed',
      'cadderd did not finish shutdown within its deadline.',
    );
  }
  async restart(): Promise<void> {
    await this.stop();
    await this.start();
  }
  async #result<T>(operation: () => Promise<T>): Promise<PortResult<T>> {
    try {
      return { ok: true, value: await operation() };
    } catch (error) {
      if (error instanceof RpcError) return { ok: false, error: error.protocolError };
      return {
        ok: false,
        error: {
          kind: 'internal',
          code: errorCode(error)?.replaceAll('-', '_') ?? 'client_failed',
          message: error instanceof CadderError ? error.message : 'Cannot contact cadderd.',
          guidance: null,
          retryable: false,
          requestId: null,
        },
      };
    }
  }
}
