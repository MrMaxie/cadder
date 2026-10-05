import { createServer, createConnection } from 'node:net';
import { createHmac, randomBytes } from 'node:crypto';
import { once } from 'node:events';
import { expect, it } from 'vitest';
import { JsonChannel } from '../src/protocol/channel.ts';
import { authenticateClient, authenticateServer } from '../src/protocol/authentication.ts';

async function channels() {
  const server = createServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Missing fixture port');
  const connected = once(server, 'connection');
  const socket = createConnection(address.port, '127.0.0.1');
  const [accepted] = await connected;
  const client = new JsonChannel(socket);
  const daemon = new JsonChannel(accepted);
  return {
    client,
    daemon,
    close() {
      client.close();
      daemon.close();
      server.close();
    },
  };
}

it('authenticates both directions without sending the secret, then protects RPC frames', async () => {
  const pair = await channels();
  const secret = randomBytes(32);
  try {
    const [client, daemon] = await Promise.all([
      authenticateClient(pair.client, secret, 'fixture'),
      authenticateServer(pair.daemon, secret, 'fixture'),
    ]);
    client.send({ method: 'status' });
    expect(await daemon.receive()).toEqual({ method: 'status' });
    daemon.send({ online: true });
    expect(await client.receive()).toEqual({ online: true });
    client.close();
    daemon.close();
  } finally {
    pair.close();
  }
});

it('rejects a fake endpoint before sending proof or RPC', async () => {
  const pair = await channels();
  try {
    const result = authenticateClient(pair.client, randomBytes(32), 'fixture');
    await pair.daemon.next();
    pair.daemon.send({ type: 'challenge', nonce: 'a'.repeat(64), proof: '0'.repeat(64) });
    await expect(result).rejects.toThrow('authentication failed');
  } finally {
    pair.close();
  }
});

it('rejects an RPC before the handshake', async () => {
  const pair = await channels();
  try {
    const result = authenticateServer(pair.daemon, randomBytes(32), 'fixture');
    pair.client.send({ method: 'shutdown' });
    await expect(result).rejects.toThrow();
  } finally {
    pair.close();
  }
});

it('rejects a wrong client secret and direction-reflected proofs', async () => {
  const pair = await channels();
  try {
    const result = authenticateServer(pair.daemon, randomBytes(32), 'fixture');
    pair.client.send({
      type: 'hello',
      nonce: 'b'.repeat(64),
      protocolVersion: 3,
      policyVersion: 2,
    });
    const challenge = (await pair.client.next()) as { proof: string };
    pair.client.send({ type: 'proof', proof: challenge.proof });
    await expect(result).rejects.toThrow('authentication failed');
  } finally {
    pair.close();
  }
});

it('rejects replayed proofs even if the client challenge is reused', async () => {
  const secret = randomBytes(32);
  const clientNonce = 'b'.repeat(64);
  const first = await channels();
  let oldProof: string;
  try {
    const authenticated = authenticateServer(first.daemon, secret, 'fixture');
    first.client.send({ type: 'hello', nonce: clientNonce, protocolVersion: 3, policyVersion: 2 });
    const challenge = (await first.client.next()) as { nonce: string };
    const context = JSON.stringify(['cadder-ipc', 3, 2, 'fixture', clientNonce, challenge.nonce]);
    oldProof = createHmac('sha256', secret).update(`client:${context}`).digest('hex');
    first.client.send({ type: 'proof', proof: oldProof });
    await first.client.next();
    await authenticated;
  } finally {
    first.close();
  }
  const second = await channels();
  try {
    const authenticated = authenticateServer(second.daemon, secret, 'fixture');
    second.client.send({ type: 'hello', nonce: clientNonce, protocolVersion: 3, policyVersion: 2 });
    await second.client.next();
    second.client.send({ type: 'proof', proof: oldProof });
    await expect(authenticated).rejects.toThrow('authentication failed');
  } finally {
    second.close();
  }
});

it('rejects replayed authenticated RPC frames', async () => {
  const pair = await channels();
  const secret = randomBytes(32);
  try {
    const [client, daemon] = await Promise.all([
      authenticateClient(pair.client, secret, 'fixture'),
      authenticateServer(pair.daemon, secret, 'fixture'),
    ]);
    let captured: Buffer = Buffer.alloc(0);
    pair.daemon.socket.on('data', (data: Buffer) => {
      captured = data;
    });
    client.send({ method: 'status' });
    await daemon.receive();
    pair.client.socket.write(captured);
    await expect(daemon.receive()).rejects.toThrow('replayed IPC frame');
  } finally {
    pair.close();
  }
});

it('rejects modified RPC payloads', async () => {
  const pair = await channels();
  const secret = randomBytes(32);
  try {
    const [client, daemon] = await Promise.all([
      authenticateClient(pair.client, secret, 'fixture'),
      authenticateServer(pair.daemon, secret, 'fixture'),
    ]);
    pair.client.send({
      sequence: 0,
      payload: JSON.stringify({ method: 'shutdown' }),
      mac: '0'.repeat(64),
    });
    await expect(daemon.receive()).rejects.toThrow('authentication failed');
    client.close();
  } finally {
    pair.close();
  }
});
