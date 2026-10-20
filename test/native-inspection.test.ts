import { createServer } from 'node:net';
import { once } from 'node:events';
import { expect, it } from 'vitest';
import { inspectPort, terminatePortOwner } from '../src/platform/inspection.ts';
import { createSocket } from 'node:dgram';

it('observes a real listener and refuses a stale PID without affecting its owner', async () => {
  const server = createServer();
  const datagram = createSocket('udp4');
  let datagramBound = false;
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  try {
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('Missing listener.');
    const owners = await inspectPort(address.port);
    expect(owners, `Native TCP owner for port ${address.port}, PID ${process.pid}`).toContainEqual(
      expect.objectContaining({ protocol: 'tcp', pid: process.pid }),
    );
    datagram.bind(0, '127.0.0.1');
    await once(datagram, 'listening');
    datagramBound = true;
    expect(await inspectPort(datagram.address().port)).toContainEqual(
      expect.objectContaining({ protocol: 'udp', pid: process.pid }),
    );
    await expect(terminatePortOwner(address.port, 2147483646)).rejects.toMatchObject({
      code: 'target-conflict',
    });
    expect(server.listening).toBe(true);
  } finally {
    if (datagramBound) datagram.close();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
}, 30000);
