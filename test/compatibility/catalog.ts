import { readFileSync } from 'node:fs';

export const compatibilityCatalogVersion = '1.0.5-rust-baseline';

export type SourceAnchor = {
  path: string;
  markers: readonly string[];
};

export const sourceAnchors: readonly SourceAnchor[] = [
  {
    path: 'crates/cadder-client/src/cli/mod.rs',
    markers: [
      'arg_required_else_help = true',
      'pub(crate) enum Command',
      'start_daemon: bool',
      'value_parser = parse_log_limit',
      'Status,',
      'Daemon {',
      'Projects {',
      'Domains {',
      'Port {',
      'Caddyfile {',
      'Diagnostics,',
      'Logs {',
      'Tui {',
      'default_value_t = 50',
    ],
  },
  {
    path: 'crates/cadder-client/src/main.rs',
    markers: [
      'DisplayHelpOnMissingArgumentOrSubcommand',
      'AppExit::InvalidUsage',
      'let _ = error.print();',
    ],
  },
  {
    path: 'crates/cadder-api/src/error.rs',
    markers: [
      'Success = 0',
      'InvalidUsage = 2',
      'DaemonUnavailable = 3',
      'DaemonStartFailure = 4',
      'TargetNotFound = 5',
      'ConflictOrRejected = 6',
      'PermissionOrElevation = 7',
      'UnsupportedOperation = 8',
      'IpcFailure = 9',
    ],
  },
  {
    path: 'crates/cadder-client/src/cli/output.rs',
    markers: [
      'cadderd  running',
      'Caddy    {}',
      'Config   {}',
      'No projects registered.',
      'No domains registered.',
      'No log entries.',
      'PROJECT STATE',
      'STATE", "DOMAIN", "UPSTREAM", "PROJECT',
      'TIME (UTC)',
      'terminal_safe',
      'list_and_status_outputs_cover_empty_and_populated_snapshots',
      'port_output_covers_socket_and_registration_availability',
      'route_outputs_cover_registered_unregistered_and_socket_notes',
      'diagnostics_and_logs_cover_empty_and_detailed_reports',
    ],
  },
  {
    path: 'crates/cadder-shim/src/command_policy.rs',
    markers: [
      'SHIM_COMMAND_POLICY_TABLE',
      'normalized_caddy_command',
      'None | Some("--help" | "-h" | "help")',
      'Some("--version" | "-v" | "version")',
      '"run"',
      '"adapt"',
      '"build-info"',
      '"environ"',
      '"help"',
      '"list-modules"',
      '"validate"',
      '"version"',
      '"completion"',
      '"file-server"',
      '"fmt"',
      '"manpage"',
      '"reverse-proxy"',
      '"add-package"',
      '"reload"',
      '"remove-package"',
      '"start"',
      '"stop"',
      '"trust"',
      '"untrust"',
      '"upgrade"',
    ],
  },
  {
    path: 'crates/cadder-shim/src/main.rs',
    markers: [
      'allow_hyphen_values',
      '--cadder-shim-info',
      'Stdio::inherit()',
      'status.code().unwrap_or(1) as u8',
      'ExitCode::FAILURE',
      'run_managed_until',
      'delegate_to_real_caddy',
    ],
  },
  {
    path: 'crates/cadder-daemon/tests/fixtures/test_process.rs',
    markers: [
      'shim-passthrough-',
      'FAKE_CADDY_PASSTHROUGH_STDOUT',
      'FAKE_CADDY_PASSTHROUGH_STDERR',
      '"stdin_bytes"',
    ],
  },
  {
    path: 'crates/cadder-shim/tests/shim_binary.rs',
    markers: [
      'real_caddy_passthrough_preserves_arguments_stdin_streams_and_success_exit',
      'real_caddy_passthrough_propagates_nonzero_child_exit',
      'CARGO_BIN_EXE_cadder-shim',
    ],
  },
  {
    path: 'crates/cadder-shim/src/registration.rs',
    markers: [
      'cwd.join("Caddyfile")',
      '"--config" | "-c"',
      '"--adapter" | "-a"',
      'shim_session_nonce',
    ],
  },
  {
    path: 'crates/cadder-daemon/src/config.rs',
    markers: ['CONFIG_FILE_NAME', 'real_command', 'real_path', 'cannot both be configured'],
  },
  {
    path: 'crates/cadder-daemon/src/caddy/resolver.rs',
    markers: [
      'ExplicitDaemonOverride',
      'PortableConfiguration',
      'UserConfiguration',
      'SystemConfiguration',
      'resolve_caddy_on_path',
      'Project files, registration working directories, environment selectors, and shim flags',
    ],
  },
  {
    path: 'crates/cadder-client/src/inspection.rs',
    markers: [
      'routes_for_port',
      'routes_for_domain',
      'canonicalize_domain',
      'local_upstream',
      'sort_routes',
    ],
  },
  {
    path: 'crates/cadder-ipc/src/message_types.rs',
    markers: [
      'register-entrypoint-request',
      'unregister-entrypoint-request',
      'heartbeat-entrypoint-request',
      'query-state-request',
      'set-entrypoint-enabled-request',
      'set-domain-enabled-request',
      'query-logs-request',
      'shutdown-daemon-request',
      'PROTOCOL_ERROR_RESPONSE',
    ],
  },
  {
    path: 'crates/cadder-ipc/src/operations.rs',
    markers: ['OperationAccess::ReadOnly', 'OperationAccess::Mutation', 'OperationShape::Unary'],
  },
  {
    path: 'crates/cadder-ipc/src/wire.rs',
    markers: [
      'deny_unknown_fields',
      'a response must contain `result` or `error`',
      'a response cannot contain both `result` and `error`',
      'the response and protocol error request IDs must match',
    ],
  },
  {
    path: 'crates/cadder-ipc/src/mutations.rs',
    markers: [
      'pub struct RegisterEntrypointPayload',
      'pub struct UnregisterEntrypointPayload',
      'pub struct HeartbeatEntrypointPayload',
      'pub struct SetEntrypointEnabledPayload',
      'pub struct SetDomainEnabledPayload',
      'pub struct ShutdownDaemonPayload',
      'pub shim_session_nonce: Option<String>',
    ],
  },
  {
    path: 'crates/cadder-ipc/src/commands.rs',
    markers: ['pub struct QueryStatePayload', 'pub fn new_request_id'],
  },
  {
    path: 'crates/cadder-ipc/src/responses.rs',
    markers: [
      'pub struct RegisterEntrypointResponse',
      'pub struct BasicResponse',
      'pub struct QueryStateResponse',
      'pub registration_id: Option<String>',
      '#[serde(default)]',
    ],
  },
  {
    path: 'crates/cadder-daemon/src/state/log_queries.rs',
    markers: ['request.limit.unwrap_or(100).clamp(1, 200)', 'Caddy logs returned.'],
  },
  {
    path: 'crates/cadder-daemon/src/database.rs',
    markers: ['limit.clamp(1, 200)', 'LIMIT 1000', 'LIMIT 5000'],
  },
  {
    path: 'crates/cadder-ipc/src/state.rs',
    markers: [
      'pub struct EntrypointRegistration',
      'pub struct EntrypointInstanceIdentity',
      'pub struct OwnerProcessIdentity',
      'pub struct RegisteredDomain',
      'pub struct RuntimeState',
      'pub struct ConfigState',
      'registration_id must match instance_id',
      'entrypoint and owner shim session nonce values must match',
    ],
  },
  {
    path: 'crates/cadder-ipc/src/logs.rs',
    markers: [
      'pub struct LogStreamIdentity',
      'pub struct LogEntry',
      'pub enum LogStreamStatus',
      'pub struct QueryLogsPayload',
    ],
  },
  {
    path: 'openspec/specs/operator-cli/spec.md',
    markers: [
      'Profile, machine-output, history, export, continuous tail, watch, IIS and autostart commands SHALL be rejected as unsupported.',
    ],
  },
  {
    path: 'openspec/specs/operator-inspection/spec.md',
    markers: ['INSPECT-001:', 'INSPECT-002:', 'INSPECT-003:', 'INSPECT-004:'],
  },
  {
    path: 'openspec/specs/operator-tui/spec.md',
    markers: ['TUI-001:', 'TUI-002:', 'TUI-003:', 'TUI-004:'],
  },
  {
    path: 'openspec/specs/local-control-plane/spec.md',
    markers: ['IPC-001:', 'IPC-002:', 'IPC-003:'],
  },
  {
    path: 'openspec/specs/observability/spec.md',
    markers: ['OBS-001:', 'OBS-002:', 'OBS-003:', 'OBS-004:'],
  },
  {
    path: 'openspec/specs/product-topology/spec.md',
    markers: ['TOP-001:', 'TOP-002:'],
  },
  {
    path: 'openspec/changes/reset-cadder-architecture/specs/project-registration/spec.md',
    markers: ['REG-004:', 'active domain conflicts SHALL preserve the previous committed state'],
  },
  {
    path: 'openspec/changes/reset-cadder-architecture/specs/caddy-shim-integration/spec.md',
    markers: ['SHIM-001:', 'SHIM-002:', 'SHIM-003:', 'standard streams and exit codes'],
  },
];

