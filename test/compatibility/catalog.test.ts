import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse as parseToml } from 'smol-toml';
import { describe, expect, it } from 'vitest';
import { errorResponseSchema, requestSchema, responseSchema } from '../../src/protocol/rpc.ts';
import {
  caddyResolutionAuthority,
  caddyResolutionPrecedence,
  caddyfileExamples,
  cliCases,
  compatibilityCatalogVersion,
  configurationCases,
  exitCodes,
  humanOutput,
  humanOutputFixture,
  operationCatalog,
  registrationFixture,
  responseContract,
  rpcExamples,
  rpcOperationFixtures,
  rpcSnapshotFixture,
  shimAliases,
  shimCommandCases,
  shimRunMetadata,
  sourceAnchors,
  stateModelFields,
} from './catalog.ts';

function findRepositoryRoot(): string {
  let candidate = resolve(dirname(fileURLToPath(import.meta.url)));
  while (candidate !== dirname(candidate)) {
    if (existsSync(join(candidate, 'crates')) && existsSync(join(candidate, 'openspec'))) {
      return candidate;
    }
    candidate = dirname(candidate);
  }
  throw new Error('Could not locate the Cadder repository root');
}

const repositoryRoot = findRepositoryRoot();

function sourceText(path: string): string {
  return readFileSync(join(repositoryRoot, path), 'utf8');
}

function hasMarker(path: string, marker: string): boolean {
  return sourceText(path).includes(marker);
}

function snakeCase(name: string): string {
  return name.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
}

