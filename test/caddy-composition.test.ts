import { expect, it } from 'vitest';
import { composeRoutePlan, type CaddyRoutePlan } from '../src/caddy/composition.ts';
import type { CaddyConfig } from '../src/contracts/ports.ts';
import {
  activeDomains,
  canonicalizeDomain,
  checkDomainOwnership,
  extractRegisteredDomains,
  maxCompositionDiagnostics,
  readPreparedRoutes,
  type CompositionRegistration,
  type JsonObject,
} from '../src/caddy/domains.ts';
import { configHash, maxCaddyConfigurationBytes } from '../src/caddy/preparation.ts';

function candidate(value: unknown): CaddyConfig {
  return bodyCandidate(JSON.stringify(value));
}
function bodyCandidate(body: string): CaddyConfig {
  return { adaptedConfig: { format: 'json', body }, effectiveConfigHash: configHash(body) };
}
function configWithRoutes(routes: unknown[]): CaddyConfig {
  return candidate({ apps: { http: { servers: { project: { routes } } } } });
}
function registration(
  id: string,
  hosts: string[] = ['app.localhost'],
  state: CompositionRegistration['activationState'] = 'active',
): CompositionRegistration {
  return {
    registrationId: id,
    activationState: state,
    sourceConfigPath: { raw: `${id}/Caddyfile`, canonical: null },
    registeredDomains: hosts.map((host) => ({
      name: { raw: host, canonical: host },
      activationState: 'active',
      logStream: { streamId: id, domainKey: host, channel: 'domain' },
    })),
  };
}
const states = ['unknown', 'registered', 'activating', 'active', 'inactive', 'faulted'] as const;

it.each([
  ['  APP.Localhost... ', 'app.localhost'],
  ['BÜCHER.localhost', 'xn--bcher-kva.localhost'],
  ['XN--BCHER-KVA.localhost.', 'xn--bcher-kva.localhost'],
  ['faß.localhost', 'xn--fa-hia.localhost'],
  ['APP。LOCALHOST', 'app.localhost'],
  ['APP．LOCALHOST', 'app.localhost'],
  ['*.LOCALHOST', '*.localhost'],
  ['_SERVICE.LOCALHOST', '_service.localhost'],
  ['foo/bar', 'foo/bar'],
  ['FOO%2eBAR', 'foo%2ebar'],
  ['0x7f.1', '0x7f.1'],
  ['127.1', '127.1'],
  ['0177.0.0.1', '0177.0.0.1'],
  ['127.0.0.1', '127.0.0.1'],
  ['FOO:80', 'foo:80'],
  ['FOO\\BAR', 'foo\\bar'],
  ['FOO@BAR', 'foo@bar'],
  ['FOO#BAR', 'foo#bar'],
  ['FOO?BAR', 'foo?bar'],
  ['[ABCD::1]', '[abcd::1]'],
  ['Ä.BAD HOST', 'Ä.bad host'],
  ['Ä.\u0000BAD', 'Ä.\u0000bad'],
  ['Ä.\u0080BAD', 'Ä.\u0080bad'],
  ['Ä.xn--', 'Ä.xn--'],
  ['', ''],
])('canonicalizes literal domains without URL host reinterpretation: %s', (raw, expected) => {
  expect(canonicalizeDomain(raw)).toBe(expected);
});

it('respects all registration/domain states and deduplicates canonical spellings', () => {
  for (const projectState of states) {
    for (const domainState of states) {
      const r = registration('a', ['APP.localhost.', 'app.localhost'], projectState);
      r.registeredDomains.forEach((domain) => {
        domain.activationState = domainState;
      });
      const enabled = ['registered', 'activating', 'active'];
      expect(activeDomains(r)).toEqual(
        enabled.includes(projectState) && enabled.includes(domainState) ? ['app.localhost'] : [],
      );
    }
  }
});

