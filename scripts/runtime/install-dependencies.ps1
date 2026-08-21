[CmdletBinding()]
param([switch]$Quiet)

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
$script:Available = 0
$script:Missing = 0
$script:Warnings = 0

function Say([string]$Message) { if (-not $Quiet) { Write-Host $Message } }
function Warn([string]$Message) { $script:Warnings++; Write-Warning $Message }

function Refresh-Path {
  $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
  $user = [Environment]::GetEnvironmentVariable('Path', 'User')
  $extra = @(
    "$env:LOCALAPPDATA\Microsoft\WinGet\Links",
    "$env:LOCALAPPDATA\Microsoft\WindowsApps",
    "$env:USERPROFILE\scoop\shims",
    "$env:ChocolateyInstall\bin",
    "$env:APPDATA\Python\Scripts",
    "$env:LOCALAPPDATA\Programs\Python\Python312\Scripts"
  ) | Where-Object { $_ -and (Test-Path $_) }
  $env:Path = (@($machine, $user) + $extra | Where-Object { $_ }) -join ';'
}

function Find-Any([string[]]$Names) {
  Refresh-Path
  foreach ($name in $Names) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
  }
  return $null
}

function Find-LibreOffice {
  $command = Find-Any @('soffice.exe', 'libreoffice.exe', 'soffice', 'libreoffice')
  if ($command) { return $command }
  foreach ($path in @(
    "$env:ProgramFiles\LibreOffice\program\soffice.exe",
    "${env:ProgramFiles(x86)}\LibreOffice\program\soffice.exe",
    "$env:LOCALAPPDATA\Programs\LibreOffice\program\soffice.exe"
  )) {
    if ($path -and (Test-Path $path)) { return $path }
  }
  return $null
}

function Is-Available([string]$Probe) {
  if ($Probe -eq '@office') { return [bool](Find-LibreOffice) }
  return [bool](Find-Any ($Probe -split '\|'))
}

function Invoke-WingetInstall([string]$Id) {
  if (-not (Get-Command winget.exe -ErrorAction SilentlyContinue)) { return $false }
  Say "[TRY] winget:$Id"
  & winget.exe install --id $Id --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity
  if ($LASTEXITCODE -eq 0) { return $true }
  Warn "winget:$Id failed once; refreshing sources and retrying."
  & winget.exe source update --disable-interactivity | Out-Null
  & winget.exe install --id $Id --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity
  return ($LASTEXITCODE -eq 0)
}

function Invoke-ChocoInstall([string]$Name) {
  if (-not (Get-Command choco.exe -ErrorAction SilentlyContinue)) { return $false }
  Say "[TRY] choco:$Name"
  & choco.exe install $Name -y --no-progress
  return ($LASTEXITCODE -eq 0)
}

function Invoke-ScoopInstall([string]$Name) {
  if (-not (Get-Command scoop -ErrorAction SilentlyContinue)) { return $false }
  Say "[TRY] scoop:$Name"
  & scoop install $Name
  return ($LASTEXITCODE -eq 0)
}

function Ensure-Pipx {
  Refresh-Path
  if (Get-Command pipx.exe -ErrorAction SilentlyContinue) { return $true }
  if (-not (Get-Command python.exe -ErrorAction SilentlyContinue) -and -not (Get-Command py.exe -ErrorAction SilentlyContinue)) {
    [void](Invoke-WingetInstall 'Python.Python.3.12')
    Refresh-Path
  }
  $python = if (Get-Command py.exe -ErrorAction SilentlyContinue) { 'py.exe' } elseif (Get-Command python.exe -ErrorAction SilentlyContinue) { 'python.exe' } else { $null }
  if (-not $python) { return $false }
  & $python -m pip install --user pipx
  Refresh-Path
  return [bool](Get-Command pipx.exe -ErrorAction SilentlyContinue)
}

function Invoke-PipxInstall([string]$Name) {
  if (-not (Ensure-Pipx)) { return $false }
  Say "[TRY] pipx:$Name"
  & pipx.exe install $Name
  if ($LASTEXITCODE -ne 0) { & pipx.exe upgrade $Name }
  Refresh-Path
  return ($LASTEXITCODE -eq 0)
}

