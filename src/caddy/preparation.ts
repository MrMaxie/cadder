import { createHash } from 'node:crypto';
import type { CaddyConfig } from '../contracts/ports.ts';
import type { ProtocolError } from '../protocol/errors.ts';
import {
  OwnedCommandError,
  type OwnedCommandOptions,
  type OwnedCommandOutput,
} from '../platform/owned-command.ts';

export const caddyConfigurationTimeoutMs = 30_000;
export const maxCaddyConfigurationBytes = 32 * 1024 * 1024;
export type PreparationOptions = Pick<OwnedCommandOptions, 'timeoutMs' | 'maxStreamBytes' | 'env'>;

export class CaddyPreparationError extends Error {
  constructor(
    readonly code: string,
    message: string,
    options?: ErrorOptions,
  ) {
    super(message.slice(0, 4096), options);
    this.name = 'CaddyPreparationError';
  }
}

export function configurationCommandOptions(
  options: PreparationOptions,
  signal?: AbortSignal,
): OwnedCommandOptions {
  return {
    ...options,
    timeoutMs: Math.min(
      options.timeoutMs ?? caddyConfigurationTimeoutMs,
      caddyConfigurationTimeoutMs,
    ),
    maxStreamBytes: Math.min(
      options.maxStreamBytes ?? maxCaddyConfigurationBytes,
      maxCaddyConfigurationBytes,
    ),
    ...(signal === undefined ? {} : { signal }),
  };
}

export function decodeOutput(output: OwnedCommandOutput, operation: 'adapt' | 'validate'): string {
  const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
  let stdout: string;
  let stderr: string;
  try {
    stdout = decoder.decode(output.stdout);
    stderr = decoder.decode(output.stderr);
  } catch (error) {
    throw new CaddyPreparationError(
      'caddy_output_invalid',
      'Caddy returned invalid UTF-8 output.',
      { cause: error },
    );
  }
  if (output.exitCode !== 0)
    throw new CaddyPreparationError(
      `caddy_${operation}_failed`,
      `Caddy ${operation} failed: ${stderr.trim().slice(0, 4000)}`,
    );
  return stdout;
}

export function configHash(body: string): string {
  return createHash('sha256').update(body, 'utf8').digest('hex');
}

export function assertJsonBody(body: string, maxBytes = maxCaddyConfigurationBytes): void {
  if (Buffer.byteLength(body, 'utf8') > maxBytes)
    throw new CaddyPreparationError(
      'caddy_candidate_invalid',
      'Caddy JSON candidate exceeds its byte bound.',
    );
  const bytes = Buffer.from(body, 'utf8');
  if (new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes) !== body)
    throw new CaddyPreparationError(
      'caddy_candidate_invalid',
      'Caddy JSON candidate has invalid encoding.',
    );
  try {
    JSON.parse(body);
  } catch (error) {
    throw new CaddyPreparationError(
      'caddy_candidate_invalid',
      'Caddy configuration candidate is not complete JSON.',
      { cause: error },
    );
  }
}

export function assertCandidate(config: CaddyConfig): void {
  if (
    config?.adaptedConfig?.format !== 'json' ||
    typeof config.adaptedConfig.body !== 'string' ||
    typeof config.effectiveConfigHash !== 'string'
  )
    throw new CaddyPreparationError(
      'caddy_candidate_invalid',
      'Expected a complete JSON Caddy candidate.',
    );
  assertJsonBody(config.adaptedConfig.body);
  if (config.effectiveConfigHash !== configHash(config.adaptedConfig.body))
    throw new CaddyPreparationError(
      'caddy_candidate_invalid',
      'Caddy candidate hash does not match its complete JSON body.',
    );
}

/** Preserve typed bounded failures, never publish arbitrary resolver/project exception text. */
export function preparationFailure(error: unknown, operation: 'adapt' | 'validate'): ProtocolError {
  let code = `caddy_${operation}_failed`;
  let message = `Cannot ${operation} the Caddy configuration.`;
  let kind: ProtocolError['kind'] = 'caddyRuntime';
  if (error instanceof CaddyPreparationError) {
    code = error.code;
    message = error.message;
    if (code === 'caddy_candidate_invalid') kind = 'configuration';
  } else if (error instanceof OwnedCommandError) {
    code = `caddy_command_${error.code}`;
    message = error.message;
    if (error.code === 'timeout') kind = 'timeout';
  } else if (error instanceof AggregateError) {
    code = 'caddy_cleanup_failed';
    message = 'Caddy command or staging cleanup failed.';
  }
  return { kind, code, message, guidance: null, retryable: false, requestId: null };
}
