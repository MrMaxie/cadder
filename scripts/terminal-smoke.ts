import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

/** Native Unix PTY; Windows acceptance uses the actual ConPTY terminal. */
export async function terminalSmoke(
  file: string,
  prefix: string[],
  environment: NodeJS.ProcessEnv,
): Promise<void> {
  if (process.platform === 'win32') return;
  const execute = promisify(execFile);
  for (const args of [[], ['tui']]) {
    const command = [file, ...prefix, ...args];
    const quoted = command.map((part) => "'" + part.replaceAll("'", "'\\''") + "'").join(' ');
    const running = execute(
      '/usr/bin/script',
      process.platform === 'darwin'
        ? ['-q', '/dev/null', ...command]
        : ['-qec', quoted, '/dev/null'],
      {
        env: { ...environment, CI: 'false', TERM: 'xterm-256color' },
        timeout: 20000,
        maxBuffer: 1024 * 1024,
      },
    );
    let frame = '';
    let quit = false;
    running.child.stdout?.on('data', (bytes: Buffer) => {
      frame = (frame + bytes.toString()).slice(-8192);
      if (args.length && !quit && frame.includes('Press Enter')) {
        quit = true;
        running.child.stdin?.write('q');
      }
    });
    const { stdout } = await running;
    assert.match(stdout, args.length ? /Press Enter/ : /Usage: cadder/);
    if (args.length) assert.ok(stdout.includes('\x1b[?25h'), 'TUI must restore cursor visibility.');
  }
}