export type CliCase = {
  argv: readonly string[];
  expected: 'success' | 'invalid-usage';
  exitCode: 0 | 2;
  stream: 'stdout' | 'stderr';
  notes: string;
};

const success = (
  argv: readonly string[],
  notes: string,
  stream: CliCase['stream'] = 'stdout',
): CliCase => ({ argv, expected: 'success', exitCode: 0, stream, notes });
const invalid = (argv: readonly string[], notes: string): CliCase => ({
  argv,
  expected: 'invalid-usage',
  exitCode: 2,
  stream: 'stderr',
  notes,
});

export const cliCases: readonly CliCase[] = [
  success(['cadder'], 'bare invocation prints help', 'stderr'),
  success(['cadder', '--help'], 'explicit help', 'stdout'),
  success(['cadder', '--version'], 'explicit version', 'stdout'),
  success(['cadder', 'help'], 'help subcommand', 'stdout'),
  success(['cadder', 'help', 'status'], 'nested command help', 'stdout'),
  success(['cadder', 'status'], 'daemon-backed status'),
  success(['cadder', 'daemon', 'start'], 'explicit daemon start'),
  success(['cadder', 'daemon', 'stop'], 'explicit daemon stop'),
  success(['cadder', 'daemon', 'restart'], 'ordered restart'),
  success(['cadder', 'daemon'], 'nested group prints help', 'stderr'),
  success(['cadder', 'projects', 'list'], 'project listing'),
  success(['cadder', 'projects', 'enable', 'Caddyfile'], 'project activation'),
  success(['cadder', 'projects', 'disable', 'Caddyfile'], 'project deactivation'),
  success(['cadder', 'projects'], 'nested group prints help', 'stderr'),
  success(['cadder', 'domains', 'list'], 'domain listing'),
  success(['cadder', 'domains', 'inspect', 'app.localhost'], 'domain inspection'),
  success(
    ['cadder', 'domains', 'inspect', 'app.localhost', '--caddyfile', 'Caddyfile'],
    'domain inspection with project selector',
  ),
  success(
    ['cadder', 'domains', 'enable', 'app.localhost', '--caddyfile', 'Caddyfile'],
    'domain activation with project selector',
  ),
  success(['cadder', 'domains', 'disable', 'app.localhost'], 'domain deactivation'),
  success(
    ['cadder', 'domains', 'disable', 'app.localhost', '--caddyfile', 'Caddyfile'],
    'domain deactivation with project selector',
  ),
  success(['cadder', 'domains'], 'nested group prints help', 'stderr'),
  success(['cadder', 'port', 'inspect', '3000'], 'port inspection'),
  success(['cadder', 'port', 'kill', '3000', '--pid', '42'], 'guarded process termination'),
  success(['cadder', 'port'], 'nested group prints help', 'stderr'),
  invalid(['cadder', 'port', 'kill', '3000'], 'kill requires observed PID'),
  success(['cadder', 'caddyfile', 'inspect', 'Caddyfile'], 'Caddyfile inspection'),
  success(['cadder', 'caddyfile'], 'nested group prints help', 'stderr'),
  success(['cadder', 'diagnostics'], 'explicit diagnostics'),
  success(['cadder', 'logs', 'runtime'], 'bounded runtime logs'),
  success(['cadder', 'logs', 'runtime', '--limit', '200'], 'bounded runtime logs at upper bound'),
  success(['cadder', 'logs', 'project', 'Caddyfile', '--limit', '200'], 'bounded project logs'),
  success(['cadder', 'logs', 'domain', 'app.localhost', '--limit', '1'], 'bounded domain logs'),
  success(
    ['cadder', 'logs', 'domain', 'app.localhost', '--caddyfile', 'Caddyfile', '--limit', '1'],
    'bounded domain logs with project selector',
  ),
  invalid(['cadder', 'logs', 'runtime', '--limit', '0'], 'log limit lower bound'),
  invalid(['cadder', 'logs', 'runtime', '--limit', '201'], 'log limit upper bound'),
  success(['cadder', 'logs'], 'nested group prints help', 'stderr'),
  success(['cadder', 'tui'], 'attach-first routes TUI'),
  success(['cadder', 'tui', '--start-daemon'], 'explicit offline/start path'),
  invalid(['cadder', '--runtime-dir', 'fixture', 'tui'], 'runtime selection is not CLI grammar'),
  invalid(['cadder', 'web'], 'unknown command is rejected'),
  invalid(['cadder', 'profile', 'dev'], 'removed profile input is clap invalid usage'),
  invalid(['cadder', '--json', 'status'], 'removed machine output is clap invalid usage'),
  invalid(['cadder', 'status', '--json'], 'removed machine output after subcommand'),
  invalid(['cadder', 'tui', '--json'], 'removed machine output on TUI'),
  invalid(['cadder', 'history'], 'removed history input is clap invalid usage'),
  invalid(['cadder', 'export'], 'removed export input is clap invalid usage'),
  invalid(['cadder', 'logs', 'runtime', '--tail'], 'removed continuous tail is clap invalid usage'),
  invalid(['cadder', 'watch'], 'removed watch input is clap invalid usage'),
  invalid(['cadder', 'autostart'], 'removed autostart input is clap invalid usage'),
  invalid(['cadder', 'iis'], 'removed IIS input is clap invalid usage'),
];

