import { useCallback, useEffect, useRef, useState } from 'react';
import { Box, Text, render, useApp, useInput, useWindowSize } from 'ink';
import { basename } from 'node:path';
import { ClientService } from '../client/service.ts';
import type { StateSnapshot } from '../protocol/dto.ts';
import { RpcError, errorCode } from '../protocol/errors.ts';
import { terminalSafe } from '../storage/redaction.ts';
import { VERSION } from '../protocol/version.ts';

export function RoutesApp({ client }: { client: ClientService }) {
  const { exit } = useApp();
  const { rows: height } = useWindowSize();
  const [snapshot, setSnapshot] = useState<StateSnapshot | null>(null);
  const [offline, setOffline] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const alive = useRef(true);
  const [confirm, setConfirm] = useState(false);
  const [selected, setSelected] = useState(0);
  const refresh = useCallback(async () => {
    try {
      const state = await client.snapshot();
      if (alive.current) {
        setSnapshot(state);
        setOffline(false);
      }
    } catch (failure) {
      if (alive.current) {
        setOffline(errorCode(failure) === 'daemon-unavailable');
        setSnapshot(null);
        if (errorCode(failure) !== 'daemon-unavailable')
          setError(
            'Cannot read daemon state. Open cadder diagnostics after restoring the connection.',
          );
      }
    }
  }, [client]);
  useEffect(() => {
    alive.current = true;
    let refreshing = false;
    const poll = (): void => {
      if (!refreshing && !busy.current) {
        refreshing = true;
        void refresh().finally(() => {
          refreshing = false;
        });
      }
    };
    poll();
    const timer = setInterval(poll, 1000);
    return () => {
      alive.current = false;
      clearInterval(timer);
    };
  }, [refresh]);
  const rows = (snapshot?.registrations ?? []).flatMap((project) => [
    {
      key: project.registrationId,
      project,
      domain: null,
      label: basename(project.sourceWorkingDirectory.raw) || project.sourceWorkingDirectory.raw,
      enabled: project.activationState === 'active',
    },
    ...project.registeredDomains.map((domain, index) => ({
      key: `${project.registrationId}:${domain.name.canonical}`,
      project,
      domain,
      label: `${index === project.registeredDomains.length - 1 ? '\u2514\u2500' : '\u251c\u2500'} ${domain.name.raw}${domain.upstream ? '  ' + domain.upstream : ''}`,
      enabled: domain.activationState === 'active',
    })),
  ]);
  const current = Math.min(selected, Math.max(0, rows.length - 1));
  const visible = Math.max(1, height - 7);
  const offset = Math.max(0, current - visible + 1);
  async function act(operation: () => Promise<unknown>): Promise<void> {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    setError(null);
    try {
      await operation();
      await refresh();
    } catch (failure) {
      if (alive.current)
        setError(
          terminalSafe(failure instanceof Error ? failure.message : 'Action failed. Try again.'),
        );
    } finally {
      busy.current = false;
      if (alive.current) setPending(false);
    }
  }
  useInput((input, key) => {
    if (input === 'q' || key.escape) {
      if (confirm) setConfirm(false);
      else exit();
      return;
    }
    if (busy.current) return;
    if (confirm) {
      if (input === 'y') {
        setConfirm(false);
        void act(() => client.stop());
      } else if (input === 'n') setConfirm(false);
      return;
    }
    if (key.upArrow) setSelected(Math.max(0, current - 1));
    if (key.downArrow) setSelected(Math.max(0, Math.min(rows.length - 1, current + 1)));
    if (input === 'r') {
      void act(() => client.restart());
      return;
    }
    if (input === 's' && snapshot) {
      setConfirm(true);
      return;
    }
    if ((key.return || input === ' ') && offline) {
      void act(() => client.start());
      return;
    }
    if ((key.return || input === ' ') && snapshot && rows[current]) {
      const row = rows[current];
      void act(async () => {
        const result = row.domain
          ? await client.setDomainEnabled({
              registrationId: row.project.registrationId,
              domainKey: row.domain.name.canonical,
              enabled: !row.enabled,
            })
          : await client.setProjectEnabled({
              registrationId: row.project.registrationId,
              enabled: !row.enabled,
            });
        if (!result.ok) throw new RpcError(result.error);
      });
    }
  });
  return (
    <Box flexDirection="column" height={Math.max(4, height - 1)} overflow="hidden">
      <Box justifyContent="space-between">
        <Text bold>Cadder {VERSION}</Text>
        <Text>
          cadderd {snapshot ? 'online' : offline ? 'offline' : 'unavailable'} Caddy{' '}
          {snapshot?.runtime.status ?? '-'}
        </Text>
      </Box>
      <Box flexDirection="column" flexGrow={1} overflow="hidden" marginTop={1}>
        {offline ? (
          <Text>Press Enter to start cadderd.</Text>
        ) : !snapshot ? (
          <Text>Connecting...</Text>
        ) : rows.length === 0 ? (
          <Text>No projects registered. Run caddy run in your project.</Text>
        ) : (
          rows.slice(offset, offset + visible).map((row, index) => (
            <Text
              key={row.key}
              {...(offset + index === current ? { color: 'green' } : {})}
              wrap="truncate-end"
            >
              {offset + index === current ? '> ' : '  '}
              {row.enabled ? '\u25cf' : '\u25cb'} {terminalSafe(row.label)}
            </Text>
          ))
        )}
      </Box>
      {confirm ? (
        <Text>Stop cadderd and its Caddy process? y / n</Text>
      ) : pending ? (
        <Text>Working...</Text>
      ) : error ? (
        <Text color="red" wrap="truncate-end">
          {error}
        </Text>
      ) : (
        <Text> </Text>
      )}
      <Text>
        <Text color="green">Enter / Space</Text> toggle <Text color="green">s</Text> stop{' '}
        <Text color="green">r</Text> restart <Text color="green">q</Text> quit
      </Text>
    </Box>
  );
}

export async function openTui(client: ClientService): Promise<void> {
  let instance: ReturnType<typeof render> | undefined;
  const stop = (): void => instance?.unmount();
  try {
    instance = render(<RoutesApp client={client} />, {
      exitOnCtrlC: true,
      patchConsole: false,
      alternateScreen: true,
    });
    process.once('SIGTERM', stop);
    await instance.waitUntilExit();
  } finally {
    process.off('SIGTERM', stop);
    instance?.unmount();
  }
}
