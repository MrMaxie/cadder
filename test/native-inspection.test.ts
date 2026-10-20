import { createServer } from 'node:net';
import { once } from 'node:events';
import { expect, it } from 'vitest';
import { inspectPort, terminatePortOwner } from '../src/platform/inspection.ts';

it('observes a real listener and refuses a stale PID without affecting its owner', async () => {
  const server = createServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  try {
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('Missing listener.');
    const owners = await inspectPort(address.port);
    expect(owners.some((owner) => owner.protocol === 'tcp' && owner.pid === process.pid)).toBe(
      true,
    );
    await expect(terminatePortOwner(address.port, 2147483646)).rejects.toMatchObject({
      code: 'target-conflict',
    });
    expect(server.listening).toBe(true);
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
}, 30000);
