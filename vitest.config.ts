import { defineConfig } from 'vitest/config';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export default defineConfig({
  test: {
    include: ['test/**/*.test.{ts,tsx}'],
    testTimeout: 15000,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.{ts,tsx}'],
      thresholds: { lines: 85 },
      reporter: ['text', 'json-summary', 'lcov'],
      reportsDirectory:
        process.env.CADDER_COVERAGE_DIR ?? join(tmpdir(), `cadder-coverage-${process.pid}`),
    },
  },
});
