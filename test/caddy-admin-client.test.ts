import 'reflect-metadata';
import { webcrypto } from 'node:crypto';
import { createServer, request } from 'node:https';
import { createServer as createTcpServer, type Server } from 'node:net';
import type { IncomingMessage, ServerResponse } from 'node:http';
import type { Duplex } from 'node:stream';
import {
  BasicConstraintsExtension,
  ExtendedKeyUsage,
  ExtendedKeyUsageExtension,
  KeyUsageFlags,
  KeyUsagesExtension,
  PemConverter,
  SubjectAlternativeNameExtension,
  X509CertificateGenerator,
} from '@peculiar/x509';
import { afterEach, beforeAll, expect, it, vi } from 'vitest';
import { CaddyAdminClient } from '../src/caddy/admin-client.ts';
import { normalizeCaddyConfig, type SecureAdminPolicy } from '../src/caddy/admin-policy.ts';
import type { CaddyConfig } from '../src/contracts/ports.ts';
import { ConfigurationTransactions } from '../src/daemon/configuration-transactions.ts';
import { configHash } from '../src/caddy/preparation.ts';

vi.mock('node:https', async (original) => {
  const actual = await original<typeof import('node:https')>();
  return { ...actual, request: vi.fn(actual.request) };
});

const crypto = webcrypto as unknown as Crypto;
const algorithm = { name: 'ECDSA', namedCurve: 'P-256' };
const signingAlgorithm = { name: 'ECDSA', hash: 'SHA-256' };
type Identity = { cert: string; key: string; keys: CryptoKeyPair };
type Handler = (req: IncomingMessage, res: ServerResponse) => void;
let root: Identity;
let foreignRoot: Identity;
let client: Identity;
let foreignClient: Identity;
let serverIdentity: Identity;
let wrongName: Identity;
let expired: Identity;
const servers: Server[] = [];
const sockets = new Set<Duplex>();
const clients: CaddyAdminClient[] = [];
const timers = new Set<ReturnType<typeof setInterval>>();

async function issue(
  role: 'root' | 'client' | 'server',
  issuer?: Identity,
  name = 'localhost',
  old = false,
): Promise<Identity> {
  const keys = await crypto.subtle.generateKey(algorithm, true, ['sign', 'verify']);
  const certificate = await X509CertificateGenerator.create(
    {
      subject: `CN=${role === 'root' ? 'Fixture Root' : name}`,
      issuer: 'CN=Fixture Root',
      publicKey: keys.publicKey,
      signingKey: issuer?.keys.privateKey ?? keys.privateKey,
      signingAlgorithm,
      notBefore: new Date(Date.now() - 60_000),
      notAfter: new Date(Date.now() + (old ? -1_000 : 3_600_000)),
      extensions: [
        new BasicConstraintsExtension(role === 'root', role === 'root' ? 1 : undefined, true),
        new KeyUsagesExtension(
          role === 'root' ? KeyUsageFlags.keyCertSign : KeyUsageFlags.digitalSignature,
          true,
        ),
        ...(role === 'root'
          ? []
          : [
              new ExtendedKeyUsageExtension(
                [role === 'client' ? ExtendedKeyUsage.clientAuth : ExtendedKeyUsage.serverAuth],
                true,
              ),
            ]),
        ...(role === 'server'
          ? [new SubjectAlternativeNameExtension([{ type: 'dns', value: name }])]
          : []),
      ],
    },
    crypto,
  );
  return {
    cert: certificate.toString('pem'),
    key: PemConverter.encode(
      await crypto.subtle.exportKey('pkcs8', keys.privateKey),
      'PRIVATE KEY',
    ),
    keys,
  };
}

beforeAll(async () => {
  root = await issue('root');
  foreignRoot = await issue('root');
  client = await issue('client', root);
  foreignClient = await issue('client', foreignRoot);
  serverIdentity = await issue('server', root);
  wrongName = await issue('server', root, 'other.localhost');
  expired = await issue('server', root, 'localhost', true);
});
afterEach(async () => {
  vi.mocked(request).mockClear();
  for (const client of clients.splice(0)) client.close();
  for (const timer of timers) clearInterval(timer);
  timers.clear();
  for (const socket of sockets) socket.destroy();
  sockets.clear();
  await Promise.all(
    servers
      .splice(0)
      .map(
        (server) =>
          new Promise<void>((resolve, reject) =>
            server.close((error) => (error ? reject(error) : resolve())),
          ),
      ),
  );
});

