import type { EntrypointRegistration, StateSnapshot } from '../protocol/dto.ts';
import type { RpcResult } from '../protocol/rpc.ts';
import { terminalSafe } from '../storage/redaction.ts';
import { localUpstreamPort } from '../client/selectors.ts';
import type { PortOwner } from '../platform/inspection.ts';

export function table(headers: string[], rows: (string | number | null | undefined)[][]): string {
  const values = [headers, ...rows].map((row) =>
    row.map((value) => terminalSafe(String(value ?? '-'))),
  );
  const widths = headers.map((_, column) =>
    Math.max(...values.map((row) => row[column]?.length ?? 0)),
  );
  return values
    .map((row) => ' ' + row.map((value, index) => value.padEnd(widths[index]!)).join('  ') + ' ')
    .join('\n');
}
export function status(snapshot: StateSnapshot): string {
  return `cadderd  running\nCaddy    ${snapshot.runtime.status.toLowerCase()}\nConfig   ${snapshot.config.status.toLowerCase()}`;
}
export function projects(snapshot: StateSnapshot): string {
  if (!snapshot.registrations.length) return 'No projects registered.';
  return table(
    ['STATE', 'PROJECT', 'CADDYFILE'],
    [...snapshot.registrations]
      .sort((a, b) => a.sourceWorkingDirectory.raw.localeCompare(b.sourceWorkingDirectory.raw))
      .map((project) => [
        project.activationState,
        project.sourceWorkingDirectory.raw,
        project.sourceConfigPath.raw,
      ]),
  );
}
export function effectiveState(
  project: EntrypointRegistration,
  domain: EntrypointRegistration['registeredDomains'][number],
): string {
  if (project.activationState === 'faulted' || domain.activationState === 'faulted')
    return 'faulted';
  if (project.activationState === 'inactive' || domain.activationState === 'inactive')
    return 'inactive';
  return project.activationState === 'active' && domain.activationState === 'active'
    ? 'active'
    : 'pending';
}
export function domains(snapshot: StateSnapshot): string {
  const rows = routes(snapshot);
  if (!rows.length) return 'No domains registered.';
  return table(
    ['STATE', 'DOMAIN', 'UPSTREAM', 'PROJECT'],
    rows.map(({ project, domain }) => [
      effectiveState(project, domain),
      domain.name.raw,
      displayUpstream(domain.upstream),
      project.sourceWorkingDirectory.raw,
    ]),
  );
}
export function routes(snapshot: StateSnapshot) {
  return [...snapshot.registrations]
    .sort((a, b) => a.sourceWorkingDirectory.raw.localeCompare(b.sourceWorkingDirectory.raw))
    .flatMap((project) =>
      [...project.registeredDomains]
        .sort((a, b) => a.name.canonical.localeCompare(b.name.canonical))
        .map((domain) => ({ project, domain })),
    );
}
export function displayUpstream(upstream: string | undefined): string {
  const port = localUpstreamPort(upstream);
  return port === undefined ? (upstream ?? '-') : `:${port}`;
}
export function routeTable(rows: ReturnType<typeof routes>): string {
  return rows.length
    ? table(
        ['PROJECT STATE', 'DOMAIN STATE', 'DOMAIN', 'UPSTREAM', 'PROJECT', 'CADDYFILE'],
        rows.map(({ project, domain }) => [
          project.activationState,
          domain.activationState,
          domain.name.raw,
          displayUpstream(domain.upstream),
          project.sourceWorkingDirectory.raw,
          project.sourceConfigPath.raw,
        ]),
      )
    : 'No matching routes.';
}
export function sockets(owners: PortOwner[]): string {
  if (!owners.length) return 'No listening socket found.';
  return table(
    ['PROTOCOL', 'ADDRESS', 'PID', 'PROCESS', 'EXECUTABLE'],
    owners.map((owner) => [
      owner.protocol,
      `${owner.address}:${owner.port}`,
      owner.pid ?? 'unknown',
      owner.processName ?? 'unknown',
      owner.executable,
    ]),
  );
}
export function diagnostics(snapshot: StateSnapshot): string {
  const runtime = snapshot.runtime;
  return [
    `Caddy    ${runtime.status.toLowerCase()}`,
    `Config   ${snapshot.config.status.toLowerCase()}`,
    ...[
      [runtime.processId, 'PID'],
      [runtime.version, 'Version'],
      [runtime.binaryPath, 'Binary'],
      [runtime.adminEndpoint, 'Admin'],
    ]
      .filter(([value]) => value !== null)
      .map(([value, label]) => `${String(label).padEnd(9)}${terminalSafe(String(value))}`),
    '',
    'Runtime diagnostics',
    runtime.diagnostics.length
      ? table(
          ['CODE', 'OPERATION', 'MESSAGE'],
          runtime.diagnostics.map((entry) => [entry.code, entry.operation, entry.message]),
        )
      : 'None.',
    '',
    'Configuration diagnostics',
    snapshot.config.diagnostics.length
      ? table(
          ['CODE', 'DOMAIN', 'CADDYFILES', 'MESSAGE'],
          snapshot.config.diagnostics.map((entry) => [
            entry.code,
            entry.domainKey,
            entry.sourceConfigPaths.join(', ') || '-',
            entry.message,
          ]),
        )
      : 'None.',
  ].join('\n');
}
export function logs(response: RpcResult<'query-logs-request'>): string {
  return [
    `Status   ${response.streamStatus.toLowerCase()}`,
    `Channel  ${terminalSafe(response.stream.channel)}`,
    ...(response.stream.domainKey ? [`Domain   ${terminalSafe(response.stream.domainKey)}`] : []),
    '',
    response.entries.length
      ? table(
          ['SEQ', 'TIME (UTC)', 'LEVEL', 'MESSAGE'],
          response.entries.map((entry) => [
            entry.sequenceNumber,
            entry.timestampUtc.replace(/\.000Z$/, '+00:00').replace(/Z$/, '+00:00'),
            entry.severity,
            entry.rawMessage,
          ]),
        )
      : 'No log entries.',
  ].join('\n');
}
