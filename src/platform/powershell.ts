import { execFile } from 'node:child_process';
import { dirname, join } from 'node:path';
import { promisify } from 'node:util';

const execute = promisify(execFile);

export function psLiteral(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

export async function powershell(script: string): Promise<string> {
  const encoded = Buffer.from(`$ErrorActionPreference = 'Stop'; ${script}`, 'utf16le').toString(
    'base64',
  );
  const executable = join(
    process.env.SystemRoot ?? 'C:\\Windows',
    'System32',
    'WindowsPowerShell',
    'v1.0',
    'powershell.exe',
  );
  // Add-Type must not inherit unrelated native compiler search paths.
  const excluded = new Set(['psmodulepath', 'lib', 'libpath', 'include']);
  const environment = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !excluded.has(key.toLowerCase())),
  );
  environment.PSModulePath = join(dirname(executable), 'Modules');
  const result = await execute(
    executable,
    ['-NoProfile', '-NonInteractive', '-EncodedCommand', encoded],
    {
      windowsHide: true,
      env: environment,
      timeout: 20000,
      maxBuffer: 1024 * 1024,
    },
  );
  return result.stdout.trim();
}
