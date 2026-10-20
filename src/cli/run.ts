import { Command, CommanderError, InvalidArgumentError } from 'commander';
import { ClientService } from '../client/service.ts';
import { domainForName, localUpstreamPort, projectForPath } from '../client/selectors.ts';
import { inspectPort, terminatePortOwner } from '../platform/inspection.ts';
import { CadderError, RpcError, errorCode } from '../protocol/errors.ts';
import { VERSION } from '../protocol/version.ts';
import { runtimeLogStream } from '../storage/schema.ts';
import type { LogInput } from '../storage/schema.ts';
import { terminalSafe } from '../storage/redaction.ts';
import * as output from './output.ts';

function integer(raw: string, maximum: number, minimum = 0): number {
  if (
    !/^\d+$/.test(raw) ||
    !Number.isSafeInteger(Number(raw)) ||
    Number(raw) > maximum ||
    Number(raw) < minimum
  )
    throw new InvalidArgumentError(`Expected an integer from ${minimum} to ${maximum}.`);
  return Number(raw);
}
export function exitCode(error: unknown): number {
  if (error instanceof CommanderError) return error.exitCode === 0 ? 0 : 2;
  const code = errorCode(error);
  if (code === 'daemon-unavailable' || code === 'daemon_unavailable') return 3;
  if (code === 'daemon-start-failed') return 4;
  if (code === 'target-not-found') return 5;
  if (code === 'target-conflict') return 6;
  if (code === 'unsupported-operation') return 8;
  if (error instanceof RpcError) {
    if (error.protocolError.kind === 'accessDenied') return 7;
    if (['configuration', 'conflict', 'staleInstance'].includes(error.protocolError.kind)) return 6;
    if (error.protocolError.kind === 'invalidInput') return 2;
  }
  if (code === 'EACCES' || code === 'EPERM' || code === 'unsafe-runtime-permissions') return 7;
  if (code === 'invalid-input') return 2;
  return 9;
}

