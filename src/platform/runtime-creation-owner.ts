import type { RuntimeOwner } from './runtime-security.ts';
import { powershell, psLiteral } from './powershell.ts';
import { CadderError } from '../protocol/errors.ts';

/** Set only the Windows primary token's default creation owner, never existing file ACLs. */
export async function normalizeRuntimeCreationOwner(owner: RuntimeOwner): Promise<void> {
  if (process.platform !== 'win32') return;
  try {
    const proof: unknown = JSON.parse(
      await powershell(`
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class CadderRuntimeCreationOwner {
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int kind, IntPtr data, int length, out int required);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool SetTokenInformation(IntPtr token, int kind, IntPtr data, int length);
  public sealed class Result { public int ProcessId; public string User; public string Before; public string After; public bool Changed; }
  static IntPtr Information(IntPtr token, int kind) {
    int length;
    bool queried = GetTokenInformation(token, kind, IntPtr.Zero, 0, out length);
    int error = Marshal.GetLastWin32Error();
    if (queried || error != 122 || length < IntPtr.Size || length > 65536) throw new Win32Exception(error);
    IntPtr data = Marshal.AllocHGlobal(length);
    try {
      if (!GetTokenInformation(token, kind, data, length, out length)) throw new Win32Exception(Marshal.GetLastWin32Error());
      return data;
    } catch { Marshal.FreeHGlobal(data); throw; }
  }
  static string Sid(IntPtr token, int kind) {
    IntPtr data = Information(token, kind);
    try { return new SecurityIdentifier(Marshal.ReadIntPtr(data)).Value; }
    finally { Marshal.FreeHGlobal(data); }
  }
  static void CloseToken(ref IntPtr token) {
    if (!CloseHandle(token)) throw new Win32Exception(Marshal.GetLastWin32Error());
    token = IntPtr.Zero;
  }
  public static Result Normalize(int pid, string expected) {
    IntPtr process = IntPtr.Zero, token = IntPtr.Zero, user = IntPtr.Zero, owner = IntPtr.Zero;
    try {
      // PROCESS_QUERY_LIMITED_INFORMATION; target the Node process, not this helper.
      process = OpenProcess(0x1000, false, pid);
      if (process == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
      if (!OpenProcessToken(process, 0x0008, out token)) throw new Win32Exception(Marshal.GetLastWin32Error());
      string actual = Sid(token, 1);
      if (actual != expected) throw new InvalidOperationException("Target token user mismatch.");
      string before = Sid(token, 4);
      bool changed = before != actual;
      if (changed) {
        CloseToken(ref token);
        // TOKEN_QUERY | TOKEN_ADJUST_DEFAULT, only when TokenOwner differs.
        if (!OpenProcessToken(process, 0x0008 | 0x0080, out token)) throw new Win32Exception(Marshal.GetLastWin32Error());
        user = Information(token, 1);
        if (new SecurityIdentifier(Marshal.ReadIntPtr(user)).Value != expected) throw new InvalidOperationException("Adjustment token user mismatch.");
        owner = Marshal.AllocHGlobal(IntPtr.Size);
        Marshal.WriteIntPtr(owner, Marshal.ReadIntPtr(user));
        if (!SetTokenInformation(token, 4, owner, IntPtr.Size)) throw new Win32Exception(Marshal.GetLastWin32Error());
      }
      // Independently reopen the process primary token for readback, with query only.
      CloseToken(ref token);
      if (!OpenProcessToken(process, 0x0008, out token)) throw new Win32Exception(Marshal.GetLastWin32Error());
      if (Sid(token, 1) != expected) throw new InvalidOperationException("Readback token user mismatch.");
      string after = Sid(token, 4);
      if (after != actual) throw new InvalidOperationException("Default owner readback mismatch.");
      return new Result { ProcessId=pid, User=actual, Before=before, After=after, Changed=changed };
    } finally {
      if (owner != IntPtr.Zero) Marshal.FreeHGlobal(owner);
      if (user != IntPtr.Zero) Marshal.FreeHGlobal(user);
      if (token != IntPtr.Zero) CloseHandle(token);
      if (process != IntPtr.Zero) CloseHandle(process);
    }
  }
}
'@;
[CadderRuntimeCreationOwner]::Normalize(${process.pid}, ${psLiteral(owner.id)}) | ConvertTo-Json -Compress
`),
    );
    if (
      !proof ||
      typeof proof !== 'object' ||
      !('ProcessId' in proof) ||
      proof.ProcessId !== process.pid ||
      !('User' in proof) ||
      proof.User !== owner.id ||
      !('Before' in proof) ||
      typeof proof.Before !== 'string' ||
      !/^S-1-\d+(?:-\d+)+$/.test(proof.Before) ||
      !('After' in proof) ||
      proof.After !== owner.id ||
      !('Changed' in proof) ||
      typeof proof.Changed !== 'boolean' ||
      proof.Changed !== (proof.Before !== proof.User)
    )
      throw new Error('Invalid native creation owner proof.');
  } catch {
    throw new CadderError(
      'unsafe-runtime-permissions',
      'Cannot verify the current process default creation owner.',
    );
  }
}
