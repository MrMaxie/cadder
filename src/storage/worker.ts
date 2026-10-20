import { parentPort, workerData } from 'node:worker_threads';
import { ApplicationDatabase } from './database.ts';
import type { DesiredState } from '../contracts/ports.ts';
import type { LogInput } from './schema.ts';

const database = new ApplicationDatabase((workerData as { path: string }).path);
parentPort!.postMessage({ id: 0, ok: true });
parentPort!.on('message', (job: { id: number; method: string; value: unknown; limit?: number }) => {
  try {
    let value: unknown;
    switch (job.method) {
      case 'load':
        value = database.loadDesiredState();
        break;
      case 'persist':
        database.persistDesiredState(job.value as DesiredState);
        break;
      case 'append':
        database.append(job.value as LogInput[]);
        break;
      case 'logs':
        value = database.queryLogs(job.value, job.limit!);
        break;
      case 'close':
        database.close();
        break;
      default:
        throw new Error('Unknown storage operation.');
    }
    parentPort!.postMessage({ id: job.id, ok: true, value });
    if (job.method === 'close') parentPort!.close();
  } catch {
    parentPort!.postMessage({ id: job.id, ok: false });
  }
});
