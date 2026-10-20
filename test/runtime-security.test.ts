import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, it } from 'vitest';
import {
  assertProtected,
  createProtectedFile,
  prepareRuntime,
  runtimeOwner,
} from '../src/platform/runtime-security.ts';
import { powershell, psLiteral } from '../src/platform/powershell.ts';

const directories: string[] = [];
afterEach(async () => {
  await Promise.all(
    directories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});

it('creates protected runtime and secret and validates existing paths', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cadder-security-'));
  directories.push(root);
  const owner = await runtimeOwner();
  const directory = join(root, 'v2');
  await prepareRuntime(directory, owner);
  await prepareRuntime(directory, owner);
  const path = join(directory, 'secret');
  expect(await createProtectedFile(path, owner, 'secret')).toBe(true);
  expect(await createProtectedFile(path, owner)).toBe(false);
  await assertProtected(path, owner);
  await expect(assertProtected(directory, owner)).rejects.toThrow('regular files');
});

it(
  'rejects insecure Windows ACL rather than repairing it',
  { skip: process.platform !== 'win32' },
  async () => {
    const root = await mkdtemp(join(tmpdir(), 'cadder-security-'));
    directories.push(root);
    const owner = await runtimeOwner();
    await expect(prepareRuntime(root, owner)).rejects.toThrow('ACL');
    const directory = join(root, 'v2');
    await prepareRuntime(directory, owner);
    await powershell(
      `$acl=New-Object System.Security.AccessControl.DirectorySecurity; $owner=New-Object System.Security.Principal.SecurityIdentifier(${psLiteral(owner.id)}); $acl.SetOwner($owner); $acl.SetAccessRuleProtection($true,$false); $acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($owner,'FullControl','Allow'))); $sid=New-Object System.Security.Principal.SecurityIdentifier('S-1-1-0'); $acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($sid,'Read','Allow'))); [System.IO.Directory]::SetAccessControl(${psLiteral(directory)}, $acl)`,
    );
    await expect(prepareRuntime(directory, owner)).rejects.toThrow('ACL');
  },
);

it('quotes PowerShell literals without allowing interpolation', () => {
  expect(psLiteral("a'b$()")).toBe("'a''b$()'");
});