function candidate(tag = 'initial'): CaddyConfig {
  return normalizeCaddyConfig(
    JSON.stringify({
      admin: {
        disabled: true,
        remote: { listen: 'fixture', access_control: [{ public_keys: ['authorized'] }] },
      },
      apps: {
        pki: { certificate_authorities: { local: { install_trust: false } } },
        http: { opaque: [tag, { z: 1, a: 2 }] },
      },
    }),
  );
}
async function fixture(
  handler: Handler,
  identity = serverIdentity,
): Promise<Pick<SecureAdminPolicy, 'config' | 'client'>> {
  const server = createServer(
    {
      key: identity.key,
      cert: identity.cert,
      ca: root.cert,
      requestCert: true,
      rejectUnauthorized: true,
    },
    handler,
  );
  servers.push(server);
  server.on('connection', (socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
  });
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  if (typeof address !== 'object' || address === null) throw new Error('Fixture address missing');
  return {
    config: candidate(),
    client: {
      host: '127.0.0.1',
      port: address.port,
      servername: 'localhost',
      rejectUnauthorized: true,
      ca: root.cert,
      cert: client.cert,
      key: client.key,
    },
  };
}
function makeClient(
  policy: Pick<SecureAdminPolicy, 'config' | 'client'>,
  options: ConstructorParameters<typeof CaddyAdminClient>[1] = {},
): CaddyAdminClient {
  const value = new CaddyAdminClient(policy, options);
  clients.push(value);
  return value;
}
async function unavailable(value: CaddyAdminClient): Promise<void> {
  const result = await value.readActive();
  expect(result).toMatchObject({
    ok: false,
    error: { code: 'caddy_admin_readback_failed', retryable: false },
  });
  expect(JSON.stringify(result)).not.toMatch(
    /PRIVATE KEY|CERTIFICATE|SECRET|localhost|127\.0\.0\.1/,
  );
}

it('uses authorized HTTPS, POST /load complete canonical JSON and independent GET /config/', async () => {
  let observedBody = '';
  let active = candidate('actually changed');
  const calls: string[] = [];
  const policy = await fixture((req, res) => {
    calls.push(`${req.method} ${req.url}`);
    if (req.method === 'POST') {
      expect(req.headers['content-type']).toBe('application/json');
      req.setEncoding('utf8');
      req.on('data', (chunk: string) => {
        observedBody += chunk;
      });
      req.on('end', () => res.end());
    } else res.end(` \n${active.adaptedConfig.body}\n`);
  });
  const value = makeClient(policy);
  expect(await value.apply(policy.config)).toEqual({ status: 'applied' });
  expect(observedBody).toBe(policy.config.adaptedConfig.body);
  expect(await value.readActive()).toEqual({ ok: true, value: active.effectiveConfigHash });
  active = candidate('second independent change');
  expect(await value.readActive()).toEqual({ ok: true, value: active.effectiveConfigHash });
  expect(calls).toEqual(['POST /load', 'GET /config/', 'GET /config/']);
});

it('verified POST /stop is bounded, cancellation-aware and only a receipt', async () => {
  const calls: string[] = [];
  const policy = await fixture((req, res) => {
    calls.push(`${req.method} ${req.url}`);
    res.end();
  });
  const value = makeClient(policy);
  expect(await value.stop()).toEqual({ status: 'applied' });
  expect(calls).toEqual(['POST /stop']);
  expect(await value.stop(AbortSignal.abort())).toEqual({ status: 'ambiguous' });
  expect(calls).toHaveLength(1);
  const stalled = makeClient(await fixture(() => undefined), { timeoutMs: 50 });
  expect(await stalled.stop()).toEqual({ status: 'ambiguous' });
  const abort = new AbortController();
  const pending = stalled.readActive(abort.signal);
  abort.abort();
  expect((await pending).ok).toBe(false);
});

