import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { ClientService } from '../client/service.ts';
import { rpc } from '../client/connection.ts';
import { RealCaddyResolver } from '../caddy/resolver.ts';
import { VERSION } from '../protocol/version.ts';
import { CadderError, errorCode } from '../protocol/errors.ts';
import type { EntrypointRegistration } from '../protocol/dto.ts';

const inspection = new Set([
  'adapt',
  'build-info',
  'environ',
  'help',
  'list-modules',
  'validate',
  'version',
]);
const passthrough = new Set(['completion', 'file-server', 'fmt', 'manpage', 'reverse-proxy']);
export function shimPolicy(
  argv: readonly string[],
): 'managed' | 'inspection' | 'passthrough' | 'unsupported' {
  const raw = argv[0] ?? 'help';
  const command = ['--help', '-h'].includes(raw)
    ? 'help'
    : ['--version', '-v'].includes(raw)
      ? 'version'
      : raw;
  return command === 'run'
    ? 'managed'
    : inspection.has(command)
      ? 'inspection'
      : passthrough.has(command)
        ? 'passthrough'
        : 'unsupported';
}

export function buildRegistration(
  argv: readonly string[],
  cwd = process.cwd(),
): EntrypointRegistration {
  let config = resolve(cwd, 'Caddyfile');
  let adapter: string | null = null;
  for (let index = 1; index < argv.length; index++) {
    if (['--config', '-c', '--adapter', '-a'].includes(argv[index]!)) {
      const flag = argv[index]!;
      const value = argv[++index];
      if (!value) throw new CadderError('invalid-input', `${flag} requires a value.`);
      if (flag === '--config' || flag === '-c') config = resolve(cwd, value);
      else adapter = value;
    }
  }
  const nonce = randomUUID();
  const instanceId = randomUUID();
  const now = new Date().toISOString();
  return {
    registrationId: instanceId,
    entrypointInstance: { instanceId, startedAtUtc: now, shimSessionNonce: nonce },
    sourceWorkingDirectory: { raw: resolve(cwd), canonical: null },
    sourceConfigPath: { raw: config, canonical: null },
    registeredDomains: [],
    activationState: 'active',
    ownerProcess: {
      processId: process.pid,
      processStartTimeUtc: new Date(Date.now() - process.uptime() * 1000).toISOString(),
      shimSessionNonce: nonce,
      executablePath: process.execPath,
    },
    logStream: { streamId: `entrypoint-${instanceId}`, domainKey: null, channel: 'caddy' },
    shimRun: { adapter, rawArguments: [...argv], commandLine: ['caddy', ...argv].join(' ') },
    createdAtUtc: now,
    lastHeartbeatUtc: now,
  };
}

export async function managedRun(
  argv: readonly string[],
  client = new ClientService(),
  signal?: AbortSignal,
): Promise<void> {
  const abort = new AbortController();
  const stop = (): void => abort.abort();
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
  signal?.addEventListener('abort', stop, { once: true });
  if (signal?.aborted) stop();
  const registration = buildRegistration(argv);
  const nonce = registration.entrypointInstance.shimSessionNonce;
  let id: string | null = null;
  let cleanupFailure: unknown;
  try {
    if (abort.signal.aborted) return;
    await client.start();
    if (abort.signal.aborted) return;
    const register = async (): Promise<string> => {
      const reply = await rpc(client.paths, 'register-entrypoint-request', { registration });
      if (!reply.accepted || !reply.registrationId)
        throw new CadderError('registration-rejected', reply.message);
      return reply.registrationId;
    };
    id = await register();
    let disconnectedAt: number | undefined;
    while (!abort.signal.aborted) {
      try {
        await delay(5000, undefined, { signal: abort.signal });
      } catch {
        break;
      }
      try {
        const reply = await rpc(client.paths, 'heartbeat-entrypoint-request', {
          registrationId: id,
          shimSessionNonce: nonce,
        });
        if (!reply.accepted) throw new CadderError('stale_session', reply.message);
        disconnectedAt = undefined;
      } catch (error) {
        if (errorCode(error) === 'stale_session') {
          id = await register();
          disconnectedAt = undefined;
        } else if (
          ['daemon-unavailable', 'daemon_shutting_down'].includes(errorCode(error) ?? '')
        ) {
          disconnectedAt ??= Date.now();
          if (Date.now() - disconnectedAt >= 15000)
            throw new CadderError(
              'daemon-unavailable',
              'Lost cadderd. Restart the daemon and run caddy run again.',
            );
        } else throw error;
      }
    }
  } finally {
    process.off('SIGINT', stop);
    process.off('SIGTERM', stop);
    signal?.removeEventListener('abort', stop);
    if (id) {
      try {
        await rpc(client.paths, 'unregister-entrypoint-request', {
          registrationId: id,
          shimSessionNonce: nonce,
        });
      } catch (error) {
        if (
          !['daemon-unavailable', 'daemon_shutting_down', 'stale_session'].includes(
            errorCode(error) ?? '',
          )
        )
          cleanupFailure = error;
      }
    }
  }
  if (cleanupFailure) throw cleanupFailure;
}

export async function runShim(argv: readonly string[]): Promise<number> {
  if (argv.includes('--cadder-shim-info')) {
    process.stdout.write(
      JSON.stringify({ role: 'caddy-shim', version: VERSION, executable: process.execPath }) + '\n',
    );
    return 0;
  }
  const policy = shimPolicy(argv);
  if (policy === 'unsupported')
    throw new CadderError(
      'unsupported-operation',
      `Cadder shim does not support caddy ${argv[0] ?? ''}.`,
    );
  if (policy === 'managed') {
    await managedRun(argv);
    return 0;
  }
  const resolver = new RealCaddyResolver();
  try {
    const image = await resolver.pin();
    await image.verify();
    if (policy === 'inspection')
      process.stderr.write('Output is real-Caddy inspection, not Cadder runtime state.\n');
    return await new Promise<number>((resolveExit, reject) => {
      const child = spawn(image.path, [...argv], {
        stdio: 'inherit',
        shell: false,
        windowsHide: true,
      });
      const interrupt = (): void => {
        child.kill('SIGINT');
      };
      const terminate = (): void => {
        child.kill('SIGTERM');
      };
      process.once('SIGINT', interrupt);
      process.once('SIGTERM', terminate);
      const cleanup = (): void => {
        process.off('SIGINT', interrupt);
        process.off('SIGTERM', terminate);
      };
      child.once('error', (error) => {
        cleanup();
        reject(error);
      });
      child.once('exit', (code, signal) => {
        cleanup();
        resolveExit(code ?? (signal === 'SIGINT' ? 130 : 1));
      });
    });
  } finally {
    await resolver.close();
  }
}
