import { spawn } from 'node:child_process';
import { existsSync, writeFileSync } from 'node:fs';

const [mode, marker] = process.argv.slice(2);
if (!marker) throw new Error('Missing owned test marker.');
if (mode === 'grandchild') {
  writeFileSync(marker, String(process.pid));
  process.stdout.write('inherited stdout\n');
  process.stderr.write('inherited stderr\n');
  setInterval(() => {}, 1000);
} else {
  const child = spawn(process.execPath, [process.argv[1]!, 'grandchild', marker], {
    shell: false,
    // Avoid Node/libuv's own non-detached child Job Object; a native adapter may leave an orphan.
    detached: process.platform === 'win32',
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  child.once('error', (error) => {
    throw error;
  });
  child.unref();
  const ready = setInterval(() => {
    if (existsSync(marker)) {
      clearInterval(ready);
      process.exit(0);
    }
  }, 10);
}
