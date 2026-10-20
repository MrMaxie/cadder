import { build } from 'esbuild';
import { copyFile, mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'win32') throw new Error('Prepare the product Sandbox on Windows.');
const [artifact, upstream] = process.argv.slice(2);
if (!artifact || !upstream)
  throw new Error('Pass the SEA artifact directory and separate native Caddy.');
const root = await mkdtemp(join(tmpdir(), 'cadder-product-sandbox-'));
const input = join(root, 'input');
const reports = join(root, 'reports');
await mkdir(input);
await mkdir(reports);
await copyFile(process.execPath, join(input, 'node.exe'));
await copyFile(resolve(upstream), join(input, 'caddy.exe'));
await copyFile(
  fileURLToPath(
    new URL(
      '../openspec/changes/reset-cadder-architecture/windows-product-gate.md',
      import.meta.url,
    ),
  ),
  join(input, 'README.md'),
);
await build({
  entryPoints: [fileURLToPath(new URL('./verify-product-journey.ts', import.meta.url))],
  outfile: join(input, 'journey.mjs'),
  bundle: true,
  platform: 'node',
  target: 'node24.18',
  format: 'esm',
});
await writeFile(
  join(input, 'bootstrap.ps1'),
  `
$ErrorActionPreference = 'Stop'
Set-Content -LiteralPath 'C:\\CadderReports\\started.txt' -Value 'Product Sandbox started'
try {
  $probe = Start-Process -FilePath 'C:\\CadderTools\\node.exe' -ArgumentList @('C:\\CadderTools\\journey.mjs','C:\\CadderProduct','C:\\CadderTools\\caddy.exe') -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput 'C:\\CadderReports\\result.json' -RedirectStandardError 'C:\\CadderReports\\stderr.txt'
  Set-Content -LiteralPath 'C:\\CadderReports\\exit.txt' -Value $probe.ExitCode
} catch {
  Set-Content -LiteralPath 'C:\\CadderReports\\error.txt' -Value $_.Exception.Message
  Set-Content -LiteralPath 'C:\\CadderReports\\exit.txt' -Value 1
}
`,
);
const escape = (value: string): string =>
  value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
const config = join(root, 'product.wsb');
const configuration = `<Configuration>
  <Networking>Disable</Networking><vGPU>Disable</vGPU><ClipboardRedirection>Disable</ClipboardRedirection>
  <AudioInput>Disable</AudioInput><VideoInput>Disable</VideoInput><PrinterRedirection>Disable</PrinterRedirection>
  <MappedFolders>
    <MappedFolder><HostFolder>${escape(resolve(artifact))}</HostFolder><SandboxFolder>C:\\CadderProduct</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>${escape(input)}</HostFolder><SandboxFolder>C:\\CadderTools</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>${escape(reports)}</HostFolder><SandboxFolder>C:\\CadderReports</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>
  </MappedFolders>
  <LogonCommand><Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\\CadderTools\\bootstrap.ps1</Command></LogonCommand>
</Configuration>`;
await writeFile(config, configuration);
const privilegeConfig = join(root, 'privilege.wsb');
await writeFile(
  privilegeConfig,
  configuration.replace(
    'powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\\CadderTools\\bootstrap.ps1',
    'powershell.exe -NoProfile -NoExit -Command &quot;Set-Location C:\\CadderTools; Get-Content README.md&quot;',
  ),
);
process.stdout.write(
  JSON.stringify({
    config,
    privilegeConfig,
    reports,
    scope: 'Actual SEA product routing; final UAC/different-account acceptance remains separate.',
  }) + '\n',
);
