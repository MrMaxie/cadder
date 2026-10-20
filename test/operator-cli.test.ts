import { resolve } from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { runCli, exitCode } from '../src/cli/run.ts';
import { ClientService } from '../src/client/service.ts';
import { CadderError, RpcError } from '../src/protocol/errors.ts';
import { fakeSnapshot } from './fixtures/rpc-data.ts';
import { cliCases } from './compatibility/catalog.ts';
import { inspectPort, terminatePortOwner } from '../src/platform/inspection.ts';
import { localUpstreamPort } from '../src/client/selectors.ts';
import { logs, table } from '../src/cli/output.ts';

vi.mock('../src/platform/inspection.ts', () => ({
  inspectPort: vi.fn(async () => []),
  terminatePortOwner: vi.fn(async () => {}),
}));
afterEach(() => vi.restoreAllMocks());
function fixture() {
  const client = new ClientService();
  const snapshot = fakeSnapshot();
  const project = snapshot.registrations[0]!;
  project.sourceWorkingDirectory.raw = process.cwd();
  project.sourceConfigPath.raw = project.sourceConfigPath.canonical = resolve('Caddyfile');
  project.registeredDomains[0]!.name = { raw: 'app.localhost', canonical: 'app.localhost' };
  project.registeredDomains[0]!.upstream = 'localhost:3000';
  const state = vi.spyOn(client, 'snapshot').mockResolvedValue(snapshot);
  const start = vi.spyOn(client, 'start').mockResolvedValue();
  vi.spyOn(client, 'stop').mockResolvedValue();
  vi.spyOn(client, 'restart').mockResolvedValue();
  const basic = {
    requestId: '00000000-0000-4000-8000-000000000001',
    accepted: true,
    message: 'Done.',
  };
  vi.spyOn(client, 'setProjectEnabled').mockResolvedValue({ ok: true, value: basic });
  vi.spyOn(client, 'setDomainEnabled').mockResolvedValue({ ok: true, value: basic });
  vi.spyOn(client, 'queryLogs').mockResolvedValue({
    ok: true,
    value: { ...basic, stream: project.logStream, streamStatus: 'empty', entries: [] },
  });
  const run = async (argv: readonly string[]) => {
    let stdout = '';
    let stderr = '';
    const code = await runCli(argv, client, {
      out: (text) => {
        stdout += text;
      },
      error: (text) => {
        stderr += text;
      },
    });
    return { code, stdout, stderr };
  };
  return { client, state, start, run };
}

it('executes the retained command catalog and rejects unsupported grammar without new surfaces', async () => {
  const f = fixture();
  for (const entry of cliCases.filter((entry) => !entry.argv.includes('tui'))) {
    const result = await f.run(entry.argv.slice(1));
    expect(result.code, entry.argv.join(' ')).toBe(entry.exitCode);
    expect(result[entry.stream], entry.argv.join(' ')).not.toBe('');
  }
});

it('bare/group help and attach-only reads never start a daemon and retain the offline exit', async () => {
  const f = fixture();
  expect((await f.run([])).stderr).toContain('Usage: cadder');
  for (const group of ['daemon', 'projects', 'domains', 'logs', 'port', 'caddyfile'])
    expect((await f.run([group])).code).toBe(0);
  expect(f.start).not.toHaveBeenCalled();
  expect(f.state).not.toHaveBeenCalled();
  f.state.mockRejectedValue(new CadderError('daemon-unavailable', 'cadderd is not running.'));
  expect((await f.run(['status'])).code).toBe(3);
  expect(f.start).not.toHaveBeenCalled();
  const inspect = await f.run(['port', 'inspect', '3000']);
  expect(inspect.code).toBe(0);
  expect(inspect.stdout).toContain('Registration data unavailable.');
  expect(inspectPort).toHaveBeenCalledWith(3000);
});

it('validates bounded logs and expected PID before any operation and maps operation errors', async () => {
  const f = fixture();
  vi.mocked(terminatePortOwner).mockClear();
  expect((await f.run(['logs', 'runtime', '--limit', '0'])).code).toBe(2);
  expect((await f.run(['port', 'kill', '3000'])).code).toBe(2);
  expect(terminatePortOwner).not.toHaveBeenCalled();
  expect((await f.run(['port', 'kill', '3000', '--pid', '42'])).code).toBe(0);
  expect(terminatePortOwner).toHaveBeenCalledWith(3000, 42);
  expect((await f.run(['domains', 'enable', 'absent.localhost'])).code).toBe(5);
  for (const [code, expected] of [
    ['daemon-start-failed', 4],
    ['target-not-found', 5],
    ['target-conflict', 6],
    ['unsafe-runtime-permissions', 7],
    ['unsupported-operation', 8],
    ['invalid-input', 2],
  ] as const)
    expect(exitCode(new CadderError(code, code))).toBe(expected);
  expect(
    exitCode(
      new RpcError({
        kind: 'conflict',
        code: 'conflict',
        message: 'Conflict',
        guidance: null,
        retryable: false,
        requestId: null,
      }),
    ),
  ).toBe(6);
});

it('renders control-safe tables and bounded log output without terminal escape execution', () => {
  expect(table(['NAME'], [['\x1b[31mvalue\x1b[0m\r\n']])).toContain('value');
  expect(table(['NAME'], [['\x1b[31mvalue\x1b[0m\r\n']])).not.toContain('\x1b');
  expect(
    logs({
      requestId: '00000000-0000-4000-8000-000000000001',
      accepted: true,
      message: '',
      stream: { streamId: 'domain-app.localhost', domainKey: 'app.localhost', channel: 'caddy' },
      streamStatus: 'empty',
      entries: [],
    }),
  ).toContain('No log entries.');
  for (const upstream of [
    ':3000',
    '3000',
    ' 127.0.0.2:3000 ',
    '[0:0:0:0:0:0:0:1]:3000',
    'localhost:3000',
    '127.0.0.1:3000',
    '[::1]:3000',
    'http://localhost:3000/path',
  ])
    expect(localUpstreamPort(upstream)).toBe(3000);
  expect(localUpstreamPort('api.example.com:3000')).toBeUndefined();
  expect(localUpstreamPort('localhost')).toBeUndefined();
  expect(localUpstreamPort('unix//tmp/app.sock')).toBeUndefined();
});
