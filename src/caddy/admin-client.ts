import type { ClientRequest, IncomingMessage } from 'node:http';
import { request } from 'node:https';
import type { CaddyConfig, PortResult } from '../contracts/ports.ts';
import type { ConfigurationApplyOutcome } from '../daemon/configuration-transactions.ts';
import { normalizeCaddyConfig, type SecureAdminPolicy } from './admin-policy.ts';
import {
  assertCandidate,
  caddyConfigurationTimeoutMs,
  maxCaddyConfigurationBytes,
} from './preparation.ts';

export type AdminClientOptions = Readonly<{
  timeoutMs?: number;
  maxResponseBytes?: number;
  maxHeaderBytes?: number;
}>;
type PreparedPolicy = Pick<SecureAdminPolicy, 'config' | 'client'>;
type Reply = Readonly<{ status: number; body: string }>;

function bounded(value: number | undefined, maximum: number): number {
  if (value === undefined) return maximum;
  if (!Number.isInteger(value) || value < 1 || value > maximum)
    throw new Error('Invalid administration client bounds.');
  return value;
}

/** Compare only the prepared security envelope; route assembly remains the authority. */
function securityEnvelope(config: CaddyConfig): string {
  const value = JSON.parse(config.adaptedConfig.body) as {
    admin?: unknown;
    apps?: { pki?: unknown };
  };
  if (!value.admin || !value.apps?.pki)
    throw new Error('Missing administration security envelope.');
  return normalizeCaddyConfig(JSON.stringify({ admin: value.admin, pki: value.apps.pki }))
    .adaptedConfig.body;
}

/**
 * HTTPS transport/readback only, not an owned-process ConfigurationRuntime.
 * A completed /load acknowledgement is not active-state proof. The transaction
 * queue must independently read back and compare before publishing a change.
 */
export class CaddyAdminClient {
  readonly #client: SecureAdminPolicy['client'];
  readonly #security: string;
  readonly #timeoutMs: number;
  readonly #maxResponseBytes: number;
  readonly #maxHeaderBytes: number;
  readonly #pending = new Set<() => void>();
  #closed = false;

