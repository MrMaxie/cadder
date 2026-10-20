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
  for (const scenario of ['help', 'lifecycle', 'explicit', 'interrupt']) {
    const args = scenario === 'help' ? [] : ['tui'];
    if (scenario === 'explicit') args.push('--start-daemon');
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
    let step = scenario === 'explicit' ? 1 : 0;
    running.child.stdout?.on('data', (bytes: Buffer) => {
      frame = (frame + bytes.toString()).slice(-8192);
      if (scenario === 'help') return;
      let input: string | undefined;
      if (step === 0 && frame.includes('Press Enter')) {
        input = scenario === 'interrupt' ? '\x03' : '\r';
      } else if (step === 1 && frame.includes('No projects registered')) {
        input = 's';
      } else if (step === 2 && frame.includes('Stop cadderd')) {
        input = 'y';
      } else if (step === 3 && frame.includes('Press Enter')) {
        input = 'q';
      }
      if (input !== undefined) {
        frame = '';
        step++;
        running.child.stdin?.write(input);
      }
    });
    const { stdout } = await running;
    assert.match(stdout, args.length ? /Press Enter/ : /Usage: cadder/);
    if (args.length) {
      assert.equal(step, scenario === 'interrupt' ? 1 : 4);
      assert.ok(stdout.includes('\x1b[?25h'), 'TUI must restore cursor visibility.');
    }
  }
}
