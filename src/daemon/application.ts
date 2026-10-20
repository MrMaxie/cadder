import { createHash } from 'node:crypto';
import { realpath } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import type { CaddyConfig, DesiredState, PortResult } from '../contracts/ports.ts';
import { canonicalizeDomain, extractRegisteredDomains, activeDomains } from '../caddy/domains.ts';
import {
  composeRoutePlan,
  type CaddyRoutePlan,
  type PreparedCompositionInput,
} from '../caddy/composition.ts';
import type { CaddyAdaptInput } from '../caddy/adapter.ts';
import type { EntrypointRegistration, StateSnapshot } from '../protocol/dto.ts';
import { registrationSchema } from '../protocol/dto.ts';
import { RpcError, type ProtocolError } from '../protocol/errors.ts';
import type { RpcHandler, RpcRequest, HandlerResult } from '../protocol/rpc.ts';
import { redact } from '../storage/redaction.ts';
import {
  ConfigurationTransactions,
  type ConfigurationSnapshot,
} from './configuration-transactions.ts';
import type { LogInput, StoredLog } from '../storage/schema.ts';
import { runtimeLogStream } from '../storage/schema.ts';

type ApplicationOptions = {
  transactions: ConfigurationTransactions;
  adapt(input: CaddyAdaptInput): Promise<PortResult<CaddyConfig>>;
  secure(plan: CaddyRoutePlan): Promise<PortResult<CaddyConfig>>;
  queryLogs(stream: LogInput['stream'], limit: number): Promise<StoredLog[]>;
  log(entry: LogInput): Promise<void>;
  shutdown(): void;
  runtime(): StateSnapshot['runtime'];
  storage: StateSnapshot['storage'];
};
function reject(
  kind: 'invalidInput' | 'conflict' | 'staleInstance' | 'configuration',
  message: string,
): never {
  throw new RpcError({
    kind,
    code: kind === 'staleInstance' ? 'stale_session' : 'operation_rejected',
    message,
    guidance: null,
    retryable: false,
    requestId: null,
  });
}
function projectKey(registration: EntrypointRegistration): string {
  const path = registration.sourceConfigPath.canonical!;
  return createHash('sha256')
    .update(process.platform === 'win32' ? path.toLowerCase() : path)
    .digest('hex')
    .slice(0, 32);
}
function unwrap<T>(result: PortResult<T>): T {
  if (!result.ok) throw new RpcError(result.error);
  return result.value;
}

/** Live leases stay in memory; only the existing queue publishes configuration changes. */
export class DaemonApplication {
  readonly #heartbeats = new Map<string, number>();
  #closing = false;
  #lastError: ProtocolError | null = null;
  #attemptedAt: string | null = null;
  #appliedAt: string | null = null;
  constructor(private readonly options: ApplicationOptions) {}

  readonly handle: RpcHandler = async (request) => {
    const mutation = [
      'register-entrypoint-request',
      'unregister-entrypoint-request',
      'set-entrypoint-enabled-request',
      'set-domain-enabled-request',
    ].includes(request.method);
    if (mutation) this.#attemptedAt = new Date().toISOString();
    try {
      const result = await this.#execute(request);
      if (mutation) {
        this.#lastError = null;
        this.#appliedAt = new Date().toISOString();
      }
      return result;
    } catch (error) {
      if (mutation && error instanceof RpcError) {
        this.#lastError = { ...error.protocolError, message: redact(error.message) };
        await this.#log(this.#lastError.message);
      }
      throw error;
    }
  };

