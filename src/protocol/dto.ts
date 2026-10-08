import { z } from 'zod';

export const safeUnsignedIntegerSchema = z.number().int().min(0).max(Number.MAX_SAFE_INTEGER);
const u32 = safeUnsignedIntegerSchema.max(0xffffffff);
const timestamp = z.string().datetime({ offset: true });
const nullableString = z.string().nullable();
const activation = z.enum(['unknown', 'registered', 'activating', 'active', 'inactive', 'faulted']);

export const logStreamSchema = z.strictObject({
  streamId: z.string(),
  domainKey: nullableString,
  channel: z.string(),
});
const sourcePathSchema = z.strictObject({ raw: z.string(), canonical: nullableString });
export const registrationSchema = z.strictObject({
  registrationId: z.string(),
  entrypointInstance: z.strictObject({
    instanceId: z.string(),
    startedAtUtc: timestamp,
    shimSessionNonce: z.string(),
  }),
  sourceWorkingDirectory: sourcePathSchema,
  sourceConfigPath: sourcePathSchema,
  registeredDomains: z.array(
    z.strictObject({
      name: z.strictObject({ raw: z.string(), canonical: z.string() }),
      activationState: activation,
      upstream: nullableString.transform((value) => value ?? undefined).optional(),
      logStream: logStreamSchema,
    }),
  ),
  activationState: activation,
  ownerProcess: z.strictObject({
    processId: u32,
    processStartTimeUtc: timestamp,
    shimSessionNonce: z.string(),
    executablePath: nullableString,
  }),
  logStream: logStreamSchema,
  shimRun: z
    .strictObject({
      adapter: nullableString,
      rawArguments: z.array(z.string()),
      commandLine: z.string(),
    })
    .nullable(),
  createdAtUtc: timestamp,
  lastHeartbeatUtc: timestamp,
});
// Serde Option inputs accept omission; wire output without skip_serializing_if emits null.
export const logStreamInputSchema = logStreamSchema.extend({
  domainKey: nullableString.default(null),
});
const sourcePathInputSchema = sourcePathSchema.extend({ canonical: nullableString.default(null) });
export const registrationInputSchema = registrationSchema.extend({
  sourceWorkingDirectory: sourcePathInputSchema,
  sourceConfigPath: sourcePathInputSchema,
  registeredDomains: z.array(
    registrationSchema.shape.registeredDomains.element.extend({ logStream: logStreamInputSchema }),
  ),
  ownerProcess: registrationSchema.shape.ownerProcess.extend({
    executablePath: nullableString.default(null),
  }),
  logStream: logStreamInputSchema,
  shimRun: registrationSchema.shape.shimRun
    .unwrap()
    .extend({ adapter: nullableString.default(null) })
    .nullable()
    .default(null),
});

const runtimeDiagnosticSchema = z.strictObject({
  code: z.string(),
  message: z.string(),
  operation: nullableString,
});
export const snapshotSchema = z.strictObject({
  capturedAtUtc: timestamp,
  registrations: z.array(registrationSchema),
  runtime: z.strictObject({
    status: z.enum(['unknown', 'notResolved', 'resolved', 'running', 'unhealthy', 'idle']),
    binaryPath: nullableString,
    version: nullableString,
    processId: u32.nullable(),
    adminEndpoint: nullableString,
    diagnostics: z.array(runtimeDiagnosticSchema),
  }),
  config: z.strictObject({
    status: z.enum(['unknown', 'notApplied', 'applied', 'failed', 'idle']),
    lastAttemptedAtUtc: timestamp.nullable(),
    lastSuccessfulReloadAtUtc: timestamp.nullable(),
    effectiveConfigHash: nullableString,
    diagnostics: z.array(
      z.strictObject({
        code: z.string(),
        message: z.string(),
        domainKey: nullableString,
        sourceConfigPaths: z.array(z.string()),
      }),
    ),
  }),
  storage: z
    .strictObject({
      backend: z.string(),
      path: nullableString,
      schemaVersion: u32,
      diagnostics: z.array(runtimeDiagnosticSchema),
    })
    .nullable(),
});
export const logEntrySchema = z.strictObject({
  sequenceNumber: safeUnsignedIntegerSchema,
  timestampUtc: timestamp,
  severity: z.enum(['unknown', 'trace', 'debug', 'info', 'warn', 'error', 'fatal']),
  stream: logStreamSchema,
  attributionKind: z.enum(['unknown', 'runtime', 'runtimeControl', 'entrypoint', 'domain']),
  entryKind: z.enum(['normal', 'lifecycle', 'ingestionOverflow', 'retentionGap']),
  rawMessage: z.string(),
  domainKey: nullableString,
  sourceRegistrationId: nullableString,
  sourceInstanceId: nullableString,
  operation: nullableString,
});
export type StateSnapshot = z.infer<typeof snapshotSchema>;
export type EntrypointRegistration = z.infer<typeof registrationSchema>;