function regexEscape(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function sourcePolicyKind(command: string): string | undefined {
  const pattern = new RegExp(
    `policy_entry\\(\\s*"${regexEscape(command)}",\\s*ShimCommandPolicyKind::(\\w+)`,
    'm',
  );
  return pattern.exec(sourceText('crates/cadder-shim/src/command_policy.rs'))?.[1];
}

function operationConstantName(operation: string): string {
  return operation.replaceAll('-', '_').toUpperCase();
}

function sourceOperationAccess(operation: string): string | undefined {
  const pattern = new RegExp(
    `message_types::${operationConstantName(operation)},\\s*OperationAccess::(\\w+)`,
    'm',
  );
  return pattern.exec(sourceText('crates/cadder-ipc/src/operations.rs'))?.[1];
}

const policyKinds = {
  managed: 'Managed',
  'read-only-inspection': 'ReadOnlyInspection',
  'explicit-passthrough': 'ExplicitPassthrough',
  unsupported: 'Unsupported',
} as const;

describe('released 1.0.5 compatibility catalog', () => {
  it('is versioned and anchored to the complete current authority set', () => {
    expect(compatibilityCatalogVersion).toBe('1.0.5-rust-baseline');
    for (const anchor of sourceAnchors) {
      for (const marker of anchor.markers) {
        expect(hasMarker(anchor.path, marker), `${anchor.path} is missing ${marker}`).toBe(true);
      }
    }
  });

  it('freezes CLI grammar, nested help, exits, and observable streams', () => {
    const successful = cliCases.filter(({ expected }) => expected === 'success');
    const rejected = cliCases.filter(({ expected }) => expected === 'invalid-usage');
    expect(successful.length).toBeGreaterThan(20);
    expect(rejected.length).toBeGreaterThan(10);
    expect(new Set(cliCases.map(({ argv }) => argv.join('\u0000'))).size).toBe(cliCases.length);

    for (const testCase of cliCases) {
      expect(testCase.argv[0]).toBe('cadder');
      expect(testCase.exitCode).toBe(
        testCase.expected === 'success' ? exitCodes.success : exitCodes.invalidUsage,
      );
      if (testCase.expected === 'invalid-usage') expect(testCase.stream).toBe('stderr');
    }
    expect(cliCases.find(({ argv }) => argv.length === 1)).toMatchObject({
      expected: 'success',
      exitCode: 0,
      stream: 'stderr',
    });
    for (const group of ['daemon', 'projects', 'domains', 'port', 'caddyfile', 'logs']) {
      expect(cliCases.find(({ argv }) => argv.join(' ') === `cadder ${group}`)).toMatchObject({
        expected: 'success',
        exitCode: 0,
        stream: 'stderr',
      });
    }
    expect(cliCases.slice(1, 5).every(({ stream }) => stream === 'stdout')).toBe(true);
    expect(cliCases.some(({ argv }) => argv.join(' ') === 'cadder daemon')).toBe(true);
    expect(cliCases.some(({ argv }) => argv.join(' ') === 'cadder projects')).toBe(true);
    expect(cliCases.some(({ argv }) => argv.join(' ') === 'cadder logs')).toBe(true);
    expect(
      cliCases.some(
        ({ argv }) =>
          argv.join(' ') === 'cadder domains inspect app.localhost --caddyfile Caddyfile',
      ),
    ).toBe(true);
    expect(
      cliCases.some(
        ({ argv }) =>
          argv.join(' ') === 'cadder logs domain app.localhost --caddyfile Caddyfile --limit 1',
      ),
    ).toBe(true);
    expect(
      cliCases
        .filter(({ notes }) => notes.includes('removed'))
        .every(({ exitCode, stream }) => exitCode === 2 && stream === 'stderr'),
    ).toBe(true);
    expect(sourceText('crates/cadder-client/src/main.rs')).toContain('let _ = error.print();');
  });

  it('agrees with captured released non-TTY exit codes and output streams', () => {
    const capture = JSON.parse(
      readFileSync(new URL('./cli-output.json', import.meta.url), 'utf8'),
    ) as {
      version: string;
      cases: { argv: string[]; exitCode: number; stdout: string; stderr: string }[];
    };
    expect(capture.version).toBe('1.0.5');
    for (const captured of capture.cases) {
      const catalogCase = cliCases.find(
        ({ argv }) => argv.slice(1).join('\u0000') === captured.argv.join('\u0000'),
      );
      expect(catalogCase, `Missing captured argv: ${captured.argv.join(' ')}`).toBeDefined();
      expect(catalogCase?.exitCode).toBe(captured.exitCode);
      expect(catalogCase?.stream).toBe(captured.stdout ? 'stdout' : 'stderr');
      expect(Boolean(captured.stdout)).not.toBe(Boolean(captured.stderr));
    }
  });

  it('freezes the released exit-code classes without introducing machine output', () => {
    expect(exitCodes).toEqual({
      success: 0,
      invalidUsage: 2,
      daemonUnavailable: 3,
      daemonStartFailure: 4,
      targetNotFound: 5,
      conflictOrRejected: 6,
      permissionOrElevation: 7,
      unsupportedOperation: 8,
      ipcFailure: 9,
    });
    expect(sourceText('crates/cadder-api/src/error.rs')).toContain('pub enum AppExit');
  });

  it('freezes exact human output captures from the retained Rust renderer', () => {
    const outputSource = sourceText('crates/cadder-client/src/cli/output.rs');
    expect(humanOutputFixture).toMatchObject({
      sourceRevision: '93942af84f555eab2bd68e0a4a9db487ff82d040',
      version: '1.0.5',
      platform: 'win32',
    });
    expect(humanOutputFixture.capture).toContain('cargo test --locked -p cadder-client');
    expect(humanOutputFixture.capture).toContain('including its final LF');
    expect(humanOutputFixture.sourceState).toContain('fixed-time test inputs');
    expect(humanOutputFixture.portability).toContain('path separators are platform-specific');
    expect(humanOutputFixture.cases.map(({ name }) => name)).toEqual(humanOutput.captureNames);
    expect(new Set(humanOutputFixture.cases.map(({ name }) => name)).size).toBe(
      humanOutputFixture.cases.length,
    );
    for (const { name, output } of humanOutputFixture.cases) {
      expect(output, `${name} must contain rendered output`).not.toBe('');
      expect(output, `${name} must use stable LF captures`).not.toContain('\r');
      expect(output.endsWith('\n'), `${name} must preserve its final LF`).toBe(true);
      expect(outputSource).toContain(name);
    }

    const captures = new Map(humanOutputFixture.cases.map(({ name, output }) => [name, output]));
    expect(captures.get(humanOutput.captureNames[0])).toContain(
      `${humanOutput.status.join('\n')}\n`,
    );
    expect(captures.get(humanOutput.captureNames[0])).toContain('No projects registered.');
    expect(captures.get(humanOutput.captureNames[0])).toContain('active.localhost');
    expect(captures.get(humanOutput.captureNames[1])).toContain('No matching routes.');
    expect(captures.get(humanOutput.captureNames[1])).toContain('socket inspection unavailable');
    expect(captures.get(humanOutput.captureNames[2])).toContain('Registered yes');
    expect(captures.get(humanOutput.captureNames[2])).toContain('Local upstream sockets');
    expect(captures.get(humanOutput.captureNames[3])).toContain('runtime-warning');
    expect(captures.get(humanOutput.captureNames[3])).toContain('2026-01-01T00:00:00+00:00');
    expect(captures.get(humanOutput.captureNames[3])).toContain(
      `Status   ${humanOutput.populatedLogs.status}\nChannel  ${humanOutput.populatedLogs.channel}\nDomain   ${humanOutput.populatedLogs.domain}\n`,
    );

    for (const line of [
      ...humanOutput.emptyProjects,
      ...humanOutput.emptyDomains,
      ...humanOutput.emptyLogs,
      ...humanOutput.logHeader,
      ...humanOutput.diagnosticHeaders,
      ...humanOutput.populatedProjects.headers,
      ...humanOutput.populatedDomains.headers,
      ...humanOutput.populatedPort.sections,
      ...humanOutput.populatedPort.ownerHeaders,
      ...humanOutput.populatedCaddyfile.sections,
      ...humanOutput.populatedCaddyfile.headers,
    ]) {
      expect(outputSource, `output.rs is missing ${line}`).toContain(line);
    }
  });

  it('freezes shim classifications independently against each Rust policy entry', () => {
    expect(shimCommandCases.filter(({ policy }) => policy === 'managed')).toHaveLength(1);
    expect(shimCommandCases.filter(({ policy }) => policy === 'read-only-inspection')).toHaveLength(
      7,
    );
    expect(shimCommandCases.filter(({ policy }) => policy === 'explicit-passthrough')).toHaveLength(
      5,
    );
    expect(shimCommandCases.filter(({ policy }) => policy === 'unsupported')).toHaveLength(9);
    for (const { command, policy } of shimCommandCases) {
      if (command === 'not-a-caddy-command') {
        expect(sourcePolicyKind(command)).toBeUndefined();
      } else {
        expect(sourcePolicyKind(command)).toBe(policyKinds[policy]);
      }
    }
    const policySource = sourceText('crates/cadder-shim/src/command_policy.rs');
    for (const alias of shimAliases) {
      for (const argument of alias.argv) {
        expect(policySource).toContain(`"${argument}"`);
      }
    }
    expect(policySource).toContain('None | Some("--help" | "-h" | "help")');
    expect(policySource).toContain('Some("--version" | "-v" | "version")');
    expect(shimRunMetadata.delegation.classes).toEqual([
      'read-only-inspection',
      'explicit-passthrough',
    ]);
  });

  it('separates managed-run lifecycle outcomes from delegated child fidelity', () => {
    const shimSource = sourceText('crates/cadder-shim/src/main.rs');
    expect(shimRunMetadata.managedRun).toMatchObject({
      streams: 'no child delegation; registration session owns no real-Caddy child',
      exitCodes: { clean: 0, backendOrRecoveryFailure: 1 },
      missingDaemon: 'start managed daemon; never delegate to independent Caddy',
    });
    expect(shimRunMetadata.delegation).toMatchObject({
      streams: ['stdin', 'stdout', 'stderr'],
      exitStatus: 'status.code() as u8, or 1 when no status exists',
    });
    expect(shimSource).toContain('run_managed_until');
    expect(shimSource).toContain('Ok(ExitCode::SUCCESS)');
    expect(shimSource).toContain('ExitCode::FAILURE');
    expect(shimSource).toContain('.stdin(Stdio::inherit())');
    expect(shimSource).toContain('status.code().unwrap_or(1) as u8');
  });

  it('freezes config cases and verifies resolver precedence from source order', () => {
    const configSource = sourceText('crates/cadder-daemon/src/config.rs');
    for (const configurationCase of configurationCases) {
      expect(configSource).toContain(configurationCase.sourceMarker);
    }
    const resolverSource = sourceText('crates/cadder-daemon/src/caddy/resolver.rs');
    const uncachedSource = resolverSource.slice(
      resolverSource.indexOf('fn resolve_uncached'),
      resolverSource.indexOf('pub fn resolution_help'),
    );
    expect(uncachedSource.indexOf('ExplicitDaemonOverride')).toBeGreaterThanOrEqual(0);
    expect(uncachedSource.indexOf('self.configured_sources()')).toBeGreaterThan(
      uncachedSource.indexOf('ExplicitDaemonOverride'),
    );
    expect(uncachedSource.indexOf('resolve_caddy_on_path')).toBeGreaterThan(
      uncachedSource.indexOf('self.configured_sources()'),
    );
    const configuredSource = resolverSource.slice(
      resolverSource.indexOf('fn configured_sources'),
      resolverSource.indexOf('fn portable_config_path'),
    );
    const configuredPositions = [
      'PortableConfiguration',
      'UserConfiguration',
      'SystemConfiguration',
    ].map((marker) => configuredSource.indexOf(marker));
    expect(configuredPositions.every((position) => position >= 0)).toBe(true);
    expect(configuredPositions).toEqual(
      [...configuredPositions].sort((left, right) => left - right),
    );
    expect(caddyResolutionPrecedence).toHaveLength(5);
    for (const test of caddyResolutionAuthority.tests) {
      expect(sourceText(caddyResolutionAuthority.path)).toContain(`fn ${test}(`);
    }
  });

  it('parses concrete supported TOML fixtures and rejects malformed syntax', () => {
    expect(parseToml(configurationCases[0].toml)).toEqual({
      caddy: { real_command: 'caddy-real' },
    });
    expect(parseToml(configurationCases[1].toml)).toEqual({ caddy: { real_path: '/opt/caddy' } });
    expect(parseToml(configurationCases[2].toml)).toEqual({
      caddy: { real_command: 'caddy-real', real_path: '/opt/caddy' },
    });
    expect(parseToml(configurationCases[3].toml)).toEqual({
      defaults: { real_caddy: '/opt/caddy' },
    });
    expect(parseToml(configurationCases[4].toml)).toEqual({ caddy: { unknown: 'caddy' } });
    expect(() => parseToml(configurationCases[5].toml)).toThrow();
  });

  it('freezes real Caddyfile newlines and inspection correlation boundaries', () => {
    expect(caddyfileExamples).toHaveLength(4);
    expect(caddyfileExamples.filter(({ expected }) => expected === 'accepted')).toHaveLength(3);
    expect(caddyfileExamples.filter(({ expected }) => expected === 'rejected')).toHaveLength(1);
    expect(caddyfileExamples.every(({ source }) => source.includes('\n'))).toBe(true);
    expect(caddyfileExamples.every(({ source }) => !source.includes('\\n'))).toBe(true);
    for (const { evidence } of caddyfileExamples) {
      expect(evidence.aspect).not.toBe('');
      expect(sourceText(evidence.path)).toContain(`fn ${evidence.test}(`);
    }
    expect(caddyfileExamples[2].evidence.aspect).toContain('not acceptance of this Caddyfile');
    expect(caddyfileExamples[0]).toMatchObject({
      domain: 'app.localhost',
      upstream: '127.0.0.1:3000',
      localPort: 3000,
    });
    expect(caddyfileExamples[1]).toMatchObject({
      domain: 'remote.example',
      upstream: 'api.example.com:3000',
      localPort: null,
    });
    expect(caddyfileExamples[2]).toMatchObject({ domain: '', upstream: null, localPort: null });
    expect(caddyfileExamples[3]).toMatchObject({ domain: null, upstream: null, localPort: null });
    const adapterSource = sourceText('crates/cadder-daemon/src/caddy/tests.rs');
    expect(adapterSource).toContain('mock_adapter_reports_invalid_caddyfile');
    expect(adapterSource).toContain('app.localhost {\\n');
    const inspectionSource = sourceText('crates/cadder-client/src/inspection.rs');
    expect(inspectionSource).toContain('routes_for_port');
    expect(inspectionSource).toContain('local_upstream');
    expect(inspectionSource).toContain('canonical_domain');
  });

  it('freezes nested registration identity, owner/session constraints, and log attribution', () => {
    expect(registrationFixture.registrationId).toBe(
      registrationFixture.entrypointInstance.instanceId,
    );
    expect(registrationFixture.entrypointInstance.shimSessionNonce).toBe(
      registrationFixture.ownerProcess.shimSessionNonce,
    );
    expect(registrationFixture.registeredDomains[0].name.canonical).toBe('app.localhost');
    expect(registrationFixture.registeredDomains[0].logStream).toEqual({
      streamId: 'domain-app.localhost',
      domainKey: 'app.localhost',
      channel: 'caddy',
    });
    const invalidInstance = { ...registrationFixture, registrationId: 'different-registration' };
    expect(invalidInstance.registrationId).not.toBe(invalidInstance.entrypointInstance.instanceId);
    const invalidNonce = {
      ...registrationFixture,
      ownerProcess: { ...registrationFixture.ownerProcess, shimSessionNonce: 'different-session' },
    };
    expect(invalidNonce.entrypointInstance.shimSessionNonce).not.toBe(
      invalidNonce.ownerProcess.shimSessionNonce,
    );
    const stateSource = sourceText('crates/cadder-ipc/src/state.rs');
    expect(stateSource).toContain('registration_id must match instance_id');
    expect(stateSource).toContain('entrypoint and owner shim session nonce values must match');
    expect(stateModelFields.entrypointInstance).toEqual([
      'instanceId',
      'startedAtUtc',
      'shimSessionNonce',
    ]);
  });

  it('freezes exactly eight operation payload/result shapes against defining Rust types', () => {
    const operationSource = sourceText('crates/cadder-ipc/src/message_types.rs');
    const payloadSources: Record<string, string> = {
      RegisterEntrypointPayload: 'crates/cadder-ipc/src/mutations.rs',
      UnregisterEntrypointPayload: 'crates/cadder-ipc/src/mutations.rs',
      HeartbeatEntrypointPayload: 'crates/cadder-ipc/src/mutations.rs',
      QueryStatePayload: 'crates/cadder-ipc/src/commands.rs',
      SetEntrypointEnabledPayload: 'crates/cadder-ipc/src/mutations.rs',
      SetDomainEnabledPayload: 'crates/cadder-ipc/src/mutations.rs',
      QueryLogsPayload: 'crates/cadder-ipc/src/logs.rs',
      ShutdownDaemonPayload: 'crates/cadder-ipc/src/mutations.rs',
    };
    const resultSources: Record<string, string> = {
      RegisterEntrypointResponse: 'crates/cadder-ipc/src/responses.rs',
      BasicResponse: 'crates/cadder-ipc/src/responses.rs',
      QueryStateResponse: 'crates/cadder-ipc/src/responses.rs',
      QueryLogsResponse: 'crates/cadder-ipc/src/logs.rs',
    };
    expect(operationCatalog).toHaveLength(8);
    expect(new Set(operationCatalog.map(({ operation }) => operation)).size).toBe(8);
    for (const operation of operationCatalog) {
      expect(operationSource).toContain(`"${operation.operation}"`);
      expect(sourceOperationAccess(operation.operation)).toBe(
        operation.access === 'read-only' ? 'ReadOnly' : 'Mutation',
      );
      const payloadPath = payloadSources[operation.payloadType];
      if (!payloadPath) {
        throw new Error(`Missing payload source mapping for ${operation.payloadType}`);
      }
      const payloadSource = sourceText(payloadPath);
      expect(payloadSource).toContain(`pub struct ${operation.payloadType}`);
      for (const field of operation.payloadFields) {
        const fieldPattern = new RegExp(`pub\\s+${snakeCase(field.name)}\\s*:`);
        expect(payloadSource).toMatch(fieldPattern);
        if (field.optional) {
          expect(payloadSource).toMatch(
            new RegExp(`pub\\s+${snakeCase(field.name)}\\s*:\\s*Option<`),
          );
        }
      }
      const resultPath = resultSources[operation.resultType];
      if (!resultPath) {
        throw new Error(`Missing result source mapping for ${operation.resultType}`);
      }
      const resultSource = sourceText(resultPath);
      expect(resultSource).toContain(`pub struct ${operation.resultType}`);
      for (const field of operation.resultFields) {
        expect(resultSource).toMatch(new RegExp(`pub\\s+${snakeCase(field.name)}\\s*:`));
        if (field.optional) {
          expect(resultSource).toMatch(
            new RegExp(`pub\\s+${snakeCase(field.name)}\\s*:\\s*Option<`),
          );
        }
      }
    }
    expect(
      operationCatalog.find(({ operation }) => operation === 'query-logs-request')?.payloadFields,
    ).toEqual([
      { name: 'stream', optional: false },
      { name: 'limit', optional: true },
    ]);
  });

  it('round trips every concrete request/result fixture through the closed Node DTO parser', () => {
    for (const fixture of rpcOperationFixtures) {
      const request = {
        protocolVersion: 3,
        requestId: '00000000-0000-4000-8000-000000000001',
        method: fixture.method,
        params: fixture.params,
      };
      expect(requestSchema.parse(request)).toEqual(request);
      const response = {
        protocolVersion: 3,
        requestId: request.requestId,
        result: fixture.result,
      };
      expect(responseSchema(fixture.method).parse(response)).toEqual(response);
      expect(responseSchema(fixture.method).safeParse({ ...response, extra: true }).success).toBe(
        false,
      );
    }
    expect(rpcSnapshotFixture.registrations[0]?.registrationId).toBe('shim-fixture-01');
  });

  it('freezes correlated RPC examples, protocol errors, streams, and both log defaults', () => {
    expect(responseContract).toMatchObject({
      requestFields: ['protocolVersion', 'operation', 'requestId', 'payload'],
      successFields: ['protocolVersion', 'operation', 'requestId', 'result'],
      errorFields: ['protocolVersion', 'operation', 'requestId', 'error'],
      protocolErrorResponse: { name: 'protocol-error-response', fields: ['requestId', 'error'] },
      errorRequestIdRule: 'response and protocol error request IDs must match',
      exactlyOneOutcome: true,
      unknownFieldsRejected: true,
      logLimit: { minimum: 1, maximum: 200, cliDefault: 50, rpcDefault: 100 },
    });
    expect(responseContract.streamStatuses).toEqual([
      'unknown',
      'empty',
      'active',
      'stale',
      'removed',
      'readError',
    ]);
    expect(rpcExamples.queryLogsRequest.requestId).toBe(rpcExamples.queryLogsSuccess.requestId);
    expect(rpcExamples.queryLogsSuccess).not.toHaveProperty('error');
    expect(rpcExamples.protocolError).not.toHaveProperty('result');
    expect(rpcExamples.protocolError.requestId).toBe(rpcExamples.protocolError.error.requestId);
    expect(
      errorResponseSchema.parse({
        protocolVersion: 3,
        requestId: rpcExamples.protocolError.requestId,
        error: rpcExamples.protocolError.error,
      }),
    ).toMatchObject({ requestId: rpcExamples.protocolError.requestId });
    expect(sourceText('crates/cadder-daemon/src/state/log_queries.rs')).toContain(
      'unwrap_or(100).clamp(1, 200)',
    );
    expect(sourceText('crates/cadder-client/src/cli/mod.rs')).toContain('default_value_t = 50');
  });
});
