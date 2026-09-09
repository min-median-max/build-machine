param([Parameter(Mandatory=$true)][string]$ConfigPath)
. "$PSScriptRoot\Common.ps1"
$machine = Read-Machine $ConfigPath
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {throw 'Run Setup-Machine.ps1 with administrator rights.'}
if ([Environment]::GetEnvironmentVariable('PROCESSOR_ARCHITECTURE','Machine') -ne 'ARM64') {throw 'This machine definition requires Windows ARM64.'}

$msvc = Get-Msvc $machine
if ($msvc) {
    Write-Output "OK: MSVC $($msvc.installationVersion), required components present. No installation."
} else {
    $cache = Join-Path $machine.windowsRoot 'downloads'
    New-Item -ItemType Directory -Path $cache -Force | Out-Null
    $installer = Join-Path $cache 'vs_BuildTools.exe'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -UseBasicParsing -Uri $machine.msvc.installerUrl -OutFile $installer
    $signature = Get-AuthenticodeSignature -FilePath $installer
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') {throw 'MSVC installer signature verification failed.'}
    $arguments = @('--quiet','--wait','--norestart','--installPath',('"' + $machine.msvc.installPath + '"'),'--addProductLang','en-US')
    foreach ($component in $machine.msvc.components) {$arguments += @('--add', $component)}
    $process = Start-Process -FilePath $installer -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -notin @(0,3010)) {throw "MSVC installer failed: $($process.ExitCode)."}
    $msvc = Get-Msvc $machine
    if (-not $msvc) {throw 'MSVC installation ended without the required components.'}
    Write-Output "Installed: MSVC $($msvc.installationVersion). Installer exit code $($process.ExitCode)."
}

$webview = Get-WebViewVersion
if ($webview) {
    Write-Output "OK: WebView2 $webview. No installation."
} else {
    $cache = Join-Path $machine.windowsRoot 'downloads'
    New-Item -ItemType Directory -Path $cache -Force | Out-Null
    $installer = Join-Path $cache 'MicrosoftEdgeWebview2Setup.exe'
    Invoke-WebRequest -UseBasicParsing -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $installer
    $signature = Get-AuthenticodeSignature -FilePath $installer
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') {throw 'WebView2 installer signature verification failed.'}
    $process = Start-Process -FilePath $installer -ArgumentList '/silent /install' -Wait -PassThru
    if ($process.ExitCode -ne 0 -or -not (Get-WebViewVersion)) {throw "WebView2 installation failed: $($process.ExitCode)."}
    Write-Output "Installed: WebView2 $(Get-WebViewVersion)."
}
