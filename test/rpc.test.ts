import { randomUUID } from 'node:crypto';
import { expect, expectTypeOf, it } from 'vitest';
import { rpc } from '../src/client/connection.ts';
import type { RuntimePaths } from '../src/daemon/paths.ts';
import {
  catalog,
  dispatchRpc,
  errorResponseSchema,
  requestSchema,
  responseSchema,
  UNCORRELATED_REQUEST_ID,
  type RpcMethod,
  type RpcParams,
  type RpcResult,
} from '../src/protocol/rpc.ts';
import { protocolErrorSchema, RpcError, toWireError } from '../src/protocol/errors.ts';
import {
  fakeRegistration,
  fakeSnapshot,
  fakeStateResult,
  basicResult,
  stream,
  timestamp,
} from './fixtures/rpc-data.ts';

const id = randomUUID();
const header = { protocolVersion: 3, requestId: id };
const params: { [M in RpcMethod]: RpcParams<M> } = {
  'register-entrypoint-request': { registration: fakeRegistration() },
  'unregister-entrypoint-request': { registrationId: 'id', shimSessionNonce: 'nonce' },
  'heartbeat-entrypoint-request': { registrationId: 'id', shimSessionNonce: 'nonce' },
  'query-state-request': {},
  'set-entrypoint-enabled-request': { registrationId: 'id', shimSessionNonce: null, enabled: true },
  'set-domain-enabled-request': { registrationId: 'id', domainKey: 'domain', enabled: false },
  'query-logs-request': { stream, limit: null },
  'shutdown-daemon-request': {},
};
const entry = {
  sequenceNumber: 1,
  timestampUtc: timestamp,
  severity: 'info' as const,
  stream,
  attributionKind: 'runtimeControl' as const,
  entryKind: 'normal' as const,
  rawMessage: 'Fixture log.',
  domainKey: null,
  sourceRegistrationId: null,
  sourceInstanceId: null,
  operation: null,
};
const results: { [M in RpcMethod]: RpcResult<M> } = {
  'register-entrypoint-request': { ...basicResult, requestId: id, registrationId: 'id' },
  'unregister-entrypoint-request': { ...basicResult, requestId: id },
  'heartbeat-entrypoint-request': { ...basicResult, requestId: id },
  'query-state-request': { ...fakeStateResult(), requestId: id },
  'set-entrypoint-enabled-request': { ...basicResult, requestId: id },
  'set-domain-enabled-request': { ...basicResult, requestId: id },
  'query-logs-request': {
    ...basicResult,
    requestId: id,
    stream,
    streamStatus: 'active',
    entries: [entry],
  },
  'shutdown-daemon-request': { ...basicResult, requestId: id },
};
const methods = Object.keys(catalog) as RpcMethod[];

it('exposes exactly the eight released operation strings', () => {
  expect(methods).toEqual([
    'register-entrypoint-request',
    'unregister-entrypoint-request',
    'heartbeat-entrypoint-request',
    'query-state-request',
    'set-entrypoint-enabled-request',
    'set-domain-enabled-request',
    'query-logs-request',
    'shutdown-daemon-request',
  ]);
});

it.each(methods)('%s round trips closed params, results and typed errors', async (method) => {
  const request = { ...header, method, params: params[method] };
  expect(requestSchema.parse(JSON.parse(JSON.stringify(request)))).toEqual(request);
  expect(requestSchema.safeParse({ ...request, extra: true }).success).toBe(false);
  expect(
    requestSchema.safeParse({ ...request, params: { ...params[method], extra: true } }).success,
  ).toBe(false);
  expect(requestSchema.safeParse({ ...request, params: null }).success).toBe(false);
  expect(requestSchema.safeParse({ ...request, params: [] }).success).toBe(false);
  const response = { ...header, result: results[method] };
  expect(responseSchema(method).parse(JSON.parse(JSON.stringify(response)))).toEqual(response);
  expect(responseSchema(method).safeParse({ ...response, extra: true }).success).toBe(false);
  expect(
    responseSchema(method).safeParse({ ...response, result: { ...results[method], extra: true } })
      .success,
  ).toBe(false);
  expect(
    responseSchema(method).safeParse({
      ...response,
      result: { ...results[method], accepted: 'true' },
    }).success,
  ).toBe(false);
  expect(
    responseSchema(method).safeParse({
      ...response,
      result: { ...results[method], requestId: randomUUID() },
    }).success,
  ).toBe(false);
  const rejected = {
    ...header,
    error: { ...toWireError(undefined, id), kind: 'invalidInput', code: 'invalid_input' },
  };
  expect(responseSchema(method).parse(rejected)).toEqual(rejected);
  expect(responseSchema(method).safeParse({ ...response, error: rejected.error }).success).toBe(
    false,
  );
  expect(responseSchema(method).safeParse(header).success).toBe(false);
  expect(
    responseSchema(method).safeParse({ ...rejected, error: { ...rejected.error, requestId: null } })
      .success,
  ).toBe(false);
  expect(await dispatchRpc(request, async () => results[method])).toEqual(response);
});

