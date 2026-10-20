import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { afterEach, expect, it } from 'vitest';
import { ApplicationDatabase } from '../src/storage/database.ts';
import { ApplicationStorage } from '../src/storage/storage.ts';
import { runtimeLogStream, type LogInput } from '../src/storage/schema.ts';
import { redact, terminalSafe } from '../src/storage/redaction.ts';
import { prepareRuntime, runtimeOwner } from '../src/platform/runtime-security.ts';
import { normalizeRuntimeCreationOwner } from '../src/platform/runtime-creation-owner.ts';

const roots: string[] = [];
afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true });
});
async function root(): Promise<string> {
  const path = await mkdtemp(join(tmpdir(), 'cadder-storage-'));
  roots.push(path);
  return path;
}
function log(rawMessage: string, streamId = 'runtime-control'): LogInput {
  return {
    timestampUtc: new Date().toISOString(),
    severity: 'info',
    stream: { ...runtimeLogStream, streamId },
    attributionKind: 'runtimeControl',
    entryKind: 'normal',
    rawMessage,
    domainKey: null,
    sourceRegistrationId: null,
    sourceInstanceId: null,
    operation: null,
  };
}

it('persists only validated intent, closes and reopens without a live lease', async () => {
  const path = join(await root(), 'state.sqlite3');
  let db = new ApplicationDatabase(path);
  expect(db.loadDesiredState()).toEqual({ projects: [] });
  const desired = {
    projects: [
      {
        projectKey: 'project',
        sourceWorkingDirectory: '/project',
        sourceConfigPath: '/project/Caddyfile',
        enabled: false,
        domains: [{ canonicalDomain: 'app.localhost', upstream: null, enabled: false }],
      },
    ],
  };
  db.persistDesiredState(desired);
  db.close();
  db = new ApplicationDatabase(path);
  try {
    expect(db.loadDesiredState()).toEqual(desired);
    const invalid = { projects: [{ ...desired.projects[0]!, lease: 'forbidden' }] };
    expect(() => db.persistDesiredState(invalid)).toThrow();
    expect(db.loadDesiredState()).toEqual(desired);
  } finally {
    db.close();
  }
});

it('rejects partial, newer, foreign and corrupt databases without changing their bytes', async () => {
  for (const sql of [
    'CREATE TABLE unrelated (value TEXT)',
    'PRAGMA user_version = 2',
    'CREATE TABLE logs (body TEXT)',
  ]) {
    const path = join(await root(), 'invalid.sqlite3');
    const seed = new DatabaseSync(path);
    seed.exec(sql);
    seed.close();
    const before = await readFile(path);
    expect(() => new ApplicationDatabase(path)).toThrow();
    expect(await readFile(path)).toEqual(before);
  }
  const path = join(await root(), 'corrupt.sqlite3');
  await writeFile(path, 'not a database');
  expect(() => new ApplicationDatabase(path)).toThrow();
  expect(await readFile(path, 'utf8')).toBe('not a database');
});

it('rolls back a failed persistence transaction instead of accepting lost state', async () => {
  const path = join(await root(), 'failure.sqlite3');
  const db = new ApplicationDatabase(path);
  const contender = new DatabaseSync(path);
  try {
    contender.exec('BEGIN EXCLUSIVE');
    expect(() => db.persistDesiredState({ projects: [] })).toThrow();
    contender.exec('ROLLBACK');
    expect(db.loadDesiredState()).toEqual({ projects: [] });
  } finally {
    contender.close();
    db.close();
  }
});

it('redacts before retaining logs and returns newest bounded rows in ascending stream order', async () => {
  const path = join(await root(), 'logs.sqlite3');
  const db = new ApplicationDatabase(path);
  try {
    for (let batch = 0; batch < 6; batch++)
      db.append(Array.from({ length: 200 }, (_, i) => log(`row=${batch * 200 + i} token=hidden`)));
    const rows = db.queryLogs(runtimeLogStream, 200);
    expect(rows).toHaveLength(200);
    expect(rows[0]?.rawMessage).toBe('row=1000 token=[redacted]');
    expect(rows.at(-1)?.sequenceNumber).toBe(1200);
    db.append([log('independent', 'other')]);
    expect(db.queryLogs(runtimeLogStream, 1)[0]?.sequenceNumber).toBe(1200);
    expect(() => db.queryLogs(runtimeLogStream, 201)).toThrow();
    expect(() => db.append(Array.from({ length: 201 }, () => log('too many')))).toThrow();
    const inspect = new DatabaseSync(path);
    try {
      expect(
        inspect
          .prepare('SELECT count(*) AS total FROM logs WHERE stream = ?')
          .get(JSON.stringify(['runtime-control', null, 'control']))?.total,
      ).toBe(1000);
      expect(
        inspect.prepare("SELECT count(*) AS total FROM logs WHERE body LIKE '%hidden%'").get()
          ?.total,
      ).toBe(0);
      for (let stream = 0; stream < 6; stream++)
        for (let batch = 0; batch < 5; batch++)
          db.append(Array.from({ length: 200 }, () => log('bounded', String(stream))));
      expect(inspect.prepare('SELECT count(*) AS total FROM logs').get()?.total).toBe(5000);
    } finally {
      inspect.close();
    }
  } finally {
    db.close();
  }
});

it('uses a real protected worker connection, drains writes and rejects work after close', async () => {
  const directory = join(await root(), 'runtime');
  const owner = await runtimeOwner();
  await normalizeRuntimeCreationOwner(owner);
  await prepareRuntime(directory, owner);
  let storage = await ApplicationStorage.open(directory, owner);
  expect(await storage.loadDesiredState()).toEqual({ ok: true, value: { projects: [] } });
  expect(await storage.persistDesiredState({ projects: [] })).toEqual({
    ok: true,
    value: undefined,
  });
  await storage.append([log('password=secret-value')]);
  expect((await storage.queryLogs(runtimeLogStream, 10))[0]?.rawMessage).toBe(
    'password=[redacted]',
  );
  await storage.close();
  await storage.close();
  expect((await storage.loadDesiredState()).ok).toBe(false);
  await expect(storage.append([log('late')])).rejects.toThrow();
  storage = await ApplicationStorage.open(directory, owner);
  try {
    expect(await storage.queryLogs(runtimeLogStream, 1)).toHaveLength(1);
  } finally {
    await storage.close();
  }
}, 30000);

it('removes complete credentials in supported forms and keeps terminal controls inert', () => {
  const messages = [
    'Authorization: Bearer hidden other words',
    'cookie: session=hidden',
    'Bearer hidden',
    'token=hidden',
    'access-token="hidden with spaces"',
    'api_token=hidden',
    '{"password":"hidden with \\"quotes\\""}',
    "'secret': 'hidden with spaces'",
    '/?token=hidden&safe=ok',
    '-----BEGIN RSA PRIVATE KEY-----\nhidden\n-----END RSA PRIVATE KEY-----',
    '-----BEGIN PRIVATE KEY-----\nhidden',
  ];
  for (const input of messages) {
    const output = redact(input);
    expect(output, input).not.toContain('hidden');
    expect(output).toContain('[redacted]');
    expect(redact(output)).toBe(output);
  }
  expect(terminalSafe('ok\x1b[31mred\x1b[0m\x00\x9b\r\n')).toBe('okred');
});