it('stop retains normal HTTPS authentication denial', async () => {
  let requests = 0;
  const policy = await fixture((_req, res) => {
    requests++;
    res.end();
  }, wrongName);
  expect(await makeClient(policy).stop()).toEqual({ status: 'ambiguous' });
  expect(requests).toBe(0);
});

it.each(['root', 'name', 'client', 'expired'] as const)(
  'denies wrong %s via actual TLS without diagnostics leakage',
  async (kind) => {
    let requests = 0;
    let identity = serverIdentity;
    if (kind === 'name') identity = wrongName;
    if (kind === 'expired') identity = expired;
    const policy = await fixture((_req, res) => {
      requests++;
      res.end('{}');
    }, identity);
    const tls = { ...policy.client };
    if (kind === 'root') tls.ca = foreignRoot.cert;
    if (kind === 'client') {
      tls.cert = foreignClient.cert;
      tls.key = foreignClient.key;
    }
    const value = makeClient({ ...policy, client: tls });
    expect(await value.apply(policy.config)).toEqual({ status: 'ambiguous' });
    await unavailable(value);
    expect(requests).toBe(0);
  },
);

it('refuses noncanonical, invalid, mismatched and changed security candidates before network', async () => {
  let requests = 0;
  const policy = await fixture((_req, res) => {
    requests++;
    res.end();
  });
  const value = makeClient(policy);
  const changed = JSON.parse(policy.config.adaptedConfig.body) as {
    admin: { disabled: boolean };
    apps: { pki: unknown };
  };
  const bad: unknown[] = [
    null,
    {},
    { ...policy.config, effectiveConfigHash: 'incorrect' },
    normalizeCaddyConfig('{}'),
  ];
  const spaced = ` ${policy.config.adaptedConfig.body}`;
  const normalized = normalizeCaddyConfig(spaced);
  // Matching raw hash is still not a canonical candidate.
  bad.push({
    ...normalized,
    adaptedConfig: { format: 'json', body: spaced },
    effectiveConfigHash: configHash(spaced),
  });
  changed.admin.disabled = false;
  bad.push(normalizeCaddyConfig(JSON.stringify(changed)));
  changed.admin.disabled = true;
  changed.apps.pki = { certificate_authorities: { local: { install_trust: true } } };
  bad.push(normalizeCaddyConfig(JSON.stringify(changed)));
  changed.apps.pki = {
    certificate_authorities: { 'cadder-admin': { root: { certificate: 'foreign CA' } } },
  };
  bad.push(normalizeCaddyConfig(JSON.stringify(changed)));
  const foreignAuthorization = JSON.parse(policy.config.adaptedConfig.body) as {
    admin: { remote: { access_control: unknown } };
  };
  foreignAuthorization.admin.remote.access_control = [{ public_keys: ['foreign client'] }];
  bad.push(normalizeCaddyConfig(JSON.stringify(foreignAuthorization)));
  for (const config of bad)
    expect(await value.apply(config as CaddyConfig)).toEqual({ status: 'definitely-rejected' });
  expect(requests).toBe(0);
});

it('explicitly refuses idle without sending an HTTPS request', async () => {
  const policy = await fixture((_req, res) => res.end());
  const value = makeClient(policy);
  expect(await value.apply(null as never)).toEqual({ status: 'definitely-rejected' });
  expect(vi.mocked(request)).not.toHaveBeenCalled();
});

it('isolates the explicit credentials and baseline from caller mutation', async () => {
  const policy = await fixture((_req, res) => res.end(candidate().adaptedConfig.body));
  const mutable = { config: policy.config, client: { ...policy.client } };
  const value = makeClient(mutable);
  mutable.client.ca = 'SECRET bad root';
  mutable.client.port = 2019;
  mutable.config = normalizeCaddyConfig('{}');
  expect(await value.apply(policy.config)).toEqual({ status: 'applied' });
  expect(await value.readActive()).toEqual({ ok: true, value: policy.config.effectiveConfigHash });
});

