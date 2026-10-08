import { execFile, spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, normalize } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { promisify } from 'node:util';
import { build } from 'esbuild';
import { expect, it, vi } from 'vitest';
import { runOwnedCommand } from '../src/platform/owned-command.ts';

vi.mock('node:child_process', async (original) => {
  const actual = await original<typeof import('node:child_process')>();
  return { ...actual, spawn: vi.fn(actual.spawn) };
});
const execute = promisify(execFile);

it('owned command preserves native argv and binary streams', async () => {
  const args = ['space value', 'quote"value', 'trailing\\', '', 'line\nvalue'];
  const output = await runOwnedCommand(process.execPath, [
    '-e',
    'process.stdout.write(JSON.stringify(process.argv.slice(1))); process.stderr.write(Buffer.from([0,255,128]));',
    '--',
    ...args,
  ]);
  expect(output.exitCode).toBe(0);
  expect(JSON.parse(output.stdout.toString())).toEqual(args);
  expect(output.stderr).toEqual(Buffer.from([0, 255, 128]));
});

it('verified startup notification precedes immediate binary output without leaking handshake bytes', async () => {
  const events: string[] = [];
  const output = await runOwnedCommand(
    process.execPath,
    [
      '-e',
      'process.stdout.write(Buffer.from([1,2,3,0,255]));process.stderr.write(Buffer.from([3,2,1,128]));setTimeout(()=>{},100);',
    ],
    {
      beforeSpawn: async () => {
        await delay(20);
        events.push('before');
      },
      afterSpawn: async () => {
        await delay(20);
        events.push('after');
      },
      onStarted: () => {
        events.push('started');
      },
    },
  );
  expect(events).toEqual(['before', 'after', 'started']);
  expect(output.stdout).toEqual(Buffer.from([1, 2, 3, 0, 255]));
  expect(output.stderr).toEqual(Buffer.from([3, 2, 1, 128]));
});

it('startup notification failure or cancellation settles the same owned boundary', async () => {
  await expect(
    runOwnedCommand(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {
      onStarted: () => {
        throw new Error('fixture startup callback failed');
      },
    }),
  ).rejects.toThrow('fixture startup callback failed');
  const abort = new AbortController();
  await expect(
    runOwnedCommand(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {
      signal: abort.signal,
      onStarted: () => abort.abort(),
    }),
  ).rejects.toMatchObject({ code: 'abort' });
});

it.each([0, 1, 252, 253, 254])(
  'preserves native exit code %s without reserving application codes',
  async (code) => {
    const output = await runOwnedCommand(process.execPath, [
      '-e',
      `process.stdout.write(Buffer.from([1,2,3,252,253,0]));process.exit(${code})`,
    ]);
    expect(output.exitCode).toBe(code);
    expect(output.stdout).toEqual(Buffer.from([1, 2, 3, 252, 253, 0]));
  },
);

it.skipIf(process.platform !== 'win32')(
  'preserves native unsigned Windows exit codes',
  async () => {
    const output = await runOwnedCommand(process.execPath, ['-e', 'process.exit(2147483648)']);
    expect(output.exitCode).toBe(2147483648);
  },
);

it.skipIf(process.platform !== 'win32')(
  'completion framing supports fragmented/coalesced binary payload at exact and zero limits',
  async () => {
    for (const payload of [Buffer.alloc(0), Buffer.from([1, 2, 3, 253, 252, 0, 1, 2])]) {
      const trailer = Buffer.alloc(4);
      trailer.writeUInt32LE(253);
      const bytes = Buffer.concat([Buffer.from([3]), payload, trailer]);
      // Separate handshake messages; split the completion code across events and coalesce payload/control bytes.
      fakeWrapper(
        [
          bytes.subarray(0, 2),
          bytes.subarray(2, bytes.length - 2),
          bytes.subarray(bytes.length - 2),
        ],
        0,
      );
      const output = await runOwnedCommand(process.execPath, [], {
        maxStreamBytes: payload.length,
      });
      expect(output.stdout).toEqual(payload);
      expect(output.exitCode).toBe(253);
    }
  },
);

it.skipIf(process.platform !== 'win32').each(['missing', 'partial', 'fault'] as const)(
  'never accepts %s wrapper finalization',
  async (kind) => {
    let bytes = Buffer.from([3, 1, 2, 3]);
    if (kind === 'missing') bytes = Buffer.from([3]);
    else if (kind === 'fault') bytes = Buffer.from([3, 0, 0, 0, 0]);
    fakeWrapper([bytes], kind === 'fault' ? 252 : 0);
    await expect(runOwnedCommand(process.execPath, [])).rejects.toMatchObject({ code: 'spawn' });
  },
);

