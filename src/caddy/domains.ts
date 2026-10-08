import { domainToASCII } from 'node:url';
import type { CaddyConfig } from '../contracts/ports.ts';
import type { EntrypointRegistration } from '../protocol/dto.ts';
import { assertCandidate } from './preparation.ts';

export type JsonValue = null | boolean | number | string | JsonValue[] | JsonObject;
export type JsonObject = { [key: string]: JsonValue };
export type CompositionRegistration = Pick<
  EntrypointRegistration,
  'registrationId' | 'activationState' | 'registeredDomains' | 'sourceConfigPath'
>;
export type CompositionDiagnostic = Readonly<{
  code: 'duplicate-registration' | 'domain-conflict' | 'prepared-input-invalid';
  message: string;
  domainKey: string | null;
  sourceConfigPaths: readonly string[];
}>;
export type CompositionResult<T> =
  | Readonly<{ ok: true; value: T }>
  | Readonly<{ ok: false; diagnostics: readonly CompositionDiagnostic[] }>;
export type ExtractedDomain = Readonly<{ canonicalDomain: string; upstream: string | null }>;
export const maxCompositionDiagnostics = 64;
const maxDiagnosticText = 4096;

export function compareText(left: string, right: string): number {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

/** Match the retained literal-domain IDNA boundary, not URL host parsing. */
export function canonicalizeDomain(raw: string): string {
  const domain = raw.trim().replace(/\.+$/, '');
  const fallback = domain.replace(/[A-Z]/g, (letter) => letter.toLowerCase());
  // These characters would invoke URL decoding, separators or bracket-host parsing.
  if (
    /[\s%/:\\@#?]/u.test(domain) ||
    domain.includes('[') ||
    domain.includes(']') ||
    [...domain].some((character) => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127)
  )
    return fallback;
  const suffix = '.cadder-idna';
  const mapped = domainToASCII(domain + suffix);
  return mapped.endsWith(suffix) ? mapped.slice(0, -suffix.length) : fallback;
}

export function isEnabled(state: CompositionRegistration['activationState']): boolean {
  return state === 'registered' || state === 'activating' || state === 'active';
}

export function activeDomains(registration: CompositionRegistration): readonly string[] {
  if (!isEnabled(registration.activationState)) return [];
  return [
    ...new Set(
      registration.registeredDomains
        .filter((domain) => isEnabled(domain.activationState))
        .map((domain) => canonicalizeDomain(domain.name.canonical)),
    ),
  ].sort(compareText);
}

/** Bound diagnostic payloads even when registration metadata is caller-controlled. */
export function diagnostic(
  code: CompositionDiagnostic['code'],
  message: string,
  registrations: readonly CompositionRegistration[],
  domainKey: string | null = null,
): CompositionDiagnostic {
  const paths = [...new Set(registrations.map((registration) => registration.sourceConfigPath.raw))]
    .sort(compareText)
    .slice(0, maxCompositionDiagnostics);
  return {
    code,
    message: message.slice(0, maxDiagnosticText),
    domainKey: domainKey?.slice(0, maxDiagnosticText) ?? null,
    sourceConfigPaths: paths.map((path) => path.slice(0, maxDiagnosticText)),
  };
}

/** Duplicate IDs are ambiguous even when inactive; domain ownership uses both states. */
export function checkDomainOwnership(
  registrations: readonly CompositionRegistration[],
): CompositionResult<ReadonlyMap<string, readonly string[]>> {
  const byId = new Map<string, CompositionRegistration[]>();
  for (const registration of registrations) {
    const group = byId.get(registration.registrationId) ?? [];
    group.push(registration);
    byId.set(registration.registrationId, group);
  }
  const diagnostics: CompositionDiagnostic[] = [];
  const active = new Map<string, readonly string[]>();
  const owners = new Map<string, CompositionRegistration[]>();
  for (const id of [...byId.keys()].sort(compareText)) {
    const group = byId.get(id)!;
    if (group.length > 1) {
      diagnostics.push(
        diagnostic('duplicate-registration', 'Registration ID occurs more than once.', group),
      );
      continue;
    }
    const registration = group[0]!;
    const hosts = activeDomains(registration);
    active.set(id, hosts);
    for (const host of hosts) {
      const group = owners.get(host) ?? [];
      group.push(registration);
      owners.set(host, group);
    }
  }
  for (const host of [...owners.keys()].sort(compareText)) {
    const group = owners.get(host)!;
    if (group.length > 1)
      diagnostics.push(
        diagnostic(
          'domain-conflict',
          'Domain is registered by multiple enabled entrypoints.',
          group,
          host,
        ),
      );
  }
  return diagnostics.length > 0
    ? { ok: false, diagnostics: diagnostics.slice(0, maxCompositionDiagnostics) }
    : { ok: true, value: active };
}

function isObject(value: JsonValue | undefined): value is JsonObject {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function object(value: JsonValue | undefined): JsonObject {
  if (!isObject(value)) throw new Error('Expected object.');
  return value;
}

function optionalObject(parent: JsonObject, key: string): JsonObject {
  return parent[key] === undefined ? {} : object(parent[key]);
}

function validateRoutes(value: JsonValue): JsonObject[] {
  if (!Array.isArray(value)) throw new Error('Expected routes.');
  return value.map((entry) => {
    const route = object(entry);
    if (route.match !== undefined) {
      if (!Array.isArray(route.match)) throw new Error('Expected matchers.');
      for (const entry of route.match) {
        const matcher = object(entry);
        if (
          matcher.host !== undefined &&
          (!Array.isArray(matcher.host) || matcher.host.some((host) => typeof host !== 'string'))
        )
          throw new Error('Expected host strings.');
      }
    }
    if (route.handle !== undefined) {
      if (!Array.isArray(route.handle)) throw new Error('Expected handlers.');
      for (const entry of route.handle) {
        const handler = object(entry);
        if (handler.handler === 'subroute' && handler.routes !== undefined)
          validateRoutes(handler.routes);
      }
    }
    return route;
  });
}

/** Parse a complete integrity-checked carrier; never reinterpret opaque handler payloads. */
export function readPreparedRoutes(
  config: CaddyConfig | undefined,
  registrations: readonly CompositionRegistration[] = [],
): CompositionResult<readonly JsonObject[]> {
  try {
    if (config === undefined) throw new Error('Missing prepared input.');
    assertCandidate(config);
    const root = object(JSON.parse(config.adaptedConfig.body) as JsonValue);
    const servers = optionalObject(optionalObject(optionalObject(root, 'apps'), 'http'), 'servers');
    const routes: JsonObject[] = [];
    for (const name of Object.keys(servers).sort(compareText)) {
      const server = object(servers[name]);
      if (server.routes !== undefined) routes.push(...validateRoutes(server.routes));
    }
    return { ok: true, value: routes };
  } catch {
    return {
      ok: false,
      diagnostics: [
        diagnostic(
          'prepared-input-invalid',
          'Expected a complete, bounded, integrity-checked adapted HTTP route tree.',
          registrations,
        ),
      ],
    };
  }
}

/** Visit only Caddy route positions; handler properties named host/routes remain opaque. */
function nestedRoutes(route: JsonObject): JsonObject[] {
  const routes: JsonObject[] = [];
  for (const handler of (route.handle ?? []) as JsonObject[]) {
    if (handler.handler === 'subroute') routes.push(...((handler.routes ?? []) as JsonObject[]));
  }
  return routes;
}

function collectHosts(route: JsonObject, hosts: Set<string>): void {
  for (const matcher of (route.match ?? []) as JsonObject[]) {
    for (const host of (matcher.host ?? []) as string[]) hosts.add(canonicalizeDomain(host));
  }
  for (const child of nestedRoutes(route)) collectHosts(child, hosts);
}

function firstUpstream(route: JsonObject): string | null {
  for (const handler of (route.handle ?? []) as JsonObject[]) {
    if (handler.handler === 'reverse_proxy' && Array.isArray(handler.upstreams)) {
      for (const upstream of handler.upstreams) {
        if (isObject(upstream) && typeof upstream.dial === 'string' && upstream.dial.trim() !== '')
          return upstream.dial;
      }
    }
    if (handler.handler === 'subroute') {
      for (const child of (handler.routes ?? []) as JsonObject[]) {
        const upstream = firstUpstream(child);
        if (upstream !== null) return upstream;
      }
    }
  }
  return null;
}

/** Retain the first upstream per top-level route and first occurrence of each domain. */
export function extractRegisteredDomains(
  config: CaddyConfig,
): CompositionResult<readonly ExtractedDomain[]> {
  const parsed = readPreparedRoutes(config);
  if (!parsed.ok) return parsed;
  const domains = new Map<string, ExtractedDomain>();
  for (const route of parsed.value) {
    const hosts = new Set<string>();
    collectHosts(route, hosts);
    const upstream = firstUpstream(route);
    for (const host of hosts) {
      if (!domains.has(host)) domains.set(host, { canonicalDomain: host, upstream });
    }
  }
  return {
    ok: true,
    value: [...domains.values()].sort((left, right) =>
      compareText(left.canonicalDomain, right.canonicalDomain),
    ),
  };
}
