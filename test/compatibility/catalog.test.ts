import { expect, it } from 'vitest';
import { requestSchema, responseSchema, errorResponseSchema } from '../../src/protocol/rpc.ts';
import { status, projects, domains, logs } from '../../src/cli/output.ts';
import { fakeSnapshot } from '../fixtures/rpc-data.ts';
import { humanOutputFixture, rpcExamples, rpcOperationFixtures } from './catalog.ts';

it('accepts the frozen released RPC catalog through the actual Node schemas', () => {
  for (const fixture of rpcOperationFixtures) {
    const request = {
      protocolVersion: 3,
      requestId: fixture.result.requestId,
      method: fixture.method,
      params: fixture.params,
    };
    expect(requestSchema.parse(request)).toEqual(request);
    const response = { protocolVersion: 3, requestId: request.requestId, result: fixture.result };
    expect(responseSchema(fixture.method).parse(response)).toEqual(response);
    expect(responseSchema(fixture.method).safeParse({ ...response, extra: true }).success).toBe(
      false,
    );
  }
  expect(
    errorResponseSchema.parse({
      protocolVersion: 3,
      requestId: rpcExamples.protocolError.requestId,
      error: rpcExamples.protocolError.error,
    }),
  ).toHaveProperty('error');
});

it('matches the released empty-state and bounded-log human output using the Node renderer', () => {
  const snapshot = fakeSnapshot();
  snapshot.registrations = [];
  snapshot.runtime.status = 'idle';
  const actual = [status(snapshot), projects(snapshot), domains(snapshot)].join('\n') + '\n';
  expect(humanOutputFixture.cases[0]!.output.startsWith(actual)).toBe(true);
  const empty = logs({
    requestId: '00000000-0000-4000-8000-000000000001',
    accepted: true,
    message: '',
    stream: {
      streamId: 'domain-active.localhost',
      domainKey: 'active.localhost',
      channel: 'caddy',
    },
    streamStatus: 'empty',
    entries: [],
  });
  expect(humanOutputFixture.cases[3]!.output).toContain(empty + '\n');
});