function fakeWrapper(chunks: readonly Buffer[], code: number): void {
  vi.mocked(spawn).mockImplementationOnce(() => {
    const child = Object.assign(new EventEmitter(), {
      stdin: new PassThrough(),
      stdout: new PassThrough(),
      stderr: new PassThrough(),
      exitCode: null as number | null,
      signalCode: null,
      kill: () => true,
    });
    child.stdin.on('data', (bytes: Buffer) => {
      if (bytes[0] === 1) child.stdout.write(Buffer.from([2]));
      if (bytes[0] === 2) {
        for (const chunk of chunks) child.stdout.write(chunk);
        child.stdout.end();
        child.stderr.end();
        queueMicrotask(() => {
          child.exitCode = code;
          child.emit('exit', code);
          child.emit('close', code);
        });
      }
    });
    queueMicrotask(() => {
      child.emit('spawn');
      child.stdout.write(Buffer.from([1]));
    });
    return child as unknown as ReturnType<typeof spawn>;
  });
}

it('owned command retains stream boundary, cwd, stdin EOF and nonzero native exit', async () => {
  const output = await runOwnedCommand(
    process.execPath,
    [
      '-e',
      'process.stdin.on("end",()=>{process.stdout.write(Buffer.alloc(16,1));process.stderr.write(Buffer.alloc(16,2));process.exitCode=7});process.stdin.resume()',
    ],
    { maxStreamBytes: 16, cwd: tmpdir() },
  );
  expect(output.exitCode).toBe(7);
  expect(output.stdout).toEqual(Buffer.alloc(16, 1));
  expect(output.stderr).toEqual(Buffer.alloc(16, 2));
  const cwd = await runOwnedCommand(
    process.execPath,
    ['-e', 'process.stdout.write(process.cwd())'],
    { cwd: tmpdir() },
  );
  expect(normalize(cwd.stdout.toString()).toLowerCase()).toBe(normalize(tmpdir()).toLowerCase());
});

it.each([0, 16])(
  'native stdout respects exact %s-byte payload bounds including its binary suffix',
  async (limit) => {
    const output = await runOwnedCommand(
      process.execPath,
      [
        '-e',
        `process.stdout.write(Buffer.alloc(${limit},253));process.stderr.write(Buffer.alloc(${limit},254));`,
      ],
      { maxStreamBytes: limit },
    );
    expect(output.stdout).toEqual(Buffer.alloc(limit, 253));
    expect(output.stderr).toEqual(Buffer.alloc(limit, 254));
    expect(output.exitCode).toBe(0);
  },
);

it.each([0, 16])(
  'hanging stdout max+1 rejects at %s-byte bounds without waiting for command deadline',
  async (limit) => {
    await expect(
      runOwnedCommand(
        process.execPath,
        ['-e', `process.stdout.write(Buffer.alloc(${limit}+1));setInterval(()=>{},1000)`],
        { maxStreamBytes: limit, timeoutMs: 10_000 },
      ),
    ).rejects.toMatchObject({ code: 'overflow' });
  },
);

it.skipIf(process.platform !== 'win32').each([254, 253])(
  'wrapper status %s distinguishes proved overflow from cleanup failure',
  async (status) => {
    fakeWrapper([Buffer.from([3, 0, 0, 0, 0])], status);
    await expect(runOwnedCommand(process.execPath, [])).rejects.toMatchObject({
      code: status === 254 ? 'overflow' : 'cleanup',
    });
  },
);

it('default metadata output bound rejects more than 1 MiB', async () => {
  await expect(
    runOwnedCommand(process.execPath, [
      '-e',
      'process.stdout.write(Buffer.alloc(1024*1024+1));setInterval(()=>{},1000)',
    ]),
  ).rejects.toMatchObject({ code: 'overflow' });
});

it('long-lived execution drains beyond command limits until owned cancellation settles', async () => {
  const abort = new AbortController();
  const command = runOwnedCommand(
    process.execPath,
    [
      '-e',
      'setInterval(()=>{process.stdout.write(Buffer.alloc(65536));process.stderr.write(Buffer.alloc(65536))},10)',
    ],
    { longLived: true, signal: abort.signal, timeoutMs: 1, maxStreamBytes: 1 },
  );
  setTimeout(() => abort.abort(), 2500);
  await expect(command).rejects.toMatchObject({ code: 'abort' });
  await expect(runOwnedCommand(process.execPath, [], { longLived: true })).rejects.toThrow(
    RangeError,
  );
});

