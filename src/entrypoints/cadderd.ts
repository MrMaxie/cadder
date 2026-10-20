#!/usr/bin/env node
import { Command, InvalidArgumentError } from 'commander';
import { startDaemon } from '../daemon/start.ts';
import { ClientService } from '../client/service.ts';
import { resolvePaths } from '../daemon/paths.ts';
import { VERSION } from '../protocol/version.ts';
import { terminalSafe } from '../storage/redaction.ts';

const program = new Command('cadderd')
  .version(VERSION)
  .description('Own local IPC, project registrations, Caddy and diagnostic storage.')
  .option('--real-caddy <path>', 'Absolute trusted upstream Caddy executable')
  .option('--runtime-dir <path>', 'Explicit isolated runtime directory')
  .option('--runtime-owner <uid>', 'Explicit Unix runtime owner UID', (value) => {
    if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)))
      throw new InvalidArgumentError('Expected a Unix UID.');
    return Number(value);
  })
  .option('--background', 'Start or attach to the packaged daemon in the background');
await program.parseAsync();
const options = program.opts<{
  realCaddy?: string;
  runtimeDir?: string;
  runtimeOwner?: number;
  background?: boolean;
}>();
try {
  if (options.background) {
    const args = process.argv.slice(2).filter((argument) => argument !== '--background');
    await new ClientService(resolvePaths(options)).start(args);
  } else {
    const runtime = await startDaemon(options);
    const stop = (): void => {
      void runtime
        .stop()
        .then(() => {
          process.off('SIGINT', stop);
          process.off('SIGTERM', stop);
        })
        .catch(() => {
          process.stderr.write('Owned daemon shutdown failed.\n');
          process.exitCode = 1;
        });
    };
    process.on('SIGINT', stop);
    process.on('SIGTERM', stop);
  }
} catch (error) {
  process.stderr.write(
    terminalSafe(error instanceof Error ? error.message : 'Daemon startup failed.') + '\n',
  );
  process.exitCode = 1;
}
