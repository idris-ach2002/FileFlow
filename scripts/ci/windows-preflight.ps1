$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root

function Require-File([string]$Path, [int64]$MinBytes = 1) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "Required Windows file is missing: $Path"
  }
  $length = (Get-Item -LiteralPath $Path).Length
  if ($length -lt $MinBytes) {
    throw "Required Windows file is empty/invalid: $Path ($length bytes)"
  }
}

function Require-Command([string]$Name) {
  $cmd = Get-Command $Name -ErrorAction SilentlyContinue
  if (-not $cmd) {
    throw "Required command is unavailable: $Name"
  }
  return $cmd.Source
}

Write-Host '== Windows deterministic preflight =='

$requiredFiles = @(
  'package.json',
  'pnpm-lock.yaml',
  'Cargo.toml',
  'Cargo.lock',
  'rust-toolchain.toml',
  'src-tauri\Cargo.toml',
  'src-tauri\tauri.conf.json',
  'src-tauri\tauri.windows.conf.json',
  'src-tauri\icons\icon.png',
  'src-tauri\icons\icon.ico'
)

foreach ($file in $requiredFiles) {
  Require-File $file
}

# ICO header: reserved=0, type=1.
$ico = [IO.File]::ReadAllBytes((Resolve-Path 'src-tauri\icons\icon.ico'))
if ($ico.Length -lt 6 -or
    $ico[0] -ne 0 -or $ico[1] -ne 0 -or
    $ico[2] -ne 1 -or $ico[3] -ne 0) {
  throw 'src-tauri/icons/icon.ico does not have a valid ICO header'
}

$base = Get-Content -LiteralPath 'src-tauri\tauri.conf.json' -Raw | ConvertFrom-Json
$windows = Get-Content -LiteralPath 'src-tauri\tauri.windows.conf.json' -Raw | ConvertFrom-Json

if ($base.bundle.icon -notcontains 'icons/icon.ico') {
  throw 'tauri.conf.json must explicitly include icons/icon.ico'
}

if (-not $windows.bundle.windows) {
  throw 'tauri.windows.conf.json is missing bundle.windows'
}

# Tauri v2 uses bundle.windows.staticVCRuntime. The legacy bundleVCRuntime key is invalid.
if ($null -ne $windows.bundle.windows.PSObject.Properties['bundleVCRuntime']) {
  throw 'Unsupported Tauri field detected: bundleVCRuntime'
}
if ($windows.bundle.windows.staticVCRuntime -ne $true) {
  throw 'tauri.windows.conf.json must set bundle.windows.staticVCRuntime=true'
}

$targets = @($windows.bundle.targets)
foreach ($requiredTarget in @('nsis', 'msi')) {
  if ($targets -notcontains $requiredTarget) {
    throw "Windows bundle target is missing: $requiredTarget"
  }
}

# Prevent regression to direct spawning of pnpm's Windows command wrapper.
# Build the forbidden token dynamically so this validator cannot match itself.
$forbiddenPnpmWrapper = 'pnpm' + '.cmd'

$badPnpmWrapper = Get-ChildItem -Path scripts -Recurse -File |
  Select-String -SimpleMatch $forbiddenPnpmWrapper -ErrorAction SilentlyContinue

if ($badPnpmWrapper) {
  $badPnpmWrapper | ForEach-Object {
    Write-Error "$($_.Path):$($_.LineNumber): $($_.Line)"
  }

  throw 'Forbidden direct pnpm Windows wrapper reference detected below scripts/'
}

$nodePath = Require-Command 'node'
$pnpmPath = Require-Command 'pnpm'
$rustcPath = Require-Command 'rustc'
$cargoPath = Require-Command 'cargo'
$pythonPath = Require-Command 'python'

Write-Host "node:   $nodePath"
Write-Host "pnpm:   $pnpmPath"
Write-Host "rustc:  $rustcPath"
Write-Host "cargo:  $cargoPath"
Write-Host "python: $pythonPath"

node --version
pnpm --version
rustc --version
cargo --version
python --version

$hostTriple = (rustc -vV | Select-String '^host: ').Line -replace '^host:\s*', ''
if ($hostTriple -ne 'x86_64-pc-windows-msvc') {
  throw "Unexpected Rust host: $hostTriple"
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path -LiteralPath $vswhere) {
  $vsInstall = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath
  if (-not $vsInstall) {
    throw 'MSVC x64 build tools were not found by vswhere'
  }
  Write-Host "MSVC tools: $vsInstall"
} else {
  Write-Warning 'vswhere.exe not found; Rust MSVC discovery will be authoritative'
}

# Cargo.lock consistency and workspace parsing before expensive compilation.
cargo metadata --locked --no-deps --format-version 1 | Out-Null

# Tauri CLI must at least be resolvable on the Windows runner.
pnpm exec tauri --version

Write-Host 'Windows deterministic preflight passed.'