it('keeps identity and nonce ownership decisions in business results', async () => {
  const registration = { ...fakeRegistration(), registrationId: '' };
  registration.entrypointInstance.shimSessionNonce = '';
  const request = { ...header, method: 'register-entrypoint-request', params: { registration } };
  expect(requestSchema.safeParse(request).success).toBe(true);
  expect(
    await dispatchRpc(request, async () => ({
      accepted: false,
      message: 'Owner mismatch.',
      registrationId: null,
    })),
  ).toEqual({
    ...header,
    result: { requestId: id, accepted: false, message: 'Owner mismatch.', registrationId: null },
  });
});

it('normalizes omitted serde Options to null without adding handler log defaults', () => {
  const omitted = {
    ...fakeRegistration(),
    sourceWorkingDirectory: { raw: '/fixture' },
    sourceConfigPath: { raw: '/fixture/Caddyfile' },
    ownerProcess: { processId: 123, processStartTimeUtc: timestamp, shimSessionNonce: 'nonce' },
    logStream: { streamId: 'runtime-control', channel: 'control' },
    shimRun: undefined,
    registeredDomains: [
      {
        name: { raw: 'x', canonical: 'x' },
        activationState: 'active',
        logStream: { streamId: 's', channel: 'c' },
      },
    ],
  };
  const normalized = catalog['register-entrypoint-request'].params.parse(
    JSON.parse(JSON.stringify({ registration: omitted })),
  ).registration;
  expect(normalized.sourceWorkingDirectory.canonical).toBe(null);
  expect(normalized.sourceConfigPath.canonical).toBe(null);
  expect(normalized.ownerProcess.executablePath).toBe(null);
  expect(normalized.logStream.domainKey).toBe(null);
  expect(normalized.shimRun).toBe(null);
  expect(normalized.registeredDomains[0]!.logStream.domainKey).toBe(null);
  expect(normalized.registeredDomains[0]).not.toHaveProperty('upstream');
  expect(
    catalog['register-entrypoint-request'].params.parse({
      registration: { ...omitted, shimRun: { rawArguments: [], commandLine: 'run' } },
    }).registration.shimRun?.adapter,
  ).toBe(null);
  expect(
    catalog['query-logs-request'].params.parse({ stream: { streamId: 's', channel: 'c' } }),
  ).toEqual({ stream: { streamId: 's', channel: 'c', domainKey: null }, limit: null });
  for (const upstream of [undefined, null, '127.0.0.1:3000']) {
    const registration = {
      ...fakeRegistration(),
      shimRun: null,
      registeredDomains: [{ ...fakeRegistration().registeredDomains[0], upstream }],
    };
    const normalized = catalog['register-entrypoint-request'].params.parse({ registration });
    const serialized = JSON.parse(JSON.stringify(normalized));
    if (upstream === undefined || upstream === null)
      expect(serialized.registration.registeredDomains[0]).not.toHaveProperty('upstream');
    else expect(serialized.registration.registeredDomains[0].upstream).toBe(upstream);
    expect(
      catalog['query-state-request'].result.safeParse({
        ...basicResult,
        requestId: id,
        snapshot: { ...fakeSnapshot(), registrations: [registration] },
      }).success,
    ).toBe(true);
  }
  const badUpstream = {
    ...fakeRegistration(),
    registeredDomains: [{ ...fakeRegistration().registeredDomains[0], upstream: 1 }],
  };
  expect(
    catalog['register-entrypoint-request'].params.safeParse({ registration: badUpstream }).success,
  ).toBe(false);
  for (const shimSessionNonce of [undefined, null, 'nonce']) {
    expect(
      catalog['set-entrypoint-enabled-request'].params.safeParse({
        registrationId: 'id',
        enabled: false,
        shimSessionNonce,
      }).success,
    ).toBe(true);
  }
  expect(
    catalog['query-state-request'].result.parse({ ...basicResult, requestId: id, snapshot: null })
      .snapshot,
  ).toBe(null);
  expect(
    catalog['register-entrypoint-request'].result.parse({
      ...basicResult,
      requestId: id,
      registrationId: null,
    }).registrationId,
  ).toBe(null);
  for (const limit of [undefined, null, 0, 1, 100, 200, 500, Number.MAX_SAFE_INTEGER]) {
    const input = { stream, limit };
    expect(catalog['query-logs-request'].params.parse(input)).toEqual({
      stream,
      limit: limit ?? null,
    });
  }
  for (const limit of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1, Infinity, '100']) {
    expect(catalog['query-logs-request'].params.safeParse({ stream, limit }).success).toBe(false);
  }
});