export const exitCodes = {
  success: 0,
  invalidUsage: 2,
  daemonUnavailable: 3,
  daemonStartFailure: 4,
  targetNotFound: 5,
  conflictOrRejected: 6,
  permissionOrElevation: 7,
  unsupportedOperation: 8,
  ipcFailure: 9,
} as const;

type HumanOutputFixture = {
  sourceRevision: string;
  version: string;
  platform: string;
  capture: string;
  sourceState: string;
  portability: string;
  cases: readonly { name: string; output: string }[];
};

export const humanOutputFixture = JSON.parse(
  readFileSync(new URL('./human-output.json', import.meta.url), 'utf8'),
) as HumanOutputFixture;

export const humanOutput = {
  status: ['cadderd  running', 'Caddy    running', 'Config   idle'],
  emptyProjects: ['No projects registered.'],
  emptyDomains: ['No domains registered.'],
  emptyLogs: ['No log entries.'],
  logHeader: ['SEQ', 'TIME (UTC)', 'LEVEL', 'MESSAGE'],
  diagnosticHeaders: ['Runtime diagnostics', 'Configuration diagnostics'],
  populatedProjects: {
    headers: ['STATE', 'PROJECT', 'CADDYFILE'],
  },
  populatedDomains: {
    headers: ['STATE', 'DOMAIN', 'UPSTREAM', 'PROJECT'],
  },
  populatedPort: {
    sections: ['Port {port}', 'Sockets', 'Cadder routes'],
    ownerHeaders: ['PROTOCOL', 'ADDRESS', 'PID', 'PROCESS', 'EXECUTABLE'],
  },
  populatedCaddyfile: {
    sections: ['Registered yes', 'Domains', 'Local upstream sockets'],
    headers: ['PROJECT STATE', 'DOMAIN STATE', 'DOMAIN', 'UPSTREAM'],
  },
  populatedLogs: {
    status: 'active',
    channel: 'caddy',
    domain: 'active.localhost',
  },
  captureNames: [
    'list_and_status_outputs_cover_empty_and_populated_snapshots',
    'port_output_covers_socket_and_registration_availability',
    'route_outputs_cover_registered_unregistered_and_socket_notes',
    'diagnostics_and_logs_cover_empty_and_detailed_reports',
  ],
} as const;

