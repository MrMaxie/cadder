const marker = '[redacted]';

/** Run before a message crosses either persistence or presentation boundaries. */
export function redact(message: string): string {
  return message
    .replace(
      /-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----[\s\S]*?(?:-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----|$)/gi,
      marker,
    )
    .replace(
      /\b(authorization|proxy-authorization|cookie|set-cookie)\s*:\s*[^\r\n]*/gi,
      '$1: ' + marker,
    )
    .replace(/\bbearer\s+[^\s,;"']+/gi, 'Bearer ' + marker)
    .replace(
      /(["']?(?:access[_-]?token|refresh[_-]?token|api[_-]?token|token|password|passwd|secret|api[_-]?key)["']?\s*[:=]\s*)(?:"(?:\\.|[^"\\])*"|'[^']*'|[^\s,;&}\]]+\]?)/gi,
      '$1' + marker,
    );
}

export function terminalSafe(message: string): string {
  // Strip complete ANSI sequences before removing the remaining C0/C1 controls.
  /* eslint-disable no-control-regex -- Terminal output must recognize and remove control bytes. */
  return message
    .replace(/\x1b\][\s\S]*?(?:\x07|\x1b\\|$)|\x1b\[[0-?]*[ -/]*[@-~]|\x1b[\s\S]?/g, '')
    .replace(/[\x00-\x1f\x7f-\x9f]/g, '');
}
/* eslint-enable no-control-regex */
