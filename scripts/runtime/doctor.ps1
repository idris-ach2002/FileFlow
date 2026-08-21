[CmdletBinding()]
param([switch]$Strict)
$found=0; $missing=0
function Refresh-Path {
  $machine=[Environment]::GetEnvironmentVariable('Path','Machine'); $user=[Environment]::GetEnvironmentVariable('Path','User')
  $extra=@("$env:LOCALAPPDATA\Microsoft\WinGet\Links","$env:LOCALAPPDATA\Microsoft\WindowsApps","$env:USERPROFILE\scoop\shims","$env:ChocolateyInstall\bin","$env:APPDATA\Python\Scripts","$env:LOCALAPPDATA\Programs\Python\Python312\Scripts") | Where-Object { $_ -and (Test-Path $_) }
  $env:Path=(@($machine,$user)+$extra | Where-Object { $_ }) -join ';'
}
function Check([string]$label,[string[]]$names){ Refresh-Path; foreach($name in $names){$cmd=Get-Command $name -ErrorAction SilentlyContinue;if($cmd){Write-Host ('[OK]   {0,-14} {1}' -f $label,$cmd.Source);$script:found++;return}};Write-Host ('[MISS] {0,-14} not installed' -f $label);$script:missing++ }
function Check-Office { foreach($path in @("$env:ProgramFiles\LibreOffice\program\soffice.exe","${env:ProgramFiles(x86)}\LibreOffice\program\soffice.exe","$env:LOCALAPPDATA\Programs\LibreOffice\program\soffice.exe")){if($path -and (Test-Path $path)){Write-Host ('[OK]   {0,-14} {1}' -f 'LibreOffice',$path);$script:found++;return}}; Check 'LibreOffice' @('soffice.exe','libreoffice.exe') }
Write-Host 'FileFlow runtime doctor - Windows'; Write-Host ''
Check 'FFmpeg' @('ffmpeg.exe'); Check 'libvips' @('vips.exe'); Check 'ImageMagick' @('magick.exe'); Check 'qpdf' @('qpdf.exe'); Check 'img2pdf' @('img2pdf.exe'); Check 'Poppler' @('pdftoppm.exe'); Check 'Ghostscript' @('gswin64c.exe','gswin32c.exe','gs.exe'); Check 'Tesseract' @('tesseract.exe'); Check 'OCRmyPDF' @('ocrmypdf.exe'); Check-Office; Check 'Pandoc' @('pandoc.exe'); Check 'ExifTool' @('exiftool.exe'); Check '7-Zip' @('7zz.exe','7z.exe'); Check 'Zstandard' @('zstd.exe'); Check 'LZ4' @('lz4.exe')
Write-Host ""; Write-Host "Result: $found available, $missing missing."
if($Strict -and $missing -gt 0){exit 1}; exit 0