  constructor(policy: PreparedPolicy, options: AdminClientOptions = {}) {
    try {
      const { host, port, servername, rejectUnauthorized, ca, cert, key } = policy.client;
      if (
        host !== '127.0.0.1' ||
        !Number.isInteger(port) ||
        port < 1 ||
        port > 65535 ||
        servername !== 'localhost' ||
        rejectUnauthorized !== true ||
        [ca, cert, key].some((value) => typeof value !== 'string' || value.length === 0)
      )
        throw new Error();
      this.#client = Object.freeze({
        host: '127.0.0.1',
        port,
        servername: 'localhost',
        rejectUnauthorized: true,
        ca,
        cert,
        key,
      });
      assertCandidate(policy.config);
      this.#security = securityEnvelope(normalizeCaddyConfig(policy.config.adaptedConfig.body));
      this.#timeoutMs = bounded(options.timeoutMs, caddyConfigurationTimeoutMs);
      this.#maxResponseBytes = bounded(options.maxResponseBytes, maxCaddyConfigurationBytes);
      this.#maxHeaderBytes = bounded(options.maxHeaderBytes, 16 * 1024);
    } catch {
      throw new Error('Invalid prepared administration policy or client bounds.');
    }
  }

  /** Shared lifecycle guard, evaluated before validation or native creation. */
  accepts(config: CaddyConfig): boolean {
    try {
      assertCandidate(config);
      const normalized = normalizeCaddyConfig(config.adaptedConfig.body);
      return (
        normalized.adaptedConfig.body === config.adaptedConfig.body &&
        securityEnvelope(normalized) === this.#security
      );
    } catch {
      return false;
    }
  }

  async apply(config: CaddyConfig, signal?: AbortSignal): Promise<ConfigurationApplyOutcome> {
    let body: string;
    try {
      // Idle transitions belong to the owned-process lifecycle, not HTTPS administration.
      if (this.#closed || config == null) return { status: 'definitely-rejected' };
      // Copy primitives before validation/network; never retain a caller-owned candidate.
      const copy: CaddyConfig = {
        adaptedConfig: { format: config.adaptedConfig.format, body: config.adaptedConfig.body },
        effectiveConfigHash: config.effectiveConfigHash,
      };
      if (!this.accepts(copy)) return { status: 'definitely-rejected' };
      body = copy.adaptedConfig.body;
    } catch {
      return { status: 'definitely-rejected' };
    }
    try {
      const reply = await this.#request('POST', '/load', body, signal);
      if (reply && reply.status >= 200 && reply.status < 300) return { status: 'applied' };
      // Caddy /load documents rollback on completed load/adapt failure (HTTP 400).
      if (reply?.status === 400) return { status: 'definitely-rejected' };
    } catch {
      // An unexpected transport exception cannot prove that no transition occurred.
    }
    return { status: 'ambiguous' };
  }

  /** A receipt is not process settlement; the owned runtime must still wait/force. */
  async stop(signal?: AbortSignal): Promise<ConfigurationApplyOutcome> {
    const reply = await this.#request('POST', '/stop', undefined, signal);
    return { status: reply && reply.status >= 200 && reply.status < 300 ? 'applied' : 'ambiguous' };
  }

  async readActive(signal?: AbortSignal): Promise<PortResult<string>> {
    try {
      const reply = await this.#request('GET', '/config/', undefined, signal);
      if (reply && reply.status >= 200 && reply.status < 300)
        return { ok: true, value: normalizeCaddyConfig(reply.body).effectiveConfigHash };
    } catch {
      // Unavailable, incomplete or invalid observation is never idle/null.
    }
    return {
      ok: false,
      error: {
        kind: 'caddyRuntime',
        code: 'caddy_admin_readback_failed',
        message: 'Cannot observe the active Caddy configuration.',
        guidance: null,
        retryable: false,
        requestId: null,
      },
    };
  }

  /** Stops admissions and aborts only this client's in-flight requests. No retry. */
  close(): void {
    this.#closed = true;
    for (const abort of this.#pending) abort();
  }

  #request(
    method: 'POST' | 'GET',
    path: '/load' | '/config/' | '/stop',
    body?: string,
    signal?: AbortSignal,
  ): Promise<Reply | null> {
    if (this.#closed || signal?.aborted) return Promise.resolve(null);
    return new Promise((resolve) => {
      let req: ClientRequest | undefined;
      let response: IncomingMessage | undefined;
      let settled = false;
      const deadline = performance.now() + this.#timeoutMs;
      const finish = (reply: Reply | null): void => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.#pending.delete(abort);
        signal?.removeEventListener('abort', abort);
        response?.destroy();
        req?.destroy();
        resolve(reply);
      };
      const abort = (): void => finish(null);
      const timer = setTimeout(abort, this.#timeoutMs);
      this.#pending.add(abort);
      signal?.addEventListener('abort', abort, { once: true });
      if (signal?.aborted) {
        abort();
        return;
      }
      try {
        req = request(
          {
            ...this.#client,
            method,
            path,
            agent: false,
            maxHeaderSize: this.#maxHeaderBytes,
            headers:
              body === undefined
                ? {}
                : {
                    'Content-Type': 'application/json',
                    'Content-Length': Buffer.byteLength(body, 'utf8'),
                  },
          },
          (res) => {
            response = res;
            if (settled) {
              res.destroy();
              return;
            }
            res.on('error', abort);
            res.on('aborted', abort);
            res.on('close', () => {
              if (!res.complete) abort();
            });
            const declared = res.headers['content-length'];
            if (
              (declared !== undefined &&
                (!/^\d+$/.test(declared) || Number(declared) > this.#maxResponseBytes)) ||
              (res.headers['content-encoding'] !== undefined &&
                res.headers['content-encoding'] !== 'identity')
            ) {
              abort();
              return;
            }
            const chunks: Buffer[] = [];
            let bytes = 0;
            res.on('data', (chunk: Buffer) => {
              bytes += chunk.length;
              if (bytes > this.#maxResponseBytes || performance.now() >= deadline) {
                abort();
                return;
              }
              chunks.push(chunk);
            });
            res.on('end', () => {
              if (settled) return;
              if (
                !res.complete ||
                performance.now() >= deadline ||
                (declared !== undefined && Number(declared) !== bytes)
              ) {
                abort();
                return;
              }
              try {
                const text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(
                  Buffer.concat(chunks, bytes),
                );
                finish({ status: res.statusCode ?? 0, body: text });
              } catch {
                abort();
              }
            });
          },
        );
        req.on('error', abort);
        req.on('close', () => {
          if (!settled) abort();
        });
        req.end(body);
      } catch {
        abort();
      }
    });
  }
}
