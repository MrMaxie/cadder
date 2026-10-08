# Interactive Windows runtime gate

Execution is deferred by the user's approved sequencing until the actual product
is complete. Tasks 2.8 and 7.4 require final product evidence before G7 acceptance,
Rust removal or release. The runtime-fixture procedure below is retained for
reference; it does not replace actual-product acceptance and must not be launched
during the current implementation stage.

This package tests the Node runtime foundation only. It is not an npm or SEA
release and does not contain product commands, Caddy, IIS or autostart mutations.
The host package is mounted read-only, networking is disabled, and all runtime
files and account changes below stay inside the disposable Sandbox.

Prepare on the host with Node 24.18.0:

```powershell
node scripts/prepare-windows-runtime-sandbox.ts
```

Open the printed `.wsb` file when ready to interact with UAC. Do not run the
account-creation commands below on the host. If this Sandbox image cannot
provide a non-elevated shell and a real UAC prompt, record the gate as blocked,
not passed. Do not disable or bypass UAC to obtain a passing result.

## Same owner, different privilege

In a non-elevated PowerShell inside Sandbox:

```powershell
Set-Location C:\CadderRuntimeGate
$cadderGateRuntime = Join-Path $env:TEMP ("cadder-uac-" + [guid]::NewGuid().ToString('N'))
.\node.exe .\runtime-windows-contact.mjs prepare $cadderGateRuntime
Start-Process -FilePath C:\CadderRuntimeGate\node.exe -Verb RunAs -WindowStyle Hidden -ArgumentList @('C:\CadderRuntimeGate\runtime-child.mjs', ('"' + $cadderGateRuntime + '"'), '--interactive-gate')
```

First cancel UAC. Verify that no daemon starts and `v2/cadder-ipc.json` does not
exist. Repeat `Start-Process`, approve the real prompt, then run:

```powershell
.\node.exe .\runtime-windows-contact.mjs elevated-status $cadderGateRuntime
```

The expected marker is `SAME_OWNER_ELEVATED_CONTACT_PASSED`. The fixture verifies
that the client is not elevated, the daemon is elevated, both have the same SID,
and an authenticated RPC succeeds. An elevated client does not pass this test.
The daemon fixture stops automatically after five minutes.

## Different account

Open an elevated PowerShell inside Sandbox, set `$cadderGateRuntime` to the exact
disposable path used above, and execute this block. It creates one temporary
standard account, grants access only to public test inputs and report files,
checks denial, and removes that account and its test inputs in `finally`.

```powershell
if ($env:USERNAME -ne 'WDAGUtilityAccount') { throw 'Run only inside the default disposable Windows Sandbox.' }
$ErrorActionPreference = 'Stop'
$cadderAccount = $null
$cadderOutsider = 'cadder-gate-' + [guid]::NewGuid().ToString('N').Substring(0, 6)
$cadderPassword = ConvertTo-SecureString ('A9!' + [guid]::NewGuid().ToString('N')) -AsPlainText -Force
$cadderInputs = Join-Path $env:PUBLIC ('cadder-gate-' + [guid]::NewGuid().ToString('N'))
try {
    $cadderAccount = New-LocalUser -Name $cadderOutsider -Password $cadderPassword
    New-Item -ItemType Directory -Path $cadderInputs | Out-Null
    Copy-Item -LiteralPath C:\CadderRuntimeGate\node.exe -Destination $cadderInputs
    Copy-Item -LiteralPath C:\CadderRuntimeGate\runtime-denied-client.mjs -Destination $cadderInputs
    $cadderInputAcl = Get-Acl -LiteralPath $cadderInputs
    $cadderReadRule = New-Object System.Security.AccessControl.FileSystemAccessRule($cadderAccount.SID, 'ReadAndExecute', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
    $cadderInputAcl.AddAccessRule($cadderReadRule)
    [System.IO.Directory]::SetAccessControl($cadderInputs, $cadderInputAcl)
    foreach ($cadderReportName in @('out.txt', 'err.txt')) {
        $cadderReport = Join-Path $cadderInputs $cadderReportName
        New-Item -ItemType File -Path $cadderReport | Out-Null
        $cadderReportAcl = Get-Acl -LiteralPath $cadderReport
        $cadderReportAcl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($cadderAccount.SID, 'Write', 'Allow')))
        [System.IO.File]::SetAccessControl($cadderReport, $cadderReportAcl)
    }
    $cadderCredential = New-Object System.Management.Automation.PSCredential(($env:COMPUTERNAME + '\' + $cadderOutsider), $cadderPassword)
    $cadderDeniedProcess = Start-Process -FilePath (Join-Path $cadderInputs 'node.exe') -ArgumentList @((Join-Path $cadderInputs 'runtime-denied-client.mjs'), ('"' + $cadderGateRuntime + '"')) -Credential $cadderCredential -LoadUserProfile -WorkingDirectory $cadderInputs -WindowStyle Hidden -RedirectStandardOutput (Join-Path $cadderInputs 'out.txt') -RedirectStandardError (Join-Path $cadderInputs 'err.txt') -Wait -PassThru
    if ($cadderDeniedProcess.ExitCode -ne 0 -or (Get-Content -LiteralPath (Join-Path $cadderInputs 'out.txt') -Raw).Trim() -ne 'DENIED') {
        throw ('Different-account gate failed: ' + (Get-Content -LiteralPath (Join-Path $cadderInputs 'err.txt') -Raw))
    }
    'DIFFERENT_ACCOUNT_DENIED'
} finally {
    if ($null -ne $cadderAccount) { Remove-LocalUser -SID $cadderAccount.SID }
    $cadderResolvedInputs = [System.IO.Path]::GetFullPath($cadderInputs)
    $cadderPublicRoot = [System.IO.Path]::GetFullPath($env:PUBLIC).TrimEnd('\') + '\'
    if (!$cadderResolvedInputs.StartsWith($cadderPublicRoot, [StringComparison]::OrdinalIgnoreCase) -or [System.IO.Path]::GetFileName($cadderResolvedInputs) -notmatch '^cadder-gate-[0-9a-f]{32}$') { throw 'Unsafe test cleanup path.' }
    if (Test-Path -LiteralPath $cadderResolvedInputs) { Remove-Item -LiteralPath $cadderResolvedInputs -Recurse -Force }
}
```

A failure to launch Node under the other account is a failed gate, not evidence
that IPC denied the account. The expected denial must come from the test client.

Back in the original non-elevated shell:

```powershell
.\node.exe .\runtime-windows-contact.mjs elevated-status $cadderGateRuntime
.\node.exe .\runtime-windows-contact.mjs shutdown $cadderGateRuntime
```

Record the three markers, UAC cancellation/approval, and any errors. Close
Sandbox to discard its runtime, test account profile and other disposable state.
Only reviewed actual-product results can close tasks 2.8 and 7.4. Runtime-fixture
results alone do not satisfy those tasks. This interactive gate has not been
verified merely by preparing the package.
