import { DatabaseSync } from 'node:sqlite';
import type { DesiredState } from '../contracts/ports.ts';
import { logStreamSchema } from '../protocol/dto.ts';
import { CadderError } from '../protocol/errors.ts';
import { desiredStateSchema, logInputSchema, type LogInput, type StoredLog } from './schema.ts';
import { redact } from './redaction.ts';

const tables = [
  'CREATE TABLE desired (id INTEGER PRIMARY KEY CHECK (id = 1), body TEXT NOT NULL) STRICT',
  'CREATE TABLE logs (sequence INTEGER PRIMARY KEY AUTOINCREMENT, stream TEXT NOT NULL, body TEXT NOT NULL) STRICT',
  'CREATE INDEX logs_stream ON logs (stream, sequence)',
];
function storageError(): CadderError {
  return new CadderError('storage-failed', 'Application storage is unavailable or incompatible.');
}
function streamKey(stream: unknown): string {
  const value = logStreamSchema.parse(stream);
  return JSON.stringify([value.streamId, value.domainKey, value.channel]);
}

/** Only the storage worker opens this connection in production. */
export class ApplicationDatabase {
  readonly #database: DatabaseSync;
  constructor(path: string) {
    this.#database = new DatabaseSync(path, { timeout: 1000 });
    try {
      const db = this.#database;
      const integrity = db.prepare('PRAGMA quick_check').all();
      if (integrity.length !== 1 || integrity[0]?.quick_check !== 'ok') throw storageError();
      const version = db.prepare('PRAGMA user_version').get()?.user_version;
      const objects = db
        .prepare("SELECT sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name")
        .all();
      if (version === 0 && objects.length === 0) {
        this.#transaction(() => {
          for (const sql of tables) db.exec(sql);
          db.prepare('INSERT INTO desired VALUES (1, ?)').run(JSON.stringify({ projects: [] }));
          db.exec('PRAGMA user_version = 1');
        });
      } else {
        if (
          version !== 1 ||
          objects.length !== tables.length ||
          objects.some((object) => !tables.includes(String(object.sql)))
        )
          throw storageError();
      }
      this.loadDesiredState();
    } catch (error) {
      this.#database.close();
      throw error instanceof CadderError ? error : storageError();
    }
  }

  loadDesiredState(): DesiredState {
    const rows = this.#database
      .prepare('SELECT body FROM desired WHERE length(CAST(body AS BLOB)) <= 1048576')
      .all();
    if (rows.length !== 1 || typeof rows[0]?.body !== 'string') throw storageError();
    return desiredStateSchema.parse(JSON.parse(rows[0].body));
  }

  persistDesiredState(state: DesiredState): void {
    const body = JSON.stringify(desiredStateSchema.parse(state));
    if (Buffer.byteLength(body) > 1024 * 1024) throw storageError();
    this.#transaction(() =>
      this.#database.prepare('UPDATE desired SET body = ? WHERE id = 1').run(body),
    );
  }

  append(entries: readonly LogInput[]): void {
    if (entries.length > 200) throw storageError();
    const values = entries.map((entry) => {
      const parsed = logInputSchema.parse(entry);
      return { ...parsed, rawMessage: redact(parsed.rawMessage).slice(0, 4096) };
    });
    this.#transaction(() => {
      const insert = this.#database.prepare('INSERT INTO logs (stream, body) VALUES (?, ?)');
      for (const entry of values) insert.run(streamKey(entry.stream), JSON.stringify(entry));
      for (const key of new Set(values.map((entry) => streamKey(entry.stream)))) {
        this.#database
          .prepare(
            'DELETE FROM logs WHERE stream = ? AND sequence NOT IN (SELECT sequence FROM logs WHERE stream = ? ORDER BY sequence DESC LIMIT 1000)',
          )
          .run(key, key);
      }
      this.#database.exec(
        'DELETE FROM logs WHERE sequence NOT IN (SELECT sequence FROM logs ORDER BY sequence DESC LIMIT 5000)',
      );
    });
  }

  queryLogs(stream: unknown, limit: number): StoredLog[] {
    if (!Number.isInteger(limit) || limit < 1 || limit > 200) throw storageError();
    const rows = this.#database
      .prepare(
        'SELECT sequence, body FROM (SELECT sequence, body FROM logs WHERE stream = ? ORDER BY sequence DESC LIMIT ?) ORDER BY sequence',
      )
      .all(streamKey(stream), limit);
    return rows.map((row) => ({
      ...logInputSchema.parse(JSON.parse(String(row.body))),
      sequenceNumber: Number(row.sequence),
    }));
  }

  close(): void {
    this.#database.close();
  }

  #transaction(operation: () => unknown): void {
    this.#database.exec('BEGIN IMMEDIATE');
    try {
      operation();
      this.#database.exec('COMMIT');
    } catch (error) {
      this.#database.exec('ROLLBACK');
      throw error;
    }
  }
}
