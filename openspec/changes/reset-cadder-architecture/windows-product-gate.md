# Interactive Windows product privilege acceptance

Use the `privilege.wsb` produced by `scripts/prepare-product-sandbox.ts` for the
exact candidate being accepted. The package mounts candidate executables and
separate upstream Caddy read-only, disables networking, and writes only reports
back to the host. This procedure changes accounts only inside disposable Sandbox.
Do not execute its account block on the host.

The automated routing report does not pass this gate. A real non-elevated client,
same-SID elevated daemon, UAC cancellation/approval and a successfully launched
different-account client are required. If Sandbox cannot provide a non-elevated
shell and a real UAC prompt, record BLOCKED. Do not bypass or disable UAC.

## Same owner, cancellation and elevation

In a non-elevated PowerShell inside Sandbox:

```powershell
$cadderIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
$cadderPrincipal = New-Object Security.Principal.WindowsPrincipal($cadderIdentity)
if ($cadderPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Use a non-elevated client shell.' }
$cadderGateRuntime = Join-Path $env:TEMP ('cadder-product-uac-' + [guid]::NewGuid().ToString('N'))
$env:CADDER_RUNTIME_DIR = $cadderGateRuntime
$cadderArguments = @('--runtime-dir', ('"' + $cadderGateRuntime + '"'), '--real-caddy', 'C:\CadderTools\caddy.exe')
Start-Process -FilePath C:\CadderProduct\cadderd.exe -Verb RunAs -WindowStyle Hidden -ArgumentList $cadderArguments
```

Cancel the actual UAC prompt. Verify that no discovery file exists and the actual
operator reports offline (exit 3):

```powershell
if (Test-Path -LiteralPath (Join-Path $cadderGateRuntime 'v2\cadder-ipc.json')) { throw 'Cancelled elevation published a daemon.' }
& C:\CadderProduct\cadder.exe status
if ($LASTEXITCODE -ne 3) { throw 'Cancelled startup was not offline.' }
```

Repeat `Start-Process`, approve the prompt, then use the same non-elevated shell:

```powershell
$cadderDeadline = (Get-Date).AddSeconds(45)
do {
    & C:\CadderProduct\cadder.exe status
    if ($LASTEXITCODE -eq 0) { break }
    Start-Sleep -Milliseconds 300
} while ((Get-Date) -lt $cadderDeadline)
if ($LASTEXITCODE -ne 0) { throw 'Ordinary client could not contact the product daemon.' }
$cadderDiscovery = Get-Content -LiteralPath (Join-Path $cadderGateRuntime 'v2\cadder-ipc.json') -Raw | ConvertFrom-Json
if (!$cadderDiscovery.elevated -or $cadderDiscovery.owner -ne $cadderIdentity.User.Value) { throw 'Expected the same SID and an elevated daemon.' }
if ($cadderPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'The client must remain non-elevated.' }
'SAME_OWNER_ELEVATED_PRODUCT_CONTACT_PASSED'
```

Record the candidate version/checksum and the observed cancellation/approval.
Do not export runtime secrets, account passwords or private key material.

## Different account

In an elevated PowerShell inside Sandbox, set `$cadderGateRuntime` to the exact
path from the first shell, then run:

```powershell
if ($env:USERNAME -ne 'WDAGUtilityAccount') { throw 'Run only inside disposable Windows Sandbox.' }
$ErrorActionPreference = 'Stop'
$cadderAccount = $null
$cadderOutsider = 'cadder-gate-' + [guid]::NewGuid().ToString('N').Substring(0, 6)
$cadderPassword = ConvertTo-SecureString ('A9!' + [guid]::NewGuid().ToString('N')) -AsPlainText -Force
$cadderInputs = Join-Path $env:PUBLIC ('cadder-product-gate-' + [guid]::NewGuid().ToString('N'))
try {
    $cadderAccount = New-LocalUser -Name $cadderOutsider -Password $cadderPassword
    $cadderUsers = Get-LocalGroup -SID 'S-1-5-32-545'
    Add-LocalGroupMember -Group $cadderUsers.Name -Member $cadderAccount
    New-Item -ItemType Directory -Path $cadderInputs | Out-Null
    Copy-Item -LiteralPath C:\CadderProduct\cadder.exe -Destination $cadderInputs
    $cadderQuotedRuntime = $cadderGateRuntime.Replace("'", "''")
    $cadderProbeSource = '$ErrorActionPreference = ''Continue''; $env:CADDER_RUNTIME_DIR = ''' + $cadderQuotedRuntime + '''; & (Join-Path $PSScriptRoot ''cadder.exe'') status; exit $LASTEXITCODE'
    Set-Content -LiteralPath (Join-Path $cadderInputs 'probe.ps1') -Value $cadderProbeSource
    $cadderInputAcl = Get-Acl -LiteralPath $cadderInputs
    $cadderInputAcl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($cadderAccount.SID, 'ReadAndExecute', 'ContainerInherit,ObjectInherit', 'None', 'Allow')))
    [IO.Directory]::SetAccessControl($cadderInputs, $cadderInputAcl)
    foreach ($cadderReportName in @('out.txt', 'err.txt')) {
        $cadderReport = Join-Path $cadderInputs $cadderReportName
        New-Item -ItemType File -Path $cadderReport | Out-Null
        $cadderReportAcl = Get-Acl -LiteralPath $cadderReport
        $cadderReportAcl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($cadderAccount.SID, 'Write', 'Allow')))
        [IO.File]::SetAccessControl($cadderReport, $cadderReportAcl)
    }
    $cadderCredential = New-Object Management.Automation.PSCredential(($env:COMPUTERNAME + '\' + $cadderOutsider), $cadderPassword)
    $cadderDenied = Start-Process -FilePath "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $cadderInputs 'probe.ps1')) -Credential $cadderCredential -LoadUserProfile -WorkingDirectory $cadderInputs -WindowStyle Hidden -RedirectStandardOutput (Join-Path $cadderInputs 'out.txt') -RedirectStandardError (Join-Path $cadderInputs 'err.txt') -Wait -PassThru
    if ($cadderDenied.ExitCode -ne 7) { throw ('Expected access denial from the actual CLI, got exit ' + $cadderDenied.ExitCode) }
    'DIFFERENT_ACCOUNT_PRODUCT_DENIED'
} finally {
    if ($null -ne $cadderAccount) { Remove-LocalUser -SID $cadderAccount.SID }
    $cadderResolvedInputs = [IO.Path]::GetFullPath($cadderInputs)
    $cadderPublicRoot = [IO.Path]::GetFullPath($env:PUBLIC).TrimEnd('\') + '\'
    if (!$cadderResolvedInputs.StartsWith($cadderPublicRoot, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($cadderResolvedInputs) -notmatch '^cadder-product-gate-[0-9a-f]{32}$') { throw 'Unsafe test cleanup path.' }
    if (Test-Path -LiteralPath $cadderResolvedInputs) { Remove-Item -LiteralPath $cadderResolvedInputs -Recurse -Force }
}
```

A failed account/process launch does not pass. Only exit 7 from the actual
operator proves the expected access-denied boundary. Back in the original
non-elevated shell, verify `cadder.exe status` still succeeds, then stop with
`C:\CadderProduct\cadder.exe daemon stop` and close Sandbox.

Record all actual results before closing parent tasks 2.8 and 7.4. Preparation
and an elevated-client success alone do not constitute acceptance.