it.each([200, 204, 299, 400, 301, 307, 401, 403, 404, 500, 503])(
  'classifies completed HTTP %i without redirects/retries',
  async (status) => {
    let calls = 0;
    const policy = await fixture((_req, res) => {
      calls++;
      res.writeHead(status, { Location: 'http://127.0.0.1:2019/', 'Content-Type': 'text/plain' });
      res.end('SECRET Caddy error /private/path');
    });
    const value = makeClient(policy);
    let expected = 'ambiguous';
    if (status >= 200 && status < 300) expected = 'applied';
    if (status === 400) expected = 'definitely-rejected';
    expect(await value.apply(policy.config)).toEqual({ status: expected });
    expect(calls).toBe(1);
  },
);

it.each([204, 301, 400, 403, 500])(
  'readback HTTP %i is unavailable, not idle or a receipt',
  async (status) => {
    const policy = await fixture((_req, res) => {
      res.writeHead(status);
      res.end('{}');
    });
    await unavailable(makeClient(policy));
  },
);

it.each(['truncated', 'array', 'null', 'text', 'utf8', 'empty', 'bom'])(
  'refuses invalid readback %s, never substitutes cached state',
  async (kind) => {
    const bodies: Record<string, string | Buffer> = {
      truncated: '{',
      array: '[]',
      null: 'null',
      text: 'SECRET exception',
      utf8: Buffer.from([0xc3, 0x28]),
      empty: '',
      bom: '\ufeff{}',
    };
    const policy = await fixture((_req, res) => res.end(bodies[kind]));
    await unavailable(makeClient(policy));
  },
);

it.each([
  'declared',
  'chunked',
  'headers',
  'encoding',
  'framing',
  'invalid-header',
  'invalid-utf8',
])('bounds and validates %s replies for both operations', async (kind) => {
  const policy = await fixture((_req, res) => {
    if (kind === 'declared') {
      res.writeHead(200, { 'Content-Length': 1024 });
      res.flushHeaders();
    } else if (kind === 'chunked') {
      res.write('{}');
      res.end('x'.repeat(40));
    } else if (kind === 'headers') {
      res.setHeader('X-Big', 'x'.repeat(1024));
      res.end('{}');
    } else if (kind === 'encoding') {
      res.setHeader('Content-Encoding', 'gzip');
      res.end('{}');
    } else if (kind === 'invalid-utf8') res.end(Buffer.from([0xff]));
    else if (kind === 'invalid-header') {
      res.socket!.end('HTTP/1.1 200 OK\r\nContent-Length: nope\r\n\r\n{}');
    } else {
      res.writeHead(400, { 'Content-Length': 10 });
      res.write('{}');
      res.socket!.end();
    }
  });
  const value = makeClient(policy, { maxResponseBytes: 32, maxHeaderBytes: 512, timeoutMs: 1000 });
  expect(await value.apply(policy.config)).toEqual({ status: 'ambiguous' });
  await unavailable(value);
});

it('uses an absolute body deadline despite slow continuous chunks and never retries a POST', async () => {
  let calls = 0;
  const policy = await fixture((_req, res) => {
    calls++;
    res.writeHead(200);
    res.write('{');
    const timer = setInterval(() => res.write(' '), 10);
    timers.add(timer);
    res.on('close', () => {
      clearInterval(timer);
      timers.delete(timer);
    });
  });
  const value = makeClient(policy, { timeoutMs: 120 });
  const start = performance.now();
  expect(await value.apply(policy.config)).toEqual({ status: 'ambiguous' });
  expect(performance.now() - start).toBeLessThan(1000);
  expect(calls).toBe(1);
});

it('absolute deadline aborts a stalled TLS handshake and destroys the owned socket', async () => {
  let clientHelloBytes = 0;
  const server = createTcpServer((socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
    socket.on('data', (bytes: Buffer) => {
      clientHelloBytes += bytes.length;
    });
  });
  servers.push(server);
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Missing fixture port');
  const value = makeClient(
    {
      config: candidate(),
      client: {
        host: '127.0.0.1',
        port: address.port,
        servername: 'localhost',
        rejectUnauthorized: true,
        ca: root.cert,
        cert: client.cert,
        key: client.key,
      },
    },
    { timeoutMs: 100 },
  );
  expect(await value.apply(candidate())).toEqual({ status: 'ambiguous' });
  expect(clientHelloBytes).toBeGreaterThan(0);
  expect(vi.mocked(request)).toHaveBeenCalledTimes(1);
  // Owned request destruction closes the peer; no fixture teardown needed to trigger it.
  await vi.waitFor(() => expect(sockets.size).toBe(0));
});

