[CmdletBinding()]
param([switch]$Strict)

$script:Found = 0
$script:Missing = 0
$script:Broken = 0

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

function Get-ProbeArgs {
  param([string]$Name)
  switch -Regex ($Name.ToLowerInvariant()) {
    '^ffmpeg(\.exe)?$' { return @('-version') }
    '^(magick|convert)(\.exe)?$' { return @('-version') }
    '^(pdftoppm|pdftotext)(\.exe)?$' { return @('-v') }
    '^exiftool(\.exe)?$' { return @('-ver') }
    '^(7zz|7z)(\.exe)?$' { return @('i') }
    default { return @('--version') }
  }
}

function Test-EngineRuntime {
  param([string]$Path)
  try {
    $name = [IO.Path]::GetFileName($Path)
    $args = @(Get-ProbeArgs $name)
    & $Path @args *> $null
    return ($LASTEXITCODE -eq 0)
  } catch {
    return $false
  }
}

function Find-Any {
  param([string[]]$Names)
  Refresh-Path
  foreach ($name in $Names) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
  }
  return $null
}

function Check {
  param([string]$Label, [string[]]$Names)
  $path = Find-Any $Names
  if (-not $path) {
    Write-Host ('[MISS]   {0,-14} not installed' -f $Label)
    $script:Missing++
    return
  }
  if (Test-EngineRuntime $path) {
    Write-Host ('[OK]     {0,-14} {1}' -f $Label, $path)
    $script:Found++
    return
  }
  Write-Host ('[BROKEN] {0,-14} {1} (runtime probe failed)' -f $Label, $path)
  $script:Broken++
}

function Check-Office {
  $paths = @(
    "$env:ProgramFiles\LibreOffice\program\soffice.exe",
    "${env:ProgramFiles(x86)}\LibreOffice\program\soffice.exe",
    "$env:LOCALAPPDATA\Programs\LibreOffice\program\soffice.exe"
  )
  foreach ($path in $paths) {
    if ($path -and (Test-Path $path)) {
      if (Test-EngineRuntime $path) {
        Write-Host ('[OK]     {0,-14} {1}' -f 'LibreOffice', $path)
        $script:Found++
      } else {
        Write-Host ('[BROKEN] {0,-14} {1} (runtime probe failed)' -f 'LibreOffice', $path)
        $script:Broken++
      }
      return
    }
  }
  Check 'LibreOffice' @('soffice.exe', 'libreoffice.exe')
}

Write-Host 'FileFlow runtime doctor - Windows'
Write-Host ''
Check 'FFmpeg' @('ffmpeg.exe')
Check 'libvips' @('vips.exe')
Check 'ImageMagick' @('magick.exe')
Check 'qpdf' @('qpdf.exe')
Check 'img2pdf' @('img2pdf.exe')
Check 'Poppler' @('pdftoppm.exe')
Check 'Ghostscript' @('gswin64c.exe', 'gswin32c.exe', 'gs.exe')
Check 'Tesseract' @('tesseract.exe')
Check 'OCRmyPDF' @('ocrmypdf.exe')
Check-Office
Check 'Pandoc' @('pandoc.exe')
Check 'ExifTool' @('exiftool.exe')
Check '7-Zip' @('7zz.exe', '7z.exe')
Check 'Zstandard' @('zstd.exe')
Check 'LZ4' @('lz4.exe')
Write-Host ''
Write-Host "Result: $script:Found available, $script:Missing missing, $script:Broken broken."
if ($Strict -and (($script:Missing + $script:Broken) -gt 0)) { exit 1 }
exit 0
