[CmdletBinding()]
param([switch]$Quiet, [string]$Engines = '', [string]$ReportPath = '')

if (-not [string]::IsNullOrWhiteSpace($Engines)) {
  $env:FILEFLOW_SETUP_ENGINES = $Engines
}
if ([string]::IsNullOrWhiteSpace($ReportPath)) {
  $ReportPath = $env:FILEFLOW_SETUP_ENGINE_REPORT
}
if (-not [string]::IsNullOrWhiteSpace($ReportPath)) {
  $reportDirectory = Split-Path -Parent $ReportPath
  if ($reportDirectory) { New-Item -ItemType Directory -Path $reportDirectory -Force | Out-Null }
  Set-Content -LiteralPath $ReportPath -Value '' -NoNewline -Encoding utf8
}

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
$script:Available = 0
$script:Missing = 0
$script:Warnings = 0

function Say([string]$Message) { if (-not $Quiet) { Write-Host $Message } }
function Warn([string]$Message) { $script:Warnings++; Write-Warning $Message }

function Write-OwnedPackage(
  [string]$Component,
  [string]$Manager,
  [string]$Package,
  [string]$Kind = 'engine'
) {
  if ([string]::IsNullOrWhiteSpace($ReportPath)) { return }
  foreach ($value in @($Component, $Manager, $Package, $Kind)) {
    if ($value -match "[`t`r`n]") { throw 'Invalid package receipt field.' }
  }
  Add-Content -LiteralPath $ReportPath -Value "$Component`t$Manager`t$Package`t$Kind" -Encoding utf8
}

function Test-EngineSelected([string]$Id) {
  if ([string]::IsNullOrWhiteSpace($env:FILEFLOW_SETUP_ENGINES)) { return $true }
  return @($env:FILEFLOW_SETUP_ENGINES -split ',' | ForEach-Object { $_.Trim().ToLowerInvariant() }) -contains $Id.ToLowerInvariant()
}

function Refresh-Path {
  $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
  $user = [Environment]::GetEnvironmentVariable('Path', 'User')

  $pythonScripts = @()
  if ($env:APPDATA) {
    $pythonScripts += @(
      Get-ChildItem -Path "$env:APPDATA\Python\Python*\Scripts" -Directory -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty FullName
    )
  }
  if ($env:LOCALAPPDATA) {
    $pythonScripts += @(
      Get-ChildItem -Path "$env:LOCALAPPDATA\Programs\Python\Python*\Scripts" -Directory -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty FullName
    )
  }

  $extra = @(
    "$env:LOCALAPPDATA\Microsoft\WinGet\Links",
    "$env:LOCALAPPDATA\Microsoft\WindowsApps",
    "$env:USERPROFILE\scoop\shims",
    "$env:ChocolateyInstall\bin"
  ) + $pythonScripts

  $extra = @($extra | Where-Object { $_ -and (Test-Path $_) } | Select-Object -Unique)
  $env:Path = (@($machine, $user) + $extra | Where-Object { $_ }) -join ';'
}

function Get-RealPythonInvocation {
  Refresh-Path
  $candidates = @()

  $py = Get-Command py.exe -ErrorAction SilentlyContinue
  if ($py -and $py.Source -and $py.Source -notlike '*\Microsoft\WindowsApps\*') {
    $candidates += [pscustomobject]@{ File = $py.Source; Prefix = @('-3') }
  }

  $python = Get-Command python.exe -ErrorAction SilentlyContinue
  if ($python -and $python.Source -and $python.Source -notlike '*\Microsoft\WindowsApps\*') {
    $candidates += [pscustomobject]@{ File = $python.Source; Prefix = @() }
  }

  if ($env:LOCALAPPDATA) {
    foreach ($item in @(
      Get-ChildItem -Path "$env:LOCALAPPDATA\Programs\Python\Python*\python.exe" -File -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending
    )) {
      $candidates += [pscustomobject]@{ File = $item.FullName; Prefix = @() }
    }
  }

  foreach ($candidate in $candidates) {
    try {
      & $candidate.File @($candidate.Prefix) -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)' *> $null
      if ($LASTEXITCODE -eq 0) {
        return $candidate
      }
    } catch {
      # Try the next candidate.
    }
  }

  return $null
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

function Find-Browser {
  $command = Find-Any @('msedge.exe', 'chrome.exe', 'chromium.exe', 'msedge', 'chrome', 'chromium')
  if ($command) { return $command }
  foreach ($path in @(
    "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe",
    "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
    "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
    "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe",
    "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe"
  )) {
    if ($path -and (Test-Path $path)) { return $path }
  }
  return $null
}

function Find-GitBash {
  $command = Find-Any @('bash.exe')
  if ($command -and $command -notlike '*\Windows\System32\*') { return $command }
  foreach ($path in @(
    "$env:ProgramFiles\Git\bin\bash.exe",
    "${env:ProgramFiles(x86)}\Git\bin\bash.exe",
    "$env:LOCALAPPDATA\Programs\Git\bin\bash.exe",
    "$env:USERPROFILE\scoop\apps\git\current\bin\bash.exe"
  )) {
    if ($path -and (Test-Path $path)) { return $path }
  }
  return $null
}