it('rejects conflicting enabled owners deterministically, not duplicates within one owner', () => {
  const a = registration('a', ['BÜCHER.localhost.', 'xn--bcher-kva.localhost']);
  const b = registration('b', ['XN--BCHER-KVA.localhost']);
  const expected = {
    ok: false,
    diagnostics: [
      {
        code: 'domain-conflict',
        message: 'Domain is registered by multiple enabled entrypoints.',
        domainKey: 'xn--bcher-kva.localhost',
        sourceConfigPaths: ['a/Caddyfile', 'b/Caddyfile'],
      },
    ],
  };
  expect(checkDomainOwnership([a, b])).toEqual(expected);
  expect(checkDomainOwnership([b, a])).toEqual(expected);
  expect(checkDomainOwnership([a])).toMatchObject({ ok: true });
  b.activationState = 'inactive';
  expect(checkDomainOwnership([a, b])).toMatchObject({ ok: true });
  b.activationState = 'active';
  b.registeredDomains[0]!.activationState = 'faulted';
  expect(checkDomainOwnership([a, b])).toMatchObject({ ok: true });
});

it('rejects duplicate registration input even when inactive and bounds diagnostics', () => {
  const a = registration('a', [], 'inactive');
  expect(checkDomainOwnership([a, a])).toMatchObject({
    ok: false,
    diagnostics: [{ code: 'duplicate-registration', sourceConfigPaths: ['a/Caddyfile'] }],
  });
  const rows = Array.from({ length: 100 }, (_, i) => registration(String(i), ['x'.repeat(5000)]));
  rows.forEach((r) => {
    r.sourceConfigPath.raw = r.registrationId + 'x'.repeat(5000);
  });
  const result = checkDomainOwnership([...rows, ...rows]);
  expect(result.ok).toBe(false);
  if (result.ok) throw new Error('Expected diagnostics.');
  expect(result.diagnostics).toHaveLength(maxCompositionDiagnostics);
  expect(result.diagnostics.every((d) => d.sourceConfigPaths.every((p) => p.length <= 4096))).toBe(
    true,
  );
  const conflicts = checkDomainOwnership(rows);
  if (conflicts.ok) throw new Error('Expected conflict.');
  expect(conflicts.diagnostics[0]!.domainKey).toHaveLength(4096);
  expect(conflicts.diagnostics[0]!.sourceConfigPaths).toHaveLength(maxCompositionDiagnostics);
});

it('does not extract proxy metadata from opaque non-route handler payloads', () => {
  const routes = [
    {
      match: [{ host: ['app.localhost'] }],
      handle: [
        {
          handler: 'plugin',
          payload: {
            handler: 'reverse_proxy',
            upstreams: [{ dial: 'opaque:8080' }],
            host: ['opaque.localhost'],
          },
        },
      ],
    },
  ];
  const config = configWithRoutes(routes);
  expect(extractRegisteredDomains(config)).toEqual({
    ok: true,
    value: [{ canonicalDomain: 'app.localhost', upstream: null }],
  });
  expect(projectRoutes(plan([{ registration: registration('a'), config }]))).toEqual(routes);
});

