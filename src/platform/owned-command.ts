import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { isAbsolute, join } from 'node:path';

export interface OwnedCommandOptions {
  timeoutMs?: number;
  maxStreamBytes?: number;
  cwd?: string;
  env?: NodeJS.ProcessEnv;
  signal?: AbortSignal;
  /** Drain without retaining output or a lifetime deadline; requires an abort owner. */
  longLived?: boolean;
  /** Verified native creation/resume, not application readiness. Callback failures stop owned work. */
  onStarted?: () => void;
  /** Verify immediately before actual native creation (after Windows wrapper startup). */
  beforeSpawn?: () => Promise<void>;
  /** Reverify the pinned image after creation; Windows does this while it is suspended. */
  afterSpawn?: () => Promise<void>;
}
export type OwnedCommandOutput = Readonly<{ stdout: Buffer; stderr: Buffer; exitCode: number }>;

export class OwnedCommandError extends Error {
  constructor(
    readonly code: 'spawn' | 'timeout' | 'overflow' | 'abort' | 'stream' | 'cleanup',
    message: string,
    options?: ErrorOptions,
  ) {
    super(message, options);
    this.name = 'OwnedCommandError';
  }
}

// One non-inheritable Job per command. Only JSON data crosses the PowerShell boundary.
// The native image is suspended until Node has reverified it, and never runs outside the Job.
const windowsScript = `
$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue';
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class CadderOwnedCommand {
  [StructLayout(LayoutKind.Sequential)] struct BasicLimits {
    public long ProcessTime, JobTime; public uint Flags; public UIntPtr Min, Max;
    public uint Active; public UIntPtr Affinity; public uint Priority, Scheduling;
  }
  [StructLayout(LayoutKind.Sequential)] struct Counters { public ulong A,B,C,D,E,F; }
  [StructLayout(LayoutKind.Sequential)] struct Accounting {
    public long User, Kernel, PeriodUser, PeriodKernel; public uint Faults, Total, Active, Terminated;
  }
  [StructLayout(LayoutKind.Sequential)] struct Limits {
    public BasicLimits Basic; public Counters Io; public UIntPtr ProcessMemory, JobMemory, PeakProcess, PeakJob;
  }
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Startup {
    public uint Size; public string Reserved, Desktop, Title; public uint X,Y,XS,YS,XC,YC,Fill,Flags;
    public ushort Show, ReservedSize; public IntPtr ReservedData, In, Out, Err;
  }
  [StructLayout(LayoutKind.Sequential)] struct StartupEx { public Startup Startup; public IntPtr Attributes; }
  [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr Process, Thread; public uint Pid, Tid; }
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job, int kind, IntPtr data, uint size);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, UIntPtr attribute, IntPtr value, UIntPtr size, IntPtr previous, IntPtr returned);
  [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateJobObject(IntPtr job, uint code);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job, int kind, out Accounting data, uint size, IntPtr returned);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool CreatePipe(out IntPtr read, out IntPtr write, IntPtr attributes, uint size);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFile(string path, uint access, uint share, IntPtr attributes, uint creation, uint flags, IntPtr template);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcess(string image, StringBuilder command, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr environment, string cwd, ref StartupEx startup, out ProcessInfo info);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
  [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle, uint timeout);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
  static void Check(bool ok) { if (!ok) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); }
  static void StopJob(IntPtr job, uint code) {
    Check(TerminateJobObject(job, code));
    var deadline = System.Diagnostics.Stopwatch.StartNew();
    while (true) {
      Accounting data;
      Check(QueryInformationJobObject(job, 1, out data, (uint)Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero));
      if (data.Active == 0) return;
      if (deadline.ElapsedMilliseconds >= 4000) throw new TimeoutException("Owned Job did not terminate within four seconds.");
      System.Threading.Thread.Sleep(10);
    }
  }
  static string Quote(string value) {
    StringBuilder b = new StringBuilder(); b.Append('\\"'); int slashes = 0;
    foreach (char c in value) {
      if (c == '\\\\') { slashes++; continue; }
      if (c == '\\"') { b.Append('\\\\', slashes * 2 + 1); b.Append(c); }
      else { b.Append('\\\\', slashes); b.Append(c); }
      slashes = 0;
    }
    b.Append('\\\\', slashes * 2); b.Append('\\"'); return b.ToString();
  }
  public static int Run(string image, string[] args, string cwd, long maxStreamBytes) {
    IntPtr job = IntPtr.Zero, input = new IntPtr(-1), attributes = IntPtr.Zero, jobValue = IntPtr.Zero;
    ProcessInfo p = new ProcessInfo(); bool attributeReady = false;
    IntPtr outRead = IntPtr.Zero, outWrite = IntPtr.Zero, errRead = IntPtr.Zero, errWrite = IntPtr.Zero;
    System.IO.FileStream outPipe = null, errPipe = null;
    System.Threading.Tasks.Task outPump = null, errPump = null;
    int overflow = 0;
    try {
      job = CreateJobObject(IntPtr.Zero, null); Check(job != IntPtr.Zero);
      Limits limits = new Limits(); limits.Basic.Flags = 0x2000;
      int size = Marshal.SizeOf(limits); IntPtr data = Marshal.AllocHGlobal(size);
      try { Marshal.StructureToPtr(limits, data, false); Check(SetInformationJobObject(job, 9, data, (uint)size)); }
      finally { Marshal.FreeHGlobal(data); }
      input = CreateFile("NUL", 0x80000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
      Check(input != new IntPtr(-1)); Check(SetHandleInformation(input, 1, 1));
      StartupEx s = new StartupEx(); s.Startup.Size = (uint)Marshal.SizeOf(s); s.Startup.Flags = 0x100;
      Check(CreatePipe(out outRead, out outWrite, IntPtr.Zero, 0));
      Check(CreatePipe(out errRead, out errWrite, IntPtr.Zero, 0));
      Check(SetHandleInformation(outWrite, 1, 1)); Check(SetHandleInformation(errWrite, 1, 1));
      s.Startup.In = input; s.Startup.Out = outWrite; s.Startup.Err = errWrite;
      IntPtr attributeSize = IntPtr.Zero;
      InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attributeSize);
      attributes = Marshal.AllocHGlobal(attributeSize);
      Check(InitializeProcThreadAttributeList(attributes, 1, 0, ref attributeSize)); attributeReady = true;
      jobValue = Marshal.AllocHGlobal(IntPtr.Size); Marshal.WriteIntPtr(jobValue, job);
      Check(UpdateProcThreadAttribute(attributes, 0, new UIntPtr(0x2000d), jobValue, new UIntPtr((uint)IntPtr.Size), IntPtr.Zero, IntPtr.Zero));
      s.Attributes = attributes;
      StringBuilder command = new StringBuilder(Quote(image));
      foreach (string arg in args) command.Append(" ").Append(Quote(arg));
      var output = Console.OpenStandardOutput(); var control = Console.OpenStandardInput();
      output.WriteByte(1); output.Flush();
      if (control.ReadByte() != 1) throw new Exception("Missing pre-creation image verification.");
      Check(CreateProcess(image, command, IntPtr.Zero, IntPtr.Zero, true, 0x08080004, IntPtr.Zero, String.IsNullOrEmpty(cwd) ? null : cwd, ref s, out p));
      Check(CloseHandle(outWrite)); outWrite = IntPtr.Zero;
      Check(CloseHandle(errWrite)); errWrite = IntPtr.Zero;
      output.WriteByte(2); output.Flush();
      if (control.ReadByte() != 2) throw new Exception("Missing post-creation image verification.");
      Check(ResumeThread(p.Thread) != 0xffffffff);
      output.WriteByte(3); output.Flush();
      outPipe = new System.IO.FileStream(new Microsoft.Win32.SafeHandles.SafeFileHandle(outRead, true), System.IO.FileAccess.Read); outRead = IntPtr.Zero;
      errPipe = new System.IO.FileStream(new Microsoft.Win32.SafeHandles.SafeFileHandle(errRead, true), System.IO.FileAccess.Read); errRead = IntPtr.Zero;
      outPump = System.Threading.Tasks.Task.Factory.StartNew(() => {
        byte[] buffer = new byte[81920]; long forwarded = 0; int count;
        while ((count = outPipe.Read(buffer, 0, buffer.Length)) != 0) {
          if (maxStreamBytes >= 0 && count > maxStreamBytes - forwarded) {
            System.Threading.Interlocked.Exchange(ref overflow, 1);
            throw new System.IO.IOException("Owned stdout payload exceeded its limit.");
          }
          output.Write(buffer, 0, count);
          if (maxStreamBytes >= 0) forwarded += count;
        }
      });
      errPump = System.Threading.Tasks.Task.Factory.StartNew(() => errPipe.CopyTo(Console.OpenStandardError()));
      var stop = System.Threading.Tasks.Task.Factory.StartNew(() => control.ReadByte());
      while (WaitForSingleObject(p.Process, 20) != 0) {
        if (outPump.IsFaulted || errPump.IsFaulted) throw new Exception("Owned output forwarding failed.");
        if (stop.IsCompleted) { StopJob(job, 1); break; }
      }
      uint code; Check(GetExitCodeProcess(p.Process, out code));
      StopJob(job, code);
      System.Threading.Tasks.Task.WaitAll(outPump, errPump);
      byte[] result = BitConverter.GetBytes(code); output.Write(result, 0, result.Length); output.Flush();
      return 0;
    } catch {
      if (job != IntPtr.Zero) StopJob(job, 1);
      // Job settlement precedes pipe settlement: descendants may hold the write ends.
      if (outPump != null) { try { outPump.Wait(); } catch (AggregateException) {} }
      if (errPump != null) { try { errPump.Wait(); } catch (AggregateException) {} }
      return overflow != 0 ? 254 : 252;
    } finally {
      if (p.Process != IntPtr.Zero) {
        TerminateJobObject(job, 1);
        WaitForSingleObject(p.Process, 5000); CloseHandle(p.Process);
      }
      if (outPipe != null) outPipe.Dispose(); if (errPipe != null) errPipe.Dispose();
      if (outRead != IntPtr.Zero) CloseHandle(outRead); if (outWrite != IntPtr.Zero) CloseHandle(outWrite);
      if (errRead != IntPtr.Zero) CloseHandle(errRead); if (errWrite != IntPtr.Zero) CloseHandle(errWrite);
      if (p.Thread != IntPtr.Zero) CloseHandle(p.Thread);
      if (input != new IntPtr(-1)) CloseHandle(input);
      if (job != IntPtr.Zero) CloseHandle(job);
      if (attributeReady) DeleteProcThreadAttributeList(attributes);
      if (attributes != IntPtr.Zero) Marshal.FreeHGlobal(attributes);
      if (jobValue != IntPtr.Zero) Marshal.FreeHGlobal(jobValue);
    }
  }
}
'@;
try {
  $data = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:CADDER_OWNED_COMMAND_DATA)) | ConvertFrom-Json;
  [Environment]::SetEnvironmentVariable('CADDER_OWNED_COMMAND_DATA', $null);
  exit [CadderOwnedCommand]::Run($data.image, [string[]]$data.args, $data.cwd, [long]$data.maxStreamBytes);
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 253; }
`;

