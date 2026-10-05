import { createHash } from 'node:crypto';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { CadderError } from '../protocol/errors.ts';

export interface RuntimePaths {
  directory: string;
  profile: 'default' | 'dev';
  instance: string;
  endpoint: string;
  secret: string;
  lock: string;
  metadata: string;
  discovery: string;
  history: string;
}

export function resolvePaths(
  options: { runtimeDir?: string; profile?: string } = {},
): RuntimePaths {
  const override = options.runtimeDir ?? process.env.CADDER_RUNTIME_DIR;
  const value = (
    options.profile ??
    (override ? 'default' : process.env.CADDER_RUNTIME_PROFILE) ??
    'default'
  )
    .trim()
    .toLowerCase();
  let profile: 'default' | 'dev';
  if (['', 'default', 'prod', 'production'].includes(value)) profile = 'default';
  else if (['dev', 'development'].includes(value)) profile = 'dev';
  else throw new CadderError('invalid-profile', 'Expected runtime profile default or dev.');
  let base: string;
  if (process.platform === 'win32')
    base = join(
      process.env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local'),
      'Cadder',
      'Cadder',
      'run',
    );
  else if (process.platform === 'darwin')
    base = join(homedir(), 'Library', 'Application Support', 'dev.Cadder.Cadder', 'run');
  else if (process.env.XDG_RUNTIME_DIR) base = join(process.env.XDG_RUNTIME_DIR, 'cadder');
  else
    base = join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local', 'share'), 'cadder', 'run');
  let directory = join(resolve(override ?? base), 'v2');
  if (!override && profile === 'dev') {
    const id =
      process.env.CADDER_DEV_ID ??
      `workspace-${digest(resolve(process.env.CADDER_DEV_WORKSPACE ?? process.cwd()))}`;
    if (!/^(?!\.{1,2}$)[a-zA-Z0-9._-]+$/.test(id))
      throw new CadderError('invalid-profile', 'Invalid CADDER_DEV_ID.');
    directory = join(directory, 'profiles', 'dev', id);
  }
  const instance = digest(directory);
  const endpoint =
    process.platform === 'win32'
      ? `\\\\.\\pipe\\cadder-v2-${instance}`
      : join(directory, 'cadder.sock');
  return {
    directory,
    profile,
    instance,
    endpoint,
    secret: join(directory, 'ipc-secret'),
    lock: join(directory, 'lock.sqlite3'),
    metadata: join(directory, 'cadder.lock.json'),
    discovery: join(directory, 'cadder-ipc.json'),
    history: join(directory, 'runtime.sqlite3'),
  };
}

function digest(value: string): string {
  return createHash('sha256').update(value).digest('hex').slice(0, 16);
}
