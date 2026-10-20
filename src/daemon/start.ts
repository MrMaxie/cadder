import { createServer } from 'node:net';
import { join } from 'node:path';
import { CaddyConfigAdapter } from '../caddy/adapter.ts';
import {
  configForRoutePlan,
  prepareSecureAdminPolicy,
  type SecureAdminPolicy,
} from '../caddy/admin-policy.ts';
import { RealCaddyResolver } from '../caddy/resolver.ts';
import { CaddyRuntime } from '../caddy/runtime.ts';
import { CaddyConfigValidator } from '../caddy/validation.ts';
import { ApplicationStorage } from '../storage/storage.ts';
import { runtimeLogStream } from '../storage/schema.ts';
import { RpcError } from '../protocol/errors.ts';
import { ConfigurationTransactions } from './configuration-transactions.ts';
import { DaemonApplication } from './application.ts';
import { CaddyLogs } from './caddy-logs.ts';
import type { StateSnapshot } from '../protocol/dto.ts';
import { startLocalRuntime, type LocalRuntime } from './local-runtime.ts';

/** Reserve a candidate address; Caddy startup still refuses conflicts instead of adoption. */
async function adminPort(): Promise<number> {
  const server = createServer();
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      server.close((error) => {
        if (error || !address || typeof address === 'string')
          reject(error ?? new Error('Cannot allocate administration address.'));
        else resolve(address.port);
      });
    });
  });
}

export async function startDaemon(
  options: {
    runtimeDir?: string;
    runtimeOwner?: number;
    installationRoot?: string;
    realCaddy?: string;
  } = {},
): Promise<LocalRuntime> {
  let application: DaemonApplication;
  let expiry: NodeJS.Timeout | undefined;
  const runtime: LocalRuntime = await startLocalRuntime(
    {
      ...options,
      initialize: async (paths, owner) => {
        const storage = await ApplicationStorage.open(paths.directory, owner);
        const resolver = new RealCaddyResolver({
          ...(options.realCaddy ? { explicitOverride: options.realCaddy } : {}),
        });
        let caddy: CaddyRuntime | undefined;
        let logs: CaddyLogs | undefined;
        let transactions: ConfigurationTransactions | undefined;
        let policy: SecureAdminPolicy | undefined;
        let validator: CaddyConfigValidator | undefined;
        try {
          const desired = await storage.loadDesiredState();
          if (!desired.ok) throw new RpcError(desired.error);
          const storageStatus: NonNullable<StateSnapshot['storage']> = {
            backend: 'sqlite',
            path: paths.history,
            schemaVersion: 1,
            diagnostics: [],
          };
          logs = new CaddyLogs(
            () => application?.snapshot().registrations ?? [],
            storage.append.bind(storage),
            () => {
              storageStatus.diagnostics = [
                {
                  code: 'log_ingestion_failed',
                  message: 'Some Caddy diagnostics could not be retained.',
                  operation: null,
                },
              ];
            },
          );
          const adapter = new CaddyConfigAdapter(resolver);
          transactions = new ConfigurationTransactions(
            { config: null, registrations: [], desiredState: desired.value },
            { validate: (config) => validator!.validate(config) },
            storage,
            {
              apply: (config) =>
                caddy
                  ? caddy.apply(config)
                  : Promise.resolve({
                      status: config === null ? 'applied' : 'definitely-rejected',
                    }),
              readActive: () =>
                caddy ? caddy.readActive() : Promise.resolve({ ok: true, value: null }),
            },
          );
          let binaryPath: string | null = null;
          let version: string | null = null;
          application = new DaemonApplication({
            transactions,
            adapt: async (input) => {
              const adapted = await adapter.adapt(input);
              if (adapted.ok) {
                const image = await resolver.pin();
                binaryPath = image.path;
                version = image.evidence.version;
              }
              return adapted;
            },
            secure: async (plan) => {
              if (!policy) {
                const secured = await prepareSecureAdminPolicy({
                  runtimeBase: join(paths.directory, 'caddy'),
                  materialDirectory: join(paths.directory, 'admin'),
                  owner,
                  adminPort: await adminPort(),
                  plan,
                });
                if (!secured.ok) return secured;
                policy = secured.value;
                caddy = new CaddyRuntime(policy, owner, resolver, { onOutput: logs!.accept });
                validator = new CaddyConfigValidator(resolver, { env: policy.environment });
              }
              return { ok: true, value: configForRoutePlan(policy, plan) };
            },
            queryLogs: storage.queryLogs.bind(storage),
            log: async (entry) => storage.append([entry]),
            shutdown: () => {
              void runtime.stop().catch(() => {
                process.exitCode = 1;
              });
            },
            runtime: () => ({
              status: caddy?.status() ?? 'idle',
              binaryPath,
              version,
              processId: null,
              adminEndpoint: policy ? `https://127.0.0.1:${policy.client.port}` : null,
              diagnostics: [],
            }),
            storage: storageStatus,
          });
          await storage.append([
            {
              timestampUtc: new Date().toISOString(),
              severity: 'info',
              stream: runtimeLogStream,
              attributionKind: 'runtimeControl',
              entryKind: 'lifecycle',
              rawMessage: 'Daemon started.',
              domainKey: null,
              sourceRegistrationId: null,
              sourceInstanceId: null,
              operation: null,
            },
          ]);
          let expiring = false;
          expiry = setInterval(() => {
            if (expiring) return;
            expiring = true;
            void application
              .expire()
              .catch(async () => {
                // An expired route must not keep serving after a failed revocation.
                await runtime.stop().catch(() => {
                  process.exitCode = 1;
                });
              })
              .finally(() => {
                expiring = false;
              });
          }, 1000);
          expiry.unref();
          return {
            close: async () => {
              clearInterval(expiry);
              application.close();
              await transactions!.close();
              await caddy?.close();
              await logs!.close();
              await resolver.close();
              await storage.close();
            },
          };
        } catch (error) {
          clearInterval(expiry);
          await transactions?.close();
          await caddy?.close();
          await logs?.close();
          await resolver.close();
          await storage.close();
          throw error;
        }
      },
    },
    (request) => application.handle(request),
  );
  return runtime;
}
