param([Parameter(Mandatory=$true)][string]$ConfigPath)
. "$PSScriptRoot\Common.ps1"
$machine = Read-Machine $ConfigPath
$null = Initialize-Environment $machine
$missing = @()
$versions = [ordered]@{}
foreach ($entry in @(
    @{name='node'; exe='node.exe'; args=@('--version'); expected="v$($machine.node.version)"},
    @{name='pnpm'; exe='pnpm.cmd'; args=@('--version'); expected=$machine.pnpm.version},
    @{name='rust'; exe='rustc.exe'; args=@('--version'); expected="rustc $($machine.rust.version) "},
    @{name='go'; exe='go.exe'; args=@('version'); expected="go version go$($machine.go.version) "},
    @{name='git'; exe='git.exe'; args=@('--version'); expected="git version $($machine.git.version)"}
)) {
    $command = Get-Command $entry.exe -ErrorAction SilentlyContinue
    if (-not $command) {$missing += $entry.name; $versions[$entry.name] = $null; continue}
    $arguments = $entry.args
    $output = (& $command.Source @arguments 2>$null) -join ' '
    $versions[$entry.name] = $output
    if ($LASTEXITCODE -ne 0 -or -not $output.StartsWith($entry.expected)) {$missing += $entry.name}
}
$msvc = Get-Msvc $machine
$versions['msvc'] = if ($msvc) {$msvc.installationVersion} else {$null}
if (-not $msvc) {$missing += 'msvc-components'}
$versions['webview2'] = Get-WebViewVersion
if (-not $versions['webview2']) {$missing += 'webview2'}
$report = [ordered]@{
    windows=(Get-CimInstance Win32_OperatingSystem).Caption
    architecture=[Environment]::GetEnvironmentVariable('PROCESSOR_ARCHITECTURE','Machine')
    user=[Security.Principal.WindowsIdentity]::GetCurrent().Name
    tools=$versions
    missing=$missing
    ready=($missing.Count -eq 0)
}
$report | ConvertTo-Json -Depth 5
if ($missing.Count -gt 0) {exit 1}
