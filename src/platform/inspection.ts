import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { readlink } from 'node:fs/promises';
import { powershell } from './powershell.ts';
import { CadderError } from '../protocol/errors.ts';

const execute = promisify(execFile);
export type PortOwner = {
  protocol: 'tcp' | 'udp';
  address: string;
  port: number;
  pid: number | null;
  processName: string | null;
  executable: string | null;
};

export async function inspectPort(port: number): Promise<PortOwner[]> {
  if (!Number.isInteger(port) || port < 0 || port > 65535)
    throw new CadderError('invalid-input', 'Port must be from 0 to 65535.');
  if (process.platform === 'win32') {
    return JSON.parse(
      await powershell(`
      $result = @();
      foreach ($kind in @('tcp', 'udp')) {
        $sockets = if ($kind -eq 'tcp') { @(Get-NetTCPConnection -State Listen | Where-Object LocalPort -eq ${port}) } else { @(Get-NetUDPEndpoint | Where-Object LocalPort -eq ${port}) };
        foreach ($socket in $sockets) {
          $ownerProcess = Get-Process -Id $socket.OwningProcess -ErrorAction SilentlyContinue;
          $result += @{ protocol=$kind; address=[string]$socket.LocalAddress; port=[int]$socket.LocalPort; pid=[int]$socket.OwningProcess; processName=$ownerProcess.ProcessName; executable=$ownerProcess.Path };
        }
      };
      ConvertTo-Json -InputObject @($result) -Compress
    `),
    ) as PortOwner[];
  }
  if (process.platform === 'linux') {
    const { stdout } = await execute('ss', ['-H', '-lntup', `sport = :${port}`], {
      timeout: 5000,
      maxBuffer: 1024 * 1024,
    });
    const owners: PortOwner[] = [];
    for (const line of stdout.trim().split('\n').filter(Boolean)) {
      const fields = line.trim().split(/\s+/);
      const protocol = fields[0];
      if (protocol !== 'tcp' && protocol !== 'udp') continue;
      const address = fields[4]!.replace(/:\d+$/, '');
      const processes = [...line.matchAll(/\("([^"]*)",pid=(\d+)/g)];
      if (!processes.length)
        owners.push({ protocol, address, port, pid: null, processName: null, executable: null });
      for (const match of processes) {
        const pid = Number(match[2]);
        owners.push({
          protocol,
          address,
          port,
          pid,
          processName: match[1] ?? null,
          executable: await readlink(`/proc/${pid}/exe`).catch(() => null),
        });
      }
    }
    return owners;
  }
  if (process.platform === 'darwin') {
    let output: string;
    try {
      output = (
        await execute(
          '/usr/sbin/lsof',
          ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN', `-iUDP:${port}`, '-FpcPn'],
          { timeout: 5000, maxBuffer: 1024 * 1024 },
        )
      ).stdout;
    } catch (error) {
      if ((error as { code?: unknown }).code === 1) return [];
      throw error;
    }
    const owners: PortOwner[] = [];
    let pid: number | null = null;
    let name: string | null = null;
    let protocol: 'tcp' | 'udp' = 'tcp';
    for (const line of output.split('\n')) {
      if (line.startsWith('p')) pid = Number(line.slice(1));
      if (line.startsWith('c')) name = line.slice(1);
      if (line.startsWith('P')) protocol = line.slice(1) === 'UDP' ? 'udp' : 'tcp';
      if (line.startsWith('n'))
        owners.push({
          protocol,
          address: line.slice(1).replace(/:\d+$/, ''),
          port,
          pid,
          processName: name,
          executable: null,
        });
    }
    return owners;
  }
  throw new CadderError(
    'unsupported-operation',
    'Socket inspection is unavailable on this platform.',
  );
}

export async function terminatePortOwner(port: number, pid: number): Promise<void> {
  if (!Number.isSafeInteger(pid) || pid <= 0 || pid === process.pid)
    throw new CadderError('invalid-input', 'Expected a different positive process ID.');
  const owners = await inspectPort(port);
  if (!owners.some((owner) => owner.pid === pid))
    throw new CadderError('target-conflict', 'The expected process no longer owns this port.');
  process.kill(pid, 'SIGTERM');
}