export async function runCli(
  argv: readonly string[],
  client = new ClientService(),
  io = {
    out: (text: string) => {
      process.stdout.write(text);
    },
    error: (text: string) => {
      process.stderr.write(text);
    },
  },
): Promise<number> {
  const print = (text: string): void => io.out(text + '\n');
  const program = new Command('cadder')
    .version(VERSION)
    .description('Inspect and manage Cadder routes')
    .exitOverride()
    .configureOutput({ writeOut: io.out, writeErr: io.error });
  program.addHelpCommand('help [command]', 'Display help for a command.');
  function group(name: string, description: string): Command {
    const command = program.command(name).description(description);
    command.action(() => command.outputHelp({ error: true }));
    return command;
  }
  program.action(() => program.outputHelp({ error: true }));
  program
    .command('status')
    .description('Show cadderd, Caddy, and configuration state.')
    .action(async () => print(output.status(await client.snapshot())));
  const daemon = group('daemon', 'Start, stop, or restart cadderd.');
  for (const action of ['start', 'stop', 'restart'] as const)
    daemon.command(action).action(async () => {
      await client[action]();
      print(action === 'stop' ? 'cadderd stopped.' : 'cadderd running.');
    });
  const projects = group('projects', 'Inspect and manage registered projects.');
  projects.command('list').action(async () => print(output.projects(await client.snapshot())));
  const domains = group('domains', 'Inspect and manage registered domains.');
  domains.command('list').action(async () => print(output.domains(await client.snapshot())));
  for (const action of ['enable', 'disable'] as const) {
    projects.command(`${action} <caddyfile>`).action(async (path: string) => {
      const project = await projectForPath(await client.snapshot(), path);
      const result = await client.setProjectEnabled({
        registrationId: project.registrationId,
        enabled: action === 'enable',
      });
      if (!result.ok) throw new RpcError(result.error);
      print(result.value.message);
    });
    domains
      .command(`${action} <domain>`)
      .option('--caddyfile <path>')
      .action(async (name: string, options: { caddyfile?: string }) => {
        const target = await domainForName(await client.snapshot(), name, options.caddyfile);
        const result = await client.setDomainEnabled({
          registrationId: target.project.registrationId,
          domainKey: target.domain.name.canonical,
          enabled: action === 'enable',
        });
        if (!result.ok) throw new RpcError(result.error);
        print(result.value.message);
      });
  }
  const port = group('port', 'Inspect a local port or terminate its expected owner.');
  port.command('inspect <port>').action(async (raw: string) => {
    const value = integer(raw, 65535);
    const owners = await inspectPort(value);
    print(`Port ${value}\n\nSockets\n${output.sockets(owners)}\n\nCadder routes`);
    try {
      print(
        output.routeTable(
          output
            .routes(await client.snapshot())
            .filter(({ domain }) => localUpstreamPort(domain.upstream) === value),
        ),
      );
    } catch (error) {
      if (errorCode(error) !== 'daemon-unavailable') throw error;
      print('Registration data unavailable.\n\nCadder: daemon unavailable');
    }
  });
  port
    .command('kill <port>')
    .requiredOption('--pid <pid>', 'Previously observed process ID')
    .action(async (raw: string, options: { pid: string }) => {
      const value = integer(raw, 65535);
      const pid = integer(options.pid, 0xffffffff, 1);
      await terminatePortOwner(value, pid);
      print(`Termination signal sent to PID ${pid} for port ${value}.`);
    });
  const caddyfile = group('caddyfile', 'Inspect a Caddyfile registration and its local upstreams.');
  caddyfile.command('inspect <caddyfile>').action(async (path: string) => {
    const snapshot = await client.snapshot();
    print(`Caddyfile ${terminalSafe(path)}\n${output.status(snapshot)}`);
    try {
      const project = await projectForPath(snapshot, path);
      print(
        'Registered yes\n\n' +
          output.projects({ ...snapshot, registrations: [project] }) +
          '\n\nDomains\n' +
          output.domains({ ...snapshot, registrations: [project] }),
      );
      for (const value of new Set(
        project.registeredDomains
          .map((domain) => localUpstreamPort(domain.upstream))
          .filter((value) => value !== undefined),
      ))
        print('\nLocal upstream sockets\n' + output.sockets(await inspectPort(value)));
    } catch (error) {
      if (errorCode(error) !== 'target-not-found') throw error;
      print('Registered no');
    }
  });
  domains
    .command('inspect <domain>')
    .option('--caddyfile <path>')
    .action(async (name: string, options: { caddyfile?: string }) => {
      const snapshot = await client.snapshot();
      const selected = await domainForName(snapshot, name, options.caddyfile);
      print(
        `Domain ${terminalSafe(name)}\n${output.status(snapshot)}\n\n${output.routeTable([selected])}`,
      );
      const value = localUpstreamPort(selected.domain.upstream);
      if (value !== undefined)
        print('\nLocal upstream sockets\n' + output.sockets(await inspectPort(value)));
    });
  program
    .command('diagnostics')
    .description('Show explicit runtime and configuration diagnostics.')
    .action(async () => print(output.diagnostics(await client.snapshot())));
  const logs = group('logs', 'Read bounded, redacted daemon logs.');
  for (const kind of ['runtime', 'project', 'domain'] as const) {
    const command = logs
      .command(
        kind === 'runtime' ? 'runtime' : `${kind} <${kind === 'project' ? 'caddyfile' : 'domain'}>`,
      )
      .option(
        '--limit <limit>',
        'Number of newest entries (1-200)',
        (raw) => integer(raw, 200, 1),
        50,
      );
    if (kind === 'domain') command.option('--caddyfile <path>');
    command.action(async (...arguments_: unknown[]) => {
      const options = arguments_[kind === 'runtime' ? 0 : 1] as {
        limit: number;
        caddyfile?: string;
      };
      let stream: LogInput['stream'] = runtimeLogStream;
      if (kind !== 'runtime') {
        const snapshot = await client.snapshot();
        const selector = arguments_[0] as string;
        stream =
          kind === 'project'
            ? (await projectForPath(snapshot, selector)).logStream
            : (await domainForName(snapshot, selector, options.caddyfile)).domain.logStream;
      }
      const result = await client.queryLogs({ stream, limit: options.limit });
      if (!result.ok) throw new RpcError(result.error);
      print(output.logs(result.value));
    });
  }
  program
    .command('tui')
    .description('Open the full-screen operator.')
    .option('--start-daemon')
    .action(async (options: { startDaemon?: boolean }) => {
      if (options.startDaemon) await client.start();
      if (!process.stdin.isTTY || !process.stdout.isTTY)
        throw new CadderError(
          'unsupported-operation',
          'cadder tui requires an interactive terminal.',
        );
      const { openTui } = await import('../tui/app.tsx');
      await openTui(client);
    });
  try {
    await program.parseAsync([...argv], { from: 'user' });
    return 0;
  } catch (error) {
    if (!(error instanceof CommanderError))
      io.error(terminalSafe(error instanceof Error ? error.message : 'Operation failed.') + '\n');
    return exitCode(error);
  }
}
