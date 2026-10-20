import { realpath } from 'node:fs/promises';
import { resolve } from 'node:path';
import { isIP } from 'node:net';
import { canonicalizeDomain } from '../protocol/domain.ts';
import type { StateSnapshot, EntrypointRegistration } from '../protocol/dto.ts';
import { CadderError } from '../protocol/errors.ts';

export async function projectForPath(
  snapshot: StateSnapshot,
  path: string,
): Promise<EntrypointRegistration> {
  const absolute = await realpath(resolve(path)).catch(() => resolve(path));
  const normalized = process.platform === 'win32' ? absolute.toLowerCase() : absolute;
  const project = snapshot.registrations.find((registration) => {
    const source = registration.sourceConfigPath.canonical ?? registration.sourceConfigPath.raw;
    return (process.platform === 'win32' ? source.toLowerCase() : source) === normalized;
  });
  if (!project) throw new CadderError('target-not-found', 'Caddyfile is not registered.');
  return project;
}

export async function domainForName(snapshot: StateSnapshot, name: string, caddyfile?: string) {
  const canonical = canonicalizeDomain(name);
  const projects = caddyfile ? [await projectForPath(snapshot, caddyfile)] : snapshot.registrations;
  const matches = projects.flatMap((project) =>
    project.registeredDomains
      .filter((domain) => domain.name.canonical === canonical)
      .map((domain) => ({ project, domain })),
  );
  if (matches.length > 1)
    throw new CadderError('target-conflict', 'Domain is ambiguous. Select its --caddyfile.');
  if (!matches[0]) throw new CadderError('target-not-found', 'Domain is not registered.');
  return matches[0];
}

export function localUpstreamPort(upstream: string | undefined): number | undefined {
  if (!upstream) return;
  const authority = upstream
    .trim()
    .replace(/^https?:\/\//, '')
    .split('/')[0]!;
  const separator = authority.lastIndexOf(':');
  const portText = separator < 0 ? authority : authority.slice(separator + 1);
  if (!/^\d+$/.test(portText)) return;
  const port = Number(portText);
  if (!Number.isInteger(port) || port > 65535) return;
  if (separator < 0) return port;
  const host = authority.slice(0, separator).replace(/^\[|\]$/g, '');
  if (!host || host.toLowerCase() === 'localhost') return port;
  if (isIP(host) === 4 && (host.startsWith('127.') || host === '0.0.0.0')) return port;
  try {
    if (
      isIP(host) === 6 &&
      !host.includes('%') &&
      ['[::1]', '[::]'].includes(new URL(`http://[${host}]`).hostname)
    )
      return port;
  } catch {
    /* Unix sockets and remote/non-network upstreams have no local port owner. */
  }
}