it('extracts sorted matcher domains/first upstream only from HTTP route positions', () => {
  const input = candidate({
    admin: { host: ['admin.localhost'] },
    apps: {
      tls: { host: ['tls.localhost'] },
      http: {
        servers: {
          z: {
            routes: [
              {
                match: [{ host: ['APP.localhost.'] }],
                handle: [{ handler: 'reverse_proxy', upstreams: [{ dial: 'later:80' }] }],
              },
            ],
          },
          a: {
            routes: [
              {
                handle: [
                  {
                    handler: 'subroute',
                    routes: [
                      {
                        match: [{ host: ['BÜCHER.localhost', 'App.localhost'] }],
                        handle: [
                          {
                            handler: 'reverse_proxy',
                            host: ['opaque.localhost'],
                            routes: [{ match: [{ host: ['opaque-route.localhost'] }] }],
                            upstreams: [
                              null,
                              { dial: '  ' },
                              { dial: 1 },
                              { dial: '127.0.0.1:8080' },
                            ],
                          },
                        ],
                      },
                    ],
                  },
                ],
              },
              { match: [{ host: ['z.localhost'] }], handle: [] },
            ],
          },
        },
      },
    },
  });
  expect(extractRegisteredDomains(input)).toEqual({
    ok: true,
    value: [
      { canonicalDomain: 'app.localhost', upstream: '127.0.0.1:8080' },
      { canonicalDomain: 'xn--bcher-kva.localhost', upstream: '127.0.0.1:8080' },
      { canonicalDomain: 'z.localhost', upstream: null },
    ],
  });
  expect(extractRegisteredDomains(candidate({}))).toEqual({ ok: true, value: [] });
  expect(extractRegisteredDomains(bodyCandidate('{'))).toMatchObject({ ok: false });
});

it.each([
  undefined,
  { adaptedConfig: { format: 'json', body: '{}' } },
  { adaptedConfig: { format: 'caddyfile', body: '{}' }, effectiveConfigHash: configHash('{}') },
  { adaptedConfig: { format: 'json', body: 1 }, effectiveConfigHash: 'x' },
  { ...candidate({}), effectiveConfigHash: 'wrong' },
  bodyCandidate('{"apps":'),
  bodyCandidate('"\ud800"'),
  bodyCandidate(' '.repeat(maxCaddyConfigurationBytes + 1)),
  candidate(null),
  candidate([]),
  candidate({ apps: [] }),
  candidate({ apps: { http: [] } }),
  candidate({ apps: { http: { servers: [] } } }),
  candidate({ apps: { http: { servers: { a: null } } } }),
  candidate({ apps: { http: { servers: { a: { routes: {} } } } } }),
  configWithRoutes([null]),
  configWithRoutes([{ match: {} }]),
  configWithRoutes([{ match: [null] }]),
  configWithRoutes([{ match: [{ host: 'a' }] }]),
  configWithRoutes([{ match: [{ host: [1] }] }]),
  configWithRoutes([{ handle: {} }]),
  configWithRoutes([{ handle: [null] }]),
  configWithRoutes([{ handle: [{ handler: 'subroute', routes: {} }] }]),
])('rejects missing/malformed/hash-mismatched/overflow prepared carriers %#', (input) => {
  expect(readPreparedRoutes(input as CaddyConfig | undefined, [registration('a')])).toEqual({
    ok: false,
    diagnostics: [
      {
        code: 'prepared-input-invalid',
        domainKey: null,
        message: 'Expected a complete, bounded, integrity-checked adapted HTTP route tree.',
        sourceConfigPaths: ['a/Caddyfile'],
      },
    ],
  });
});

function plan(inputs: Parameters<typeof composeRoutePlan>[0]): CaddyRoutePlan {
  const result = composeRoutePlan(inputs);
  if (!result.ok) throw new Error(JSON.stringify(result.diagnostics));
  return result.value;
}
function projectRoutes(value: CaddyRoutePlan, projectIndex = 0): JsonObject[] {
  const guard = value.servers.cadder_https.routes[projectIndex]!;
  return (guard.handle as JsonObject[])[0]!.routes as JsonObject[];
}
function response(body: string, hosts?: string[]): JsonObject {
  return {
    ...(hosts === undefined ? {} : { match: [{ host: hosts }] }),
    handle: [{ handler: 'static_response', body }],
  };
}

