# Exercises the real Windows path on a CI runner (which runs as admin): service install, the
# named pipe, Wintun adapter creation and its network settings, and clean teardown.
$ErrorActionPreference = 'Stop'
$exe = Resolve-Path dist\local-mates.exe
$alias = 'Local Mates'

function Wait-Until([scriptblock]$check, [string]$what, [int]$seconds = 30) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while (-not (& $check)) {
        if ((Get-Date) -gt $deadline) { throw "Timed out waiting for: $what" }
        Start-Sleep -Milliseconds 500
    }
    Write-Host "ok: $what"
}

& $exe service install
if ($LASTEXITCODE) { throw 'service install failed' }
Wait-Until { (Get-Service LocalMates).Status -eq 'Running' } 'service running'

$rooms = & $exe rooms
if ($LASTEXITCODE -or -not ($rooms -match "Your room's code: \w{3}-\w{3}")) {
    throw "rooms over the pipe failed: $rooms"
}
Write-Host 'ok: client talks to service'

$hostProc = Start-Process $exe -ArgumentList host -PassThru -NoNewWindow `
    -RedirectStandardOutput host.log -RedirectStandardError host.err
try {
    Wait-Until { Get-NetIPAddress -InterfaceAlias $alias -IPAddress 10.77.0.1 -ErrorAction SilentlyContinue } 'adapter has 10.77.0.1'
    Wait-Until { (Get-NetIPInterface -InterfaceAlias $alias -AddressFamily IPv4).InterfaceMetric -eq 1 } 'adapter metric is 1'
    Wait-Until { (Get-NetConnectionProfile -InterfaceAlias $alias -ErrorAction SilentlyContinue).NetworkCategory -eq 'Private' } 'adapter is a Private network'

    & $exe leave
    Wait-Until { -not (Get-NetAdapter -Name $alias -ErrorAction SilentlyContinue) } 'adapter removed after leave'
    Wait-Until { $hostProc.HasExited } 'host command exited'
} finally {
    Write-Host '--- host output ---'
    Get-Content host.log, host.err -ErrorAction SilentlyContinue
    if (-not $hostProc.HasExited) { $hostProc.Kill() }
}

& $exe service uninstall
if ($LASTEXITCODE) { throw 'service uninstall failed' }
Wait-Until { -not (Get-Service LocalMates -ErrorAction SilentlyContinue) } 'service removed'
