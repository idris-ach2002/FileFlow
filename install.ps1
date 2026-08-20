[CmdletBinding()]
param(
    [ValidateSet('user', 'dev')]
    [string]$Mode = $(if ($env:FILEFLOW_INSTALL_MODE) { $env:FILEFLOW_INSTALL_MODE } else { 'user' }),

    [string]$Version = $env:FILEFLOW_VERSION,

    [switch]$NoLaunch,

    [switch]$AllowUnsigned
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = 'idris-ach2002/FileFlow'
$Step = 'initialisation'
$Asset = ''
$Tag = ''
$DownloadUrl = ''

$LogDir = Join-Path $env:LOCALAPPDATA 'FileFlow\Logs'
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$LogFile = Join-Path $LogDir ("install-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))

function Write-Log {
    param([string]$Message)

    try {
        "{0} [{1}] {2}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $script:Step, $Message |
            Out-File -FilePath $script:LogFile -Append -Encoding utf8
    } catch {
        # Logging must never hide the original installer error.
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
    Write-Host "FileFlow n’a pas pu terminer l’installation."
    Write-Host "Code : $Code"
    Write-Host $UserMessage

    if ($Mode -eq 'dev') {
        Write-Host ''
        Write-Host '--- Diagnostic développeur ---'
        Write-Host "Étape       : $script:Step"
        Write-Host "Windows     : $([Environment]::OSVersion.VersionString)"
        Write-Host "Architecture: $env:PROCESSOR_ARCHITECTURE"
        Write-Host "Version     : $(if ($Version) { $Version } else { 'inconnue' })"
        Write-Host "Tag         : $(if ($script:Tag) { $script:Tag } else { 'non résolu' })"
        Write-Host "Asset       : $(if ($script:Asset) { $script:Asset } else { 'non résolu' })"
        Write-Host "URL         : $(if ($script:DownloadUrl) { $script:DownloadUrl } else { 'non résolue' })"
        Write-Host "Détail      : $DeveloperMessage"
        Write-Host "Log         : $script:LogFile"
    } else {
        Write-Host 'Relance install.ps1 avec -Mode dev si un diagnostic technique est nécessaire.'
    }

    Write-Log "FAIL $Code : $DeveloperMessage"
    exit 1
}

if ($AllowUnsigned -and $Mode -ne 'dev') {
    Fail-Install `
        -Code 'FF-I-006' `
        -UserMessage 'L’option AllowUnsigned est réservée au mode développeur.' `
        -DeveloperMessage 'AllowUnsigned requested outside dev mode'
}

function Download-File {
    param(
        [Parameter(Mandatory)]
        [string]$Url,

        [Parameter(Mandatory)]
        [string]$Destination
    )

    Write-Dev "GET $Url -> $Destination"

    try {
        Invoke-WebRequest `
            -Uri $Url `
            -OutFile $Destination `
            -MaximumRedirection 10 `
            -UseBasicParsing
    } catch {
        throw "DOWNLOAD_FAILED::$($_.Exception.Message)"
    }
}

function Resolve-Version {
    $script:Step = 'résolution de la version'

    if ($Version) {
        return
    }

    $localConfig = Join-Path $PSScriptRoot 'src-tauri\tauri.conf.json'

    if (Test-Path $localConfig) {
        try {
            $config = Get-Content $localConfig -Raw | ConvertFrom-Json
            if ($config.version) {
                $script:Version = [string]$config.version
                Write-Dev "version from local tauri.conf.json: $Version"
                return
            }
        } catch {
            Write-Dev "local tauri.conf.json could not be parsed: $($_.Exception.Message)"
        }
    }

    $remote = "https://raw.githubusercontent.com/$Repo/main/src-tauri/tauri.conf.json"

    try {
        $response = Invoke-RestMethod -Uri $remote -MaximumRedirection 10
        if ($response.version) {
            $script:Version = [string]$response.version
            return
        }
    } catch {
        Fail-Install `
            -Code 'FF-I-002' `
            -UserMessage 'Impossible de contacter le serveur FileFlow. Vérifie la connexion Internet puis réessaie.' `
            -DeveloperMessage "version request failed: $($_.Exception.Message)"
    }

    Fail-Install `
        -Code 'FF-I-011' `
        -UserMessage 'La version FileFlow disponible n’a pas pu être déterminée.' `
        -DeveloperMessage 'version missing in local and remote config'
}

function Verify-Version {
    if ($Version -notmatch '^\d+\.\d+\.\d+$') {
        Fail-Install `
            -Code 'FF-I-011' `
            -UserMessage 'La version FileFlow demandée est invalide.' `
            -DeveloperMessage "invalid version: $Version"
    }
}

function Verify-Checksum {
    param(
        [string]$File,
        [string]$ChecksumFile
    )

    $script:Step = 'vérification d’intégrité'

    $name = Split-Path $File -Leaf
    $line = Get-Content $ChecksumFile |
        Where-Object {
            $parts = $_ -split '\s+', 2
            if ($parts.Count -lt 2) { return $false }

            $path = $parts[1].TrimStart('*')
            (Split-Path $path -Leaf) -eq $name
        } |
        Select-Object -First 1

    if (-not $line) {
        Fail-Install `
            -Code 'FF-I-004' `
            -UserMessage 'Le fichier de contrôle ne contient pas le paquet Windows attendu. Installation arrêtée par sécurité.' `
            -DeveloperMessage "checksum entry not found for $name"
    }

    $expected = ($line -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -Path $File -Algorithm SHA256).Hash.ToLowerInvariant()

    if ($actual -ne $expected) {
        Fail-Install `
            -Code 'FF-I-004' `
            -UserMessage 'Le paquet téléchargé ne correspond pas au SHA-256 publié. Il ne sera pas installé.' `
            -DeveloperMessage "checksum mismatch expected=$expected actual=$actual"
    }

    Write-Dev "SHA256 verified: $actual"
}

function Find-FileFlowExecutable {
    $registryRoots = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    )

    foreach ($root in $registryRoots) {
        if (-not (Test-Path $root)) {
            continue
        }

        foreach ($key in Get-ChildItem $root -ErrorAction SilentlyContinue) {
            $item = Get-ItemProperty $key.PSPath -ErrorAction SilentlyContinue

            if ($item.DisplayName -notlike '*FileFlow*') {
                continue
            }

            if ($item.InstallLocation) {
                $candidate = Join-Path $item.InstallLocation 'FileFlow.exe'
                if (Test-Path $candidate) {
                    return $candidate
                }

                $candidate = Join-Path $item.InstallLocation 'fileflow-desktop.exe'
                if (Test-Path $candidate) {
                    return $candidate
                }
            }

            if ($item.DisplayIcon) {
                $iconPath = ([string]$item.DisplayIcon).Trim('"') -replace ',\d+$', ''
                if (Test-Path $iconPath) {
                    return $iconPath
                }
            }
        }
    }

    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'FileFlow\FileFlow.exe'),
        (Join-Path $env:LOCALAPPDATA 'FileFlow\fileflow-desktop.exe'),
        (Join-Path $env:ProgramFiles 'FileFlow\FileFlow.exe'),
        (Join-Path $env:ProgramFiles 'FileFlow\fileflow-desktop.exe')
    )

    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    return $null
}