it('leaves RPC log default and clamping to the handler, not the schemas', async () => {
  for (const limit of [undefined, null, 0, 1, 50, 100, 200, 500]) {
    let observed: number | null | undefined;
    const result = await dispatchRpc(
      { ...header, method: 'query-logs-request', params: { stream, limit } },
      async (request) => {
        if (request.method !== 'query-logs-request') throw new Error('Wrong catalog dispatch.');
        observed = request.params.limit;
        const count = Math.max(1, Math.min(200, request.params.limit ?? 100));
        return {
          ...results['query-logs-request'],
          entries: Array.from({ length: count }, () => entry),
        };
      },
    );
    expect(observed).toBe(limit ?? null);
    const response = responseSchema('query-logs-request').parse(result);
    expect(
      'result' in response && 'entries' in response.result ? response.result.entries.length : -1,
    ).toBe(Math.max(1, Math.min(200, limit ?? 100)));
  }
});

it('rejects nested unknown fields, missing fields, bad enum/time/number representations', () => {
  const registration = fakeRegistration();
  const badRegistrations = [
    { ...registration, entrypointInstance: { ...registration.entrypointInstance, extra: true } },
    { ...registration, ownerProcess: { ...registration.ownerProcess, processId: 0x100000000 } },
    { ...registration, sourceConfigPath: { raw: 'x', canonical: 1 } },
    { ...registration, activationState: 'enabled' },
    { ...registration, createdAtUtc: 'yesterday' },
    { ...registration, logStream: { ...stream, extra: 1 } },
    {
      ...registration,
      registeredDomains: [
        { ...registration.registeredDomains[0], name: { raw: 'x', canonical: 'x', extra: true } },
      ],
    },
    { ...registration, shimRun: { ...registration.shimRun, extra: true } },
  ];
  for (const candidate of badRegistrations)
    expect(
      catalog['register-entrypoint-request'].params.safeParse({ registration: candidate }).success,
    ).toBe(false);
  for (const method of ['unregister-entrypoint-request', 'heartbeat-entrypoint-request'] as const)
    expect(catalog[method].params.safeParse({ registrationId: 'id' }).success).toBe(false);
  expect(
    catalog['set-domain-enabled-request'].params.safeParse({ registrationId: 'id', enabled: true })
      .success,
  ).toBe(false);
  expect(
    catalog['set-entrypoint-enabled-request'].params.safeParse({ registrationId: 'id', enabled: 1 })
      .success,
  ).toBe(false);
  const snapshot = fakeSnapshot();
  for (const candidate of [
    { ...snapshot, recovered: true },
    { ...snapshot, runtime: { ...snapshot.runtime, status: 'online' } },
    {
      ...snapshot,
      runtime: {
        ...snapshot.runtime,
        diagnostics: [{ code: 'x', message: 'x', operation: null, extra: true }],
      },
    },
    {
      ...snapshot,
      config: {
        ...snapshot.config,
        diagnostics: [
          { code: 'x', message: 'x', domainKey: null, sourceConfigPaths: [], extra: true },
        ],
      },
    },
    { ...snapshot, storage: { ...snapshot.storage, schemaVersion: -1 } },
  ])
    expect(
      catalog['query-state-request'].result.safeParse({
        ...basicResult,
        requestId: id,
        snapshot: candidate,
      }).success,
    ).toBe(false);
  const logs = results['query-logs-request'];
  expect(
    catalog['query-logs-request'].result.safeParse({
      ...logs,
      entries: Array.from({ length: 200 }, () => entry),
    }).success,
  ).toBe(true);
  expect(
    catalog['query-logs-request'].result.safeParse({
      ...logs,
      entries: Array.from({ length: 201 }, () => entry),
    }).success,
  ).toBe(false);
  for (const candidate of [
    { ...entry, sequenceNumber: Number.MAX_SAFE_INTEGER + 1 },
    { ...entry, severity: 'verbose' },
    { ...entry, attributionKind: 'server' },
    { ...entry, entryKind: 'gap' },
    { ...entry, stream: { ...stream, extra: 1 } },
  ])
    expect(
      catalog['query-logs-request'].result.safeParse({ ...logs, entries: [candidate] }).success,
    ).toBe(false);
});