it('pre-aborted and invalid command options create no child', async () => {
  await expect(runOwnedCommand('relative-caddy', [])).rejects.toMatchObject({ code: 'spawn' });
  await expect(runOwnedCommand(process.execPath, ['bad\0argument'])).rejects.toMatchObject({
    code: 'spawn',
  });
  await expect(
    runOwnedCommand(process.execPath, [], { signal: AbortSignal.abort() }),
  ).rejects.toMatchObject({ code: 'abort' });
  await expect(runOwnedCommand(process.execPath, [], { timeoutMs: 0 })).rejects.toThrow(RangeError);
  await expect(runOwnedCommand(process.execPath, [], { maxStreamBytes: -1 })).rejects.toThrow(
    RangeError,
  );
});

it('post-creation verification cannot be lost when a fast child exits', async () => {
  let started = false;
  await expect(
    runOwnedCommand(process.execPath, ['-e', 'process.exit(0)'], {
      afterSpawn: async () => {
        await delay(200);
        throw new Error('delayed pin mismatch');
      },
      onStarted: () => {
        started = true;
      },
    }),
  ).rejects.toThrow('delayed pin mismatch');
  expect(started).toBe(false);
});

it('owned command fails native creation without usable partial output', async () => {
  await expect(
    runOwnedCommand(join(tmpdir(), 'missing-caddy-native-image'), []),
  ).rejects.toMatchObject({ code: 'spawn' });
});

it.each(['stdout', 'stderr'])('owned command rejects %s overflow immediately', async (stream) => {
  await expect(
    runOwnedCommand(
      process.execPath,
      ['-e', `process.${stream}.write(Buffer.alloc(2048)); setInterval(()=>{},1000)`],
      { maxStreamBytes: 1024 },
    ),
  ).rejects.toMatchObject({ code: 'overflow' });
});

it.each(['timeout', 'abort'] as const)(
  'owned %s settles descendants, not an unrelated process',
  async (mode) => {
    const directory = await mkdtemp(join(tmpdir(), 'cadder-owned-command-'));
    const marker = join(directory, 'descendant.pid');
    const unrelated = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {
      stdio: 'ignore',
    });
    let descendant: number | undefined;
    const abort = new AbortController();
    const timer = mode === 'abort' ? setTimeout(() => abort.abort(), 4000) : undefined;
    try {
      const source = `const {spawn}=require('node:child_process'); const {writeFileSync}=require('node:fs'); const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:process.platform==='win32',stdio:['ignore','inherit','inherit']}); writeFileSync(process.argv[1],String(child.pid)); setInterval(()=>{},1000);`;
      await expect(
        runOwnedCommand(process.execPath, ['-e', source, marker], {
          timeoutMs: 4000,
          ...(mode === 'abort' ? { longLived: true, signal: abort.signal } : {}),
        }),
      ).rejects.toMatchObject({ code: mode });
      descendant = Number(await readFile(marker, 'utf8'));
      expect(alive(descendant)).toBe(false);
      expect(alive(unrelated.pid!)).toBe(true);
    } finally {
      clearTimeout(timer);
      if (descendant && alive(descendant)) process.kill(descendant, 'SIGKILL');
      unrelated.kill('SIGKILL');
      await new Promise<void>((resolve) => {
        if (unrelated.exitCode !== null) resolve();
        else unrelated.once('exit', () => resolve());
      });
      await rm(directory, { recursive: true, force: true });
    }
  },
);

it('owned command cancellation and failed post-creation pin check settle resources', async () => {
  const abort = new AbortController();
  const command = runOwnedCommand(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {
    signal: abort.signal,
  });
  setTimeout(() => abort.abort(), 2000);
  await expect(command).rejects.toMatchObject({ code: 'abort' });
  await expect(
    runOwnedCommand(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {
      afterSpawn: async () => {
        throw new Error('post-spawn pin mismatch');
      },
    }),
  ).rejects.toThrow('pin mismatch');
});

