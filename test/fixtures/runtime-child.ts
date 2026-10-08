import { startLocalRuntime } from '../../src/daemon/local-runtime.ts';
import { basicResult, fakeStateResult } from './rpc-data.ts';

const options: { runtimeDir?: string; runtimeOwner?: number } = {};
if (process.argv[2] !== undefined) options.runtimeDir = process.argv[2];
if (process.getuid && process.argv[3] !== undefined) options.runtimeOwner = Number(process.argv[3]);
const runtime = await startLocalRuntime(options, async (request) => {
  if (request.method === 'query-state-request') return fakeStateResult();
  if (request.method === 'shutdown-daemon-request') {
    setTimeout(() => {
      void shutdown();
    }, 30);
    return basicResult;
  }
  throw new Error('Unknown fixture request');
});
process.stdout.write(`READY recovered=${runtime.recovered}\n`);
async function shutdown() {
  await runtime.stop();
  process.exitCode = 0;
}
process.on('SIGTERM', () => {
  void shutdown();
});
setTimeout(
  () => {
    void shutdown();
  },
  process.argv.includes('--interactive-gate') ? 300000 : 30000,
).unref();
