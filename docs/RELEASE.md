# FileFlow — distribution multi-plateforme

## Cibles officielles

| OS | Architecture | Livrables |
| --- | --- | --- |
| macOS 11+ | Apple Silicon | APP + DMG |
| macOS 11+ | Intel | APP + DMG |
| Windows | x86_64 | NSIS EXE + MSI |
| Linux | x86_64 | AppImage + DEB + RPM |
| Linux | ARM64 | AppImage + DEB + RPM |

Linux est construit sur Ubuntu 22.04 afin de conserver une baseline glibc relativement ancienne. Les releases sont construites sur des runners natifs pour éviter de masquer les problèmes spécifiques à un OS/CPU.

## Préparer un checkout de release

Après un changement de dépendances ou l'ajout d'un nouveau crate :

```bash
pnpm run release:bootstrap
```

Cette commande :

1. synchronise `pnpm-lock.yaml` ;
2. synchronise `Cargo.lock` ;
3. contrôle la cohérence des versions ;
4. exécute le gate cross-platform `pnpm run verify`.

Les deux lockfiles modifiés doivent être revus et commités avant de pousser un tag.

## Gate cross-platform

`pnpm run verify` est implémenté en Node et ne dépend pas de `/bin/sh`. Il exécute les mêmes six contrôles sous macOS, Linux et Windows :

1. Angular production build ;
2. Angular tests ;
3. rustfmt ;
4. cargo check `--locked` ;
5. cargo test `--locked` ;
6. Clippy strict `-D warnings`.

`.github/workflows/ci-platforms.yml` exécute ce gate sur Ubuntu, macOS et Windows à chaque PR/push vers les branches principales.

## Modèle moteurs

FileFlow recherche d'abord `resources/engines/bin` dans son bundle, puis le PATH système et enfin quelques emplacements natifs connus. Les sous-processus issus d'un pack reçoivent également les chemins `lib` et `share/tessdata` adaptés.

Les packs vivent hors Git sous :

```text
release/engines/packs/<target-triple>/
```

Le contrat complet est documenté dans `release/engines/README.md` et décrit par `release/engines/manifest.json`.

Trois modes existent :

- `optional`: aucun moteur embarqué obligatoire, fallback système autorisé ;
- `core`: FFmpeg/FFprobe, ImageMagick, libvips, qpdf, 7-Zip, zstd et lz4 obligatoires ;
- `full`: tous les moteurs déclarés obligatoires.

Ne publiez pas un pack `full` avant audit des licences et bibliothèques dynamiques de chaque binaire. En particulier, la licence effective de FFmpeg dépend des options de compilation et Ghostscript/Poppler/LZ4 CLI demandent une décision explicite de redistribution.

## Build local

macOS/Linux :

```bash
FILEFLOW_ENGINE_MODE=optional sh scripts/release/build-local.sh
```

Windows PowerShell :

```powershell
$env:FILEFLOW_ENGINE_MODE = "optional"
./scripts/release/build-local.ps1
```

Pour une release publique autonome, utilisez `core` ou `full` uniquement une fois les packs correspondants préparés et audités.

## Auto-update

L'updater est opt-in et non bloquant au démarrage. `generate-release-config.py` n'active les artifacts updater que lorsque les trois éléments suivants existent :

- `TAURI_UPDATER_PUBKEY` ;
- `TAURI_SIGNING_PRIVATE_KEY` ;
- `FILEFLOW_UPDATE_ENDPOINT`.

L'application vérifie les mises à jour après le bootstrap et demande explicitement à l'utilisateur avant téléchargement/installation.

## GitHub secrets / variables

### Updater

- secret `TAURI_UPDATER_PUBKEY`
- secret `TAURI_SIGNING_PRIVATE_KEY`
- secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- variable `FILEFLOW_UPDATE_ENDPOINT` — par exemple l'URL de `latest.json`

### macOS

- secret `APPLE_CERTIFICATE` — certificat Developer ID Application encodé en base64
- secret `APPLE_CERTIFICATE_PASSWORD`
- secret `APPLE_ID`
- secret `APPLE_PASSWORD` — mot de passe spécifique à l'app
- secret `APPLE_TEAM_ID`

Sans certificat, la CI peut produire un build de test signé ad-hoc ; ce build n'est pas une release publique notarée.

### Windows

- secret `WINDOWS_CERTIFICATE` — PFX encodé en base64
- secret `WINDOWS_CERTIFICATE_PASSWORD`
- variable `WINDOWS_TIMESTAMP_URL`

Sans certificat, les installateurs peuvent être construits pour smoke-test mais ne doivent pas être présentés comme release publique signée.

### Engine packs

- variable `FILEFLOW_ENGINE_MODE=optional|core|full`
- variable `FILEFLOW_ENGINE_PACK_URL_TEMPLATE` — obligatoire pour `core/full`, contient `{target}` et pointe vers un `.tar.gz`/`.zip` accompagné de `<archive>.sha256`

## Processus de release

1. `pnpm run release:version -- X.Y.Z`
2. `pnpm run release:bootstrap`
3. revoir et committer les lockfiles
4. `git status` doit être propre
5. tag annoté `vX.Y.Z`
6. pousser le tag
7. `.github/workflows/release.yml` construit chaque OS/architecture nativement
8. la CI crée/complète une GitHub Release en brouillon
9. SHA-256 est produit pour chaque target
10. smoke-test de chaque installateur sur machine propre
11. publier la GitHub Release seulement après validation

La release `full` n'est considérée autonome que si le stage `FILEFLOW_ENGINE_MODE=full` passe pour toutes les cibles et que les licences/dépendances du pack ont été auditées.

## Smoke-test des moteurs

Après staging, `smoke-engines.py` lance chaque exécutable réellement embarqué avec une commande de version/diagnostic. Une DLL, dylib, `.so` ou donnée runtime manquante fait échouer la release avant le bundling. Cette étape est exécutée automatiquement par les builds locaux et GitHub Actions.