it('owned Job/group closes inherited pipes after the leader exits and cleans the orphan', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'cadder-owned-orphan-'));
  const fixture = join(directory, 'orphan.cjs');
  const marker = join(directory, 'grandchild.pid');
  let grandchild: number | undefined;
  try {
    await build({
      entryPoints: ['test/fixtures/caddy-orphan.ts'],
      bundle: true,
      platform: 'node',
      format: 'cjs',
      outfile: fixture,
    });
    const output = await runOwnedCommand(process.execPath, [fixture, 'parent', marker], {
      timeoutMs: 5000,
    });
    grandchild = Number(await readFile(marker, 'utf8'));
    expect(output.exitCode).toBe(0);
    expect(output.stdout.toString()).toBe('inherited stdout\n');
    expect(output.stderr.toString()).toBe('inherited stderr\n');
    expect(alive(grandchild)).toBe(false);
  } finally {
    if (grandchild && alive(grandchild)) process.kill(grandchild, 'SIGKILL');
    await rm(directory, { recursive: true, force: true });
  }
});
it.skipIf(process.platform === 'win32')(
  'escaped Unix pipe holders produce a bounded stream-settle error, not an operation timeout',
  async () => {
    const directory = await mkdtemp(join(tmpdir(), 'cadder-owned-escape-'));
    const marker = join(directory, 'escaped.pid');
    let escaped: number | undefined;
    try {
      const source = `const {spawn}=require('node:child_process'); const {writeFileSync}=require('node:fs'); const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:['ignore','inherit','inherit']});writeFileSync(process.argv[1],String(child.pid));child.unref();process.exit(0);`;
      await expect(
        runOwnedCommand(process.execPath, ['-e', source, marker], { timeoutMs: 10000 }),
      ).rejects.toMatchObject({ code: 'stream' });
      escaped = Number(await readFile(marker, 'utf8'));
      expect(alive(escaped)).toBe(true);
    } finally {
      // A fixture-known escaped PID is not a production process-enumeration permission.
      if (escaped === undefined) {
        try {
          escaped = Number(await readFile(marker, 'utf8'));
        } catch {
          /* No native child was created. */
        }
      }
      if (escaped && alive(escaped)) process.kill(escaped, 'SIGKILL');
      await rm(directory, { recursive: true, force: true });
    }
  },
);

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** A capability reproducer, not acceptance of taskkill as an owned-tree implementation. */
it.skipIf(process.platform !== 'win32')(
  'native Windows taskkill cannot clean an exited parent with inherited pipes',
  async () => {
    const directory = await mkdtemp(join(tmpdir(), 'cadder-caddy-orphan-'));
    const fixture = join(directory, 'orphan.cjs');
    const marker = join(directory, 'grandchild.pid');
    let grandchild: number | undefined;
    let child: ReturnType<typeof spawn> | undefined;
    try {
      await build({
        entryPoints: ['test/fixtures/caddy-orphan.ts'],
        bundle: true,
        platform: 'node',
        format: 'cjs',
        outfile: fixture,
      });
      child = spawn(process.execPath, [fixture, 'parent', marker], {
        shell: false,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      let stdoutClosed = false;
      let stderrClosed = false;
      child.stdout!.on('close', () => {
        stdoutClosed = true;
      });
      child.stderr!.on('close', () => {
        stderrClosed = true;
      });
      child.stdout!.resume();
      let diagnostic = '';
      child.stderr!.on('data', (bytes: Buffer) => {
        diagnostic += bytes.toString();
      });
      const leader = child.pid!;
      await new Promise<void>((resolve, reject) => {
        child!.once('error', reject);
        child!.once('exit', (code) =>
          code === 0 ? resolve() : reject(new Error(`Leader exited ${code}`)),
        );
      });
      for (let attempt = 0; attempt < 100; attempt++) {
        try {
          grandchild = Number(await readFile(marker, 'utf8'));
          break;
        } catch {
          await delay(20);
        }
      }
      expect(grandchild, diagnostic).toBeDefined();
      expect(alive(grandchild!)).toBe(true);
      const outcome = await execute(
        join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'taskkill.exe'),
        ['/PID', String(leader), '/T', '/F'],
        { timeout: 5000, maxBuffer: 1024 * 1024 },
      ).then(
        () => 'unexpected success',
        (error: unknown) => String(error),
      );
      await delay(100);
      expect(outcome).not.toBe('unexpected success');
      expect(alive(grandchild!)).toBe(true);
      expect(stdoutClosed).toBe(false);
      expect(stderrClosed).toBe(false);
      console.info(
        `Owned-tree blocker: leader=${leader} exited; taskkill /T /F failed; orphan=${grandchild} alive and both inherited pipes open. ${outcome}`,
      );
    } finally {
      // Test owns this exact fixture PID; product code may not substitute this for tree tracking.
      if (grandchild !== undefined && alive(grandchild)) process.kill(grandchild, 'SIGKILL');
      child?.stdout?.destroy();
      child?.stderr?.destroy();
      if (child?.exitCode === null) child.kill('SIGKILL');
      await rm(directory, { recursive: true, force: true });
    }
  },
);