it('accepts the exact byte bound and UTF-8 characters split across chunks', async () => {
  const body = '{"emoji":"😀"}';
  const bytes = Buffer.from(body);
  const policy = await fixture((_req, res) => {
    res.write(bytes.subarray(0, 11));
    setImmediate(() => res.end(bytes.subarray(11)));
  });
  const value = makeClient(policy, { maxResponseBytes: bytes.length });
  expect(await value.readActive()).toEqual({
    ok: true,
    value: normalizeCaddyConfig(body).effectiveConfigHash,
  });
});

it('close aborts in-flight work, settles once and blocks new admissions', async () => {
  let entered!: () => void;
  const entry = new Promise<void>((resolve) => {
    entered = resolve;
  });
  let calls = 0;
  const policy = await fixture((_req, res) => {
    calls++;
    res.writeHead(200);
    res.write(' ');
    entered();
  });
  const value = makeClient(policy);
  const pending = value.apply(policy.config);
  await entry;
  value.close();
  value.close();
  expect(await pending).toEqual({ status: 'ambiguous' });
  expect(await value.apply(policy.config)).toEqual({ status: 'definitely-rejected' });
  await unavailable(value);
  expect(calls).toBe(1);
});

it.each([
  'port',
  'host',
  'servername',
  'verify',
  'ca',
  'cert',
  'key',
  'config',
  'missing-envelope',
  'timeout',
  'response-bound',
  'header-bound',
] as const)('rejects invalid constructor %s with bounded secret-free errors', async (kind) => {
  const policy = await fixture((_req, res) => res.end('{}'));
  const mutable = { config: policy.config, client: { ...policy.client } };
  const options: { timeoutMs?: number; maxResponseBytes?: number; maxHeaderBytes?: number } = {};
  if (kind === 'port') mutable.client.port = 1.5;
  if (kind === 'host') Object.assign(mutable.client, { host: 'SECRET.example' });
  if (kind === 'servername') Object.assign(mutable.client, { servername: 'SECRET.example' });
  if (kind === 'verify') Object.assign(mutable.client, { rejectUnauthorized: false });
  if (kind === 'ca' || kind === 'cert' || kind === 'key') mutable.client[kind] = '';
  if (kind === 'config') mutable.config = { ...policy.config, effectiveConfigHash: 'SECRET' };
  if (kind === 'missing-envelope') mutable.config = normalizeCaddyConfig('{}');
  if (kind === 'timeout') options.timeoutMs = 0;
  if (kind === 'response-bound') options.maxResponseBytes = 32 * 1024 * 1024 + 1;
  if (kind === 'header-bound') options.maxHeaderBytes = Infinity;
  expect(() => new CaddyAdminClient(mutable, options)).toThrow(
    'Invalid prepared administration policy or client bounds.',
  );
  expect(vi.mocked(request)).not.toHaveBeenCalled();
});

it('unexpected HTTPS throws are ambiguous, sanitized and never retried', async () => {
  const policy = await fixture((_req, res) => res.end('{}'));
  const value = makeClient(policy);
  vi.mocked(request).mockImplementationOnce(() => {
    throw new Error('SECRET /private/key.pem');
  });
  expect(await value.apply(policy.config)).toEqual({ status: 'ambiguous' });
  expect(vi.mocked(request)).toHaveBeenCalledTimes(1);
  vi.mocked(request).mockImplementationOnce(() => {
    throw new Error('SECRET /private/key.pem');
  });
  await unavailable(value);
  expect(vi.mocked(request)).toHaveBeenCalledTimes(2);
});

it.each(['disconnect', 'no-headers'])('handles %s without proving no transition', async (kind) => {
  const policy = await fixture((req) => {
    if (kind === 'disconnect') req.socket.destroy();
  });
  const value = makeClient(policy, { timeoutMs: 100 });
  expect(await value.apply(policy.config)).toEqual({ status: 'ambiguous' });
  await unavailable(value);
});

