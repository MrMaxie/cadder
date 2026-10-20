import type { CaddyConfig, PortResult } from '../contracts/ports.ts';
import type { EntrypointRegistration } from '../protocol/dto.ts';
import { RealCaddyResolver } from './resolver.ts';
import { absoluteCaddyPath } from './executable.ts';
import {
  CaddyPreparationError,
  assertJsonBody,
  configHash,
  configurationCommandOptions,
  decodeOutput,
  preparationFailure,
  type PreparationOptions,
} from './preparation.ts';

export type CaddyAdaptInput = Pick<
  EntrypointRegistration,
  'sourceConfigPath' | 'sourceWorkingDirectory' | 'shimRun'
>;

/** Adapt only; no registration mutation, composition or server lifecycle. */
export class CaddyConfigAdapter {
  constructor(
    private readonly resolver: RealCaddyResolver,
    private readonly options: PreparationOptions = {},
  ) {}

  async adapt(input: CaddyAdaptInput, signal?: AbortSignal): Promise<PortResult<CaddyConfig>> {
    try {
      const cwd = input.sourceWorkingDirectory.canonical ?? input.sourceWorkingDirectory.raw;
      const path = input.sourceConfigPath.canonical ?? input.sourceConfigPath.raw;
      const adapter = input.shimRun?.adapter ?? 'caddyfile';
      if (
        !absoluteCaddyPath(cwd) ||
        !path ||
        cwd.includes('\0') ||
        path.includes('\0') ||
        adapter.includes('\0') ||
        (input.sourceConfigPath.canonical !== null && !absoluteCaddyPath(path))
      )
        throw new CaddyPreparationError(
          'caddy_candidate_invalid',
          'Caddy adaptation requires an absolute source directory and a valid configuration path.',
        );
      const image = await this.resolver.pin(signal);
      const output = await image.run(['adapt', '--config', path, '--adapter', adapter], {
        ...configurationCommandOptions(this.options, signal),
        cwd,
      });
      const body = decodeOutput(output, 'adapt');
      assertJsonBody(body);
      return {
        ok: true,
        value: Object.freeze({
          effectiveConfigHash: configHash(body),
          adaptedConfig: Object.freeze({ format: 'json', body }),
        }),
      };
    } catch (error) {
      return { ok: false, error: preparationFailure(error, 'adapt') };
    }
  }
}
