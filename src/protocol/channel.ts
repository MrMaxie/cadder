import type { Socket } from 'node:net';
import { CadderError } from './errors.ts';
import { AUTH_TIMEOUT_MS, MAX_FRAME_BYTES } from './version.ts';

export class JsonChannel {
  private buffer: Buffer = Buffer.alloc(0);
  private queue: unknown[] = [];
  private failure: Error | undefined;
  private pending: { resolve(value: unknown): void; reject(error: Error): void } | undefined;

  constructor(public readonly socket: Socket) {
    socket.on('data', (data: Buffer) => this.consume(data));
    socket.on('error', (error) => this.fail(error));
    socket.on('close', () =>
      this.fail(new CadderError('connection-closed', 'Daemon connection closed.')),
    );
  }

  send(value: unknown): void {
    const frame = Buffer.from(`${JSON.stringify(value)}\n`);
    if (frame.length > MAX_FRAME_BYTES)
      throw new CadderError('frame-too-large', 'IPC frame exceeds the size limit.');
    if (this.failure) throw this.failure;
    this.socket.write(frame);
  }

  async next(timeout = AUTH_TIMEOUT_MS): Promise<unknown> {
    if (this.queue.length) return this.queue.shift();
    if (this.failure) throw this.failure;
    if (this.pending) throw new CadderError('concurrent-read', 'Only one IPC read can be pending.');
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const error = new CadderError('ipc-timeout', 'Daemon did not respond before the deadline.');
        this.fail(error);
        this.socket.destroy();
      }, timeout);
      this.pending = {
        resolve(value) {
          clearTimeout(timer);
          resolve(value);
        },
        reject(error) {
          clearTimeout(timer);
          reject(error);
        },
      };
    });
  }

  close(): void {
    this.socket.destroy();
  }

  private consume(data: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, data]);
    let newline: number;
    while ((newline = this.buffer.indexOf(10)) !== -1) {
      if (newline > MAX_FRAME_BYTES || this.queue.length >= 8) return this.rejectFrame();
      let frame: unknown;
      try {
        frame = JSON.parse(this.buffer.subarray(0, newline).toString('utf8'));
      } catch {
        return this.rejectFrame();
      }
      this.buffer = this.buffer.subarray(newline + 1);
      if (this.pending) {
        const pending = this.pending;
        this.pending = undefined;
        pending.resolve(frame);
      } else this.queue.push(frame);
    }
    if (this.buffer.length > MAX_FRAME_BYTES) this.rejectFrame();
  }

  private rejectFrame(): void {
    this.fail(new CadderError('invalid-frame', 'Invalid or oversized IPC frame.'));
    this.socket.destroy();
  }

  private fail(error: Error): void {
    this.failure ??= error;
    if (this.pending) {
      const pending = this.pending;
      this.pending = undefined;
      pending.reject(error);
    }
  }
}