export type ShimPolicy =
  | 'managed'
  | 'read-only-inspection'
  | 'explicit-passthrough'
  | 'unsupported';

export const shimCommandCases: readonly { command: string; policy: ShimPolicy }[] = [
  { command: 'run', policy: 'managed' },
  ...['adapt', 'build-info', 'environ', 'help', 'list-modules', 'validate', 'version'].map(
    (command) => ({ command, policy: 'read-only-inspection' as const }),
  ),
  ...['completion', 'file-server', 'fmt', 'manpage', 'reverse-proxy'].map((command) => ({
    command,
    policy: 'explicit-passthrough' as const,
  })),
  ...[
    'add-package',
    'reload',
    'remove-package',
    'start',
    'stop',
    'trust',
    'untrust',
    'upgrade',
  ].map((command) => ({ command, policy: 'unsupported' as const })),
  { command: 'not-a-caddy-command', policy: 'unsupported' },
];

export const shimAliases = [
  { argv: [], normalized: 'help' },
  { argv: ['--help'], normalized: 'help' },
  { argv: ['-h'], normalized: 'help' },
  { argv: ['help'], normalized: 'help' },
  { argv: ['--version'], normalized: 'version' },
  { argv: ['-v'], normalized: 'version' },
  { argv: ['version'], normalized: 'version' },
] as const;

