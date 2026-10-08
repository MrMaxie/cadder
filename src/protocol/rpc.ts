import { z } from 'zod';
import {
  logEntrySchema,
  logStreamInputSchema,
  logStreamSchema,
  registrationInputSchema,
  safeUnsignedIntegerSchema,
  snapshotSchema,
} from './dto.ts';
import { protocolErrorSchema, requestIdSchema, toWireError } from './errors.ts';
import { PROTOCOL_VERSION } from './version.ts';

const basicResultSchema = z.strictObject({
  requestId: requestIdSchema,
  accepted: z.boolean(),
  message: z.string(),
});
const ownerParamsSchema = z.strictObject({
  registrationId: z.string(),
  shimSessionNonce: z.string(),
});
const emptyParamsSchema = z.strictObject({});
export const catalog = {
  'register-entrypoint-request': {
    params: z.strictObject({ registration: registrationInputSchema }),
    result: basicResultSchema.extend({ registrationId: z.string().nullable() }),
  },
  'unregister-entrypoint-request': { params: ownerParamsSchema, result: basicResultSchema },
  'heartbeat-entrypoint-request': { params: ownerParamsSchema, result: basicResultSchema },
  'query-state-request': {
    params: emptyParamsSchema,
    result: basicResultSchema.extend({ snapshot: snapshotSchema.nullable() }),
  },
  'set-entrypoint-enabled-request': {
    params: z.strictObject({
      registrationId: z.string(),
      shimSessionNonce: z.string().nullable().default(null),
      enabled: z.boolean(),
    }),
    result: basicResultSchema,
  },
  'set-domain-enabled-request': {
    params: z.strictObject({
      registrationId: z.string(),
      domainKey: z.string(),
      enabled: z.boolean(),
    }),
    result: basicResultSchema,
  },
  'query-logs-request': {
    params: z.strictObject({
      stream: logStreamInputSchema,
      limit: safeUnsignedIntegerSchema.nullable().default(null),
    }),
    result: basicResultSchema.extend({
      stream: logStreamSchema,
      streamStatus: z.enum(['unknown', 'empty', 'active', 'stale', 'removed', 'readError']),
      entries: z.array(logEntrySchema).max(200),
    }),
  },
  'shutdown-daemon-request': { params: emptyParamsSchema, result: basicResultSchema },
} as const;
export type RpcMethod = keyof typeof catalog;
export type RpcParams<M extends RpcMethod> = z.input<(typeof catalog)[M]['params']>;
export type RpcResult<M extends RpcMethod> = z.infer<(typeof catalog)[M]['result']>;
export type RpcRequest = {
  [M in RpcMethod]: {
    protocolVersion: typeof PROTOCOL_VERSION;
    requestId: string;
    method: M;
    params: z.output<(typeof catalog)[M]['params']>;
  };
}[RpcMethod];
export type HandlerResult = {
  [M in RpcMethod]: Omit<z.input<(typeof catalog)[M]['result']>, 'requestId'> & {
    requestId?: string;
  };
}[RpcMethod];
export type RpcHandler = (request: RpcRequest) => Promise<HandlerResult>;

function methodRequestSchema<M extends RpcMethod, S extends z.ZodType>(method: M, params: S) {
  return z.strictObject({
    protocolVersion: z.literal(PROTOCOL_VERSION),
    requestId: requestIdSchema,
    method: z.literal(method),
    params,
  });
}
export const requestSchema = z.discriminatedUnion('method', [
  methodRequestSchema('register-entrypoint-request', catalog['register-entrypoint-request'].params),
  methodRequestSchema(
    'unregister-entrypoint-request',
    catalog['unregister-entrypoint-request'].params,
  ),
  methodRequestSchema(
    'heartbeat-entrypoint-request',
    catalog['heartbeat-entrypoint-request'].params,
  ),
  methodRequestSchema('query-state-request', catalog['query-state-request'].params),
  methodRequestSchema(
    'set-entrypoint-enabled-request',
    catalog['set-entrypoint-enabled-request'].params,
  ),
  methodRequestSchema('set-domain-enabled-request', catalog['set-domain-enabled-request'].params),
  methodRequestSchema('query-logs-request', catalog['query-logs-request'].params),
  methodRequestSchema('shutdown-daemon-request', catalog['shutdown-daemon-request'].params),
]);
const responseHeader = { protocolVersion: z.literal(PROTOCOL_VERSION), requestId: requestIdSchema };
export const errorResponseSchema = z
  .strictObject({ ...responseHeader, error: protocolErrorSchema })
  .refine(
    (response) => response.error.requestId === response.requestId,
    'Error request ID must match its envelope.',
  );

export function responseSchema<M extends RpcMethod>(method: M) {
  const success = z
    .strictObject({ ...responseHeader, result: catalog[method].result })
    .refine(
      (response) => response.result.requestId === response.requestId,
      'Result request ID must match its envelope.',
    );
  return z.union([success, errorResponseSchema]);
}

export type RpcResponse =
  | { protocolVersion: typeof PROTOCOL_VERSION; requestId: string; result: RpcResult<RpcMethod> }
  | z.infer<typeof errorResponseSchema>;

export const UNCORRELATED_REQUEST_ID = '00000000-0000-4000-8000-000000000000';

// Authentication and framing are enforced before this boundary.
export async function dispatchRpc(input: unknown, handler: RpcHandler): Promise<RpcResponse> {
  const parsed = requestSchema.safeParse(input);
  if (!parsed.success) {
    const envelope = z.object({ method: z.string().optional() }).safeParse(input);
    let requestId = UNCORRELATED_REQUEST_ID;
    // Correlate only validated IDs; never reflect unbounded peer diagnostic text.
    if (input && typeof input === 'object' && 'requestId' in input) {
      const id = requestIdSchema.safeParse(input.requestId);
      if (id.success) requestId = id.data;
    }
    const unknownMethod =
      envelope.success &&
      envelope.data.method !== undefined &&
      !Object.hasOwn(catalog, envelope.data.method);
    return errorResponseSchema.parse({
      protocolVersion: PROTOCOL_VERSION,
      requestId,
      error: {
        kind: unknownMethod ? 'unsupportedOperation' : 'payloadDecodeFailed',
        code: unknownMethod ? 'unsupported_operation' : 'invalid_payload',
        message: unknownMethod ? 'Unsupported RPC operation.' : 'Invalid RPC request.',
        guidance: null,
        retryable: false,
        requestId,
      },
    });
  }
  const request: RpcRequest = parsed.data;
  try {
    const result = await handler(request);
    return responseSchema(request.method).parse({
      protocolVersion: PROTOCOL_VERSION,
      requestId: request.requestId,
      result: { ...result, requestId: request.requestId },
    });
  } catch (error) {
    return errorResponseSchema.parse({
      protocolVersion: PROTOCOL_VERSION,
      requestId: request.requestId,
      error: toWireError(error, request.requestId),
    });
  }
}
