import { render, cleanup } from 'ink-testing-library';
import { afterEach, expect, it, vi } from 'vitest';
import { RoutesApp } from '../src/tui/app.tsx';
import { ClientService } from '../src/client/service.ts';
import { CadderError } from '../src/protocol/errors.ts';
import { fakeSnapshot } from './fixtures/rpc-data.ts';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it('renders real client state with tree rows and toggles the selected project/domain', async () => {
  const client = new ClientService();
  vi.spyOn(client, 'snapshot').mockResolvedValue(fakeSnapshot());
  const result = {
    ok: true as const,
    value: { requestId: '00000000-0000-4000-8000-000000000001', accepted: true, message: 'Done' },
  };
  const project = vi.spyOn(client, 'setProjectEnabled').mockResolvedValue(result);
  const domain = vi.spyOn(client, 'setDomainEnabled').mockResolvedValue(result);
  const screen = render(<RoutesApp client={client} />);
  await vi.waitFor(() => expect(screen.lastFrame()).toContain('example.localhost'));
  expect(screen.lastFrame()).toContain('cadderd online');
  expect(screen.lastFrame()).not.toContain('runtime-control');
  screen.stdin.write(' ');
  await vi.waitFor(() =>
    expect(project).toHaveBeenCalledWith({ registrationId: 'shim-fixture', enabled: true }),
  );
  await vi.waitFor(() => expect(screen.lastFrame()).not.toContain('Working...'));
  screen.stdin.write('\x1b[B');
  await vi.waitFor(() => expect(screen.lastFrame()).toMatch(/>.*example\.localhost/));
  screen.stdin.write('\r');
  await vi.waitFor(() =>
    expect(domain).toHaveBeenCalledWith({
      registrationId: 'shim-fixture',
      domainKey: 'example.localhost',
      enabled: false,
    }),
  );
});

it('keeps offline entry attach-only, starts explicitly and confirms stop', async () => {
  const client = new ClientService();
  const snapshot = vi
    .spyOn(client, 'snapshot')
    .mockRejectedValue(new CadderError('daemon-unavailable', 'Offline'));
  const start = vi.spyOn(client, 'start').mockImplementation(async () => {
    snapshot.mockResolvedValue(fakeSnapshot());
  });
  const stop = vi.spyOn(client, 'stop').mockResolvedValue();
  const screen = render(<RoutesApp client={client} />);
  await vi.waitFor(() => expect(screen.lastFrame()).toContain('Press Enter'));
  expect(start).not.toHaveBeenCalled();
  screen.stdin.write('\r');
  await vi.waitFor(() => expect(screen.lastFrame()).toContain('cadderd online'));
  screen.stdin.write('s');
  await vi.waitFor(() => expect(screen.lastFrame()).toContain('y / n'));
  expect(stop).not.toHaveBeenCalled();
  screen.stdin.write('n');
  await vi.waitFor(() => expect(screen.lastFrame()).not.toContain('y / n'));
  screen.stdin.write('s');
  await vi.waitFor(() => expect(screen.lastFrame()).toContain('y / n'));
  screen.stdin.write('y');
  await vi.waitFor(() => expect(stop).toHaveBeenCalledOnce());
});