export const shimRunMetadata = {
  defaultConfig: 'Caddyfile',
  configOptions: ['--config', '-c'],
  adapterOptions: ['--adapter', '-a'],
  preservesRawArguments: true,
  preservesJoinedCommandLine: true,
  managedRun: {
    streams: 'no child delegation; registration session owns no real-Caddy child',
    exitCodes: { clean: 0, backendOrRecoveryFailure: 1 },
    missingDaemon: 'start managed daemon; never delegate to independent Caddy',
  },
  delegation: {
    classes: ['read-only-inspection', 'explicit-passthrough'],
    streams: ['stdin', 'stdout', 'stderr'],
    exitStatus: 'status.code() as u8, or 1 when no status exists',
  },
} as const;

export const caddyResolutionPrecedence = [
  'absolute daemon-start override',
  'portable cadder.toml beside the executable',
  'per-user trusted cadder.toml',
  'system trusted cadder.toml',
  'safe native caddy on PATH',
] as const;

export const caddyResolutionAuthority = {
  path: 'crates/cadder-daemon/src/caddy/tests.rs',
  tests: [
    'trusted_caddy_source_explicit_override_precedes_user_and_system_configuration',
    'trusted_caddy_source_user_configuration_precedes_system_configuration',
    'trusted_caddy_source_uses_system_default_after_empty_user_config',
    'trusted_caddy_source_invalid_higher_priority_config_fails_without_fallback',
    'trusted_caddy_source_uses_portable_configuration_and_ignores_project_and_environment_selectors',
    'trusted_caddy_source_portable_command_selects_the_named_path_executable',
    'trusted_caddy_source_path_skips_shim_identity_and_uses_next_candidate',
  ],
} as const;

export const configurationCases = [
  {
    toml: '[caddy]\nreal_command = "caddy-real"',
    result: 'command:caddy-real',
    sourceMarker: 'real_command',
  },
  {
    toml: '[caddy]\nreal_path = "/opt/caddy"',
    result: 'path:/opt/caddy',
    sourceMarker: 'real_path',
  },
  {
    toml: '[caddy]\nreal_command = "caddy-real"\nreal_path = "/opt/caddy"',
    result: 'reject:both selectors',
    sourceMarker: 'cannot both be configured',
  },
  {
    toml: '[defaults]\nreal_caddy = "/opt/caddy"',
    result: 'reject:removed defaults shape',
    sourceMarker: 'deny_unknown_fields',
  },
  {
    toml: '[caddy]\nunknown = "caddy"',
    result: 'reject:unknown field',
    sourceMarker: 'deny_unknown_fields',
  },
  {
    toml: '[caddy\nreal_path = "/opt/caddy"',
    result: 'reject:invalid TOML',
    sourceMarker: 'from_reader',
  },
] as const;

