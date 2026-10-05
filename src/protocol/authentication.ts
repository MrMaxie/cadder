import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import { z } from 'zod';
import { CadderError } from './errors.ts';
import { JsonChannel } from './channel.ts';
import { PROTOCOL_VERSION, SECURITY_POLICY_VERSION } from './version.ts';

const nonce = z.string().regex(/^[a-f0-9]{64}$/);
const hello = z.object({
  type: z.literal('hello'),
  nonce,
  protocolVersion: z.literal(PROTOCOL_VERSION),
  policyVersion: z.literal(SECURITY_POLICY_VERSION),
});
const challenge = z.object({ type: z.literal('challenge'), nonce, proof: nonce });
const proof = z.object({ type: z.literal('proof'), proof: nonce });
const ready = z.object({ type: z.literal('ready'), proof: nonce });
const message = z.object({
  sequence: z.number().int().nonnegative(),
  payload: z.string(),
  mac: nonce,
});

function mac(secret: Buffer, value: string): string {
  return createHmac('sha256', secret).update(value).digest('hex');
}

function verify(secret: Buffer, value: string, received: string): void {
  if (!timingSafeEqual(Buffer.from(mac(secret, value), 'hex'), Buffer.from(received, 'hex'))) {
    throw new CadderError('authentication-failed', 'IPC authentication failed.');
  }
}

function transcript(instance: string, clientNonce: string, serverNonce: string): string {
  return JSON.stringify([
    'cadder-ipc',
    PROTOCOL_VERSION,
    SECURITY_POLICY_VERSION,
    instance,
    clientNonce,
    serverNonce,
  ]);
}

export async function authenticateServer(
  channel: JsonChannel,
  secret: Buffer,
  instance: string,
): Promise<AuthenticatedSession> {
  const client = hello.parse(await channel.next());
  const serverNonce = randomBytes(32).toString('hex');
  const context = transcript(instance, client.nonce, serverNonce);
  channel.send({ type: 'challenge', nonce: serverNonce, proof: mac(secret, `server:${context}`) });
  const response = proof.parse(await channel.next());
  verify(secret, `client:${context}`, response.proof);
  channel.send({ type: 'ready', proof: mac(secret, `ready:${context}`) });
  return new AuthenticatedSession(
    channel,
    Buffer.from(mac(secret, `session:${context}`), 'hex'),
    'server',
  );
}

export async function authenticateClient(
  channel: JsonChannel,
  secret: Buffer,
  instance: string,
): Promise<AuthenticatedSession> {
  const clientNonce = randomBytes(32).toString('hex');
  channel.send({
    type: 'hello',
    nonce: clientNonce,
    protocolVersion: PROTOCOL_VERSION,
    policyVersion: SECURITY_POLICY_VERSION,
  });
  const server = challenge.parse(await channel.next());
  const context = transcript(instance, clientNonce, server.nonce);
  verify(secret, `server:${context}`, server.proof);
  channel.send({ type: 'proof', proof: mac(secret, `client:${context}`) });
  const response = ready.parse(await channel.next());
  verify(secret, `ready:${context}`, response.proof);
  return new AuthenticatedSession(
    channel,
    Buffer.from(mac(secret, `session:${context}`), 'hex'),
    'client',
  );
}

export class AuthenticatedSession {
  private sent = 0;
  private received = 0;
  constructor(
    private channel: JsonChannel,
    private key: Buffer,
    private role: 'client' | 'server',
  ) {}

  send(value: unknown): void {
    const payload = JSON.stringify(value);
    const sequence = this.sent++;
    this.channel.send({
      sequence,
      payload,
      mac: mac(this.key, JSON.stringify([this.role, sequence, payload])),
    });
  }

  async receive(timeout = 15000): Promise<unknown> {
    const frame = message.parse(await this.channel.next(timeout));
    if (frame.sequence !== this.received)
      throw new CadderError('replayed-frame', 'Out-of-order or replayed IPC frame.');
    const peer = this.role === 'client' ? 'server' : 'client';
    verify(this.key, JSON.stringify([peer, frame.sequence, frame.payload]), frame.mac);
    this.received++;
    return JSON.parse(frame.payload);
  }

  close(): void {
    this.key.fill(0);
    this.channel.close();
  }
}
