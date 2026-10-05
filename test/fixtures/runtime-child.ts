import { startLocalRuntime } from '../../src/daemon/local-runtime.ts';

const options: { runtimeDir?: string; runtimeOwner?: number } = {};
if (process.argv[2] !== undefined) options.runtimeDir = process.argv[2];
if (process.argv[3] !== undefined) options.runtimeOwner = Number(process.argv[3]);
const runtime = await startLocalRuntime(options, async (request) => {
  if (request.method === 'status') return { processId: process.pid, recovered: runtime.recovered };
  if (request.method === 'shutdown') {
    setTimeout(() => {
      void shutdown();
    }, 30);
    return { stopped: true };
  }
  throw new Error('Unknown fixture request');
});
process.stdout.write('READY\n');
async function shutdown() {
  await runtime.stop();
  process.exitCode = 0;
}
process.on('SIGTERM', () => {
  void shutdown();
});
setTimeout(() => {
  void shutdown();
}, 30000).unref();
