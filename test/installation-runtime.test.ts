import { expectTypeOf, it } from 'vitest';
import type { startLocalRuntime } from '../src/daemon/local-runtime.ts';
import { verifyInstallationRuntimes } from './fixtures/installation-runtime-smoke.ts';

it(
  'holds two real SQLite lifetime locks and authenticated local listeners independently',
  {
    skip: process.getuid?.() === 0,
    timeout: 30000,
  },
  async () => {
    expectTypeOf<Parameters<typeof startLocalRuntime>[0]['installationRoot']>().toEqualTypeOf<
      string | undefined
    >();
    await verifyInstallationRuntimes();
  },
);
