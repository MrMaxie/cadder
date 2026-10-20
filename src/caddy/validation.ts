import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { CaddyConfig, CaddyPort, CaddyValidation, PortResult } from '../contracts/ports.ts';
import { RealCaddyResolver } from './resolver.ts';
import {
  CaddyPreparationError,
  assertCandidate,
  configurationCommandOptions,
  decodeOutput,
  preparationFailure,
  type PreparationOptions,
} from './preparation.ts';

/** Implements the existing validation signature only; deliberately has no apply method. */
export class CaddyConfigValidator implements Pick<CaddyPort, 'validate'> {
  constructor(
    private readonly resolver: RealCaddyResolver,
    private readonly options: PreparationOptions = {},
  ) {}

  async validate(config: CaddyConfig, signal?: AbortSignal): Promise<PortResult<CaddyValidation>> {
    let directory: string | undefined;
    let failure: unknown;
    let validated: CaddyValidation | undefined;
    try {
      assertCandidate(config);
      const body = config.adaptedConfig.body;
      const effectiveConfigHash = config.effectiveConfigHash;
      const image = await this.resolver.pin(signal);
      directory = await mkdtemp(join(tmpdir(), 'cadder-caddy-validation-'));
      const path = join(directory, 'candidate.json');
      await writeFile(path, body, { flag: 'wx', mode: 0o600 });
      const output = await image.run(['validate', '--config', path], {
        ...configurationCommandOptions(this.options, signal),
        cwd: directory,
      });
      decodeOutput(output, 'validate');
      validated = Object.freeze({ effectiveConfigHash, diagnostics: Object.freeze([]) });
    } catch (error) {
      failure = error;
    }
    // run() has completed all owned child/pipe finalizers before owned staging is removed.
    if (directory !== undefined) {
      try {
        await rm(directory, { recursive: true, maxRetries: 3, retryDelay: 100 });
      } catch (error) {
        failure = new CaddyPreparationError(
          'caddy_cleanup_failed',
          'Caddy validation staging cleanup failed.',
          {
            cause:
              failure === undefined
                ? error
                : new AggregateError([failure, error], 'Validation and staging cleanup failed.', {
                    cause: failure,
                  }),
          },
        );
      }
    }
    if (validated !== undefined && failure === undefined) return { ok: true, value: validated };
    return { ok: false, error: preparationFailure(failure, 'validate') };
  }
}
