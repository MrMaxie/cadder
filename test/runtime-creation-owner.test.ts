import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { normalizeRuntimeCreationOwner } from '../src/platform/runtime-creation-owner.ts';
import { powershell } from '../src/platform/powershell.ts';

vi.mock('../src/platform/powershell.ts', async (original) => ({
  ...(await original<typeof import('../src/platform/powershell.ts')>()),
  powershell: vi.fn(),
}));
const owner = { id: 'S-1-5-21-123-456-789-1001', elevated: true };
const proof = {
  ProcessId: process.pid,
  User: owner.id,
  Before: owner.id,
  After: owner.id,
  Changed: false,
};

beforeEach(() => {
  vi.resetAllMocks();
  vi.spyOn(process, 'platform', 'get').mockReturnValue('win32');
  vi.mocked(powershell).mockResolvedValue(JSON.stringify(proof));
});
afterEach(() => vi.restoreAllMocks());

it.each(['linux', 'darwin'] as const)('is a no-op on %s', async (platform) => {
  vi.spyOn(process, 'platform', 'get').mockReturnValue(platform);
  await normalizeRuntimeCreationOwner({ id: 'uid:1001', uid: 1001, elevated: false });
  expect(powershell).not.toHaveBeenCalled();
});

it('targets the actual Node primary token with limited process access and user proof', async () => {
  await normalizeRuntimeCreationOwner(owner);
  expect(powershell).toHaveBeenCalledOnce();
  const script = vi.mocked(powershell).mock.calls[0]![0];
  expect(script).toContain(`::Normalize(${process.pid}, '${owner.id}')`);
  expect(script).toContain('OpenProcess(0x1000, false, pid)');
  expect(script).toContain('OpenProcessToken(process, 0x0008, out token)');
  expect(script).toContain('string actual = Sid(token, 1)');
  expect(script).toContain('if (actual != expected) throw');
  expect(script).not.toMatch(/WindowsIdentity|GetCurrentProcess|AdjustTokenPrivileges|Impersonate/);
});

it('keeps adjustment access and TokenOwner mutation inside the mismatch branch', async () => {
  await normalizeRuntimeCreationOwner(owner);
  const script = vi.mocked(powershell).mock.calls[0]![0];
  const branch = script.slice(script.indexOf('if (changed) {'), script.indexOf('// Independently'));
  expect(branch).toContain('OpenProcessToken(process, 0x0008 | 0x0080, out token)');
  expect(branch).toContain('user = Information(token, 1)');
  expect(branch).toContain('Marshal.WriteIntPtr(owner, Marshal.ReadIntPtr(user))');
  expect(branch).toContain('SetTokenInformation(token, 4, owner, IntPtr.Size)');
  expect(script.match(/0x0080/g)).toHaveLength(1);
  expect(script.match(/if \(!SetTokenInformation/g)).toHaveLength(1);
  expect(script).toContain('bool changed = before != actual');
  const readback = script.slice(
    script.indexOf('// Independently'),
    script.indexOf('return new Result'),
  );
  expect(readback).toContain('CloseToken(ref token)');
  expect(readback).toContain('OpenProcessToken(process, 0x0008, out token)');
  expect(readback).toContain('if (Sid(token, 1) != expected) throw');
  expect(readback).toContain('string after = Sid(token, 4)');
  expect(readback).toContain('if (after != actual) throw');
  expect(script).toContain('finally { Marshal.FreeHGlobal(data); }');
  for (const resource of ['owner', 'user'])
    expect(script).toContain(`if (${resource} != IntPtr.Zero) Marshal.FreeHGlobal(${resource})`);
  for (const resource of ['token', 'process'])
    expect(script).toContain(`if (${resource} != IntPtr.Zero) CloseHandle(${resource})`);
});

it('accepts a mismatched default owner only after successful user-owner readback', async () => {
  vi.mocked(powershell).mockResolvedValue(
    JSON.stringify({ ...proof, Before: 'S-1-5-32-544', Changed: true }),
  );
  await expect(normalizeRuntimeCreationOwner(owner)).resolves.toBeUndefined();
});

it('passes the owner as an escaped PowerShell literal', async () => {
  const quoted = { ...owner, id: "S-1-5-21-1001'; throw 'injected" };
  vi.mocked(powershell).mockResolvedValue(
    JSON.stringify({ ...proof, User: quoted.id, After: quoted.id }),
  );
  await expect(normalizeRuntimeCreationOwner(quoted)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
  expect(vi.mocked(powershell).mock.calls[0]![0]).toContain("'S-1-5-21-1001''; throw ''injected'");
});

it.each([
  null,
  [],
  {},
  { ...proof, ProcessId: process.pid + 1 },
  { ...proof, ProcessId: String(process.pid) },
  { ...proof, User: 'S-1-5-18' },
  { ...proof, After: 'S-1-5-32-544' },
  { ...proof, Before: '' },
  { ...proof, Before: 'not-a-sid' },
  { ...proof, Before: null },
  { ...proof, Changed: true },
  { ...proof, Changed: 'false' },
  { ...proof, Before: 'S-1-5-32-544', Changed: false },
])('rejects invalid native proof %#', async (invalid) => {
  vi.mocked(powershell).mockResolvedValue(JSON.stringify(invalid));
  await expect(normalizeRuntimeCreationOwner(owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
});

it.each(['', 'not JSON', '{', 'null'])('fails closed on malformed output %j', async (output) => {
  vi.mocked(powershell).mockResolvedValue(output);
  await expect(normalizeRuntimeCreationOwner(owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
});

it.each([
  'OpenProcess',
  'TokenUser',
  'TokenOwner',
  'adjust access',
  'SetTokenInformation',
  'readback',
  'timeout',
])('fails closed on native/helper failure: %s', async (phase) => {
  vi.mocked(powershell).mockRejectedValue(new Error(phase));
  await expect(normalizeRuntimeCreationOwner(owner)).rejects.toMatchObject({
    code: 'unsafe-runtime-permissions',
  });
});
