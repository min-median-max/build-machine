param(
    [Parameter(Mandatory=$true)][string]$ConfigPath,
    [Parameter(Mandatory=$true)][string]$RequestPath,
    [switch]$CI
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
    if ($CI -or $request.ci) {
        $report = [ordered]@{
            platform='windows'; target=$request.target; success=$false; status='failed'; sourceHash=$request.sourceHash;
            revision=$request.revision; dirty=$request.dirty; stages=[ordered]@{}; artifacts=@();
            limits=@('Local CI never signs, notarizes or uploads to GitHub.'); commands=@(); signing='unverified'; publication=@{mode='local'; uploaded=$false}
        }
        try {
            foreach ($stageName in @('setup','test','build','smoke','release')) {
                $stageSteps = @($request.stages.$stageName)
                if ($stageSteps.Count -eq 0) {continue}
                $stage = [ordered]@{status='passed'; startedAt=(Get-Date).ToUniversalTime().ToString('o'); steps=@()}
                foreach ($step in $stageSteps) {
                    $entry = [ordered]@{index=$step.index; name=$step.name; adapter=$step.adapter; startedAt=(Get-Date).ToUniversalTime().ToString('o')}
                    $condition = [string]$step.if
                    if ($condition -and ($condition -match 'secrets\.|SIGNING_CONFIGURED|github\.token')) {
                        $entry.status='passed_with_limits'; $entry.skipped=$true; $stage.status='passed_with_limits'
                        $report.limits += 'A GitHub secret or token condition is false in the local runner.'
                    } elseif ($condition -and $condition -notmatch '^(true|false|''true''|''false''|always\(\)|1|0)$') {
                        throw "Unsupported workflow condition: $condition"
                    } elseif ($condition -match '^(false|''false''|0)$') {
                        $entry.status='passed_with_limits'; $entry.skipped=$true; $stage.status='passed_with_limits'
                    } elseif ($step.adapter -eq 'skip') {
                        $entry.status='passed_with_limits'; $entry.reason=$step.reason; $stage.status='passed_with_limits'
                        $report.limits += "$stageName skipped: $($step.reason)"
                    } elseif ($step.adapter -in @('checkout','pnpm-setup','node-setup','rust-setup','cache')) {
                        $entry.status='passed'; $entry.localAdapter=$true
                    } elseif ($step.adapter -in @('artifact-upload','release')) {
                        $entry.status='passed_with_limits'; $entry.localAdapter=$true; $stage.status='passed_with_limits'
                        $report.limits += "$($step.name): external GitHub service replaced by local artifact store"
                    } elseif ($step.adapter -eq 'run') {
                        if (-not $step.run) {throw "Workflow step $($step.index) has an empty run command."}
                        $working = $source
                        if ($step.'working-directory') {
                            $working = [IO.Path]::GetFullPath((Join-Path $source $step.'working-directory'))
                            $sourcePrefix = [IO.Path]::GetFullPath($source).TrimEnd('\') + '\'
                            if (-not $working.StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase)) {throw 'Workflow working-directory must be inside the source snapshot.'}
                        }
                        Push-Location $working
                        try {
                            $workingDirectory = if ($step.'working-directory') {[string]$step.'working-directory'} else {'.'}
                            $report.commands += [ordered]@{stage=$stageName; command=[string]$step.run; workingDirectory=$workingDirectory}
                            Invoke-Checked 'cmd.exe' @('/d','/s','/c',[string]$step.run)
                            $entry.status='passed'; $entry.exitCode=0
                        } catch {
                            $entry.status='failed'; $entry.exitCode=if ($LASTEXITCODE) {$LASTEXITCODE} else {1}; $stage.status='failed'; $stage.error=$_.Exception.Message
                        } finally {Pop-Location}
                    } elseif ($step.adapter -eq 'tauri-build') {
                        if (Test-Path -LiteralPath 'pnpm-lock.yaml') {
                            Invoke-Checked 'pnpm.cmd' @('install','--frozen-lockfile')
                            Invoke-Checked 'pnpm.cmd' @('exec','tauri','build','--no-sign','--ci','--target',$request.target,'--bundles',$request.bundle,'--','--locked')
                        } elseif (Test-Path -LiteralPath 'package-lock.json') {
                            Invoke-Checked 'npm.cmd' @('ci')
                            Invoke-Checked 'npm.cmd' @('exec','--','tauri','build','--no-sign','--ci','--target',$request.target,'--bundles',$request.bundle,'--','--locked')
                        } else {throw 'The Tauri action requires a pnpm or npm lockfile.'}
                        $output = Join-Path $source ("src-tauri\target\$($request.target)\release")
                        $candidate = if ($request.artifact) {Resolve-ProjectArtifact $source $request.artifact} else {@(Get-ChildItem -LiteralPath $output -Filter '*.exe' -File)[0].FullName}
                        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {throw 'The workflow did not produce the expected executable.'}
                        $entry.status='passed'; $entry.executable=$candidate; $entry.executableSHA256=(Get-FileHash -Algorithm SHA256 -LiteralPath $candidate).Hash
                    } else {throw "Unsupported workflow adapter: $($step.adapter)"}
                    $entry.finishedAt=(Get-Date).ToUniversalTime().ToString('o'); $stage.steps += $entry
                    if ($stage.status -eq 'failed') {break}
                }
                $stage.finishedAt=(Get-Date).ToUniversalTime().ToString('o'); $report.stages[$stageName]=$stage
                if ($stage.status -eq 'failed') {throw $stage.error}
            }
            $report.success=$true; $report.status=if ($report.limits.Count -gt 1) {'passed_with_limits'} else {'passed'}; $report.finishedAt=(Get-Date).ToUniversalTime().ToString('o')
        } catch {
            $report.success=$false; $report.status='failed'; $report.error=$_.Exception.Message; $report.finishedAt=(Get-Date).ToUniversalTime().ToString('o')
        }
        $report | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $request.resultPath -Encoding UTF8
        $report | ConvertTo-Json -Depth 20
        if (-not $report.success) {exit 1}
        exit 0
    }
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
