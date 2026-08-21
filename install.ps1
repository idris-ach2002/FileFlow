[CmdletBinding()]
param(
  [ValidateSet('user','dev')][string]$Mode='user',
  [switch]$Force,
  [switch]$NoLaunch,
  [switch]$SkipDependencies,
  [switch]$Doctor
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root

$Step = 'initialisation'
$Target = 'windows-x64'
$DistBranch = 'distribution/windows-x64'
$Ref = 'refs/fileflow/install/windows-x64'
$Remote = if ($env:FILEFLOW_INSTALL_REMOTE) { $env:FILEFLOW_INSTALL_REMOTE } else { 'origin' }

$StateDir = Join-Path $env:LOCALAPPDATA 'FileFlow'
$LogDir = Join-Path $StateDir 'Logs'
$Marker = Join-Path $StateDir 'install.env'
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$Log = Join-Path $LogDir ("install-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))

function Write-Log {
  param([string]$Message)

  try {
    "{0} [{1}] {2}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $script:Step, $Message |
      Out-File $script:Log -Append -Encoding utf8
  } catch {
    # Logging must never hide the original installer failure.
  }
}

function Write-Dev {
  param([string]$Message)

  Write-Log "[DEV] $Message"
  if ($Mode -eq 'dev') {
    Write-Host "[DEV] $Message"
  }
}

function Fail-Install {
  param(
    [string]$Code,
    [string]$UserMessage,
    [string]$DeveloperMessage = ''
  )

  Write-Host ''
  Write-Host "FileFlow n'a pas pu terminer l'installation."
  Write-Host "Code : $Code"
  Write-Host $UserMessage

  if ($Mode -eq 'dev') {
    Write-Host ''
    Write-Host '--- Diagnostic developpeur ---'
    Write-Host "Etape       : $script:Step"
    Write-Host "Cible       : $script:Target"
    Write-Host "Distribution: $script:DistBranch"
    Write-Host "Detail      : $DeveloperMessage"
    Write-Host "Log         : $script:Log"
  } else {
    Write-Host 'Relance install.ps1 avec -Mode dev pour le diagnostic technique.'
  }

  Write-Log "FAIL $Code : $DeveloperMessage"
  exit 1
}

function Invoke-BestEffortScript {
  param(
    [string]$Path,
    [string]$Label
  )

  try {
    & $Path 2>&1 | Tee-Object -FilePath $Log -Append | Write-Host
    if ($LASTEXITCODE -ne 0) {
      Write-Dev "$Label returned exit code $LASTEXITCODE; installation continues."
    }
  } catch {
    Write-Dev "$Label failed but installation continues: $($_.Exception.Message)"
  }
}

if ($Doctor) {
  & "$Root\scripts\runtime\doctor.ps1"
  exit $LASTEXITCODE
}

if (-not (Get-Command git.exe -ErrorAction SilentlyContinue)) {
  Fail-Install 'FF-I-010' 'Git est necessaire pour recuperer le paquet FileFlow.' 'git.exe absent'
}

git rev-parse --is-inside-work-tree 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) {
  Fail-Install 'FF-I-010' 'Execute install.ps1 depuis le depot FileFlow clone.' 'not a git worktree'
}

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($arch -ne 'X64') {
  Fail-Install 'FF-I-001' 'Cette architecture Windows n est pas encore publiee par FileFlow.' "arch=$arch"
}

$temp = $null

try {
  if (-not $SkipDependencies) {
    $script:Step = 'installation des moteurs locaux'
    Write-Host ''
    Write-Host '== 1/3 Moteurs de conversion locaux =='
    Invoke-BestEffortScript "$Root\scripts\runtime\install-dependencies.ps1" 'dependency helper'
  } else {
    Write-Host ''
    Write-Host '== 1/3 Moteurs de conversion locaux =='
    Write-Host 'Ignore (-SkipDependencies).'
  }

  $script:Step = 'diagnostic des moteurs'
  Write-Host ''
  Write-Host '== 2/3 Verification du runtime =='
  Invoke-BestEffortScript "$Root\scripts\runtime\doctor.ps1" 'runtime doctor'

  $script:Step = 'recuperation du paquet'
  Write-Host ''
  Write-Host '== 3/3 Installation de FileFlow =='

  git update-ref -d $Ref 2>$null | Out-Null
  git fetch --quiet --depth=1 $Remote "refs/heads/${DistBranch}:${Ref}"
  if ($LASTEXITCODE -ne 0) {
    Fail-Install 'FF-I-003' 'Le paquet FileFlow Windows x64 n est pas encore publie.' "branch=$DistBranch"
  }

  $temp = Join-Path ([IO.Path]::GetTempPath()) ('fileflow-install-' + [Guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Force -Path $temp | Out-Null

  $manifestPath = Join-Path $temp 'manifest.env'
  $spec = "${Ref}:manifest.env"
  $manifestCommand = "git show `"$spec`" > `"$manifestPath`""
  & cmd.exe /d /s /c $manifestCommand
  if ($LASTEXITCODE -ne 0) {
    Fail-Install 'FF-I-004' 'Le manifeste Windows est absent.' 'manifest extraction failed'
  }

  $manifest = @{}
  foreach ($line in Get-Content -LiteralPath $manifestPath) {
    if ($line -match '^([^=]+)=(.*)$') {
      $manifest[$matches[1]] = $matches[2]
    }
  }

  foreach ($key in @('VERSION','SOURCE_SHA','PACKAGE_NAME','PACKAGE_SHA256','PACKAGE_SIZE','CHANNEL','RUNTIME_MODE')) {
    if (-not $manifest[$key]) {
      Fail-Install 'FF-I-004' 'Le manifeste Windows est incomplet.' "missing=$key"
    }
  }

  if ($manifest['RUNTIME_MODE'] -ne 'system') {
    Fail-Install 'FF-I-013' 'Ce paquet utilise encore un ancien runtime moteur embarque.' "runtime=$($manifest['RUNTIME_MODE'])"
  }

  if ((Test-Path -LiteralPath $Marker) -and -not $Force) {
    $installed = @{}
    foreach ($line in Get-Content -LiteralPath $Marker) {
      if ($line -match '^([^=]+)=(.*)$') {
        $installed[$matches[1]] = $matches[2]
      }
    }

    if ($installed['PACKAGE_SHA256'] -eq $manifest['PACKAGE_SHA256']) {
      Write-Host ''
      Write-Host "FileFlow $($manifest['VERSION']) est deja installe."
      Write-Host 'Les moteurs locaux ont ete verifies ou mis a jour.'
      Write-Host 'Le depot clone peut etre supprime.'
      exit 0
    }
  }

  $package = Join-Path $temp $manifest['PACKAGE_NAME']
  $chunks = @(git ls-tree -r --name-only $Ref 'payload/' | Sort-Object)
  if (-not $chunks) {
    Fail-Install 'FF-I-004' 'Le paquet Windows ne contient aucun fragment.' 'payload empty'
  }

  foreach ($chunk in $chunks) {
    Write-Dev "assemblage $chunk"
    $chunkSpec = "${Ref}:$chunk"
    $chunkCommand = "git show `"$chunkSpec`" >> `"$package`""
    & cmd.exe /d /s /c $chunkCommand
    if ($LASTEXITCODE -ne 0) {
      Fail-Install 'FF-I-004' 'Le paquet Windows est incomplet.' "chunk=$chunk"
    }
  }

  $actualSize = (Get-Item -LiteralPath $package).Length
  if ([string]$actualSize -ne [string]$manifest['PACKAGE_SIZE']) {
    Fail-Install 'FF-I-004' 'Le paquet Windows est incomplet.' "size=$actualSize"
  }

  $actualSha = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actualSha -ne $manifest['PACKAGE_SHA256'].ToLowerInvariant()) {
    Fail-Install 'FF-I-004' 'Le controle d integrite FileFlow a echoue.' "sha=$actualSha"
  }

  $script:Step = 'signature'
  $signature = Get-AuthenticodeSignature -FilePath $package
  if ($manifest['CHANNEL'] -eq 'production' -and $signature.Status -ne 'Valid') {
    Fail-Install 'FF-I-006' 'La signature Windows de FileFlow n est pas valide.' "status=$($signature.Status)"
  }
  if ($manifest['CHANNEL'] -ne 'production' -and $signature.Status -ne 'Valid') {
    Write-Dev "candidate: Authenticode=$($signature.Status)"
  }

  $script:Step = 'installation Windows'
  $process = Start-Process -FilePath $package -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    Fail-Install 'FF-I-008' 'L installateur FileFlow Windows a echoue.' "exit=$($process.ExitCode)"
  }

  New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
  @(
    "VERSION=$($manifest['VERSION'])",
    "SOURCE_SHA=$($manifest['SOURCE_SHA'])",
    "TARGET=$Target",
    "CHANNEL=$($manifest['CHANNEL'])",
    "PACKAGE_SHA256=$($manifest['PACKAGE_SHA256'])",
    'RUNTIME_MODE=system',
    "INSTALLED_AT=$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))"
  ) | Set-Content -LiteralPath $Marker -Encoding ascii

  Write-Host ''
  Write-Host '============================================================'
  Write-Host "FileFlow $($manifest['VERSION']) est installe definitivement"
  Write-Host '============================================================'
  Write-Host 'FileFlow est disponible depuis le menu Demarrer.'
  Write-Host 'Les moteurs sont installes localement et survivent a la suppression du clone.'
  Write-Host 'Le depot clone peut etre supprime.'

  if (-not $NoLaunch) {
    $candidate = Join-Path $env:LOCALAPPDATA 'FileFlow\FileFlow.exe'
    if (Test-Path -LiteralPath $candidate) {
      Start-Process -FilePath $candidate
    }
  }
} catch {
  Fail-Install 'FF-I-999' 'Une erreur systeme inattendue est survenue.' $_.Exception.ToString()
} finally {
  if ($temp -and (Test-Path -LiteralPath $temp)) {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
  }
}
