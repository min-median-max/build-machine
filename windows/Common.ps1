Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$global:ProgressPreference = 'SilentlyContinue'

function Read-Machine {
    param([string]$Path)
    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
}

function Get-UserTools {
    Join-Path $env:LOCALAPPDATA 'WindowsBuildMachine'
}

function Initialize-Environment {
    param($Machine)
    $tools = Get-UserTools
    $paths = @(
        (Join-Path $tools "node-v$($Machine.node.version)-win-arm64"),
        (Join-Path $tools "pnpm-$($Machine.pnpm.version)"),
        (Join-Path $tools "go-$($Machine.go.version)\go\bin"),
        (Join-Path $tools "git-$($Machine.git.version)\cmd"),
        (Join-Path $env:USERPROFILE '.cargo\bin'),
        (Join-Path $env:USERPROFILE 'go\bin')
    )
    $env:Path = ($paths -join ';') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')
    $env:RUSTUP_TOOLCHAIN = "$($Machine.rust.version)-$($Machine.rust.host)"
    return ,$paths
}

function Invoke-Checked {
    param([string]$Executable, [string[]]$Arguments = @())
    Write-Output (('> ' + $Executable + ' ' + ($Arguments -join ' ')).Trim())
    # Windows PowerShell wraps native stderr in ErrorRecord objects. Cargo writes
    # ordinary progress to stderr, so check the exit code after forwarding it.
    $ErrorActionPreference = 'Continue'
    & $Executable @Arguments 2>&1 | ForEach-Object { Write-Output "$_" }
    $code = $LASTEXITCODE
    if ($code -ne 0) {throw "$Executable failed with exit code $code."}
}

function Get-VerifiedDownload {
    param([string]$Url, [string]$Sha256, [string]$Destination)
    if (Test-Path -LiteralPath $Destination) {
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath $Destination).Hash -eq $Sha256) {return $Destination}
    }
    New-Item -ItemType Directory -Path (Split-Path -Parent $Destination) -Force | Out-Null
    $partial = "$Destination.partial"
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    [Console]::WriteLine("Downloading $Url")
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $partial
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $partial).Hash -ne $Sha256) {throw "Checksum mismatch: $Url"}
    Move-Item -LiteralPath $partial -Destination $Destination -Force
    return $Destination
}

function Get-Msvc {
    param($Machine)
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) {return $null}
    $arguments = @('-products','*','-version','[17.0,18.0)','-requires') + @($Machine.msvc.components) + @('-format','json','-utf8')
    $raw = & $vswhere @arguments
    if ($LASTEXITCODE -ne 0) {throw 'vswhere could not inspect MSVC.'}
    $instances = @($raw | ConvertFrom-Json)
    $complete = @($instances | Where-Object {$_.isComplete -and $_.isLaunchable})
    if ($complete.Count -eq 0) {return $null}
    return $complete[0]
}

function Get-WebViewVersion {
    $id = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    foreach ($base in @('HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients','HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients','HKCU:\Software\Microsoft\EdgeUpdate\Clients')) {
        $entry = Get-ItemProperty -LiteralPath "$base\$id" -ErrorAction SilentlyContinue
        if ($entry -and $entry.PSObject.Properties['pv'] -and $entry.pv -ne '0.0.0.0') {return $entry.pv}
    }
    return $null
}

function Write-JsonFile {
    param($Value, [string]$Path)
    New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
    $Value | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath "$Path.partial" -Encoding UTF8
    Move-Item -LiteralPath "$Path.partial" -Destination $Path -Force
}

function Resolve-ProjectArtifact {
    param([string]$Source, [string]$Relative)
    $root = [IO.Path]::GetFullPath($Source).TrimEnd('\') + '\'
    $full = [IO.Path]::GetFullPath((Join-Path $Source $Relative))
    if (-not $full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {throw 'The executable must be inside the project build directory.'}
    if ([IO.Path]::GetExtension($full) -ne '.exe') {throw 'The artifact must be a Windows .exe file.'}
    return $full
}