function Try-Candidate([string]$Candidate) {
  $parts = $Candidate.Split(':', 2)
  $kind = $parts[0]; $value = $parts[1]
  switch ($kind) {
    'winget' { return Invoke-WingetInstall $value }
    'choco'  { return Invoke-ChocoInstall $value }
    'scoop'  { return Invoke-ScoopInstall $value }
    'pipx'   { return Invoke-PipxInstall $value }
    default  { return $false }
  }
}

function Ensure-Engine([string]$Label, [string]$Probe, [string[]]$Candidates) {
  if (Is-Available $Probe) {
    Write-Host ('[OK]   {0,-14} already available' -f $Label)
    $script:Available++
    return
  }
  foreach ($candidate in $Candidates) {
    $worked = Try-Candidate $candidate
    Refresh-Path
    if ($worked -and (Is-Available $Probe)) {
      Write-Host ('[OK]   {0,-14} installed via {1}' -f $Label, $candidate.Split(':', 2)[0])
      $script:Available++
      return
    }
    Warn "${Label}: $candidate unavailable or installation failed; trying next source."
  }
  Write-Warning ('[MISS] {0,-14} not installed; related features will be disabled.' -f $Label)
  $script:Missing++
}

Write-Host "FileFlow runtime dependency setup - Windows / $([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)"
Refresh-Path

Ensure-Engine 'FFmpeg'       'ffmpeg.exe|ffmpeg'             @('winget:Gyan.FFmpeg','choco:ffmpeg','scoop:ffmpeg')
Ensure-Engine 'libvips'      'vips.exe|vips'                 @('winget:libvips.libvips','choco:vips','scoop:vips')
Ensure-Engine 'ImageMagick'  'magick.exe|magick'             @('winget:ImageMagick.ImageMagick','choco:imagemagick','scoop:imagemagick')
Ensure-Engine 'qpdf'         'qpdf.exe|qpdf'                 @('winget:QPDF.QPDF','choco:qpdf','scoop:qpdf')
Ensure-Engine 'img2pdf'      'img2pdf.exe|img2pdf'           @('pipx:img2pdf','scoop:img2pdf')
Ensure-Engine 'Poppler'      'pdftoppm.exe|pdftoppm'         @('winget:oschwartz10612.Poppler','choco:poppler','scoop:poppler')
Ensure-Engine 'Ghostscript'  'gswin64c.exe|gswin32c.exe|gs'  @('winget:ArtifexSoftware.GhostScript','choco:ghostscript','scoop:ghostscript')
Ensure-Engine 'Tesseract'    'tesseract.exe|tesseract'       @('winget:tesseract-ocr.tesseract','choco:tesseract','scoop:tesseract')
Ensure-Engine 'OCRmyPDF'     'ocrmypdf.exe|ocrmypdf'         @('pipx:ocrmypdf')
Ensure-Engine 'LibreOffice'  '@office'                       @('winget:TheDocumentFoundation.LibreOffice','choco:libreoffice-fresh','scoop:libreoffice')
Ensure-Engine 'Pandoc'       'pandoc.exe|pandoc'             @('winget:JohnMacFarlane.Pandoc','choco:pandoc','scoop:pandoc')
Ensure-Engine 'ExifTool'     'exiftool.exe|exiftool'         @('winget:OliverBetz.ExifTool','choco:exiftool','scoop:exiftool')
Ensure-Engine '7-Zip'        '7zz.exe|7z.exe|7z'             @('winget:7zip.7zip','choco:7zip','scoop:7zip')
Ensure-Engine 'Zstandard'    'zstd.exe|zstd'                 @('winget:Facebook.Zstandard','choco:zstandard','scoop:zstd')
Ensure-Engine 'LZ4'          'lz4.exe|lz4'                   @('winget:LZ4.LZ4','choco:lz4','scoop:lz4')

Write-Host ""
Write-Host "Runtime dependencies: $script:Available available, $script:Missing missing, $script:Warnings fallback warnings."
Write-Host 'Missing engines do not prevent FileFlow from being installed; only their related actions are unavailable.'
exit 0
