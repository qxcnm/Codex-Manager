$ErrorActionPreference = "Stop"
$service = "D:\CPA-Dashboard\target\debug\codexmanager-service.exe"
$candidate = "D:\CPA-Dashboard\target\openruntime-hot\debug\codexmanager-service.exe"
Copy-Item -LiteralPath $candidate -Destination $service -Force
$env:CODEXMANAGER_DB_PATH = "D:\CPA-Dashboard\target\debug\codexmanager.db"
$env:CODEXMANAGER_SERVICE_ADDR = "localhost:48764"
$process = Start-Process -FilePath $service -WorkingDirectory "D:\CPA-Dashboard\target\debug" -WindowStyle Hidden `
    -RedirectStandardOutput "D:\CPA-Dashboard\exports\logs\openruntime-service-48764.stdout.log" `
    -RedirectStandardError "D:\CPA-Dashboard\exports\logs\openruntime-service-48764.stderr.log" -PassThru
Write-Output "started_pid=$($process.Id)"
for ($i = 0; $i -lt 80; $i++) {
    try {
        $response = Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:48764/health" -TimeoutSec 2
        if ($response.StatusCode -eq 200) { Write-Output "health=ok"; break }
    } catch {}
    Start-Sleep -Milliseconds 500
}
if ($i -ge 80) { Write-Output "health=timeout" }
