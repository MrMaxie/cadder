import type { EntrypointRegistration, StateSnapshot } from '../../src/protocol/dto.ts';

export const timestamp = '2026-01-01T00:00:00.000Z';
export const stream = { streamId: 'runtime-control', domainKey: null, channel: 'control' };
export function fakeRegistration(): EntrypointRegistration {
  return {
    registrationId: 'shim-fixture',
    entrypointInstance: {
      instanceId: 'shim-fixture',
      startedAtUtc: timestamp,
      shimSessionNonce: 'nonce',
    },
    sourceWorkingDirectory: { raw: '/fixture', canonical: null },
    sourceConfigPath: { raw: '/fixture/Caddyfile', canonical: '/fixture/Caddyfile' },
    registeredDomains: [
      {
        name: { raw: 'example.localhost', canonical: 'example.localhost' },
        activationState: 'active',
        logStream: stream,
      },
    ],
    activationState: 'registered',
    ownerProcess: {
      processId: 123,
      processStartTimeUtc: timestamp,
      shimSessionNonce: 'nonce',
      executablePath: null,
    },
    logStream: stream,
    shimRun: { adapter: null, rawArguments: ['run'], commandLine: 'caddy run' },
    createdAtUtc: timestamp,
    lastHeartbeatUtc: timestamp,
  };
}
export function fakeSnapshot(processId = process.pid): StateSnapshot {
  return {
    capturedAtUtc: timestamp,
    registrations: [fakeRegistration()],
    runtime: {
      status: 'running',
      binaryPath: null,
      version: null,
      processId,
      adminEndpoint: null,
      diagnostics: [],
    },
    config: {
      status: 'idle',
      lastAttemptedAtUtc: null,
      lastSuccessfulReloadAtUtc: null,
      effectiveConfigHash: null,
      diagnostics: [],
    },
    storage: { backend: 'fixture', path: null, schemaVersion: 1, diagnostics: [] },
  };
}
export function fakeStateResult(processId = process.pid) {
  return { accepted: true, message: 'Fixture state.', snapshot: fakeSnapshot(processId) };
}
export const basicResult = { accepted: true, message: 'Fixture accepted.' };
