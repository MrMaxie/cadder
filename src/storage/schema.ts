import { z } from 'zod';
import { logEntrySchema } from '../protocol/dto.ts';

export const desiredStateSchema = z.strictObject({
  projects: z.array(
    z.strictObject({
      projectKey: z.string(),
      sourceWorkingDirectory: z.string(),
      sourceConfigPath: z.string(),
      enabled: z.boolean(),
      domains: z.array(
        z.strictObject({
          canonicalDomain: z.string(),
          upstream: z.string().nullable(),
          enabled: z.boolean(),
        }),
      ),
    }),
  ),
});
export type StoredLog = z.infer<typeof logEntrySchema>;
export const logInputSchema = logEntrySchema.omit({ sequenceNumber: true });
export type LogInput = z.input<typeof logInputSchema>;

export const runtimeLogStream = Object.freeze({
  streamId: 'runtime-control',
  domainKey: null,
  channel: 'control',
});
