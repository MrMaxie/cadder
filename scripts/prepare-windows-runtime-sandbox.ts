import { execFile } from 'node:child_process';
import { copyFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

if (process.platform !== 'win32') throw new Error('Prepare this test package on Windows.');
if (process.versions.node !== '24.18.0') throw new Error('Use Node 24.18.0 for this runtime gate.');
const environment = { ...process.env };
delete environment.NODE_OPTIONS;
const result = await promisify(execFile)(
  process.execPath,
  [fileURLToPath(new URL('./prepare-runtime-gate.ts', import.meta.url))],
  { env: environment },
);
const stage = result.stdout.trim();
await copyFile(process.execPath, join(stage, 'node.exe'));
await copyFile(
  fileURLToPath(
    new URL(
      '../openspec/changes/reset-cadder-architecture/windows-runtime-gate.md',
      import.meta.url,
    ),
  ),
  join(stage, 'README.md'),
);
const configuration = join(stage, 'runtime-gate.wsb');
await writeFile(
  configuration,
  `<Configuration>
  <Networking>Disable</Networking>
  <vGPU>Disable</vGPU>
  <AudioInput>Disable</AudioInput>
  <VideoInput>Disable</VideoInput>
  <PrinterRedirection>Disable</PrinterRedirection>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>${escapeXml(stage)}</HostFolder>
      <SandboxFolder>C:\\CadderRuntimeGate</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -NoProfile -NoExit -Command &quot;Set-Location 'C:\\CadderRuntimeGate'; Get-Content README.md&quot;</Command>
  </LogonCommand>
</Configuration>
`,
);
process.stdout.write(`${configuration}\n`);

function escapeXml(value: string): string {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
}
