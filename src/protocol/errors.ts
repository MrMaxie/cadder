import { z } from 'zod';

export const requestIdSchema = z.string().uuid();
export const protocolErrorSchema = z.strictObject({
  kind: z.enum([
    'incompatibleProtocolVersion',
    'unsupportedOperation',
    'payloadDecodeFailed',
    'accessDenied',
    'invalidInput',
    'conflict',
    'configuration',
    'caddyRuntime',
    'storage',
    'busy',
    'frame',
    'timeout',
    'protocolViolation',
    'shuttingDown',
    'staleInstance',
    'internal',
  ]),
  code: z
    .string()
    .max(64)
    .regex(/^[a-z](?:[a-z0-9_]*[a-z0-9])?$/),
  message: z.string(),
  guidance: z.string().nullable(),
  retryable: z.boolean(),
  requestId: requestIdSchema.nullable(),
  deniedOperation: z
    .string()
    .nullable()
    .transform((value) => value ?? undefined)
    .optional(),
});
export type ProtocolError = z.infer<typeof protocolErrorSchema>;

export class CadderError extends Error {
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'CadderError';
  }
}

export class RpcError extends CadderError {
  readonly protocolError: ProtocolError;

  constructor(error: z.input<typeof protocolErrorSchema>) {
    const parsed = protocolErrorSchema.parse(error);
    super(parsed.code, parsed.message);
    this.protocolError = parsed;
  }
}

export function toWireError(error: unknown, requestId: string): ProtocolError {
  if (error instanceof RpcError) {
    const parsed = protocolErrorSchema.safeParse({ ...error.protocolError, requestId });
    if (parsed.success) return parsed.data;
  }
  return {
    kind: 'internal',
    code: 'internal',
    message: 'Daemon request failed. See diagnostics.',
    guidance: null,
    retryable: false,
    requestId,
  };
}

export function errorCode(error: unknown): string | undefined {
  if (error && typeof error === 'object' && 'code' in error) {
    return String(error.code);
  }
  return undefined;
}