try {
    $script:Step = 'préparation'
    Write-Dev "log=$LogFile"

    if (-not [Environment]::Is64BitOperatingSystem) {
        Fail-Install `
            -Code 'FF-I-001' `
            -UserMessage 'FileFlow pour Windows nécessite Windows 64 bits.' `
            -DeveloperMessage '32-bit Windows detected'
    }

    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()

    if ($arch -notin @('X64', 'Arm64')) {
        Fail-Install `
            -Code 'FF-I-001' `
            -UserMessage 'Cette architecture Windows n’est pas prise en charge.' `
            -DeveloperMessage "unsupported Windows architecture: $arch"
    }

    # Current FileFlow Windows distribution is x64 MSVC.
    # Windows ARM64 can execute x64 applications under emulation on supported systems.
    if ($arch -eq 'Arm64') {
        Write-Dev 'Windows ARM64 detected; x64 installer will be used through Windows emulation.'
    }

    Resolve-Version
    Verify-Version

    $script:Tag = "windows-v$Version"
    $script:Asset = 'FileFlow-Windows-x64-Setup.exe'
    $checksumName = 'SHA256SUMS-windows'

    Write-Host ''
    Write-Host "FileFlow $Version"
    Write-Host "Plateforme détectée : Windows $arch"

    $tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("fileflow-install-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null

    try {
        $installer = Join-Path $tempRoot $Asset
        $checksumFile = Join-Path $tempRoot $checksumName

        $script:Step = 'téléchargement Windows'
        $script:DownloadUrl = "https://github.com/$Repo/releases/download/$Tag/$Asset"

        try {
            Download-File -Url $DownloadUrl -Destination $installer
        } catch {
            Fail-Install `
                -Code 'FF-I-003' `
                -UserMessage 'Cette version de FileFlow n’est pas encore publiée pour Windows, ou le téléchargement est momentanément indisponible.' `
                -DeveloperMessage $_.Exception.Message
        }

        $checksumUrl = "https://github.com/$Repo/releases/download/$Tag/$checksumName"

        try {
            Download-File -Url $checksumUrl -Destination $checksumFile
        } catch {
            Fail-Install `
                -Code 'FF-I-004' `
                -UserMessage 'Le fichier de contrôle SHA-256 Windows est introuvable. Installation arrêtée par sécurité.' `
                -DeveloperMessage $_.Exception.Message
        }

        Verify-Checksum -File $installer -ChecksumFile $checksumFile

        $script:Step = 'contrôle Authenticode'
        $signature = Get-AuthenticodeSignature -FilePath $installer

        if ($signature.Status -ne 'Valid') {
            if ($AllowUnsigned) {
                Write-Dev "WARNING: Authenticode status=$($signature.Status), allowed in dev mode"
            } else {
                Fail-Install `
                    -Code 'FF-I-006' `
                    -UserMessage 'Windows ne reconnaît pas cet installateur FileFlow comme correctement signé. Installation bloquée par sécurité.' `
                    -DeveloperMessage "Authenticode status=$($signature.Status) message=$($signature.StatusMessage)"
            }
        }

        $script:Step = 'installation Windows'

        try {
            $process = Start-Process `
                -FilePath $installer `
                -ArgumentList '/S' `
                -Wait `
                -PassThru

            if ($process.ExitCode -ne 0) {
                Fail-Install `
                    -Code 'FF-I-008' `
                    -UserMessage 'L’installateur Windows FileFlow n’a pas terminé correctement.' `
                    -DeveloperMessage "NSIS exit code=$($process.ExitCode)"
            }
        } catch {
            Fail-Install `
                -Code 'FF-I-008' `
                -UserMessage 'Windows n’a pas pu exécuter l’installateur FileFlow.' `
                -DeveloperMessage $_.Exception.Message
        }

        Write-Host ''
        Write-Host "✓ FileFlow $Version est installé."
        Write-Host '✓ FileFlow est disponible depuis le menu Démarrer et la recherche Windows.'

        if (-not $NoLaunch) {
            $script:Step = 'lancement Windows'
            $exe = Find-FileFlowExecutable

            if ($exe) {
                try {
                    Start-Process -FilePath $exe
                    Write-Host '✓ FileFlow a été lancé.'
                } catch {
                    Write-Host '✓ Installation terminée. Lance FileFlow depuis le menu Démarrer.'
                    Write-Dev "installed executable found but launch failed: $($_.Exception.Message)"
                }
            } else {
                Write-Host '✓ Installation terminée. Lance FileFlow depuis le menu Démarrer.'
                Write-Dev 'installation succeeded but executable could not be discovered automatically'
            }
        }
    } finally {
        Remove-Item $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
} catch {
    Fail-Install `
        -Code 'FF-I-999' `
        -UserMessage 'Une erreur système inattendue est survenue. Réessaie. Si le problème persiste, transmets le code FF-I-999.' `
        -DeveloperMessage $_.Exception.ToString()
}