// Expected adaptation outcomes are migration examples, not executed real-Caddy parser goldens.
// Each citation proves only its named inspection/composition/mock aspect.
export const caddyfileExamples = [
  {
    source: 'app.localhost {\n  reverse_proxy 127.0.0.1:3000\n}',
    domain: 'app.localhost',
    upstream: '127.0.0.1:3000',
    localPort: 3000,
    expected: 'accepted',
    evidence: {
      aspect: 'local upstream port correlation, not Caddyfile adaptation',
      path: 'crates/cadder-client/src/inspection.rs',
      test: 'recognizes_local_upstream_aliases',
    },
  },
  {
    source: 'remote.example {\n  reverse_proxy api.example.com:3000\n}',
    domain: 'remote.example',
    upstream: 'api.example.com:3000',
    localPort: null,
    expected: 'accepted',
    evidence: {
      aspect: 'remote upstream exclusion from local ports, not Caddyfile adaptation',
      path: 'crates/cadder-client/src/inspection.rs',
      test: 'excludes_remote_and_dynamic_upstreams',
    },
  },
  {
    source: ':3000 {\n  respond "ok"\n}',
    domain: '',
    upstream: null,
    localPort: null,
    expected: 'accepted',
    evidence: {
      aspect: 'adapted hostless sibling route composition, not acceptance of this Caddyfile',
      path: 'crates/cadder-daemon/src/caddy/tests.rs',
      test: 'compose_config_guards_hostless_sibling_routes_with_registration_hosts',
    },
  },
  {
    source: 'app.localhost {\n',
    domain: null,
    upstream: null,
    localPort: null,
    expected: 'rejected',
    evidence: {
      aspect: 'malformed input denial by the retained test-only mock adapter',
      path: 'crates/cadder-daemon/src/caddy/tests.rs',
      test: 'mock_adapter_reports_invalid_caddyfile',
    },
  },
] as const;

export const registrationFixture = {
  registrationId: 'shim-fixture-01',
  entrypointInstance: {
    instanceId: 'shim-fixture-01',
    startedAtUtc: '2026-01-01T00:00:00Z',
    shimSessionNonce: 'session-fixture-01',
  },
  sourceWorkingDirectory: { raw: '/workspace/project', canonical: '/workspace/project' },
  sourceConfigPath: { raw: 'Caddyfile', canonical: '/workspace/project/Caddyfile' },
  registeredDomains: [
    {
      name: { raw: 'App.Localhost.', canonical: 'app.localhost' },
      activationState: 'active',
      upstream: '127.0.0.1:3000',
      logStream: { streamId: 'domain-app.localhost', domainKey: 'app.localhost', channel: 'caddy' },
    },
  ],
  activationState: 'active',
  ownerProcess: {
    processId: 42,
    processStartTimeUtc: '2026-01-01T00:00:00Z',
    shimSessionNonce: 'session-fixture-01',
    executablePath: null,
  },
  logStream: { streamId: 'entrypoint-shim-fixture-01', domainKey: null, channel: 'caddy' },
  shimRun: {
    adapter: 'caddyfile',
    rawArguments: ['run', '--config', 'Caddyfile', '--adapter', 'caddyfile'],
    commandLine: 'run --config Caddyfile --adapter caddyfile',
  },
  createdAtUtc: '2026-01-01T00:00:00Z',
  lastHeartbeatUtc: '2026-01-01T00:00:00Z',
} as const;

