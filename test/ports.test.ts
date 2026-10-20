import { readFile } from 'node:fs/promises';
import { expect, it } from 'vitest';

it('keeps shared port contracts independent of runtime implementations', async () => {
  const source = await readFile(new URL('../src/contracts/ports.ts', import.meta.url), 'utf8');
  const imports = [...source.matchAll(/\bfrom\s+['"]([^'"]+)['"]/g)].map(
    ([, specifier]) => specifier,
  );
  expect(imports.length).toBeGreaterThan(0);
  expect(imports.every((specifier) => specifier?.startsWith('../protocol/'))).toBe(true);
});
