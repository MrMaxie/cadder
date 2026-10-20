import { StringDecoder } from 'node:string_decoder';
import type { EntrypointRegistration } from '../protocol/dto.ts';
import { canonicalizeDomain } from '../protocol/domain.ts';
import { runtimeLogStream, type LogInput } from '../storage/schema.ts';

/** Bounded line decoding; private-key continuation lines never become separate events. */
export class CaddyLogs {
  readonly #streams = {
    stdout: {
      decoder: new StringDecoder('utf8'),
      line: '',
      marker: '',
      begin: false,
      oversized: false,
      privateKey: false,
    },
    stderr: {
      decoder: new StringDecoder('utf8'),
      line: '',
      marker: '',
      begin: false,
      oversized: false,
      privateKey: false,
    },
  };
  #pending = new Set<Promise<void>>();
  constructor(
    private readonly registrations: () => readonly EntrypointRegistration[],
    private readonly append: (entries: LogInput[]) => Promise<void>,
    private readonly failed: () => void,
  ) {}
  accept = (channel: 'stdout' | 'stderr', bytes: Buffer): void => {
    const stream = this.#streams[channel];
    for (const character of stream.decoder.write(bytes)) {
      stream.marker = (stream.marker + character).slice(-11);
      if (stream.marker.toUpperCase() === '-----BEGIN ') stream.begin = true;
      if (stream.oversized && stream.begin) stream.privateKey = true;
      if (character === '\n') {
        if (!stream.oversized) this.#line(stream.line, stream);
        stream.line = '';
        stream.marker = '';
        stream.begin = false;
        stream.oversized = false;
      } else if (!stream.oversized) {
        if (stream.line.length < 8192) stream.line += character;
        else {
          stream.line = '';
          stream.oversized = true;
          if (stream.begin) stream.privateKey = true;
          this.failed();
        }
      }
    }
  };
  async close(): Promise<void> {
    for (const stream of Object.values(this.#streams)) {
      stream.line += stream.decoder.end();
      if (stream.line && !stream.oversized) this.#line(stream.line, stream);
      stream.line = '';
    }
    await Promise.all(this.#pending);
  }
  #line(line: string, stream: { privateKey: boolean }): void {
    if (stream.privateKey) {
      if (/-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----/i.test(line)) stream.privateKey = false;
      return;
    }
    if (
      /-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----/i.test(line) &&
      !/-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----/i.test(line)
    )
      stream.privateKey = true;
    let message = line;
    let host: string | undefined;
    let level: LogInput['severity'] = 'info';
    try {
      const parsed = JSON.parse(line) as {
        msg?: unknown;
        level?: unknown;
        request?: { host?: unknown };
        identifier?: unknown;
      };
      if (typeof parsed.msg === 'string') message = parsed.msg;
      const candidate = parsed.request?.host ?? parsed.identifier;
      if (typeof candidate === 'string') host = canonicalizeDomain(candidate.replace(/:\d+$/, ''));
      if (['trace', 'debug', 'info', 'warn', 'error', 'fatal'].includes(String(parsed.level)))
        level = parsed.level as LogInput['severity'];
    } catch {
      /* Plain upstream diagnostic lines use the same redaction boundary. */
    }
    const event: LogInput = {
      timestampUtc: new Date().toISOString(),
      severity: level,
      stream: runtimeLogStream,
      attributionKind: 'runtime',
      entryKind: 'normal',
      rawMessage: message,
      domainKey: null,
      sourceRegistrationId: null,
      sourceInstanceId: null,
      operation: null,
    };
    const entries = [event];
    for (const project of this.registrations()) {
      const domain = project.registeredDomains.find((entry) => entry.name.canonical === host);
      if (domain)
        for (const logStream of [project.logStream, domain.logStream])
          entries.push({
            ...event,
            stream: logStream,
            attributionKind: logStream.domainKey ? 'domain' : 'entrypoint',
            domainKey: logStream.domainKey,
            sourceRegistrationId: project.registrationId,
            sourceInstanceId: project.entrypointInstance.instanceId,
          });
    }
    if (this.#pending.size >= 64) {
      this.failed();
      return;
    }
    const pending = this.append(entries)
      .catch(this.failed)
      .finally(() => this.#pending.delete(pending));
    this.#pending.add(pending);
  }
}
