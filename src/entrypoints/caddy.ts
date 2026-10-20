#!/usr/bin/env node
import { runShim } from '../shim/run.ts';
import { terminalSafe } from '../storage/redaction.ts';
try {
  process.exitCode = await runShim(process.argv.slice(2));
} catch (error) {
  process.stderr.write(
    terminalSafe(error instanceof Error ? error.message : 'Caddy shim failed.') + '\n',
  );
  process.exitCode = 1;
}
