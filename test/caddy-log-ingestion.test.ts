import { expect, it, vi } from 'vitest';
import { CaddyLogs } from '../src/daemon/caddy-logs.ts';
import { ApplicationDatabase } from '../src/storage/database.ts';
import { runtimeLogStream, type LogInput } from '../src/storage/schema.ts';
import { fakeRegistration } from './fixtures/rpc-data.ts';

it('decodes bounded native output, attributes actual domains and redacts before SQLite', async () => {
  const database = new ApplicationDatabase(':memory:');
  const project = fakeRegistration();
  project.logStream = { streamId: 'entrypoint-fixture', domainKey: null, channel: 'caddy' };
  project.registeredDomains[0]!.logStream = {
    streamId: 'domain-example.localhost',
    domainKey: 'example.localhost',
    channel: 'caddy',
  };
  const failed = vi.fn();
  const logs = new CaddyLogs(
    () => [project],
    async (entries) => database.append(entries),
    failed,
  );
  const line = Buffer.from(
    JSON.stringify({
      level: 'warn',
      msg: 'Zażółć password=hidden',
      request: { host: 'EXAMPLE.localhost:443' },
    }) + '\n',
  );
  logs.accept('stderr', line.subarray(0, 29));
  logs.accept('stderr', line.subarray(29));
  logs.accept(
    'stdout',
    Buffer.from('-----BEGIN PRIVATE KEY-----\nhidden\n-----END PRIVATE KEY-----\nplain tail'),
  );
  await logs.close();
  try {
    const attributed = database.queryLogs(project.registeredDomains[0]!.logStream, 10);
    expect(attributed).toHaveLength(1);
    expect(attributed[0]).toMatchObject({
      rawMessage: 'Zażółć password=[redacted]',
      attributionKind: 'domain',
      sourceRegistrationId: project.registrationId,
      severity: 'warn',
    });
    expect(database.queryLogs(project.logStream, 10)[0]?.attributionKind).toBe('entrypoint');
    const runtime = database.queryLogs(runtimeLogStream, 10);
    expect(runtime.some((entry) => entry.rawMessage === 'plain tail')).toBe(true);
    expect(JSON.stringify(runtime)).not.toContain('hidden');
    expect(failed).not.toHaveBeenCalled();
  } finally {
    database.close();
  }
});

it('bounds incomplete lines and pending writes, and does not leak a key after overflow', async () => {
  const entries: LogInput[] = [];
  const settle: (() => void)[] = [];
  const failed = vi.fn();
  const logs = new CaddyLogs(
    () => [],
    (batch) => {
      entries.push(...batch);
      return new Promise<void>((resolve) => settle.push(resolve));
    },
    failed,
  );
  logs.accept(
    'stderr',
    Buffer.from(
      'x'.repeat(9000) + '-----begin private key-----\nhidden\n-----end private key-----\n',
    ),
  );
  logs.accept('stdout', Buffer.from('row\n'.repeat(70)));
  expect(entries).toHaveLength(64);
  expect(JSON.stringify(entries)).not.toContain('hidden');
  expect(failed).toHaveBeenCalled();
  for (const finish of settle) finish();
  await logs.close();
  const failing = new CaddyLogs(
    () => [],
    async () => {
      throw new Error('Disk full');
    },
    failed,
  );
  failing.accept('stderr', Buffer.from('null\n'));
  await failing.close();
});