it('composes multiple projects deterministically behind active guards with exact owned listeners', () => {
  const a = {
    registration: registration('a', ['BÜCHER.localhost.', 'app.localhost']),
    config: configWithRoutes([response('a1', ['APP.LOCALHOST.']), response('a2')]),
  };
  const z = {
    registration: registration('z', ['z.localhost']),
    config: configWithRoutes([response('z', ['z.localhost'])]),
  };
  const value = plan([z, a]);
  expect(plan([a, z])).toEqual(value);
  expect(value.kind).toBe('caddy-route-plan');
  expect(value.servers.cadder_http.listen).toEqual(['127.0.0.1:80', '[::1]:80']);
  expect(value.servers.cadder_https.listen).toEqual(['127.0.0.1:443', '[::1]:443']);
  expect(value.tlsSubjects).toEqual(['app.localhost', 'xn--bcher-kva.localhost', 'z.localhost']);
  expect(value.servers.cadder_https.routes.map((r) => r.match)).toEqual([
    [{ host: ['app.localhost', 'xn--bcher-kva.localhost'] }],
    [{ host: ['z.localhost'] }],
  ]);
  expect(projectRoutes(value)).toEqual([response('a1', ['app.localhost']), response('a2')]);
  expect(projectRoutes(value, 1)).toEqual([response('z', ['z.localhost'])]);
  expect(value.servers.cadder_https.routes.every((route) => route.terminal === true)).toBe(true);
});

it('composition honors every activation combination and ignores unselected missing carriers', () => {
  for (const projectState of states) {
    for (const domainState of states) {
      const r = registration('a', ['app.localhost'], projectState);
      r.registeredDomains[0]!.activationState = domainState;
      const enabled =
        ['registered', 'activating', 'active'].includes(projectState) &&
        ['registered', 'activating', 'active'].includes(domainState);
      const value = plan([
        {
          registration: r,
          ...(enabled ? { config: configWithRoutes([response('active', ['app.localhost'])]) } : {}),
        },
      ]);
      expect(value.servers.cadder_https.routes).toHaveLength(enabled ? 1 : 0);
      expect(value.tlsSubjects).toEqual(enabled ? ['app.localhost'] : []);
    }
  }
  expect(plan([{ registration: registration('empty', []) }]).servers.cadder_https.routes).toEqual(
    [],
  );
  expect(plan([]).tlsSubjects).toEqual([]);
});

it('prunes disabled-only nested branches/OR alternatives, preserves nested and separate hostless siblings', () => {
  const r = registration('a', ['app.localhost', 'disabled.localhost']);
  r.registeredDomains[1]!.activationState = 'inactive';
  const opaque = {
    handler: 'plugin',
    host: ['disabled.localhost', 42],
    routes: [{ match: [{ host: ['disabled.localhost'] }] }],
    nested: { host: { arbitrary: true } },
    dial: 'opaque',
  };
  const nested = {
    handle: [
      {
        handler: 'subroute',
        routes: [
          response('mixed', ['APP.localhost.', 'disabled.localhost']),
          response('disabled', ['disabled.localhost']),
          {
            handle: [
              { handler: 'subroute', routes: [response('deep disabled', ['disabled.localhost'])] },
            ],
          },
          response('nested hostless'),
          {
            match: [
              { host: ['disabled.localhost'], path: ['/disabled/*'] },
              { host: ['app.localhost'], path: ['/active/*'] },
              { path: ['/shared/*'] },
            ],
            handle: [opaque],
          },
        ],
      },
    ],
  };
  const value = plan([
    {
      registration: r,
      config: configWithRoutes([
        nested,
        response('top disabled', ['disabled.localhost']),
        response('separate hostless'),
        { match: [{ host: [] }], handle: [opaque] },
      ]),
    },
  ]);
  expect(projectRoutes(value)).toEqual([
    {
      handle: [
        {
          handler: 'subroute',
          routes: [
            response('mixed', ['app.localhost']),
            response('nested hostless'),
            {
              match: [{ host: ['app.localhost'], path: ['/active/*'] }, { path: ['/shared/*'] }],
              handle: [opaque],
            },
          ],
        },
      ],
    },
    response('separate hostless'),
  ]);
  expect(value.servers.cadder_https.routes[0]!.match).toEqual([{ host: ['app.localhost'] }]);
  expect(value.tlsSubjects).toEqual(['app.localhost']);
});

