$ErrorActionPreference = 'Stop'
$baseUrl = if ($env:FILEFLOW_DOWNLOAD_PORTAL) { $env:FILEFLOW_DOWNLOAD_PORTAL } else { 'https://fileflow-downloads.pages.dev' }
$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($architecture -ne 'X64') { throw "FileFlow Setup Windows ne prend pas encore en charge $architecture." }
$manifest = Invoke-RestMethod "$baseUrl/api/downloads" -TimeoutSec 60
$artifact = $manifest.platforms.'windows-x86_64'.setup
if (-not $artifact.url -or -not $artifact.sha256) { throw 'Manifeste FileFlow invalide.' }
if (-not $artifact.url.StartsWith('https://github.com/idris-ach2002/FileFlow/releases/download/')) { throw 'URL FileFlow non autorisée.' }
if ($artifact.sha256 -notmatch '^[0-9a-fA-F]{64}$') { throw 'SHA-256 FileFlow invalide.' }
$temporary = Join-Path ([IO.Path]::GetTempPath()) "FileFlowSetup-$([guid]::NewGuid()).exe"
try {
  Write-Host 'Téléchargement de FileFlow Setup pour Windows x64…'
  Invoke-WebRequest $artifact.url -OutFile $temporary -TimeoutSec 900
  $actual = (Get-FileHash $temporary -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne $artifact.sha256.ToLowerInvariant()) { throw 'Échec SHA-256 : téléchargement refusé.' }
  Write-Host '✓ SHA-256 vérifié'
  Start-Process -FilePath $temporary -Wait
} finally {
  Remove-Item $temporary -Force -ErrorAction SilentlyContinue
}
