import { constants } from 'node:fs';
import { chown, lstat, mkdir, open, realpath, unlink } from 'node:fs/promises';
import { dirname, isAbsolute, join, parse, relative, resolve, sep } from 'node:path';
import { CadderError, errorCode } from '../protocol/errors.ts';
import { powershell, psLiteral } from './powershell.ts';
import type { RuntimeOwner } from '../contracts/ports.ts';

export type { RuntimeOwner } from '../contracts/ports.ts';

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

function runtimeLeaf(path: string): string {
  const rootLength = parse(path).root.length;
  let end = path.length;
  while (end > rootLength && (path[end - 1] === sep || (sep === '\\' && path[end - 1] === '/')))
    end -= 1;
  return path.slice(0, end);
}

async function assertRuntimeAncestors(path: string): Promise<void> {
  // Preserve lexical components: resolve(path) could erase a linked prefix before '..'.
  const leaf = runtimeLeaf(path);
  const root = parse(leaf).root;
  const absolute = isAbsolute(leaf)
    ? leaf
    : `${root ? resolve(root) : process.cwd()}${sep}${leaf.slice(root.length)}`;
  const parent = dirname(absolute);
  let ancestor = parse(parent).root;
  const components = parent.slice(ancestor.length).split(sep === '\\' ? /[\\/]+/ : /\/+/);
  for (const component of ['', ...components]) {
    if (component === '..') ancestor = dirname(ancestor);
    else if (component && component !== '.') ancestor = join(ancestor, component);
    let info;
    try {
      info = await lstat(ancestor);
    } catch (error) {
      // A later '..' can re-enter an existing linked prefix; do not stop at missing parents.
      if (errorCode(error) === 'ENOENT') continue;
      throw error;
    }
    if (!info.isSymbolicLink()) continue;
    // Only these standard Darwin top-level OS aliases are permitted, never
    // arbitrary root-owned or same-named nested links. Parent ACLs are unchanged.
    if (
      process.platform === 'darwin' &&
      info.uid === 0 &&
      ['/tmp', '/var', '/etc'].includes(ancestor)
    ) {
      const target = `/private${ancestor}`;
      try {
        if ((await realpath(ancestor)) === target) {
          const physical = await lstat(target);
          if (physical.isDirectory() && !physical.isSymbolicLink()) {
            // Inspection follows only this verified OS alias; caller paths stay unchanged.
            ancestor = target;
            continue;
          }
        }
      } catch {
        // An unverified OS alias is unsafe, including missing or inaccessible targets.
      }
    }
    throw new CadderError(
      'unsafe-runtime-permissions',
      'Runtime ancestors must not be links or junctions.',
    );
  }
}

export async function prepareRuntime(directory: string, owner: RuntimeOwner): Promise<void> {
  await assertRuntimeAncestors(directory);
  try {
    await lstat(runtimeLeaf(directory));
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
      $actualOwner = (Get-Acl -LiteralPath ${psLiteral(path)}).GetOwner([System.Security.Principal.SecurityIdentifier]).Value;
      $acl = New-Object System.Security.AccessControl.${type};
      # An already-owned new path needs WRITE_DAC, not a redundant WRITE_OWNER request.
      if ($actualOwner -ne $sid.Value) { $acl.SetOwner($sid) };
      $acl.SetAccessRuleProtection($true, $false);
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
  await assertRuntimeAncestors(path);
  await assertRuntimePath(path, owner, directory, true);
}

/** Read-only native storage check beneath a strictly protected product directory. */
export async function assertRuntimeDescendant(
  root: string,
  path: string,
  owner: RuntimeOwner,
  directory = false,
): Promise<void> {
  const separators = sep === '\\' ? /[\\/]+/ : /\/+/;
  // Reject lexical aliases before normalization could erase a linked component.
  if (
    !isAbsolute(root) ||
    !isAbsolute(path) ||
    (!directory && path !== runtimeLeaf(path)) ||
    [root, path].some((value) =>
      value
        .slice(parse(value).root.length)
        .split(separators)
        .some((part) => part === '.' || part === '..'),
    )
  )
    throw new CadderError(
      'unsafe-runtime-permissions',
      'Expected an unaliased runtime descendant.',
    );
  const base = runtimeLeaf(root);
  const leaf = runtimeLeaf(path);
  const inside = relative(base, leaf);
  if (!inside || isAbsolute(inside) || inside.split(separators)[0] === '..')
    throw new CadderError(
      'unsafe-runtime-permissions',
      'Runtime descendant must be inside its root.',
    );
  await assertProtected(base, owner, true);
  const components = inside.split(separators);
  let current = base;
  for (let index = 0; index < components.length; index++) {
    current = join(current, components[index]!);
    await assertRuntimePath(current, owner, index < components.length - 1 || directory, false);
  }
}

async function assertRuntimePath(
  path: string,
  owner: RuntimeOwner,
  directory: boolean,
  strict: boolean,
): Promise<void> {
  // A final separator can make lstat follow a directory link instead of inspecting it.
  const info = await lstat(runtimeLeaf(path));
  if (info.isSymbolicLink() || (directory ? !info.isDirectory() : !info.isFile())) {
    throw new CadderError(
      'unsafe-runtime-permissions',
      'Runtime paths must be real directories or regular files, not links.',
    );
  }
  if (process.platform === 'win32') {
    await powershell(`
      $acl = Get-Acl -LiteralPath ${psLiteral(path)};
      if ($acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -ne ${psLiteral(owner.id)}${strict ? ' -or !$acl.AreAccessRulesProtected' : ''}) { throw 'Unsafe runtime owner or ACL inheritance' };
      $ownerAllowed = $false;
      foreach ($rule in $acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
        if ($rule.IdentityReference.Value -ne ${psLiteral(owner.id)} -or $rule.AccessControlType -ne 'Allow') { throw 'Runtime ACL permits a different account' };
        if (($rule.FileSystemRights -band [System.Security.AccessControl.FileSystemRights]::FullControl) -eq [System.Security.AccessControl.FileSystemRights]::FullControl${strict ? '' : ' -and ($rule.PropagationFlags -band [System.Security.AccessControl.PropagationFlags]::InheritOnly) -eq 0'}) { $ownerAllowed = $true };
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
  await assertRuntimeAncestors(path);
  let handle;
  try {
    handle = await open(path, constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY, 0o600);
  } catch (error) {
    if (errorCode(error) !== 'EEXIST') throw error;
    await assertProtected(path, owner);
    return false;
  }
  const failures: unknown[] = [];
  try {
    await protectCreated(path, owner);
    await handle.writeFile(data);
    await handle.sync();
  } catch (error) {
    failures.push(error);
  }
  try {
    await handle.close();
  } catch (error) {
    failures.push(error);
  }
  if (failures.length === 0) {
    try {
      await assertProtected(path, owner);
    } catch (error) {
      failures.push(error);
    }
  }
  if (failures.length === 0) return true;
  // Only the successful exclusive open above grants ownership of this new path.
  try {
    await unlink(path);
  } catch (error) {
    if (errorCode(error) !== 'ENOENT') failures.push(error);
  }
  if (failures.length === 1) throw failures[0];
  throw new AggregateError(failures, 'Protected file creation and cleanup failed.', {
    cause: failures[0],
  });
}
