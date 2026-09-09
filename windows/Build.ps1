param(
    [Parameter(Mandatory=$true)][string]$ConfigPath,
    [Parameter(Mandatory=$true)][string]$RequestPath
)
. "$PSScriptRoot\Common.ps1"
$machine = Read-Machine $ConfigPath
$null = Initialize-Environment $machine
$request = Get-Content -Raw -LiteralPath $RequestPath | ConvertFrom-Json
if ($request.projectKey -notmatch '^[A-Za-z0-9._-]+$' -or $request.buildId -notmatch '^[a-f0-9]{64}$') {throw 'Invalid project key or build ID.'}
$projectRoot = Join-Path $machine.windowsRoot ("projects\" + $request.projectKey)
$buildRoot = Join-Path $projectRoot $request.buildId
$source = Join-Path $buildRoot 'source'
$receiptPath = Join-Path $buildRoot 'result.json'
$latestPath = Join-Path $projectRoot 'latest.json'

if (Test-Path -LiteralPath $receiptPath) {
    $receipt = Get-Content -Raw -LiteralPath $receiptPath | ConvertFrom-Json
    if ($receipt.buildId -eq $request.buildId -and (Test-Path -LiteralPath $receipt.executable) -and
        (Get-FileHash -Algorithm SHA256 -LiteralPath $receipt.executable).Hash -eq $receipt.executableSHA256) {
        Write-JsonFile $receipt $latestPath
        Write-Output "REUSED BUILD: $($receipt.executable)"
        $receipt | ConvertTo-Json -Depth 6
        exit 0
    }
}

New-Item -ItemType Directory -Path $buildRoot -Force | Out-Null
$log = Join-Path $buildRoot ('build-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '.log')
Start-Transcript -LiteralPath $log | Out-Null
try {
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $request.archive).Hash -ne $request.sourceHash) {throw 'Source archive checksum mismatch.'}
    if (-not (Test-Path -LiteralPath (Join-Path $buildRoot 'source-ready'))) {
        if (Test-Path -LiteralPath $source) {throw "An incomplete extraction exists at $source. Inspect it before retrying."}
        Expand-Archive -LiteralPath $request.archive -DestinationPath $source
        $request.sourceHash | Set-Content -LiteralPath (Join-Path $buildRoot 'source-ready')
    }
    Set-Location -LiteralPath $source
    $target = $machine.rust.host
    $artifact = $request.artifact
    $buildCommand = $request.command
    if ($request.framework -eq 'tauri') {
        if (-not (Test-Path -LiteralPath 'src-tauri\Cargo.lock')) {throw 'Tauri builds require src-tauri/Cargo.lock.'}
        if (Test-Path -LiteralPath 'pnpm-lock.yaml') {
            Invoke-Checked 'pnpm.cmd' @('install','--frozen-lockfile')
            $buildCommand = "pnpm exec tauri build --no-bundle --ci --target $target -- --locked"
            Invoke-Checked 'pnpm.cmd' @('exec','tauri','build','--no-bundle','--ci','--target',$target,'--','--locked')
        } elseif (Test-Path -LiteralPath 'package-lock.json') {
            Invoke-Checked 'npm.cmd' @('ci')
            $buildCommand = "npm exec -- tauri build --no-bundle --ci --target $target -- --locked"
            Invoke-Checked 'npm.cmd' @('exec','--','tauri','build','--no-bundle','--ci','--target',$target,'--','--locked')
        } else {throw 'The Tauri recipe requires a pnpm or npm lockfile. Use a custom recipe for another package manager.'}
        if (-not $artifact) {
            $executables = @(Get-ChildItem -LiteralPath "src-tauri\target\$target\release" -Filter '*.exe' -File)
            if ($executables.Count -ne 1) {throw 'Specify --artifact when the build produces zero or multiple candidate executables.'}
            $artifact = "src-tauri\target\$target\release\" + $executables[0].Name
        }
    } elseif ($request.framework -eq 'wails2') {
        $module = 'github.com/wailsapp/wails/v2'
        $version = & go.exe list -m -f '{{.Version}}' $module
        if ($LASTEXITCODE -ne 0 -or -not $version -or $version -notmatch '^v[0-9]') {throw 'The Wails v2 version must be declared in go.mod.'}
        $bin = Join-Path (Get-UserTools) ("wails2-$version")
        $wails = Join-Path $bin 'wails.exe'
        if (-not (Test-Path -LiteralPath $wails)) {
            $env:GOBIN = $bin
            Invoke-Checked 'go.exe' @('install',"$module/cmd/wails@$version")
            Remove-Item Env:GOBIN
        }
        $buildCommand = "wails $version build -platform windows/arm64"
        Invoke-Checked $wails @('build','-platform','windows/arm64')
        if (-not $artifact) {
            $executables = @(Get-ChildItem -LiteralPath 'build\bin' -Filter '*.exe' -File)
            if ($executables.Count -ne 1) {throw 'Specify --artifact when the build produces zero or multiple candidate executables.'}
            $artifact = 'build\bin\' + $executables[0].Name
        }
    } elseif ($request.framework -eq 'custom') {
        if (-not $buildCommand -or -not $artifact) {throw 'Custom builds require a command and an artifact.'}
        Invoke-Checked 'cmd.exe' @('/d','/s','/c',$buildCommand)
    } else {throw 'Unsupported framework.'}

    $executable = Resolve-ProjectArtifact $source $artifact
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {throw "Build did not produce $executable."}
    $msvc = Get-Msvc $machine
    $receipt = [ordered]@{
        project=$request.project
        projectKey=$request.projectKey
        buildId=$request.buildId
        revision=$request.revision
        dirty=$request.dirty
        sourceSHA256=$request.sourceHash
        controllerSHA256=$request.controllerHash
        builtAt=(Get-Date).ToUniversalTime().ToString('o')
        command=$buildCommand
        executable=$executable
        executableSHA256=(Get-FileHash -Algorithm SHA256 -LiteralPath $executable).Hash
        log=$log
        tools=@{node=(& node.exe --version); pnpm=(& pnpm.cmd --version); rust=(& rustc.exe --version); go=(& go.exe version); git=(& git.exe --version); msvc=$msvc.installationVersion}
    }
    Write-JsonFile $receipt $receiptPath
    Write-JsonFile $receipt $latestPath
    Write-Output "BUILT: $executable"
    $receipt | ConvertTo-Json -Depth 6
} finally {Stop-Transcript | Out-Null}