  async #execute(request: RpcRequest): Promise<HandlerResult> {
    if (this.#closing)
      throw new RpcError({
        kind: 'shuttingDown',
        code: 'daemon_shutting_down',
        message: 'The daemon is shutting down.',
        guidance: null,
        retryable: true,
        requestId: null,
      });
    const basic = { accepted: true, message: 'Operation completed.' };
    switch (request.method) {
      case 'query-state-request':
        return { ...basic, snapshot: this.snapshot() };
      case 'query-logs-request': {
        const stream = request.params.stream;
        const limit = request.params.limit ?? 100;
        if (limit < 1 || limit > 200) reject('invalidInput', 'Log limit must be from 1 to 200.');
        const canonical = this.#stream(stream);
        const entries = await this.options.queryLogs(canonical, limit);
        return {
          ...basic,
          stream: canonical,
          entries,
          streamStatus: entries.length ? 'active' : 'empty',
        };
      }
      case 'shutdown-daemon-request':
        this.#closing = true;
        // Let IPC flush the response before stopping admission and the owned runtime.
        setTimeout(() => this.options.shutdown(), 30);
        return basic;
      case 'heartbeat-entrypoint-request': {
        const registration = this.#owned(
          request.params.registrationId,
          request.params.shimSessionNonce,
        );
        this.#heartbeats.set(registration.registrationId, Date.now());
        return basic;
      }
      case 'register-entrypoint-request': {
        let id = '';
        const result = await this.options.transactions.submit(async (committed) => {
          const supplied = registrationSchema.parse(request.params.registration);
          const nonce = supplied.entrypointInstance.shimSessionNonce;
          if (
            !nonce ||
            nonce.length > 256 ||
            supplied.ownerProcess.shimSessionNonce !== nonce ||
            supplied.registrationId !== supplied.entrypointInstance.instanceId
          )
            reject('invalidInput', 'Registration requires one matching session nonce.');
          if (
            !isAbsolute(supplied.sourceWorkingDirectory.raw) ||
            !isAbsolute(supplied.sourceConfigPath.raw)
          )
            reject('invalidInput', 'Registration source paths must be absolute.');
          const registration = {
            ...supplied,
            sourceWorkingDirectory: {
              ...supplied.sourceWorkingDirectory,
              canonical: await realpath(supplied.sourceWorkingDirectory.raw),
            },
            sourceConfigPath: {
              ...supplied.sourceConfigPath,
              canonical: await realpath(supplied.sourceConfigPath.raw),
            },
          };
          const key = projectKey(registration);
          id = supplied.registrationId;
          const previous = committed.registrations.find((entry) => projectKey(entry) === key);
          if (
            (previous &&
              (previous.entrypointInstance.shimSessionNonce !== nonce ||
                previous.registrationId !== id)) ||
            committed.registrations.some(
              (entry) => entry.registrationId === id && projectKey(entry) !== key,
            )
          )
            reject('conflict', 'This project already belongs to a live shim session.');
          const adapted = unwrap(await this.options.adapt(registration));
          const domains = extractRegisteredDomains(adapted);
          if (!domains.ok)
            reject('configuration', 'Cannot read project domains from its adapted configuration.');
          const existing = committed.desiredState.projects.find(
            (entry) => entry.projectKey === key,
          );
          const desired = {
            projectKey: key,
            sourceWorkingDirectory: registration.sourceWorkingDirectory.canonical,
            sourceConfigPath: registration.sourceConfigPath.canonical,
            enabled: existing?.enabled ?? true,
            domains: domains.value.map((domain) => ({
              ...domain,
              enabled:
                existing?.domains.find((entry) => entry.canonicalDomain === domain.canonicalDomain)
                  ?.enabled ?? true,
            })),
          };
          const now = new Date().toISOString();
          registration.registrationId = id;
          registration.createdAtUtc = previous?.createdAtUtc ?? now;
          registration.lastHeartbeatUtc = now;
          registration.logStream = {
            streamId: `entrypoint-${id}`,
            domainKey: null,
            channel: 'caddy',
          };
          registration.registeredDomains = desired.domains.map((domain) => ({
            name: { raw: domain.canonicalDomain, canonical: domain.canonicalDomain },
            activationState: domain.enabled ? 'active' : 'inactive',
            upstream: domain.upstream ?? undefined,
            logStream: {
              streamId: `domain-${domain.canonicalDomain}`,
              domainKey: domain.canonicalDomain,
              channel: 'caddy',
            },
          }));
          registration.activationState = desired.enabled ? 'active' : 'inactive';
          const desiredState = {
            projects: [
              ...committed.desiredState.projects.filter((entry) => entry.projectKey !== key),
              desired,
            ],
          };
          return this.#prepare(
            [
              ...committed.registrations.filter((entry) => entry.registrationId !== id),
              registration,
            ],
            desiredState,
            new Map([[id, adapted]]),
          );
        });
        unwrap(result);
        this.#heartbeats.set(id, Date.now());
        await this.#log('Project registered.');
        return { ...basic, registrationId: id };
      }
      case 'unregister-entrypoint-request': {
        unwrap(
          await this.options.transactions.submit(async (committed) => {
            this.#owned(request.params.registrationId, request.params.shimSessionNonce, committed);
            return this.#prepare(
              committed.registrations.filter(
                (entry) => entry.registrationId !== request.params.registrationId,
              ),
              committed.desiredState,
            );
          }),
        );
        this.#heartbeats.delete(request.params.registrationId);
        await this.#log('Project unregistered.');
        return basic;
      }
      case 'set-entrypoint-enabled-request':
      case 'set-domain-enabled-request': {
        const params = request.params;
        unwrap(
          await this.options.transactions.submit(async (committed) => {
            const registration = committed.registrations.find(
              (entry) => entry.registrationId === params.registrationId,
            );
            if (!registration) reject('invalidInput', 'Project was not found.');
            if (
              request.method === 'set-entrypoint-enabled-request' &&
              request.params.shimSessionNonce !== null
            )
              this.#owned(params.registrationId, request.params.shimSessionNonce, committed);
            const registrations = structuredClone(
              committed.registrations,
            ) as EntrypointRegistration[];
            const desiredState = structuredClone(committed.desiredState) as {
              projects: DesiredState['projects'][number][];
            };
            const desired = desiredState.projects.find(
              (entry) => entry.projectKey === projectKey(registration),
            )!;
            const target = registrations.find(
              (entry) => entry.registrationId === params.registrationId,
            )!;
            if (request.method === 'set-entrypoint-enabled-request') {
              desiredState.projects = desiredState.projects.map((entry) =>
                entry === desired ? { ...entry, enabled: request.params.enabled } : entry,
              );
              target.activationState = request.params.enabled ? 'active' : 'inactive';
            } else {
              const domain = canonicalizeDomain(request.params.domainKey);
              if (!target.registeredDomains.some((entry) => entry.name.canonical === domain))
                reject('invalidInput', 'Domain was not found.');
              desiredState.projects = desiredState.projects.map((entry) =>
                entry === desired
                  ? {
                      ...entry,
                      domains: entry.domains.map((item) =>
                        item.canonicalDomain === domain
                          ? { ...item, enabled: request.params.enabled }
                          : item,
                      ),
                    }
                  : entry,
              );
              for (const entry of target.registeredDomains)
                if (entry.name.canonical === domain)
                  entry.activationState = request.params.enabled ? 'active' : 'inactive';
            }
            return this.#prepare(registrations, desiredState);
          }),
        );
        return basic;
      }
    }
  }

  snapshot(): StateSnapshot {
    const committed = this.options.transactions.readCommitted();
    const fence = this.options.transactions.readFence() ?? this.#lastError;
    return {
      capturedAtUtc: new Date().toISOString(),
      registrations: committed.registrations.map((entry) => ({
        ...structuredClone(entry),
        lastHeartbeatUtc: new Date(
          this.#heartbeats.get(entry.registrationId) ?? Date.parse(entry.lastHeartbeatUtc),
        ).toISOString(),
      })),
      runtime: this.options.runtime(),
      config: {
        status: fence ? 'failed' : committed.config ? 'applied' : 'idle',
        lastAttemptedAtUtc: this.#attemptedAt,
        lastSuccessfulReloadAtUtc: this.#appliedAt,
        effectiveConfigHash: committed.config?.effectiveConfigHash ?? null,
        diagnostics: fence
          ? [{ code: fence.code, message: fence.message, domainKey: null, sourceConfigPaths: [] }]
          : [],
      },
      storage: structuredClone(this.options.storage),
    };
  }

  async expire(cutoff = Date.now() - 15000): Promise<void> {
    const stale = this.options.transactions
      .readCommitted()
      .registrations.filter(
        (entry) =>
          (this.#heartbeats.get(entry.registrationId) ?? Date.parse(entry.lastHeartbeatUtc)) <
          cutoff,
      );
    if (!stale.length || this.#closing) return;
    unwrap(
      await this.options.transactions.submit(async (committed) =>
        this.#prepare(
          committed.registrations.filter(
            (entry) =>
              (this.#heartbeats.get(entry.registrationId) ?? Date.parse(entry.lastHeartbeatUtc)) >=
              cutoff,
          ),
          committed.desiredState,
        ),
      ),
    );
    for (const entry of stale)
      if (
        !this.options.transactions
          .readCommitted()
          .registrations.some((current) => current.registrationId === entry.registrationId)
      )
        this.#heartbeats.delete(entry.registrationId);
  }
  close(): void {
    this.#closing = true;
  }

  #owned(
    id: string,
    nonce: string,
    committed = this.options.transactions.readCommitted(),
  ): EntrypointRegistration {
    const registration = committed.registrations.find((entry) => entry.registrationId === id);
    if (!registration || registration.entrypointInstance.shimSessionNonce !== nonce)
      reject('staleInstance', 'This shim no longer owns the project session.');
    return registration;
  }
  async #prepare(
    registrations: readonly EntrypointRegistration[],
    desiredState: DesiredState,
    prepared = new Map<string, CaddyConfig>(),
  ): Promise<PortResult<ConfigurationSnapshot>> {
    const inputs: PreparedCompositionInput[] = [];
    for (const registration of registrations) {
      const config = activeDomains(registration).length
        ? (prepared.get(registration.registrationId) ??
          unwrap(await this.options.adapt(registration)))
        : undefined;
      inputs.push({ registration, ...(config ? { config } : {}) });
    }
    const plan = composeRoutePlan(inputs);
    if (!plan.ok) reject('conflict', 'Project domains conflict or cannot be composed.');
    const config = plan.value.tlsSubjects.length
      ? unwrap(await this.options.secure(plan.value))
      : null;
    return { ok: true, value: { registrations, desiredState, config } };
  }
  #stream(input: LogInput['stream']): LogInput['stream'] {
    if (
      input.streamId === runtimeLogStream.streamId &&
      input.domainKey === null &&
      input.channel === runtimeLogStream.channel
    )
      return runtimeLogStream;
    for (const registration of this.options.transactions.readCommitted().registrations) {
      for (const stream of [
        registration.logStream,
        ...registration.registeredDomains.map((domain) => domain.logStream),
      ]) {
        if (
          stream.streamId === input.streamId &&
          stream.domainKey === input.domainKey &&
          stream.channel === input.channel
        )
          return stream;
      }
    }
    reject('invalidInput', 'Select one current project, domain or runtime log stream.');
  }
  async #log(rawMessage: string): Promise<void> {
    try {
      await this.options.log({
        timestampUtc: new Date().toISOString(),
        severity: 'info',
        stream: runtimeLogStream,
        attributionKind: 'runtimeControl',
        entryKind: 'lifecycle',
        rawMessage,
        domainKey: null,
        sourceRegistrationId: null,
        sourceInstanceId: null,
        operation: null,
      });
    } catch {
      if (this.options.storage)
        this.options.storage.diagnostics = [
          {
            code: 'log_persistence_failed',
            message: 'A diagnostic could not be retained; committed routes are unchanged.',
            operation: null,
          },
        ];
    }
  }
}
