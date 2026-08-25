# FileFlow — distribution multi-plateforme

## Principe

La CI construit l’application FileFlow, FileFlow Setup et son CLI. Elle **ne construit, ne relocalise et ne certifie plus les moteurs tiers**.

Les moteurs sont installés une seule fois sur la machine utilisateur par `install.sh` / `install.ps1` et sont découverts à l'exécution.

Cette séparation élimine de la CI les chaînes fragiles Conda/micromamba, les copies de bibliothèques natives, RPATH, fermeture ELF/Mach-O/PE et les packs moteurs multi-gigaoctets.

## Cibles

| OS | Architecture | Runner | Livrables |
| --- | --- | --- | --- |
| macOS 11+ | Apple Silicon | `macos-15` | FileFlow APP/DMG + Setup APP/DMG + CLI |
| macOS 11+ | Intel | `macos-15-intel` | FileFlow APP/DMG + Setup APP/DMG + CLI |
| Windows | x86_64 | `windows-2025` | FileFlow NSIS/MSI + Setup EXE + CLI |
| Linux | x86_64 | `ubuntu-22.04` | FileFlow + Setup AppImage/DEB/RPM + CLI |
| Linux | ARM64 | `ubuntu-22.04-arm` | FileFlow + Setup AppImage/DEB/RPM + CLI |

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
RUNTIME_MODE=system
```

Le paquet est fragmenté uniquement pour son transport Git. Aucun moteur n'est inclus dedans.

## Releases signées

Un tag unique `vX.Y.Z` déclenche `fileflow-release.yml`. Celui-ci appelle les trois workflows de build réutilisables et ne publie la release qu’après la réussite des cinq cibles natives. `latest.json` alimente l’Updater intégré ; `downloads.json` alimente FileFlow Setup et le portail Cloudflare. Les deux manifestes, les signatures et `SHA256SUMS` sont contrôlés avant publication.

Le portail est déployé par `site-cloudflare.yml` après validation de `website/`. Il nécessite les secrets GitHub `CLOUDFLARE_API_TOKEN` et `CLOUDFLARE_ACCOUNT_ID`.

### Updater

Secrets :

- `TAURI_UPDATER_PUBKEY`
- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Variable :

- `FILEFLOW_UPDATE_ENDPOINT`

La valeur recommandée est `https://github.com/OWNER/REPOSITORY/releases/latest/download/latest.json`. L’application consulte ainsi uniquement une release atomique publiée, jamais les exécutions GitHub Actions ni un artefact intermédiaire.

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
      │      └── moteurs locaux persistants
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
- payload `RUNTIME_MODE=system` ;
- `git diff --check`.

## Tests des bundles

`scripts/release/smoke-packaged-app.mjs` lance le vrai artefact :

- `.app` sur macOS ;
- AppImage sous Xvfb sur Linux ;
- installation NSIS temporaire sur Windows.

Le test valide le handshake frontend Angular -> backend Tauri. Il ne nécessite aucun moteur de conversion, conformément à l'architecture runtime système.

`scripts/release/smoke-packaged-setup.mjs` lance séparément le vrai FileFlow Setup, le maintient hors écran, attend son handshake UI -> Tauri et vérifie le diagnostic de plateforme. Les groupes de processus des deux tests sont arrêtés avant que la publication puisse continuer.