it('retains terminal chain position after disabled subroutes are pruned', () => {
  for (const match of [undefined, [{ host: ['app.localhost'] }]]) {
    const terminalRoute = {
      ...(match === undefined ? {} : { match }),
      handle: [{ handler: 'subroute', routes: [response('disabled', ['disabled.localhost'])] }],
      terminal: true,
    };
    const later = response('later', ['app.localhost']);
    const config = configWithRoutes([terminalRoute, later]);
    const before = config.adaptedConfig.body;
    const value = plan([{ registration: registration('a'), config }]);
    expect(projectRoutes(value)).toEqual([{ ...terminalRoute, handle: [] }, later]);
    expect(value.servers.cadder_https.routes[0]!.match).toEqual([{ host: ['app.localhost'] }]);
    expect(config.adaptedConfig.body).toBe(before);
  }
});

it('retains complete hostless handler payloads, multiple handlers and empty/optional subroutes', () => {
  const routes = [
    { match: [], handle: [{ handler: 'subroute', routes: [] }, { handler: 'subroute' }] },
    {
      handle: [
        { handler: 'subroute', routes: [response('disabled', ['disabled.localhost'])] },
        { handler: 'headers', response: { set: { Host: ['unchanged'] } } },
      ],
    },
    { arbitrary: null, host: 'opaque top-level', payload: [true, 123, 'keep'], handle: [] },
  ];
  const value = plan([{ registration: registration('a'), config: configWithRoutes(routes) }]);
  expect(projectRoutes(value)).toEqual([
    routes[0],
    { handle: [{ handler: 'headers', response: { set: { Host: ['unchanged'] } } }] },
    routes[2],
  ]);
});

it('ignores root admin/TLS/server settings, preserves JSON and namespaces only HTTP ID copies', () => {
  const routes = [
    {
      '@id': 'route',
      match: [{ host: ['app.localhost'] }],
      handle: [
        {
          '@id': 'proxy',
          handler: 'reverse_proxy',
          upstreams: [{ dial: '127.0.0.1:8080' }],
          host: ['opaque.localhost'],
          tls: { arbitrary: true },
          opaque: [
            { '@id': 'payload', host: ['not-a-domain'], number: 42, flag: false, null: null },
          ],
          '@other': 'unchanged',
        },
      ],
      terminal: false,
    },
  ];
  const config = candidate({
    admin: { listen: '0.0.0.0:2019' },
    apps: {
      tls: {
        automation: { policies: [{ subjects: ['evil.example'], issuers: [{ module: 'acme' }] }] },
      },
      http: {
        servers: {
          project: { listen: [':8080'], tls_connection_policies: [{ injected: true }], routes },
        },
      },
    },
  });
  const input = { registration: registration('a'), config };
  const before = structuredClone(input);
  const value = plan([input]);
  const snapshot = structuredClone(value);
  expect(input).toEqual(before);
  expect(Object.keys(value).sort()).toEqual(['kind', 'servers', 'tlsSubjects']);
  expect(Object.keys(value.servers.cadder_https).sort()).toEqual(['listen', 'routes']);
  expect(projectRoutes(value)).toEqual(routes);
  const httpGuard = value.servers.cadder_http.routes[0]!;
  const httpRoute = ((httpGuard.handle as JsonObject[])[0]!.routes as JsonObject[])[0]!;
  expect(httpRoute['@id']).toBe('http_route');
  const proxy = (httpRoute.handle as JsonObject[])[0]!;
  expect(proxy['@id']).toBe('http_proxy');
  expect((proxy.opaque as JsonObject[])[0]!['@id']).toBe('http_payload');
  httpRoute['@id'] = 'changed copy';
  expect(projectRoutes(value)[0]!['@id']).toBe('route');
  expect(plan([input])).toEqual(snapshot);
  expect(input).toEqual(before);
  expect(composeRoutePlan([input, { registration: registration('b'), config }]).ok).toBe(false);
  expect(snapshot).toEqual(plan([input]));
});

