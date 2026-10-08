import { beforeEach, expect, it, vi } from 'vitest';
import { startLocalRuntime } from '../src/daemon/local-runtime.ts';
import * as security from '../src/platform/runtime-security.ts';
import { acquireRuntimeLock } from '../src/daemon/runtime-lock.ts';
import { serveIpc } from '../src/daemon/ipc-server.ts';

vi.mock('../src/platform/runtime-security.ts', () => ({
  runtimeOwner: vi.fn(),
  prepareRuntime: vi.fn(),
}));
vi.mock('../src/daemon/runtime-lock.ts', () => ({ acquireRuntimeLock: vi.fn() }));
vi.mock('../src/daemon/ipc-server.ts', () => ({ serveIpc: vi.fn() }));
const release = vi.fn();
const close = vi.fn();
const failure = new Error('startup failure');
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(security.runtimeOwner).mockResolvedValue({ id: 'test', elevated: false });
  vi.mocked(acquireRuntimeLock).mockResolvedValue({
    recovered: false,
    secret: Buffer.alloc(32),
    release,
  });
  vi.mocked(serveIpc).mockResolvedValue({ close });
});

it.each(['owner', 'directory', 'lock'])(
  'does not bind or publish after %s failure',
  async (phase) => {
    if (phase === 'owner') vi.mocked(security.runtimeOwner).mockRejectedValueOnce(failure);
    if (phase === 'directory') vi.mocked(security.prepareRuntime).mockRejectedValueOnce(failure);
    if (phase === 'lock') vi.mocked(acquireRuntimeLock).mockRejectedValueOnce(failure);
    await expect(startLocalRuntime({ runtimeDir: '/fault-runtime' }, vi.fn())).rejects.toBe(
      failure,
    );
    expect(serveIpc).not.toHaveBeenCalled();
    expect(release).not.toHaveBeenCalled();
  },
);

it('retains startup failure when releasing exclusion also fails', async () => {
  vi.mocked(serveIpc).mockRejectedValueOnce(failure);
  release.mockRejectedValueOnce(new Error('release failure'));
  const error = await startLocalRuntime({ runtimeDir: '/fault-runtime' }, vi.fn()).catch(
    (error: unknown) => error,
  );
  expect(error).toMatchObject({ cause: failure });
  expect((error as AggregateError).errors).toHaveLength(2);
  expect(release).toHaveBeenCalledOnce();
});

it('settles concurrent stop consistently while attempting listener and lock finalizers', async () => {
  const runtime = await startLocalRuntime({ runtimeDir: '/fault-runtime' }, vi.fn());
  close.mockRejectedValueOnce(failure);
  release.mockRejectedValueOnce(new Error('release failure'));
  const stopping = runtime.stop();
  expect(runtime.stop()).toBe(stopping);
  const error = await stopping.catch((error: unknown) => error);
  expect((error as AggregateError).errors).toHaveLength(2);
  await expect(runtime.stop()).rejects.toBe(error);
  expect(close).toHaveBeenCalledOnce();
  expect(release).toHaveBeenCalledOnce();
});