it('round trips all typed error kinds and rejects malformed closed errors', () => {
  for (const kind of protocolErrorSchema.shape.kind.options) {
    const error = { ...toWireError(undefined, id), kind };
    expect(protocolErrorSchema.parse(JSON.parse(JSON.stringify(error)))).toEqual(error);
    expect(new RpcError(error).protocolError).toEqual(error);
  }
  const error = {
    ...toWireError(undefined, id),
    kind: 'accessDenied',
    deniedOperation: 'shutdown-daemon-request',
  };
  expect(protocolErrorSchema.parse(error)).toEqual(error);
  for (const deniedOperation of [undefined, null]) {
    const normalized = protocolErrorSchema.parse({ ...error, deniedOperation });
    expect(JSON.parse(JSON.stringify(normalized))).not.toHaveProperty('deniedOperation');
    expect(
      errorResponseSchema.safeParse({ ...header, error: { ...error, deniedOperation } }).success,
    ).toBe(true);
  }
  expect(protocolErrorSchema.safeParse({ ...error, deniedOperation: 1 }).success).toBe(false);
  for (const code of ['', '_code', 'a_', 'A', 'a-b', 'a'.repeat(65)])
    expect(protocolErrorSchema.safeParse({ ...error, code }).success).toBe(false);
  for (const candidate of [
    { ...error, extra: true },
    { ...error, guidance: undefined },
    { ...error, requestId: undefined },
    { ...error, retryable: 'false' },
    { ...error, kind: 'generic' },
  ])
    expect(protocolErrorSchema.safeParse(candidate).success).toBe(false);
});

it('dispatch sanitizes invalid handler output and untrusted throws', async () => {
  const request = { ...header, method: 'query-state-request', params: {} };
  const result = await dispatchRpc(request, async () => ({
    ...fakeStateResult(),
    requestId: 'handler cannot choose id',
  }));
  expect(responseSchema('query-state-request').parse(result)).toEqual({
    ...header,
    result: { ...fakeStateResult(), requestId: id },
  });
  const rejected = await dispatchRpc(request, async () => {
    throw new Error('sensitive details');
  });
  expect(errorResponseSchema.parse(rejected).error).toEqual(toWireError(undefined, id));
  expect(JSON.stringify(rejected)).not.toContain('sensitive');
  let called = false;
  expect(
    errorResponseSchema.parse(
      await dispatchRpc({}, async () => {
        called = true;
        return basicResult;
      }),
    ).requestId,
  ).toBe(UNCORRELATED_REQUEST_ID);
  expect(called).toBe(false);
});

// Checked by tsc, never invoked: payload requirements and method-indexed returns.
function staticContract(paths: RuntimePaths) {
  expectTypeOf(rpc(paths, 'query-state-request')).toEqualTypeOf<
    Promise<RpcResult<'query-state-request'>>
  >();
  expectTypeOf(rpc(paths, 'query-logs-request', { stream })).toEqualTypeOf<
    Promise<RpcResult<'query-logs-request'>>
  >();
  // @ts-expect-error No arbitrary method surface.
  void rpc(paths, 'status');
  // @ts-expect-error Mutation params are required.
  void rpc(paths, 'heartbeat-entrypoint-request');
  // @ts-expect-error Domain key is required.
  void rpc(paths, 'set-domain-enabled-request', { registrationId: 'id', enabled: true });
  // @ts-expect-error State return cannot be mistaken for a log result.
  const logs: Promise<RpcResult<'query-logs-request'>> = rpc(paths, 'query-state-request');
  void logs;
}
void staticContract;
