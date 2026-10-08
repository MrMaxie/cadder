import type { CaddyConfig } from '../contracts/ports.ts';
import {
  canonicalizeDomain,
  checkDomainOwnership,
  compareText,
  diagnostic,
  maxCompositionDiagnostics,
  readPreparedRoutes,
  type CompositionDiagnostic,
  type CompositionRegistration,
  type CompositionResult,
  type JsonObject,
  type JsonValue,
} from './domains.ts';

export type PreparedCompositionInput = Readonly<{
  registration: CompositionRegistration;
  config?: CaddyConfig;
}>;
export type RouteServer = Readonly<{
  listen: readonly string[];
  routes: readonly JsonObject[];
}>;
/** Not a runnable CaddyConfig: secure admin/TLS assembly and apply belong to later work. */
export type CaddyRoutePlan = Readonly<{
  kind: 'caddy-route-plan';
  servers: Readonly<{ cadder_http: RouteServer; cadder_https: RouteServer }>;
  tlsSubjects: readonly string[];
}>;

/** Mutate only a private parsed tree, and only actual host matcher/subroute positions. */
function filterRoute(route: JsonObject, hosts: ReadonlySet<string>): JsonObject | null {
  if (Array.isArray(route.match) && route.match.length > 0) {
    const matchers: JsonObject[] = [];
    for (const entry of route.match) {
      const matcher = entry as JsonObject;
      if (Array.isArray(matcher.host)) {
        const retained = [
          ...new Set(
            (matcher.host as string[]).map(canonicalizeDomain).filter((host) => hosts.has(host)),
          ),
        ];
        // An impossible matcher is removed as an OR alternative, never made hostless.
        if (retained.length === 0) continue;
        matcher.host = retained;
      }
      matchers.push(matcher);
    }
    if (matchers.length === 0) return null;
    route.match = matchers;
  }
  if (Array.isArray(route.handle) && route.handle.length > 0) {
    const handlers: JsonObject[] = [];
    for (const entry of route.handle) {
      const handler = entry as JsonObject;
      if (
        handler.handler === 'subroute' &&
        Array.isArray(handler.routes) &&
        handler.routes.length > 0
      ) {
        const children = filterRoutes(handler.routes as JsonObject[], hosts);
        if (children.length === 0) continue;
        handler.routes = children;
      }
      handlers.push(handler);
    }
    // An empty terminal route still stops the chain; keep its position and matchers.
    if (handlers.length === 0 && route.terminal !== true) return null;
    route.handle = handlers;
  }
  return route;
}

function filterRoutes(routes: readonly JsonObject[], hosts: ReadonlySet<string>): JsonObject[] {
  const retained: JsonObject[] = [];
  for (const route of routes) {
    const filtered = filterRoute(route, hosts);
    if (filtered !== null) retained.push(filtered);
  }
  return retained;
}

function namespaceHttpIds(value: JsonValue): void {
  if (Array.isArray(value)) {
    for (const child of value) namespaceHttpIds(child);
  } else if (value !== null && typeof value === 'object') {
    if (typeof value['@id'] === 'string') value['@id'] = `http_${value['@id']}`;
    for (const child of Object.values(value)) namespaceHttpIds(child);
  }
}

/** Stable registration-ID order, sorted server keys, original project route/handler order. */
export function composeRoutePlan(
  inputs: readonly PreparedCompositionInput[],
): CompositionResult<CaddyRoutePlan> {
  const ownership = checkDomainOwnership(inputs.map((input) => input.registration));
  if (!ownership.ok) return ownership;
  const diagnostics: CompositionDiagnostic[] = [];
  const routes: JsonObject[] = [];
  const httpRoutes: JsonObject[] = [];
  const tlsSubjects = new Set<string>();
  const ordered = [...inputs].sort((left, right) =>
    compareText(left.registration.registrationId, right.registration.registrationId),
  );
  for (const input of ordered) {
    const hosts = ownership.value.get(input.registration.registrationId)!;
    if (hosts.length === 0) continue;
    const parsed = readPreparedRoutes(input.config, [input.registration]);
    if (!parsed.ok) {
      diagnostics.push(...parsed.diagnostics);
      continue;
    }
    try {
      // The carrier is a string: JSON parsing already produced a private clone.
      const retained = filterRoutes(parsed.value, new Set(hosts));
      for (const host of hosts) tlsSubjects.add(host);
      if (retained.length === 0) continue;
      const guard: JsonObject = {
        match: [{ host: [...hosts] }],
        handle: [{ handler: 'subroute', routes: retained }],
        terminal: true,
      };
      const httpGuard = structuredClone(guard);
      namespaceHttpIds(httpGuard);
      routes.push(guard);
      httpRoutes.push(httpGuard);
    } catch {
      // Even complete bounded JSON may exceed the native clone/traversal capacity.
      diagnostics.push(
        diagnostic(
          'prepared-input-invalid',
          'Cannot safely clone and filter the complete adapted route tree.',
          [input.registration],
        ),
      );
    }
  }
  if (diagnostics.length > 0)
    return { ok: false, diagnostics: diagnostics.slice(0, maxCompositionDiagnostics) };
  return {
    ok: true,
    value: {
      kind: 'caddy-route-plan',
      servers: {
        cadder_http: { listen: ['127.0.0.1:80', '[::1]:80'], routes: httpRoutes },
        cadder_https: { listen: ['127.0.0.1:443', '[::1]:443'], routes },
      },
      tlsSubjects: [...tlsSubjects].sort(compareText),
    },
  };
}
