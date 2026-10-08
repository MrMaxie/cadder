import { readFileSync, realpathSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { isSea } from 'node:sea';
import { fileURLToPath } from 'node:url';
import { CadderError, errorCode } from '../protocol/errors.ts';

export interface InstallationRootInput {
  installationRoot?: string;
  sea?: boolean;
  executable?: string;
  moduleUrl?: string;
}

// Internal inputs let tests exercise distribution layouts without launch wrappers.
export function resolveInstallationRoot(input: InstallationRootInput = {}): string {
  try {
    if (input.installationRoot !== undefined) {
      const root = realpathSync.native(resolve(input.installationRoot));
      if (!statSync(root).isDirectory()) throw new Error('Installation root is not a directory.');
      return root;
    }
    if (input.sea ?? isSea())
      return dirname(realpathSync.native(input.executable ?? process.execPath));
    let directory = dirname(realpathSync.native(fileURLToPath(input.moduleUrl ?? import.meta.url)));
    for (;;) {
      try {
        const manifest = JSON.parse(readFileSync(join(directory, 'package.json'), 'utf8')) as {
          name?: string;
        };
        if (manifest.name === 'cadder') return directory;
      } catch (error) {
        if (errorCode(error) !== 'ENOENT') throw error;
      }
      const parent = dirname(directory);
      if (parent === directory) throw new Error('No cadder package anchor found.');
      directory = parent;
    }
  } catch (error) {
    const failure = new CadderError(
      'installation-root-unavailable',
      'Cannot resolve the physical Cadder installation root; refusing a shared default runtime.',
    );
    failure.cause = error;
    throw failure;
  }
}