/** Direct argv, separately bounded binary streams, and cleanup limited to this owned group/Job. */
export async function runOwnedCommand(
  image: string,
  args: readonly string[],
  options: OwnedCommandOptions = {},
): Promise<OwnedCommandOutput> {
  if (!isAbsolute(image) || image.includes('\0') || args.some((arg) => arg.includes('\0')))
    throw new OwnedCommandError(
      'spawn',
      'Owned command requires an absolute image and NUL-free argv.',
    );
  if (options.longLived && !options.signal)
    throw new RangeError('Long-lived execution requires an abort owner.');
  const timeoutMs = options.timeoutMs ?? 30_000;
  const limit = options.maxStreamBytes ?? 1024 * 1024;
  if (
    !Number.isSafeInteger(timeoutMs) ||
    timeoutMs <= 0 ||
    !Number.isSafeInteger(limit) ||
    limit < 0
  )
    throw new RangeError('Command deadline and stream limit must be bounded nonnegative integers.');
  if (options.signal?.aborted) throw new OwnedCommandError('abort', 'Owned command cancelled.');
  const windows = process.platform === 'win32';
  if (!windows) await options.beforeSpawn?.();
  if (options.signal?.aborted) throw new OwnedCommandError('abort', 'Owned command cancelled.');
  const env = { ...(options.env ?? process.env) };
  let executable = image;
  let argv = [...args];
  if (windows) {
    executable = join(
      process.env.SystemRoot ?? 'C:\\Windows',
      'System32',
      'WindowsPowerShell',
      'v1.0',
      'powershell.exe',
    );
    argv = [
      '-NoProfile',
      '-NonInteractive',
      '-EncodedCommand',
      Buffer.from(windowsScript, 'utf16le').toString('base64'),
    ];
    env.CADDER_OWNED_COMMAND_DATA = Buffer.from(
      JSON.stringify({
        image,
        args,
        cwd: options.cwd ?? null,
        maxStreamBytes: options.longLived ? -1 : limit,
      }),
    ).toString('base64');
    for (const key of Object.keys(env)) if (key.toLowerCase() === 'psmodulepath') delete env[key];
    env.PSModulePath = join(executable, '..', 'Modules');
  }
  return await new Promise<OwnedCommandOutput>((resolve, reject) => {
    let child: ChildProcessWithoutNullStreams;
    try {
      child = spawn(executable, argv, {
        shell: false,
        detached: !windows,
        windowsHide: true,
        env,
        ...(options.cwd === undefined ? {} : { cwd: options.cwd }),
        stdio: 'pipe',
      });
    } catch (error) {
      reject(new OwnedCommandError('spawn', 'Cannot create owned command.', { cause: error }));
      return;
    }
    const chunks: Buffer[][] = [[], []];
    const counts = [0, 0];
    let stdoutTail = Buffer.alloc(0);
    let failure: unknown;
    let phase = windows ? 0 : 3;
    let settled = false;
    let cleanupTimer: ReturnType<typeof setTimeout> | undefined;
    let pendingVerification = Promise.resolve();
    const terminate = () => {
      try {
        if (!windows && child.pid !== undefined) process.kill(-child.pid, 'SIGKILL');
        else if (phase >= 2) child.stdin.end(Buffer.from([3]));
        else child.stdin.end();
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ESRCH' && failure === undefined)
          failure = error;
      }
    };
    const finish = (code: number | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(cleanupTimer);
      options.signal?.removeEventListener('abort', abort);
      child.stdin.destroy();
      child.stdout.destroy();
      child.stderr.destroy();
      if (windows && code === 253)
        failure = new OwnedCommandError('cleanup', 'Owned Job settlement could not be proved.');
      if (windows && code === 254 && failure === undefined)
        failure = new OwnedCommandError(
          'overflow',
          'Owned command output exceeded its per-stream limit.',
        );
      if (failure !== undefined) reject(failure);
      else if (phase !== 3 || code === null || (windows && (code !== 0 || stdoutTail.length !== 4)))
        reject(
          new OwnedCommandError('spawn', 'Owned command did not complete native execution.', {
            cause: new Error(Buffer.concat(chunks[1]!).subarray(0, 4096).toString('utf8')),
          }),
        );
      else
        resolve({
          stdout: Buffer.concat(chunks[0]!),
          stderr: Buffer.concat(chunks[1]!),
          exitCode: windows ? stdoutTail.readUInt32LE(0) : code,
        });
    };
    const fail = (error: unknown) => {
      if (failure !== undefined || settled) return;
      failure = error;
      terminate();
      clearTimeout(cleanupTimer);
      cleanupTimer = setTimeout(() => {
        failure = new AggregateError(
          [failure, new OwnedCommandError('cleanup', 'Owned command cleanup exceeded 5 seconds.')],
          'Owned command and cleanup failed.',
          { cause: failure },
        );
        child.kill('SIGKILL');
        finish(null);
      }, 5000);
    };
    const verify = async (before = false) => {
      try {
        if (before) await options.beforeSpawn?.();
        else await options.afterSpawn?.();
        if (failure === undefined && !settled) {
          if (windows && before) child.stdin.write(Buffer.from([1]));
          else if (windows) child.stdin.write(Buffer.from([2]));
          else {
            child.stdin.end();
            if (child.exitCode === null && child.signalCode === null) options.onStarted?.();
          }
        }
      } catch (error) {
        fail(error);
      }
    };
    const abort = () => fail(new OwnedCommandError('abort', 'Owned command cancelled.'));
    const timer = options.longLived
      ? undefined
      : setTimeout(
          () => fail(new OwnedCommandError('timeout', 'Owned command exceeded its deadline.')),
          timeoutMs,
        );
    options.signal?.addEventListener('abort', abort, { once: true });
    if (options.signal?.aborted) abort();
    child.once('error', (error) =>
      fail(new OwnedCommandError('spawn', 'Cannot start owned command.', { cause: error })),
    );
    child.once('spawn', () => {
      if (!windows) pendingVerification = verify();
    });
    child.once('exit', (code) => {
      // Kill residual Unix group members even when the leader exited successfully.
      if (!windows) terminate();
      if (settled || failure !== undefined) return;
      clearTimeout(timer);
      cleanupTimer = setTimeout(() => {
        failure = new OwnedCommandError(
          'stream',
          'Owned command streams did not settle after exit.',
        );
        finish(code);
      }, 5000);
    });
    child.once('close', (code) => {
      void pendingVerification.then(async () => {
        if (!windows && child.pid !== undefined) {
          const deadline = performance.now() + 4000;
          while (!settled) {
            try {
              process.kill(-child.pid, 0);
            } catch (error) {
              if ((error as NodeJS.ErrnoException).code === 'ESRCH') break;
              failure = new OwnedCommandError('cleanup', 'Cannot observe owned group settlement.');
              break;
            }
            if (performance.now() >= deadline) {
              failure = new OwnedCommandError(
                'cleanup',
                'Owned group settlement exceeded its deadline.',
              );
              break;
            }
            await new Promise((resolve) => setTimeout(resolve, 10));
          }
        }
        finish(code);
      });
    });
    child.stdin.on('error', (error) =>
      fail(new OwnedCommandError('stream', 'Owned command stdin failed.', { cause: error })),
    );
    for (const [index, stream] of [child.stdout, child.stderr].entries()) {
      stream.on('error', (error) =>
        fail(new OwnedCommandError('stream', 'Owned command output failed.', { cause: error })),
      );
      stream.on('data', (bytes: Buffer) => {
        if (settled || failure !== undefined) return;
        while (windows && index === 0 && phase < 3 && bytes.length > 0) {
          if (bytes[0] !== phase + 1) {
            fail(new OwnedCommandError('spawn', 'Invalid owned command creation handshake.'));
            return;
          }
          phase++;
          bytes = bytes.subarray(1);
          if (phase < 3) pendingVerification = verify(phase === 1);
          else {
            try {
              if (child.exitCode === null && child.signalCode === null) options.onStarted?.();
            } catch (error) {
              fail(error);
              return;
            }
          }
        }
        if (windows && index === 0 && phase === 3) {
          // Only wrapper exit 0 authorizes decoding this fixed-size completion trailer.
          const combined = Buffer.concat([stdoutTail, bytes]);
          const payloadBytes = Math.max(0, combined.length - 4);
          bytes = combined.subarray(0, payloadBytes);
          stdoutTail = Buffer.from(combined.subarray(payloadBytes));
        }
        if (options.longLived) return;
        counts[index]! += bytes.length;
        if (counts[index]! > limit) {
          fail(
            new OwnedCommandError(
              'overflow',
              'Owned command output exceeded its per-stream limit.',
            ),
          );
          return;
        }
        chunks[index]!.push(bytes);
      });
    }
  });
}
