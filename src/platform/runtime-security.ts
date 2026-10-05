import { constants } from 'node:fs';
import { chown, lstat, mkdir, open } from 'node:fs/promises';
import { dirname } from 'node:path';
import { CadderError, errorCode } from '../protocol/errors.ts';
import { powershell, psLiteral } from './powershell.ts';

export interface RuntimeOwner {
  id: string;
  uid?: number;
  elevated: boolean;
}

export async function runtimeOwner(explicitUid?: number): Promise<RuntimeOwner> {
  if (process.platform === 'win32') {
    return JSON.parse(
      await powershell(`
      $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent();
      $principal = New-Object System.Security.Principal.WindowsPrincipal($identity);
      @{ id = $identity.User.Value; elevated = $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator) } | ConvertTo-Json -Compress
    `),
    ) as RuntimeOwner;
  }
  const uid = process.getuid!();
  if (uid === 0 && explicitUid === undefined)
    throw new CadderError(
      'runtime-owner-required',
      'Root must specify --runtime-owner and --runtime-dir.',
    );
  if (
    explicitUid !== undefined &&
    (!Number.isSafeInteger(explicitUid) || explicitUid < 0 || (uid !== 0 && explicitUid !== uid))
  ) {
    throw new CadderError(
      'invalid-runtime-owner',
      'Runtime owner must be the current user unless the daemon runs as root.',
    );
  }
  const ownerUid = explicitUid ?? uid;
  return { id: `uid:${ownerUid}`, uid: ownerUid, elevated: uid === 0 };
}

export async function prepareRuntime(directory: string, owner: RuntimeOwner): Promise<void> {
  try {
    await lstat(directory);
  } catch (error) {
    if (errorCode(error) !== 'ENOENT') throw error;
    try {
      await mkdir(dirname(directory), { mode: 0o700, recursive: true });
      await mkdir(directory, { mode: 0o700 });
      await protectCreated(directory, owner, true);
    } catch (error) {
      if (errorCode(error) !== 'EEXIST') throw error;
    }
  }
  await assertProtected(directory, owner, true);
}

export async function protectCreated(
  path: string,
  owner: RuntimeOwner,
  directory = false,
): Promise<void> {
  if (process.platform === 'win32') {
    const type = directory ? 'DirectorySecurity' : 'FileSecurity';
    const inheritance = directory ? "'ContainerInherit,ObjectInherit'" : "'None'";
    await powershell(`
      $sid = New-Object System.Security.Principal.SecurityIdentifier(${psLiteral(owner.id)});
      $acl = New-Object System.Security.AccessControl.${type};
      $acl.SetOwner($sid); $acl.SetAccessRuleProtection($true, $false);
      $rule = New-Object System.Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', ${inheritance}, 'None', 'Allow');
      $acl.AddAccessRule($rule);
      [System.IO.${directory ? 'Directory' : 'File'}]::SetAccessControl(${psLiteral(path)}, $acl);
    `);
  } else if (process.getuid!() === 0 && owner.uid !== undefined) {
    await chown(path, owner.uid, owner.uid);
  }
}

export async function assertProtected(
  path: string,
  owner: RuntimeOwner,
  directory = false,
): Promise<void> {
  const info = await lstat(path);
  if (info.isSymbolicLink() || (directory ? !info.isDirectory() : !info.isFile())) {
    throw new CadderError(
      'unsafe-runtime-permissions',
      'Runtime paths must be real directories or regular files, not links.',
    );
  }
  if (process.platform === 'win32') {
    await powershell(`
      $acl = Get-Acl -LiteralPath ${psLiteral(path)};
      if ($acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -ne ${psLiteral(owner.id)} -or !$acl.AreAccessRulesProtected) { throw 'Unsafe runtime owner or ACL inheritance' };
      $ownerAllowed = $false;
      foreach ($rule in $acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
        if ($rule.IdentityReference.Value -ne ${psLiteral(owner.id)} -or $rule.AccessControlType -ne 'Allow') { throw 'Runtime ACL permits a different account' };
        if (($rule.FileSystemRights -band [System.Security.AccessControl.FileSystemRights]::FullControl) -eq [System.Security.AccessControl.FileSystemRights]::FullControl) { $ownerAllowed = $true };
      };
      if (!$ownerAllowed) { throw 'Runtime owner lacks full access' };
    `).catch(() => {
      throw new CadderError('unsafe-runtime-permissions', 'Unsafe runtime ACL; refusing to start.');
    });
  } else if (info.uid !== owner.uid || (info.mode & 0o777) !== (directory ? 0o700 : 0o600)) {
    throw new CadderError(
      'unsafe-runtime-permissions',
      'Runtime owner and permissions must be 0700 for directories and 0600 for files.',
    );
  }
}

export async function createProtectedFile(
  path: string,
  owner: RuntimeOwner,
  data: string | Uint8Array = '',
): Promise<boolean> {
  let handle;
  try {
    handle = await open(path, constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY, 0o600);
  } catch (error) {
    if (errorCode(error) !== 'EEXIST') throw error;
    await assertProtected(path, owner);
    return false;
  }
  try {
    await protectCreated(path, owner);
    await handle.writeFile(data);
    await handle.sync();
  } finally {
    await handle.close();
  }
  await assertProtected(path, owner);
  return true;
}
