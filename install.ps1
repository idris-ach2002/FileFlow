[CmdletBinding()]
param(
  [ValidateSet('user', 'dev')][string]$Mode = 'user',
  [switch]$Force,
  [switch]$NoLaunch,
  [switch]$SkipDependencies,
  [switch]$Doctor
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root

$script:Step = 'initialisation'
$script:Target = 'windows-x64'
$script:DistBranch = 'distribution/windows-x64'
$script:Ref = 'refs/fileflow/install/windows-x64'
$script:Remote = 'origin'
if ($env:FILEFLOW_INSTALL_REMOTE) {
  $script:Remote = $env:FILEFLOW_INSTALL_REMOTE
}

$script:StateDir = Join-Path $env:LOCALAPPDATA 'FileFlow'
$script:LogDir = Join-Path $script:StateDir 'Logs'
$script:Marker = Join-Path $script:StateDir 'install.env'
New-Item -ItemType Directory -Force -Path $script:LogDir | Out-Null
$script:Log = Join-Path $script:LogDir ("install-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))

function Write-Log {
  param([string]$Message)
  try {
    "{0} [{1}] {2}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $script:Step, $Message |
      Out-File $script:Log -Append -Encoding utf8
  } catch {
    # Logging must never mask the installation error.
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
    [string]$Detail = ''
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
    Write-Host "Detail      : $Detail"
    Write-Host "Log         : $script:Log"
  } else {
    Write-Host 'Relance install.ps1 avec -Mode dev pour le diagnostic technique.'
  }
  Write-Log "FAIL $Code : $Detail"
  exit 1
}

function Export-GitBlob {
  param(
    [Parameter(Mandatory = $true)][string]$Spec,
    [Parameter(Mandatory = $true)][string]$Destination,
    [switch]$Append
  )

  $git = (Get-Command git.exe -ErrorAction Stop).Source
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $git
  $psi.Arguments = 'show --no-ext-diff "' + $Spec.Replace('"', '\"') + '"'
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true

  $process = New-Object System.Diagnostics.Process
  $process.StartInfo = $psi
  [void]$process.Start()

  $fileMode = [System.IO.FileMode]::Create
  if ($Append) {
    $fileMode = [System.IO.FileMode]::Append
  }

  $stream = [System.IO.File]::Open(
    $Destination,
    $fileMode,
    [System.IO.FileAccess]::Write,
    [System.IO.FileShare]::None
  )
  try {
    $process.StandardOutput.BaseStream.CopyTo($stream)
  } finally {
    $stream.Dispose()
  }

  $stderr = $process.StandardError.ReadToEnd()
  $process.WaitForExit()
  if ($process.ExitCode -ne 0) {
    throw "git show failed for $Spec : $stderr"
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
  Fail-Install 'FF-I-001' "Cette architecture Windows n'est pas encore publiee par FileFlow." "arch=$arch"
}

$temp = $null
try {
  Write-Host ''
  Write-Host '== 1/2 Runtime FileFlow =='
  Write-Host 'Les moteurs coeur certifies sont inclus dans le paquet FileFlow.'
  if ($SkipDependencies) {
    Write-Host 'Installation des dependances systeme ignoree (-SkipDependencies).'
  } else {
    Write-Host 'Installation/verification des moteurs de conversion Windows...'
    try {
      & "$Root\scripts\runtime\install-dependencies.ps1" -Quiet 2>&1 |
        Tee-Object -FilePath $script:Log -Append |
        Write-Host
    } catch {
      Write-Dev "system dependency helper failed; FileFlow will use available engines: $($_.Exception.Message)"
    }
  }

  $script:Step = 'recuperation du paquet'
  Write-Host ''
  Write-Host '== 2/2 Installation de FileFlow =='

  git update-ref -d $script:Ref 2>$null | Out-Null
  git fetch --quiet --depth=1 $script:Remote "refs/heads/$($script:DistBranch):$($script:Ref)"
  if ($LASTEXITCODE -ne 0) {
    Fail-Install 'FF-I-003' 'Le paquet FileFlow Windows x64 n est pas encore publie.' "branch=$script:DistBranch"
  }

  $temp = Join-Path ([IO.Path]::GetTempPath()) ('fileflow-install-' + [Guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Force -Path $temp | Out-Null

  $manifestPath = Join-Path $temp 'manifest.env'
  Export-GitBlob -Spec "$($script:Ref):manifest.env" -Destination $manifestPath

  $manifest = @{}
  foreach ($line in Get-Content $manifestPath) {
    if ($line -match '^([^=]+)=(.*)$') {
      $manifest[$matches[1]] = $matches[2]
    }
  }

  foreach ($key in @('VERSION', 'SOURCE_SHA', 'PACKAGE_NAME', 'PACKAGE_SHA256', 'PACKAGE_SIZE', 'CHANNEL', 'RUNTIME_MODE')) {
    if (-not $manifest[$key]) {
      Fail-Install 'FF-I-004' 'Le manifeste Windows est incomplet.' "missing=$key"
    }
  }

  if ($manifest['RUNTIME_MODE'] -ne 'system-managed') {
    Fail-Install 'FF-I-013' 'Ce paquet ne contient pas le runtime FileFlow attendu.' "runtime=$($manifest['RUNTIME_MODE'])"
  }

  if ((Test-Path $script:Marker) -and -not $Force) {
    $installed = @{}
    foreach ($line in Get-Content $script:Marker) {
      if ($line -match '^([^=]+)=(.*)$') {
        $installed[$matches[1]] = $matches[2]
      }
    }
    if ($installed['PACKAGE_SHA256'] -eq $manifest['PACKAGE_SHA256']) {
      Write-Host ''
      Write-Host "FileFlow $($manifest['VERSION']) est deja installe."
      Write-Host 'Runtime FileFlow: deja present dans le paquet installe.'
      Write-Host 'Le depot clone peut etre supprime.'
      exit 0
    }
  }

  $package = Join-Path $temp $manifest['PACKAGE_NAME']
  $chunks = @(git ls-tree -r --name-only $script:Ref 'payload/' | Sort-Object)
  if (-not $chunks -or $chunks.Count -eq 0) {
    Fail-Install 'FF-I-004' 'Le paquet Windows ne contient aucun fragment.' 'payload empty'
  }

  foreach ($chunk in $chunks) {
    Write-Dev "assemblage $chunk"
    Export-GitBlob -Spec "$($script:Ref):$chunk" -Destination $package -Append
  }

  $actualSize = (Get-Item $package).Length
  if ([string]$actualSize -ne [string]$manifest['PACKAGE_SIZE']) {
    Fail-Install 'FF-I-004' 'Le paquet Windows est incomplet.' "size=$actualSize expected=$($manifest['PACKAGE_SIZE'])"
  }

  $actualSha = (Get-FileHash $package -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actualSha -ne $manifest['PACKAGE_SHA256'].ToLowerInvariant()) {
    Fail-Install 'FF-I-004' 'Le controle d integrite FileFlow a echoue.' "sha=$actualSha"
  }

  $script:Step = 'signature'
  $signature = Get-AuthenticodeSignature $package
  if ($manifest['CHANNEL'] -eq 'production' -and $signature.Status -ne 'Valid') {
    Fail-Install 'FF-I-006' 'La signature Windows de FileFlow n est pas valide.' "status=$($signature.Status)"
  }
  if ($manifest['CHANNEL'] -ne 'production' -and $signature.Status -ne 'Valid') {
    Write-Dev "candidate: Authenticode=$($signature.Status)"
  }

  $script:Step = 'installation Windows'
  $process = Start-Process $package -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    Fail-Install 'FF-I-008' 'L installateur FileFlow Windows a echoue.' "exit=$($process.ExitCode)"
  }

  New-Item -ItemType Directory -Force -Path $script:StateDir | Out-Null
  @(
    "VERSION=$($manifest['VERSION'])",
    "SOURCE_SHA=$($manifest['SOURCE_SHA'])",
    "TARGET=$script:Target",
    "CHANNEL=$($manifest['CHANNEL'])",
    "PACKAGE_SHA256=$($manifest['PACKAGE_SHA256'])",
    "RUNTIME_MODE=$($manifest['RUNTIME_MODE'])",
    "INSTALLED_AT=$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))"
  ) | Set-Content $script:Marker -Encoding ascii

  Write-Host ''
  Write-Host '============================================================'
  Write-Host "FileFlow $($manifest['VERSION']) est installe"
  Write-Host '============================================================'
  Write-Host 'Runtime : moteurs installes et verifies sur Windows.'
  Write-Host 'Le depot clone peut etre supprime.'

  if (-not $NoLaunch) {
    $candidates = @(
      (Join-Path $env:LOCALAPPDATA 'Programs\FileFlow\FileFlow.exe'),
      (Join-Path $env:LOCALAPPDATA 'FileFlow\FileFlow.exe'),
      (Join-Path $env:ProgramFiles 'FileFlow\FileFlow.exe')
    )
    foreach ($candidate in $candidates) {
      if ($candidate -and (Test-Path $candidate)) {
        Start-Process $candidate
        break
      }
    }
  }
} catch {
  Fail-Install 'FF-I-999' 'Une erreur systeme inattendue est survenue.' $_.Exception.ToString()
} finally {
  if ($temp -and (Test-Path $temp)) {
    Remove-Item $temp -Recurse -Force -ErrorAction SilentlyContinue
  }
}