export const stateModelFields = {
  runtime: ['status', 'binaryPath', 'version', 'processId', 'adminEndpoint', 'diagnostics'],
  config: [
    'status',
    'lastAttemptedAtUtc',
    'lastSuccessfulReloadAtUtc',
    'effectiveConfigHash',
    'diagnostics',
  ],
  registration: [
    'registrationId',
    'entrypointInstance',
    'sourceWorkingDirectory',
    'sourceConfigPath',
    'registeredDomains',
    'activationState',
    'ownerProcess',
    'logStream',
    'shimRun',
    'createdAtUtc',
    'lastHeartbeatUtc',
  ],
  entrypointInstance: ['instanceId', 'startedAtUtc', 'shimSessionNonce'],
  ownerProcess: ['processId', 'processStartTimeUtc', 'shimSessionNonce', 'executablePath'],
  domain: ['name', 'activationState', 'upstream', 'logStream'],
  logEntry: [
    'sequenceNumber',
    'timestampUtc',
    'severity',
    'stream',
    'attributionKind',
    'entryKind',
    'rawMessage',
    'domainKey',
    'sourceRegistrationId',
    'sourceInstanceId',
    'operation',
  ],
} as const;

type Field = { name: string; optional: boolean };
type OperationFixture = {
  operation: string;
  payloadType: string;
  payloadFields: readonly Field[];
  result: string;
  resultType: string;
  resultFields: readonly Field[];
  access: 'mutation' | 'read-only';
};

const required = (name: string): Field => ({ name, optional: false });
const optional = (name: string): Field => ({ name, optional: true });

export const operationCatalog: readonly OperationFixture[] = [
  {
    operation: 'register-entrypoint-request',
    payloadType: 'RegisterEntrypointPayload',
    payloadFields: [required('registration')],
    result: 'register-entrypoint-response',
    resultType: 'RegisterEntrypointResponse',
    resultFields: [
      required('requestId'),
      required('accepted'),
      required('message'),
      optional('registrationId'),
    ],
    access: 'mutation',
  },
  {
    operation: 'unregister-entrypoint-request',
    payloadType: 'UnregisterEntrypointPayload',
    payloadFields: [required('registrationId'), required('shimSessionNonce')],
    result: 'unregister-entrypoint-response',
    resultType: 'BasicResponse',
    resultFields: [required('requestId'), required('accepted'), required('message')],
    access: 'mutation',
  },
  {
    operation: 'heartbeat-entrypoint-request',
    payloadType: 'HeartbeatEntrypointPayload',
    payloadFields: [required('registrationId'), required('shimSessionNonce')],
    result: 'heartbeat-entrypoint-response',
    resultType: 'BasicResponse',
    resultFields: [required('requestId'), required('accepted'), required('message')],
    access: 'mutation',
  },
  {
    operation: 'query-state-request',
    payloadType: 'QueryStatePayload',
    payloadFields: [],
    result: 'query-state-response',
    resultType: 'QueryStateResponse',
    resultFields: [
      required('requestId'),
      required('accepted'),
      required('message'),
      optional('snapshot'),
    ],
    access: 'read-only',
  },
  {
    operation: 'set-entrypoint-enabled-request',
    payloadType: 'SetEntrypointEnabledPayload',
    payloadFields: [required('registrationId'), optional('shimSessionNonce'), required('enabled')],
    result: 'set-entrypoint-enabled-response',
    resultType: 'BasicResponse',
    resultFields: [required('requestId'), required('accepted'), required('message')],
    access: 'mutation',
  },
  {
    operation: 'set-domain-enabled-request',
    payloadType: 'SetDomainEnabledPayload',
    payloadFields: [required('registrationId'), required('domainKey'), required('enabled')],
    result: 'set-domain-enabled-response',
    resultType: 'BasicResponse',
    resultFields: [required('requestId'), required('accepted'), required('message')],
    access: 'mutation',
  },
  {
    operation: 'query-logs-request',
    payloadType: 'QueryLogsPayload',
    payloadFields: [required('stream'), optional('limit')],
    result: 'query-logs-response',
    resultType: 'QueryLogsResponse',
    resultFields: [
      required('requestId'),
      required('accepted'),
      required('message'),
      required('stream'),
      required('streamStatus'),
      required('entries'),
    ],
    access: 'read-only',
  },
  {
    operation: 'shutdown-daemon-request',
    payloadType: 'ShutdownDaemonPayload',
    payloadFields: [],
    result: 'shutdown-daemon-response',
    resultType: 'BasicResponse',
    resultFields: [required('requestId'), required('accepted'), required('message')],
    access: 'mutation',
  },
];

