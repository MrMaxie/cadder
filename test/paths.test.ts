import { describe, expect, it } from 'vitest';
import { join, resolve } from 'node:path';
import { resolvePaths } from '../src/daemon/paths.ts';

describe('v2 runtime identity', () => {
  it('isolates explicitly configured runtime without changing the old runtime', () => {
    const paths = resolvePaths({ runtimeDir: 'fixture-runtime' });
    expect(paths.directory).toBe(join(resolve('fixture-runtime'), 'v2'));
    expect(resolvePaths({ runtimeDir: 'fixture-runtime' })).toEqual(paths);
    expect(paths.lock).toContain('lock.sqlite3');
    expect(paths.profile).toBe('default');
  });
  it.each(['default', 'prod', 'production', ''])('accepts default profile %s', (profile) => {
    expect(resolvePaths({ runtimeDir: 'fixture', profile }).profile).toBe('default');
  });
  it.each(['dev', 'development'])('accepts dev profile %s', (profile) => {
    expect(resolvePaths({ runtimeDir: 'fixture', profile }).profile).toBe('dev');
  });
  it('rejects unrecognized profiles', () => {
    expect(() => resolvePaths({ profile: 'other' })).toThrow('Expected runtime profile');
  });
});
