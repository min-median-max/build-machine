param([Parameter(Mandatory=$true)][string]$ConfigPath)
. "$PSScriptRoot\Common.ps1"
$machine = Read-Machine $ConfigPath
$null = Initialize-Environment $machine
$missing = @()
$issues = @()
$versions = [ordered]@{}
foreach ($entry in @(
    @{name='node'; exe='node.exe'; args=@('--version'); expected="v$($machine.node.version)"},
    @{name='pnpm'; exe='pnpm.cmd'; args=@('--version'); expected=$machine.pnpm.version},
    @{name='rust'; exe='rustc.exe'; args=@('--version'); expected="rustc $($machine.rust.version) "},
    @{name='go'; exe='go.exe'; args=@('version'); expected="go version go$($machine.go.version) "},
    @{name='git'; exe='git.exe'; args=@('--version'); expected="git version $($machine.git.version)"}
)) {
    $command = Get-Command $entry.exe -ErrorAction SilentlyContinue
    if (-not $command) {
        $missing += $entry.name
        $versions[$entry.name] = $null
        $issues += "$($entry.name): expected $($entry.expected.Trim()); executable not found"
        continue
    }
    $arguments = $entry.args
    $output = (& $command.Source @arguments 2>$null) -join ' '
    $versions[$entry.name] = $output
    if ($LASTEXITCODE -ne 0 -or -not $output.StartsWith($entry.expected)) {
        $missing += $entry.name
        $issues += "$($entry.name): expected $($entry.expected.Trim()); found $output"
    }
}
$msvc = Get-Msvc $machine
$versions['msvc'] = if ($msvc) {$msvc.installationVersion} else {$null}
if (-not $msvc) {$missing += 'msvc-components'; $issues += 'MSVC: required compiler or SDK components are missing'}
$versions['webview2'] = Get-WebViewVersion
if (-not $versions['webview2']) {$missing += 'webview2'; $issues += 'WebView2 runtime is missing'}
$report = [ordered]@{
    windows=(Get-CimInstance Win32_OperatingSystem).Caption
    architecture=[Environment]::GetEnvironmentVariable('PROCESSOR_ARCHITECTURE','Machine')
    user=[Security.Principal.WindowsIdentity]::GetCurrent().Name
    tools=$versions
    missing=$missing
    issues=$issues
    ready=($missing.Count -eq 0)
}
$report | ConvertTo-Json -Depth 5
if ($missing.Count -gt 0) {Write-Output ('ERROR: ' + ($issues -join '; ')); exit 1}