function Ensure-GitBashSupport {
  if ($env:FILEFLOW_SETUP_INSTALL_SUPPORT_TOOLS -ne '1') { return }
  if (Find-GitBash) {
    Write-Host '[OK]   Git Bash       support terminal disponible'
    return
  }
  foreach ($candidate in @('winget:Git.Git','choco:git','scoop:git')) {
    $worked = Try-Candidate $candidate
    Refresh-Path
    if ($worked -and (Find-GitBash)) {
      $parts = $candidate.Split(':', 2)
      Write-Host ('[OK]   Git Bash       installed via {0}' -f $parts[0])
      Write-OwnedPackage -Component 'support:git-bash' -Manager $parts[0] -Package $parts[1] -Kind 'integration'
      return
    }
    Warn "Git Bash: $candidate unavailable or installation failed; trying next source."
  }
  Write-Warning '[MISS] Git Bash       support terminal indisponible; FileFlow continuera avec PowerShell natif.'
}

function Is-Available([string]$Probe) {
  if ($Probe -eq '@office') { return [bool](Find-LibreOffice) }
  if ($Probe -eq '@browser') { return [bool](Find-Browser) }
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

  $python = Get-RealPythonInvocation
  if (-not $python) {
    [void](Invoke-WingetInstall 'Python.Python.3.12')
    Refresh-Path
    $python = Get-RealPythonInvocation
  }

  if (-not $python) {
    Warn 'Python 3.10+ is unavailable after installation attempt.'
    return $false
  }

  & $python.File @($python.Prefix) -m pip install --user pipx
  if ($LASTEXITCODE -ne 0) { return $false }

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

function Ensure-Engine([string]$Id, [string]$Label, [string]$Probe, [string[]]$Candidates) {
  if (-not (Test-EngineSelected $Id)) {
    Write-Host ('[SKIP] {0,-14} not selected by FileFlow Setup' -f $Label)
    return
  }
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
      $parts = $candidate.Split(':', 2)
      Write-OwnedPackage -Component $Id -Manager $parts[0] -Package $parts[1]
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
Ensure-GitBashSupport

Ensure-Engine 'ffmpeg' 'FFmpeg'       'ffmpeg.exe|ffmpeg'             @('winget:Gyan.FFmpeg','choco:ffmpeg','scoop:ffmpeg')
Ensure-Engine 'vips' 'libvips'      'vips.exe|vips'                 @('winget:libvips.libvips','choco:vips','scoop:vips')
Ensure-Engine 'imagemagick' 'ImageMagick'  'magick.exe|magick'             @('winget:ImageMagick.ImageMagick','choco:imagemagick','scoop:imagemagick')
Ensure-Engine 'qpdf' 'qpdf'         'qpdf.exe|qpdf'                 @('winget:QPDF.QPDF','choco:qpdf','scoop:qpdf')
Ensure-Engine 'img2pdf' 'img2pdf'      'img2pdf.exe|img2pdf'           @('pipx:img2pdf','scoop:img2pdf')
Ensure-Engine 'poppler' 'Poppler'      'pdftoppm.exe|pdftoppm'         @('winget:oschwartz10612.Poppler','choco:poppler','scoop:poppler')
Ensure-Engine 'ghostscript' 'Ghostscript'  'gswin64c.exe|gswin32c.exe|gs'  @('winget:ArtifexSoftware.GhostScript','choco:ghostscript','scoop:ghostscript')
Ensure-Engine 'tesseract' 'Tesseract'    'tesseract.exe|tesseract'       @('winget:tesseract-ocr.tesseract','choco:tesseract','scoop:tesseract')
Ensure-Engine 'ocrmypdf' 'OCRmyPDF'     'ocrmypdf.exe|ocrmypdf'         @('pipx:ocrmypdf')
Ensure-Engine 'libreoffice' 'LibreOffice'  '@office'                       @('winget:TheDocumentFoundation.LibreOffice','choco:libreoffice-fresh','scoop:libreoffice')
Ensure-Engine 'pandoc' 'Pandoc'       'pandoc.exe|pandoc'             @('winget:JohnMacFarlane.Pandoc','choco:pandoc','scoop:pandoc')
Ensure-Engine 'browser' 'Navigateur PDF' '@browser'                    @('winget:Microsoft.Edge','winget:Google.Chrome')
Ensure-Engine 'exiftool' 'ExifTool'     'exiftool.exe|exiftool'         @('winget:OliverBetz.ExifTool','choco:exiftool','scoop:exiftool')
Ensure-Engine 'sevenzip' '7-Zip'        '7zz.exe|7z.exe|7z'             @('winget:7zip.7zip','choco:7zip','scoop:7zip')
Ensure-Engine 'zstd' 'Zstandard'    'zstd.exe|zstd'                 @('winget:Facebook.Zstandard','choco:zstandard','scoop:zstd')
Ensure-Engine 'lz4' 'LZ4'          'lz4.exe|lz4'                   @('winget:LZ4.LZ4','choco:lz4','scoop:lz4')

Write-Host ""
Write-Host "Runtime dependencies: $script:Available available, $script:Missing missing, $script:Warnings fallback warnings."
Write-Host 'Missing engines do not prevent FileFlow from being installed; only their related actions are unavailable.'
exit 0
