param([Parameter(Mandatory=$true)][string]$ConfigPath)
. "$PSScriptRoot\Common.ps1"
$machine = Read-Machine $ConfigPath
$tools = Get-UserTools
$downloads = Join-Path $tools 'downloads'
New-Item -ItemType Directory -Path $tools -Force | Out-Null
$paths = Initialize-Environment $machine

$nodeDir = Join-Path $tools "node-v$($machine.node.version)-win-arm64"
$nodeExe = Join-Path $nodeDir 'node.exe'
if (-not (Test-Path -LiteralPath $nodeExe)) {
    $zip = Get-VerifiedDownload $machine.node.url $machine.node.sha256 (Join-Path $downloads "node-$($machine.node.version).zip")
    $staging = Join-Path $tools ('.node-' + [guid]::NewGuid().ToString('N'))
    Expand-Archive -LiteralPath $zip -DestinationPath $staging
    Move-Item -LiteralPath (Join-Path $staging (Split-Path -Leaf $nodeDir)) -Destination $nodeDir
    Remove-Item -LiteralPath $staging
    Write-Output "Installed: Node.js $($machine.node.version)."
} else {Write-Output "OK: Node.js already installed. No installation."}
if ((& $nodeExe --version) -ne "v$($machine.node.version)") {throw 'The managed Node.js version does not match machine.json.'}

$pnpmDir = Join-Path $tools "pnpm-$($machine.pnpm.version)"
$pnpmCmd = Join-Path $pnpmDir 'pnpm.cmd'
if (-not (Test-Path -LiteralPath $pnpmCmd)) {
    Invoke-Checked (Join-Path $nodeDir 'npm.cmd') @('install','--global',"pnpm@$($machine.pnpm.version)",'--prefix',$pnpmDir)
    Write-Output "Installed: pnpm $($machine.pnpm.version)."
} else {Write-Output 'OK: pnpm already installed. No installation.'}
if ((& $pnpmCmd --version) -ne $machine.pnpm.version) {throw 'The managed pnpm version does not match machine.json.'}

$rustup = Join-Path $env:USERPROFILE '.cargo\bin\rustup.exe'
if (-not (Test-Path -LiteralPath $rustup)) {
    $installer = Get-VerifiedDownload $machine.rust.installerUrl $machine.rust.installerSha256 (Join-Path $downloads 'rustup-init-1.29.1.exe')
    Invoke-Checked $installer @('-y','--profile','minimal','--default-host',$machine.rust.host,'--default-toolchain',$machine.rust.version,'--no-modify-path')
} else {
    $wanted = "$($machine.rust.version)-$($machine.rust.host)"
    $installed = @(& $rustup toolchain list)
    if (-not ($installed | Where-Object {$_ -match ('^' + [regex]::Escape($wanted) + '(\s|$)')})) {
        Invoke-Checked $rustup @('toolchain','install',$wanted,'--profile','minimal')
    } else {Write-Output "OK: Rust $wanted already installed. No installation."}
}
Invoke-Checked (Join-Path $env:USERPROFILE '.cargo\bin\rustc.exe') @('--version')

$goDir = Join-Path $tools "go-$($machine.go.version)"
$goExe = Join-Path $goDir 'go\bin\go.exe'
if (-not (Test-Path -LiteralPath $goExe)) {
    $zip = Get-VerifiedDownload $machine.go.url $machine.go.sha256 (Join-Path $downloads "go-$($machine.go.version).zip")
    $staging = "$goDir.extracting"
    if (Test-Path -LiteralPath $staging) {throw "An incomplete Go extraction exists at $staging. Inspect it before retrying."}
    Expand-Archive -LiteralPath $zip -DestinationPath $staging
    Move-Item -LiteralPath $staging -Destination $goDir
    Write-Output "Installed: Go $($machine.go.version)."
} else {Write-Output 'OK: Go already installed. No installation.'}
if ((& $goExe version) -notmatch ('go' + [regex]::Escape($machine.go.version) + '\s')) {throw 'The managed Go version does not match machine.json.'}

$gitDir = Join-Path $tools "git-$($machine.git.version)"
$gitExe = Join-Path $gitDir 'cmd\git.exe'
if (-not (Test-Path -LiteralPath $gitExe)) {
    $installer = Get-VerifiedDownload $machine.git.url $machine.git.sha256 (Join-Path $downloads "git-$($machine.git.version).exe")
    $arguments = @('/CURRENTUSER','/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/NOCANCEL','/SP-',('/DIR="' + $gitDir + '"'))
    $process = Start-Process -FilePath $installer -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {throw "Git installer failed: $($process.ExitCode)."}
    Write-Output "Installed: Git $($machine.git.version)."
} else {Write-Output 'OK: Git already installed. No installation.'}
if ((& $gitExe --version) -ne "git version $($machine.git.version)") {throw 'The managed Git version does not match machine.json.'}

$userPath = [Environment]::GetEnvironmentVariable('Path','User')
$parts = @($userPath -split ';' | Where-Object {$_})
foreach ($path in $paths) {if ($parts -notcontains $path) {$parts += $path}}
$updated = $parts -join ';'
if ($updated -ne $userPath) {[Environment]::SetEnvironmentVariable('Path',$updated,'User'); Write-Output 'Updated user PATH.'}
else {Write-Output 'OK: user PATH already contains the required entries. No change.'}
Write-Output 'User tool setup completed.'