it('preserves route order across deterministically sorted source servers', () => {
  const config = candidate({
    apps: {
      http: {
        servers: {
          z: { routes: [response('z1'), response('z2')] },
          a: { routes: [response('a1'), response('a2')] },
          missing: {},
        },
      },
    },
  });
  expect(projectRoutes(plan([{ registration: registration('a'), config }]))).toEqual([
    response('a1'),
    response('a2'),
    response('z1'),
    response('z2'),
  ]);
});

it('returns no plan for conflict/duplicate input and never fabricates routes', () => {
  const a = { registration: registration('a'), config: configWithRoutes([response('a')]) };
  const b = { registration: registration('b'), config: configWithRoutes([response('b')]) };
  const conflict = composeRoutePlan([a, b]);
  expect(conflict).toMatchObject({ ok: false, diagnostics: [{ code: 'domain-conflict' }] });
  expect(conflict).not.toHaveProperty('value');
  expect(composeRoutePlan([a, a])).toMatchObject({
    ok: false,
    diagnostics: [{ code: 'duplicate-registration' }],
  });
  expect(
    plan([{ registration: registration('a'), config: candidate({}) }]).servers.cadder_https.routes,
  ).toEqual([]);
});

it('fails the whole plan for each invalid selected carrier and sorts/bounds failures', () => {
  const invalid = [
    undefined,
    bodyCandidate('{'),
    candidate([]),
    { ...candidate({}), effectiveConfigHash: 'incorrect' },
    bodyCandidate(' '.repeat(maxCaddyConfigurationBytes + 1)),
    configWithRoutes([{ handle: [{ handler: 'subroute', routes: [null] }] }]),
  ];
  for (const config of invalid) {
    const bad = { registration: registration('a'), ...(config === undefined ? {} : { config }) };
    const good = {
      registration: registration('z', ['z.localhost']),
      config: configWithRoutes([response('good')]),
    };
    const before = structuredClone([good, bad]);
    const result = composeRoutePlan([good, bad]);
    expect(result).toMatchObject({ ok: false, diagnostics: [{ code: 'prepared-input-invalid' }] });
    expect(result).not.toHaveProperty('value');
    expect([good, bad]).toEqual(before);
  }
  const inputs = Array.from({ length: 100 }, (_, i) => ({
    registration: registration(String(i), [`${i}.localhost`]),
  }));
  const result = composeRoutePlan(inputs);
  expect(result).toEqual(composeRoutePlan([...inputs].reverse()));
  if (result.ok) throw new Error('Expected failure.');
  expect(result.diagnostics).toHaveLength(maxCompositionDiagnostics);
});

it('bounds clone/traversal capacity failures without returning a partial plan or mutating inputs', () => {
  const deep = '{"next":'.repeat(50_000) + 'null' + '}'.repeat(50_000);
  const config = bodyCandidate(
    configWithRoutes([
      { handle: [{ handler: 'opaque', payload: 'DEEP' }] },
    ]).adaptedConfig.body.replace('"DEEP"', deep),
  );
  const input = { registration: registration('a'), config };
  const before = { ...config, adaptedConfig: { ...config.adaptedConfig } };
  expect(readPreparedRoutes(config).ok).toBe(true);
  expect(composeRoutePlan([input])).toEqual({
    ok: false,
    diagnostics: [
      {
        code: 'prepared-input-invalid',
        domainKey: null,
        sourceConfigPaths: ['a/Caddyfile'],
        message: 'Cannot safely clone and filter the complete adapted route tree.',
      },
    ],
  });
  expect(config).toEqual(before);
});