it('normalizes actual readback with lexical numeric keys, arrays and opaque data intact', async () => {
  const actual =
    '{"z":[3,1,{"2":"b","10":"a"}],"admin":{"unknown":true},"a":{"unknown_module":{"payload":[null,false]}}}';
  const policy = await fixture((_req, res) => res.end(actual));
  const value = makeClient(policy);
  expect(await value.readActive()).toEqual({
    ok: true,
    value: normalizeCaddyConfig(actual).effectiveConfigHash,
  });
  expect(normalizeCaddyConfig(actual).adaptedConfig.body).toContain('"10":"a","2":"b"');
});

it('preserves late POST uncertainty even if a delayed response attempts to succeed', async () => {
  let reply!: ServerResponse;
  let entered!: () => void;
  const entry = new Promise<void>((resolve) => {
    entered = resolve;
  });
  let calls = 0;
  const policy = await fixture((_req, res) => {
    calls++;
    reply = res;
    entered();
  });
  const value = makeClient(policy, { timeoutMs: 100 });
  const operation = value.apply(policy.config);
  await entry;
  expect(await operation).toEqual({ status: 'ambiguous' });
  reply.end();
  expect(await operation).toEqual({ status: 'ambiguous' });
  expect(calls).toBe(1);
});

it('queue restores and independently verifies the committed configuration after persist failure', async () => {
  let active = candidate();
  const applied: string[] = [];
  const policy = await fixture((req, res) => {
    if (req.method !== 'POST') {
      res.end(active.adaptedConfig.body);
      return;
    }
    let text = '';
    req.setEncoding('utf8');
    req.on('data', (chunk: string) => {
      text += chunk;
    });
    req.on('end', () => {
      active = normalizeCaddyConfig(text);
      applied.push(active.effectiveConfigHash);
      res.end();
    });
  });
  const value = makeClient(policy);
  const initial = { config: policy.config, desiredState: { projects: [] }, registrations: [] };
  const next = candidate('new');
  const queue = new ConfigurationTransactions(
    initial,
    {
      validate: async (config) => ({
        ok: true,
        value: { effectiveConfigHash: config.effectiveConfigHash, diagnostics: [] },
      }),
    },
    {
      persistDesiredState: async () => ({
        ok: false,
        error: {
          kind: 'storage',
          code: 'failed',
          message: 'failed',
          guidance: null,
          retryable: false,
          requestId: null,
        },
      }),
    },
    {
      apply: async (config) => (config === null ? { status: 'ambiguous' } : value.apply(config)),
      readActive: () => value.readActive(),
    },
  );
  expect(
    await queue.submit(() => ({ ok: true, value: { ...initial, config: next } })),
  ).toMatchObject({ ok: false, error: { code: 'config_persist_failed' } });
  expect(applied).toEqual([next.effectiveConfigHash, policy.config.effectiveConfigHash]);
  expect(queue.readFence()).toBeNull();
  expect(queue.readCommitted()).toEqual(initial);
  await queue.close();
});

it('fences queue publication on independently changed readback', async () => {
  const policy = await fixture((_req, res) =>
    res.end(candidate('unexpected native difference').adaptedConfig.body),
  );
  const value = makeClient(policy);
  let persisted = 0;
  const initial = { config: policy.config, desiredState: { projects: [] }, registrations: [] };
  const queue = new ConfigurationTransactions(
    initial,
    {
      validate: async (config) => ({
        ok: true,
        value: { effectiveConfigHash: config.effectiveConfigHash, diagnostics: [] },
      }),
    },
    {
      persistDesiredState: async () => {
        persisted++;
        return { ok: true, value: undefined };
      },
    },
    {
      apply: async (config) => (config === null ? { status: 'ambiguous' } : value.apply(config)),
      readActive: () => value.readActive(),
    },
  );
  expect(
    await queue.submit(() => ({ ok: true, value: { ...initial, config: candidate('submitted') } })),
  ).toMatchObject({ ok: false, error: { code: 'config_verification_failed' } });
  expect(queue.readFence()?.code).toBe('config_verification_failed');
  expect(persisted).toBe(0);
  expect(queue.readCommitted()).toEqual(initial);
  await queue.close();
});
