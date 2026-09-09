param(
    [Parameter(Mandatory=$true)][string]$ConfigPath,
    [Parameter(Mandatory=$true)][string]$ProjectKey
)
. "$PSScriptRoot\Common.ps1"
$machine = Read-Machine $ConfigPath
if ($ProjectKey -notmatch '^[A-Za-z0-9._-]+$') {throw 'Invalid project key.'}
$projectRoot = Join-Path $machine.windowsRoot ("projects\$ProjectKey")
$receiptPath = Join-Path $projectRoot 'latest.json'
if (-not (Test-Path -LiteralPath $receiptPath)) {throw 'No successful build exists for this project. Run build first.'}
$receipt = Get-Content -Raw -LiteralPath $receiptPath | ConvertFrom-Json
if (-not (Test-Path -LiteralPath $receipt.executable)) {throw 'The built executable is missing.'}
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $receipt.executable).Hash -ne $receipt.executableSHA256) {throw 'The executable checksum no longer matches its build receipt.'}
$process = Get-Process -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $receipt.executable} | Select-Object -First 1
$reused = ($null -ne $process)
if (-not $process) {$process = Start-Process -FilePath $receipt.executable -WorkingDirectory (Split-Path -Parent $receipt.executable) -PassThru}
for ($attempt=0; $attempt -lt 30; $attempt++) {
    Start-Sleep -Milliseconds 500
    $process.Refresh()
    if ($process.HasExited) {throw "The application exited with code $($process.ExitCode)."}
    if ($process.MainWindowHandle -ne 0) {break}
}
$report = [ordered]@{executable=$receipt.executable; processId=$process.Id; reusedProcess=$reused; windowTitle=$process.MainWindowTitle; windowHandle=$process.MainWindowHandle.ToInt64(); responding=$process.Responding}
Write-JsonFile $report (Join-Path $projectRoot 'running.json')
$report | ConvertTo-Json
if ($process.MainWindowHandle -eq 0) {throw 'The process is running but no application window was detected.'}