export const responseContract = {
  requestFields: ['protocolVersion', 'operation', 'requestId', 'payload'],
  successFields: ['protocolVersion', 'operation', 'requestId', 'result'],
  errorFields: ['protocolVersion', 'operation', 'requestId', 'error'],
  protocolErrorResponse: { name: 'protocol-error-response', fields: ['requestId', 'error'] },
  errorRequestIdRule: 'response and protocol error request IDs must match',
  exactlyOneOutcome: true,
  unknownFieldsRejected: true,
  streamStatuses: ['unknown', 'empty', 'active', 'stale', 'removed', 'readError'],
  logLimit: { minimum: 1, maximum: 200, cliDefault: 50, rpcDefault: 100 },
} as const;

const rpcRequestId = '00000000-0000-4000-8000-000000000001';
const rpcTimestamp = '2026-01-01T00:00:00.000Z';
const rpcBasicResult = {
  requestId: rpcRequestId,
  accepted: true,
  message: 'Fixture operation accepted.',
};

export const rpcSnapshotFixture = {
  capturedAtUtc: rpcTimestamp,
  registrations: [registrationFixture],
  runtime: {
    status: 'idle',
    binaryPath: null,
    version: null,
    processId: null,
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
  storage: null,
} as const;

export const rpcOperationFixtures = [
  {
    method: 'register-entrypoint-request',
    params: { registration: registrationFixture },
    result: { ...rpcBasicResult, registrationId: registrationFixture.registrationId },
  },
  {
    method: 'unregister-entrypoint-request',
    params: {
      registrationId: registrationFixture.registrationId,
      shimSessionNonce: 'session-fixture-01',
    },
    result: rpcBasicResult,
  },
  {
    method: 'heartbeat-entrypoint-request',
    params: {
      registrationId: registrationFixture.registrationId,
      shimSessionNonce: 'session-fixture-01',
    },
    result: rpcBasicResult,
  },
  {
    method: 'query-state-request',
    params: {},
    result: { ...rpcBasicResult, snapshot: rpcSnapshotFixture },
  },
  {
    method: 'set-entrypoint-enabled-request',
    params: {
      registrationId: registrationFixture.registrationId,
      shimSessionNonce: null,
      enabled: true,
    },
    result: rpcBasicResult,
  },
  {
    method: 'set-domain-enabled-request',
    params: {
      registrationId: registrationFixture.registrationId,
      domainKey: 'app.localhost',
      enabled: false,
    },
    result: rpcBasicResult,
  },
  {
    method: 'query-logs-request',
    params: {
      stream: { streamId: 'domain-app.localhost', domainKey: 'app.localhost', channel: 'caddy' },
      limit: 100,
    },
    result: {
      ...rpcBasicResult,
      stream: { streamId: 'domain-app.localhost', domainKey: 'app.localhost', channel: 'caddy' },
      streamStatus: 'active',
      entries: [],
    },
  },
  {
    method: 'shutdown-daemon-request',
    params: {},
    result: rpcBasicResult,
  },
] as const;

export const rpcExamples = {
  queryLogsRequest: {
    protocolVersion: 3,
    operation: 'query-logs-request',
    requestId: '00000000-0000-4000-8000-000000000002',
    payload: {
      stream: { streamId: 'domain-app.localhost', domainKey: 'app.localhost', channel: 'caddy' },
      limit: 100,
    },
  },
  queryLogsSuccess: {
    protocolVersion: 3,
    operation: 'query-logs-response',
    requestId: '00000000-0000-4000-8000-000000000002',
    result: {
      requestId: '00000000-0000-4000-8000-000000000002',
      accepted: true,
      message: 'Caddy logs returned.',
      streamStatus: 'active',
      entries: [],
    },
  },
  protocolError: {
    protocolVersion: 3,
    operation: 'query-logs-request',
    requestId: '00000000-0000-4000-8000-000000000003',
    error: {
      kind: 'unsupportedOperation',
      code: 'unsupported_operation',
      message: 'Unsupported operation',
      guidance: null,
      retryable: false,
      requestId: '00000000-0000-4000-8000-000000000003',
    },
  },
} as const;
