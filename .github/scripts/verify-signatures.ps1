# Fails the release unless every exe/dll that ships is validly signed and timestamped: ours by
# Starkey Digital, wintun.dll by WireGuard.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$pkg = Join-Path $env:RUNNER_TEMP pkg
[IO.Compression.ZipFile]::ExtractToDirectory((Resolve-Path "Releases\*$env:VERSION*-full.nupkg").Path, $pkg)
$files = @(Get-Item Releases\*Setup.exe) + @(Get-ChildItem $pkg -Recurse -Include *.exe, *.dll)

foreach ($name in 'local-mates.exe', 'local-mates-app.exe', 'wintun.dll') {
    if ($files.Name -notcontains $name) { throw "$name missing from the package" }
}

foreach ($f in $files) {
    $sig = Get-AuthenticodeSignature $f.FullName
    $signer = $sig.SignerCertificate.Subject
    $expected = if ($f.Name -eq 'wintun.dll') { 'CN=WireGuard LLC' } else { 'CN=Starkey Digital Ltd' }
    if ($sig.Status -ne 'Valid' -or -not $sig.TimeStamperCertificate -or $signer -notmatch $expected) {
        throw "$($f.Name): $($sig.Status), signer '$signer', timestamped: $([bool]$sig.TimeStamperCertificate)"
    }
    Write-Host "ok: $($f.Name) signed by $signer"
}
