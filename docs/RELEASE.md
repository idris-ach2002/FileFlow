# FileFlow — distribution multi-plateforme

## Principe

La CI construit uniquement l'application FileFlow. Elle **ne construit, ne relocalise et ne certifie plus les moteurs tiers**.

Les moteurs sont installés une seule fois sur la machine utilisateur par `install.sh` / `install.ps1` et sont découverts à l'exécution.

Cette séparation élimine de la CI les chaînes fragiles Conda/micromamba, les copies de bibliothèques natives, RPATH, fermeture ELF/Mach-O/PE et les packs moteurs multi-gigaoctets.

## Cibles

| OS | Architecture | Runner | Livrables |
| --- | --- | --- | --- |
| macOS 11+ | Apple Silicon | `macos-15` | APP + DMG |
| macOS 11+ | Intel | `macos-15-intel` | APP + DMG |
| Windows | x86_64 | `windows-2025` | NSIS EXE + MSI |
| Linux | x86_64 | `ubuntu-22.04` | AppImage + DEB + RPM |
| Linux | ARM64 | `ubuntu-22.04-arm` | AppImage + DEB + RPM |

## Workflows courants

### `ci.yml`

Qualité source commune : Angular, Rust, formatage, tests et Clippy.

### `native-linux.yml`, `native-macos.yml`, `native-windows.yml`

Chaque cible native effectue :

1. installation des prérequis **de build Tauri uniquement** ;
2. `pnpm install --frozen-lockfile` ;
3. preflight source / versions ;
4. compilation Rust pour la cible ;
5. génération de la configuration Tauri légère ;
6. build du paquet FileFlow ;
7. smoke test Angular -> Tauri ;
8. upload de l'artefact ;
9. sur `main` ou `workflow_dispatch`, publication du payload Git utilisé par `install.sh`.

Aucune étape n'installe FFmpeg, LibreOffice, Ghostscript, OCRmyPDF ou un autre moteur de conversion.

## Payload d'installation Git

`scripts/release/publish-git-payload.py` publie le paquet précompilé dans :

- `distribution/linux-x64`
- `distribution/linux-arm64`
- `distribution/macos-arm64`
- `distribution/macos-x64`
- `distribution/windows-x64`

Le manifeste contient notamment :

```text
VERSION=...
SOURCE_SHA=...
PACKAGE_NAME=...
PACKAGE_SHA256=...
PACKAGE_SIZE=...
CHANNEL=...
RUNTIME_MODE=system-managed
```

Le paquet est fragmenté uniquement pour son transport Git. Le runtime natif certifié est inclus dans le paquet FileFlow ; LibreOffice et ExifTool restent des fallbacks hôte.

## Releases signées

Les workflows `release-linux.yml`, `release-macos.yml` et `release-windows.yml` restent indépendants.

### Updater

Secrets :

- `TAURI_UPDATER_PUBKEY`
- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Variable :

- `FILEFLOW_UPDATE_ENDPOINT`

### macOS

Secrets :

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

La production exige Developer ID, hardened runtime et notarisation.

### Windows

Secrets :

- `WINDOWS_CERTIFICATE`
- `WINDOWS_CERTIFICATE_PASSWORD`

Variable :

- `WINDOWS_TIMESTAMP_URL`

Le bundle conserve `staticVCRuntime=true` et WebView2 bootstrapper afin de réduire les prérequis système de l'application elle-même.

## Runtime utilisateur

Les moteurs sont gérés hors CI :

```text
clone temporaire
      │
      ▼
 install.sh / install.ps1
      │
      ├── gestionnaire de paquets OS
      │      └── runtime fourni par le système cible + fallback système
      │
      └── paquet FileFlow précompilé
             └── application persistante + tray/widget
```

Le clone peut ensuite être supprimé.

Un moteur absent n'empêche pas l'application de démarrer : la capability correspondante est simplement indisponible jusqu'à l'installation du moteur.

## Quality gate de release

`python scripts/release/check-release.py` vérifie notamment :

- versions synchronisées ;
- bundles attendus par plateforme ;
- absence de l'ancienne infrastructure de packs moteurs ;
- présence des installateurs runtime et de leurs fallbacks ;
- absence de micromamba/engine-certify dans les workflows ;
- payload `RUNTIME_MODE=system-managed` ;
- `git diff --check`.

## Test du bundle

`scripts/release/smoke-packaged-app.mjs` lance le vrai artefact :

- `.app` sur macOS ;
- AppImage sous Xvfb sur Linux ;
- installation NSIS temporaire sur Windows.

Le test valide le handshake frontend Angular -> backend Tauri. Il ne nécessite aucun moteur de conversion, conformément à l'architecture runtime système.

## Politique des moteurs de conversion

Les workflows CI construisent et empaquettent uniquement l'application FileFlow.
Les moteurs tiers (FFmpeg, libvips, ImageMagick, qpdf, Poppler, Ghostscript,
Tesseract, Pandoc, 7-Zip, Zstd, LZ4, img2pdf, OCRmyPDF, LibreOffice et
ExifTool) sont installés ou vérifiés sur la machine cible par `install.sh` ou
`install.ps1`. FileFlow nettoie l'environnement de ses sous-processus avant de
lancer ces moteurs afin d'éviter qu'un AppImage contamine Python ou les
bibliothèques système. Les scripts de staging restent des outils de diagnostic
optionnels et ne font plus partie du chemin de publication.
