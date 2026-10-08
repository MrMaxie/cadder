import { createConnection, createServer } from 'node:net';
import { once } from 'node:events';
import { expect, it } from 'vitest';
import { JsonChannel } from '../src/protocol/channel.ts';
import { MAX_FRAME_BYTES } from '../src/protocol/version.ts';

async function fixture() {
  const server = createServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Missing fixture port');
  const incoming = once(server, 'connection');
  const socket = createConnection(address.port, '127.0.0.1');
  const [peer] = await incoming;
  const channel = new JsonChannel(socket);
  return {
    peer,
    channel,
    close() {
      peer.destroy();
      channel.close();
      server.close();
    },
  };
}

it('parses split and batched NDJSON frames', async () => {
  const pair = await fixture();
  try {
    pair.peer.write('{"a":');
    pair.peer.write('1}\n{"b":2}\n');
    expect(await pair.channel.next()).toEqual({ a: 1 });
    expect(await pair.channel.next()).toEqual({ b: 2 });
  } finally {
    pair.close();
  }
});
it('bounds incomplete frames and refuses malformed JSON', async () => {
  for (const frame of ['{invalid}\n', 'x'.repeat(MAX_FRAME_BYTES + 1)]) {
    const pair = await fixture();
    try {
      const result = pair.channel.next();
      pair.peer.write(frame);
      await expect(result).rejects.toThrow('Invalid or oversized');
    } finally {
      pair.close();
    }
  }
});
it('bounds outgoing frames and rejects concurrent reads and timeouts', async () => {
  const pair = await fixture();
  try {
    expect(() => pair.channel.send('x'.repeat(MAX_FRAME_BYTES))).toThrow('size limit');
    const pending = pair.channel.next(10);
    await expect(pair.channel.next()).rejects.toThrow('Only one IPC read');
    await expect(pending).rejects.toThrow('deadline');
  } finally {
    pair.close();
  }
});
