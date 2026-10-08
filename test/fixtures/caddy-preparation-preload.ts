import { spawn } from 'node:child_process';
import { appendFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import Module from 'node:module';
import { basename } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { once } from 'node:events';

// A copied native Node image is the disposable executable. Its owned preload handles Caddy argv
// instead of letting Node interpret 'adapt'/'validate' as JavaScript source paths.
Module.runMain = () => {};
const args = process.argv.slice(1);
args[0] = basename(args[0]!);
type Behavior = {
  mode?:
    | 'success'
    | 'malformed'
    | 'invalid-utf8'
    | 'invalid-stderr'
    | 'exit'
    | 'stdout'
    | 'stderr'
    | 'block'
    | 'orphan'
    | 'tree'
    | 'wait';
  body?: string;
  bytes?: number;
  marker?: string;
  release?: string;
};
const settings = JSON.parse(readFileSync(`${process.execPath}.json`, 'utf8')) as {
  modules: string[];
  log: string;
  adapt?: Behavior;
  validate?: Behavior;
};

async function write(stream: NodeJS.WriteStream, bytes: Buffer | string): Promise<void> {
  if (!stream.write(bytes)) await once(stream, 'drain');
}
async function main(): Promise<void> {
  const command = args[0];
  if (command === 'version') {
    await write(process.stdout, 'v2.11.4 h1:fixture\n');
    return;
  }
  if (command === 'list-modules') {
    await write(
      process.stdout,
      JSON.stringify(settings.modules.map((module_name) => ({ module_name }))),
    );
    return;
  }
  if (command === 'descendant') {
    writeFileSync(args[1]!, String(process.pid));
    await write(process.stdout, 'descendant stdout\n');
    await write(process.stderr, 'descendant stderr\n');
    setInterval(() => {}, 1000);
    return;
  }
  if (command !== 'adapt' && command !== 'validate')
    throw new Error('Unexpected fake-Caddy command.');
  const path = args[args.indexOf('--config') + 1]!;
  const body = command === 'validate' ? readFileSync(path, 'utf8') : undefined;
  const sourceBody = command === 'adapt' ? readFileSync(path, 'utf8') : undefined;
  appendFileSync(
    settings.log,
    JSON.stringify({
      command,
      args,
      cwd: process.cwd(),
      pid: process.pid,
      image: process.execPath,
      path,
      body,
      sourceBody,
    }) + '\n',
  );
  const behavior = settings[command] ?? {};
  if (behavior.mode === 'tree' || behavior.mode === 'orphan') {
    const child = spawn(process.execPath, ['descendant', behavior.marker!], {
      shell: false,
      detached: process.platform === 'win32',
      stdio: ['ignore', 'inherit', 'inherit'],
    });
    child.once('error', (error) => {
      throw error;
    });
    child.unref();
    while (!existsSync(behavior.marker!)) await delay(10);
    if (behavior.mode === 'tree') {
      setInterval(() => {}, 1000);
      return;
    }
  }
  if (behavior.mode === 'block') {
    setInterval(() => {}, 1000);
    return;
  }
  if (behavior.mode === 'wait') while (!existsSync(behavior.release!)) await delay(10);
  if (behavior.mode === 'invalid-utf8') {
    await write(process.stdout, Buffer.from([0xff]));
    return;
  }
  if (behavior.mode === 'invalid-stderr') {
    await write(process.stderr, Buffer.from([0xff]));
    return;
  }
  if (behavior.mode === 'exit') {
    await write(process.stderr, behavior.body ?? 'fixture validation rejected');
    process.exitCode = 7;
    return;
  }
  if (behavior.mode === 'stdout' || behavior.mode === 'stderr') {
    const size = behavior.bytes!;
    if (behavior.mode === 'stdout' && command === 'adapt') {
      const prefix = '{"padding":"';
      const suffix = '"}';
      await write(
        process.stdout,
        prefix + 'x'.repeat(size - prefix.length - suffix.length) + suffix,
      );
    } else
      await write(
        behavior.mode === 'stdout' ? process.stdout : process.stderr,
        Buffer.alloc(size, 0x20),
      );
    if (behavior.mode === 'stderr' && command === 'adapt') await write(process.stdout, '{}');
    return;
  }
  if (command === 'adapt')
    await write(
      process.stdout,
      behavior.mode === 'malformed'
        ? '{"partial":'
        : (behavior.body ?? '{"apps":{"http":{"servers":{}}}}\n'),
    );
}
void main().then(
  () => {
    // Tree/block modes deliberately retain the event loop until the owned helper kills them.
    const behavior = args[0] === 'adapt' ? settings.adapt : settings.validate;
    if (args[0] !== 'descendant' && behavior?.mode !== 'tree' && behavior?.mode !== 'block')
      process.exit(process.exitCode ?? 0);
  },
  (error: unknown) => {
    console.error(error);
    process.exit(1);
  },
);
