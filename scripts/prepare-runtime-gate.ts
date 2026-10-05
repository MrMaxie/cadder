import { build } from 'esbuild';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const stage = await mkdtemp(join(tmpdir(), 'cadder-runtime-gate-'));
await build({
  entryPoints: {
    'runtime-child': fileURLToPath(new URL('../test/fixtures/runtime-child.ts', import.meta.url)),
    'runtime-smoke': fileURLToPath(new URL('../test/fixtures/runtime-smoke.ts', import.meta.url)),
  },
  outdir: stage,
  outExtension: { '.js': '.mjs' },
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node24.18',
});
process.stdout.write(`${stage}\n`);
