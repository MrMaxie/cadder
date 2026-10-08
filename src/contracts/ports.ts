import type { ProtocolError } from '../protocol/errors.ts';
import type { RpcParams, RpcResult } from '../protocol/rpc.ts';

/** The identity used to protect one installation's runtime boundary. */
export type RuntimeOwner = Readonly<{
  id: string;
  uid?: number;
  elevated: boolean;
}>;

/**
 * A complete adapted Caddy configuration candidate.
 * CADDY-005 bounds the JSON body to 32 MiB before this port is called.
 */
export type CaddyConfig = Readonly<{
  effectiveConfigHash: string;
  adaptedConfig: Readonly<{
    format: 'json';
    body: string;
  }>;
}>;

export type CaddyDiagnostic = Readonly<{
  code: string;
  message: string;
  domainKey: string | null;
}>;

export type CaddyValidation = Readonly<{
  effectiveConfigHash: string;
  diagnostics: readonly CaddyDiagnostic[];
}>;

export type CaddyApplyReceipt = Readonly<{
  effectiveConfigHash: string;
}>;

/** Stable project/domain intent; it contains no session, process, or live lease data. */
export type DesiredState = Readonly<{
  projects: readonly Readonly<{
    projectKey: string;
    sourceWorkingDirectory: string;
    sourceConfigPath: string;
    enabled: boolean;
    domains: readonly Readonly<{
      canonicalDomain: string;
      upstream: string | null;
      enabled: boolean;
    }>[];
  }>[];
}>;

export type ProtectedPathKind = 'directory' | 'file';

/** A discriminated result shared by every port without a runtime result engine. */
export type PortResult<T> =
  | Readonly<{ ok: true; value: T }>
  | Readonly<{ ok: false; error: ProtocolError }>;

export type QueryLogsResult = RpcResult<'query-logs-request'>;
export type QueryStateResult = RpcResult<'query-state-request'>;
export type ProjectActivationResult = RpcResult<'set-entrypoint-enabled-request'>;
export type DomainActivationResult = RpcResult<'set-domain-enabled-request'>;
export type ShutdownResult = RpcResult<'shutdown-daemon-request'>;

/** Caddy configuration validation and application; process lifecycle is out of scope. */
export interface CaddyPort {
  validate(config: CaddyConfig): Promise<PortResult<CaddyValidation>>;
  apply(config: CaddyConfig): Promise<PortResult<CaddyApplyReceipt>>;
}

/** Current-owner lookup and fail-closed protection checks for runtime paths. */
export interface PlatformPort {
  currentOwner(explicitUid?: number): Promise<PortResult<RuntimeOwner>>;
  assertProtected(
    path: string,
    owner: RuntimeOwner,
    kind: ProtectedPathKind,
  ): Promise<PortResult<void>>;
}

/** Durable stable intent only; live registration/session/process state is not persisted here. */
export interface StoragePort {
  loadDesiredState(): Promise<PortResult<DesiredState>>;
  persistDesiredState(state: DesiredState): Promise<PortResult<void>>;
}

/** The shared daemon-backed client surface used by CLI and TUI. */
export interface ClientServicePort {
  queryState(): Promise<PortResult<QueryStateResult>>;
  queryLogs(params: RpcParams<'query-logs-request'>): Promise<PortResult<QueryLogsResult>>;
  setProjectEnabled(
    params: RpcParams<'set-entrypoint-enabled-request'>,
  ): Promise<PortResult<ProjectActivationResult>>;
  setDomainEnabled(
    params: RpcParams<'set-domain-enabled-request'>,
  ): Promise<PortResult<DomainActivationResult>>;
  shutdown(): Promise<PortResult<ShutdownResult>>;
}
