import * as fs from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import * as shell from '../src/platform/powershell.ts';
import {
  assertProtected,
  createProtectedFile,
  prepareRuntime,
  runtimeOwner,
  type RuntimeOwner,
} from '../src/platform/runtime-security.ts';

vi.mock('node:fs/promises', async (original) => {
  const actual = await original<typeof import('node:fs/promises')>();
  return { ...actual, open: vi.fn(actual.open) };
});

type SecuritySnapshot = {
  ownerMatches: boolean;
  protected: boolean;
  rules: { owner: boolean; fullControl: boolean; writeOwner: boolean; inherited: boolean }[];
};

async function security(path: string, owner: RuntimeOwner): Promise<SecuritySnapshot> {
  return JSON.parse(
    await shell.powershell(`
      $acl = Get-Acl -LiteralPath ${shell.psLiteral(path)};
      $rules = @();
      foreach ($rule in $acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])) {
        $rules += @{
          owner=($rule.IdentityReference.Value -eq ${shell.psLiteral(owner.id)});
          fullControl=(($rule.FileSystemRights -band [Security.AccessControl.FileSystemRights]::FullControl) -eq [Security.AccessControl.FileSystemRights]::FullControl);
          writeOwner=(($rule.FileSystemRights -band [Security.AccessControl.FileSystemRights]::TakeOwnership) -ne 0);
          inherited=$rule.IsInherited;
        };
      };
      @{ ownerMatches=($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -eq ${shell.psLiteral(owner.id)}); protected=$acl.AreAccessRulesProtected; rules=@($rules) } | ConvertTo-Json -Depth 4 -Compress
    `),
  ) as SecuritySnapshot;
}

// This is an ordinary-token native regression, not an elevated or modeled acceptance claim.
describe.skipIf(process.platform !== 'win32')(
  'ordinary-token Windows paths under Modify-only inheritance',
  () => {
    let owner: RuntimeOwner;
    let parent: string | undefined;
    let parentSecurity: SecuritySnapshot;

    beforeAll(async () => {
      owner = await runtimeOwner();
    });
    beforeEach(async (context) => {
      vi.clearAllMocks();
      if (owner.elevated) context.skip();
      parent = await fs.mkdtemp(join(tmpdir(), 'cadder-created-security-'));
      // Mutate only this exclusively created empty fixture, never the system temp parent.
      await shell.powershell(`
      $sid = New-Object Security.Principal.SecurityIdentifier(${shell.psLiteral(owner.id)});
      $current = Get-Acl -LiteralPath ${shell.psLiteral(parent)};
      if ($current.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid.Value) { throw 'Fixture owner mismatch' };
      $acl = New-Object Security.AccessControl.DirectorySecurity;
      $acl.SetAccessRuleProtection($true,$false);
      $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid,'Modify','ContainerInherit,ObjectInherit','None','Allow')));
      [IO.Directory]::SetAccessControl(${shell.psLiteral(parent)},$acl);
    `);
      parentSecurity = await security(parent, owner);
      expect(owner.elevated).toBe(false);
      expect(parentSecurity).toEqual({
        ownerMatches: true,
        protected: true,
        rules: [{ owner: true, fullControl: false, writeOwner: false, inherited: false }],
      });
    });
    afterEach(async () => {
      vi.restoreAllMocks();
      if (parent !== undefined) await fs.rm(parent, { recursive: true, force: true });
      parent = undefined;
    });

    it.each(['directory', 'file'] as const)(
      'protects a newly created %s without requiring owner reassignment',
      { timeout: 30000 },
      async (kind) => {
        const path = join(parent!, kind);
        if (kind === 'directory') await prepareRuntime(path, owner);
        else expect(await createProtectedFile(path, owner, 'fixture')).toBe(true);
        await assertProtected(path, owner, kind === 'directory');
        expect(await security(path, owner)).toEqual({
          ownerMatches: true,
          protected: true,
          rules: [{ owner: true, fullControl: true, writeOwner: true, inherited: false }],
        });
        expect(await security(parent!, owner)).toEqual(parentSecurity);
      },
    );

    it(
      'rejects unsafe existing directory and file without repairing them',
      { timeout: 30000 },
      async () => {
        const directory = join(parent!, 'existing-directory');
        const file = join(parent!, 'existing-file');
        await fs.mkdir(directory);
        await fs.writeFile(file, 'retained fixture');
        const beforeDirectory = await security(directory, owner);
        const beforeFile = await security(file, owner);
        await expect(prepareRuntime(directory, owner)).rejects.toMatchObject({
          code: 'unsafe-runtime-permissions',
        });
        await expect(createProtectedFile(file, owner)).rejects.toMatchObject({
          code: 'unsafe-runtime-permissions',
        });
        expect(await security(directory, owner)).toEqual(beforeDirectory);
        expect(await security(file, owner)).toEqual(beforeFile);
        expect(await fs.readFile(file, 'utf8')).toBe('retained fixture');
      },
    );

    it(
      'retains owner-reassignment refusal and cleans only its new file',
      { timeout: 30000 },
      async () => {
        const file = join(parent!, 'different-owner');
        const inspect = vi.spyOn(shell, 'powershell');
        // An ordinary token cannot assign its new file to the well-known SYSTEM account.
        await expect(
          createProtectedFile(file, { id: 'S-1-5-18', elevated: false }, 'must-not-be-written'),
        ).rejects.toMatchObject({ code: 1 });
        expect(inspect).toHaveBeenCalledOnce();
        expect(inspect.mock.calls[0]![0]).toContain(
          'if ($actualOwner -ne $sid.Value) { $acl.SetOwner($sid) }',
        );
        const handle = await vi.mocked(fs.open).mock.results[0]!.value;
        expect(handle.fd).toBe(-1);
        await expect(fs.lstat(file)).rejects.toMatchObject({ code: 'ENOENT' });
        expect(await security(parent!, owner)).toEqual(parentSecurity);
      },
    );

    it(
      'closes and unlinks its exclusive file when reading the new owner fails',
      { timeout: 30000 },
      async () => {
        const file = join(parent!, 'owner-read-failure');
        const failure = new Error('Simulated owner inspection failure');
        const inspect = vi.spyOn(shell, 'powershell').mockRejectedValueOnce(failure);
        await expect(createProtectedFile(file, owner)).rejects.toBe(failure);
        expect(inspect).toHaveBeenCalledOnce();
        expect(inspect.mock.calls[0]![0]).toContain('.GetOwner(');
        const handle = await vi.mocked(fs.open).mock.results[0]!.value;
        expect(handle.fd).toBe(-1);
        await expect(fs.lstat(file)).rejects.toMatchObject({ code: 'ENOENT' });
      },
    );
  },
);
