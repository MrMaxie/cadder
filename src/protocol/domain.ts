import { domainToASCII } from 'node:url';

/** Match literal-domain IDNA without invoking URL decoding or host parsing. */
export function canonicalizeDomain(raw: string): string {
  const domain = raw.trim().replace(/\.+$/, '');
  const fallback = domain.replace(/[A-Z]/g, (letter) => letter.toLowerCase());
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
