[CmdletBinding()]
param(
  [ValidateSet('user','dev')][string]$Mode='user',
  [switch]$Force,
  [switch]$NoLaunch,
  [switch]$SkipDependencies,
  [switch]$Doctor
)
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$Root=Split-Path -Parent $MyInvocation.MyCommand.Path; Set-Location $Root
$Step='initialisation'; $Target='windows-x64'; $DistBranch='distribution/windows-x64'; $Ref='refs/fileflow/install/windows-x64'; $Remote=if($env:FILEFLOW_INSTALL_REMOTE){$env:FILEFLOW_INSTALL_REMOTE}else{'origin'}
$StateDir=Join-Path $env:LOCALAPPDATA 'FileFlow'; $LogDir=Join-Path $StateDir 'Logs'; $Marker=Join-Path $StateDir 'install.env'; New-Item -ItemType Directory -Force -Path $LogDir|Out-Null; $Log=Join-Path $LogDir ("install-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
function Write-Log([string]$m){try{"{0} [{1}] {2}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'),$script:Step,$m|Out-File $script:Log -Append -Encoding utf8}catch{}}
function Write-Dev([string]$m){Write-Log "[DEV] $m";if($Mode -eq 'dev'){Write-Host "[DEV] $m"}}
function Fail-Install([string]$c,[string]$u,[string]$d=''){Write-Host '';Write-Host "FileFlow n’a pas pu terminer l’installation.";Write-Host "Code : $c";Write-Host $u;if($Mode -eq 'dev'){Write-Host '';Write-Host '--- Diagnostic développeur ---';Write-Host "Étape       : $script:Step";Write-Host "Cible       : $script:Target";Write-Host "Distribution: $script:DistBranch";Write-Host "Détail      : $d";Write-Host "Log         : $script:Log"}else{Write-Host 'Relance install.ps1 avec -Mode dev pour le diagnostic technique.'};Write-Log "FAIL $c : $d";exit 1}

if($Doctor){& "$Root\scripts\runtime\doctor.ps1";exit $LASTEXITCODE}
if(-not(Get-Command git.exe -ErrorAction SilentlyContinue)){Fail-Install 'FF-I-010' 'Git est nécessaire pour récupérer le paquet FileFlow depuis le dépôt cloné.' 'git.exe absent'}
git rev-parse --is-inside-work-tree 2>$null|Out-Null;if($LASTEXITCODE -ne 0){Fail-Install 'FF-I-010' 'Exécute install.ps1 depuis le dépôt FileFlow cloné.' 'not a git worktree'}
$arch=[System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString();if($arch -ne 'X64'){Fail-Install 'FF-I-001' 'Cette architecture Windows n’est pas encore publiée par FileFlow.' "arch=$arch"}

try {
  if(-not $SkipDependencies){
    $script:Step='installation des moteurs locaux'; Write-Host ''; Write-Host '== 1/3 Moteurs de conversion locaux =='
    try { & "$Root\scripts\runtime\install-dependencies.ps1" 2>&1 | Tee-Object -FilePath $Log -Append | Write-Host } catch { Write-Dev "dependency helper failed but installation continues: $($_.Exception.Message)" }
  } else { Write-Host ''; Write-Host '== 1/3 Moteurs de conversion locaux =='; Write-Host 'Ignoré (-SkipDependencies).' }
  $script:Step='diagnostic des moteurs'; Write-Host ''; Write-Host '== 2/3 Vérification du runtime =='
  try { & "$Root\scripts\runtime\doctor.ps1" 2>&1 | Tee-Object -FilePath $Log -Append | Write-Host } catch { Write-Dev "doctor warning: $($_.Exception.Message)" }

  $script:Step='récupération du paquet'; Write-Host ''; Write-Host '== 3/3 Installation de FileFlow =='
  git update-ref -d $Ref 2>$null|Out-Null; git fetch --quiet --depth=1 $Remote "refs/heads/${DistBranch}:${Ref}";if($LASTEXITCODE -ne 0){Fail-Install 'FF-I-003' 'Le paquet FileFlow Windows x64 n’est pas encore publié.' "branch=$DistBranch"}
  $temp=Join-Path ([IO.Path]::GetTempPath()) ('fileflow-install-'+[Guid]::NewGuid().ToString('N'));New-Item -ItemType Directory -Force -Path $temp|Out-Null
  try {
    $manifestPath=Join-Path $temp 'manifest.env';$spec="${Ref}:manifest.env";cmd.exe /d /s /c "git show `"$spec`" > `"$manifestPath`"";if($LASTEXITCODE -ne 0){Fail-Install 'FF-I-004' 'Le manifeste Windows est absent.' 'manifest extraction failed'}
    $manifest=@{};foreach($line in Get-Content $manifestPath){if($line -match '^([^=]+)=(.*)$'){$manifest[$matches[1]]=$matches[2]}};foreach($k in @('VERSION','SOURCE_SHA','PACKAGE_NAME','PACKAGE_SHA256','PACKAGE_SIZE','CHANNEL','RUNTIME_MODE')){if(-not $manifest[$k]){Fail-Install 'FF-I-004' 'Le manifeste Windows est incomplet.' "missing=$k"}}
    if($manifest['RUNTIME_MODE'] -ne 'system'){Fail-Install 'FF-I-013' 'Ce paquet utilise encore l’ancien runtime moteur embarqué.' "runtime=$($manifest['RUNTIME_MODE'])"}
    if((Test-Path $Marker)-and -not $Force){$installed=@{};foreach($line in Get-Content $Marker){if($line -match '^([^=]+)=(.*)$'){$installed[$matches[1]]=$matches[2]}};if($installed['PACKAGE_SHA256'] -eq $manifest['PACKAGE_SHA256']){Write-Host '';Write-Host "✓ FileFlow $($manifest['VERSION']) est déjà installé.";Write-Host 'Les moteurs locaux ont été vérifiés/mis à jour.';Write-Host 'Le dépôt cloné peut être supprimé.';exit 0}}
    $package=Join-Path $temp $manifest['PACKAGE_NAME'];$chunks=@(git ls-tree -r --name-only $Ref 'payload/'|Sort-Object);if(-not $chunks){Fail-Install 'FF-I-004' 'Le paquet Windows ne contient aucun fragment.' 'payload empty'}
    foreach($chunk in $chunks){Write-Dev "assemblage $chunk";$cspec="${Ref}:$chunk";cmd.exe /d /s /c "git show `"$cspec`" >> `"$package`"";if($LASTEXITCODE -ne 0){Fail-Install 'FF-I-004' 'Le paquet Windows est incomplet.' "chunk=$chunk"}}
    $actualSize=(Get-Item $package).Length;if([string]$actualSize -ne [string]$manifest['PACKAGE_SIZE']){Fail-Install 'FF-I-004' 'Le paquet Windows est incomplet.' "size=$actualSize"};$actualSha=(Get-FileHash $package -Algorithm SHA256).Hash.ToLowerInvariant();if($actualSha -ne $manifest['PACKAGE_SHA256'].ToLowerInvariant()){Fail-Install 'FF-I-004' 'Le contrôle d’intégrité FileFlow a échoué.' "sha=$actualSha"}
    $script:Step='signature';$sig=Get-AuthenticodeSignature $package;if($manifest['CHANNEL'] -eq 'production' -and $sig.Status -ne 'Valid'){Fail-Install 'FF-I-006' 'La signature Windows de FileFlow n’est pas valide.' "status=$($sig.Status)"};if($manifest['CHANNEL'] -ne 'production' -and $sig.Status -ne 'Valid'){Write-Dev "candidate: Authenticode=$($sig.Status)"}
    $script:Step='installation Windows';$proc=Start-Process $package -ArgumentList '/S' -Wait -PassThru;if($proc.ExitCode -ne 0){Fail-Install 'FF-I-008' 'L’installateur FileFlow Windows a échoué.' "exit=$($proc.ExitCode)"}
    New-Item -ItemType Directory -Force -Path $StateDir|Out-Null;@("VERSION=$($manifest['VERSION'])","SOURCE_SHA=$($manifest['SOURCE_SHA'])","TARGET=$Target","CHANNEL=$($manifest['CHANNEL'])","PACKAGE_SHA256=$($manifest['PACKAGE_SHA256'])","RUNTIME_MODE=system","INSTALLED_AT=$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))")|Set-Content $Marker -Encoding ascii
    Write-Host '';Write-Host '============================================================';Write-Host "✓ FileFlow $($manifest['VERSION']) est installé définitivement";Write-Host '============================================================';Write-Host '✓ FileFlow est disponible depuis le menu Démarrer.';Write-Host '✓ Les moteurs sont installés localement et survivront à la suppression du clone.';Write-Host 'Le dépôt cloné peut être supprimé.'
    if(-not $NoLaunch){$candidate=Join-Path $env:LOCALAPPDATA 'FileFlow\FileFlow.exe';if(Test-Path $candidate){Start-Process $candidate}}
  } finally { Remove-Item $temp -Recurse -Force -ErrorAction SilentlyContinue }
} catch { Fail-Install 'FF-I-999' 'Une erreur système inattendue est survenue.' $_.Exception.ToString() }
