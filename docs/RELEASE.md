# FileFlow — distribution native multi-plateforme

## Cibles certifiées

| OS | Architecture | Runner GitHub | Livrables |
| --- | --- | --- | --- |
| macOS 11+ | Apple Silicon | `macos-15` | APP + DMG + updater |
| macOS 11+ | Intel | `macos-15-intel` | APP + DMG + updater |
| Windows | x86_64 | `windows-2025` | NSIS EXE + MSI + updater |
| Linux | x86_64 | `ubuntu-22.04` | AppImage + DEB + RPM + updater |
| Linux | ARM64 | `ubuntu-22.04-arm` | AppImage + DEB + RPM + updater |

Linux est construit sur Ubuntu 22.04 pour conserver une baseline glibc raisonnablement ancienne. Les cinq bundles de release sont construits nativement ; aucune release certifiée ne repose sur un cross-build pour masquer les différences WebView, filesystem, signature ou dépendances dynamiques.

## Toolchain figée

- Node `22.22.3` dans la CI (le projet accepte aussi les lignes Angular supportées 24.15.x et 26.x) ;
- pnpm `11.20.0` ;
- Rust `1.97.1` dans `rust-toolchain.toml` et GitHub Actions ;
- lockfiles `pnpm-lock.yaml` et `Cargo.lock` obligatoires.

`pnpm run ci:preflight` refuse une toolchain incompatible ou des versions FileFlow désynchronisées.

## Workflows de certification et de release

### `ci.yml`

Pull requests et pushes `main/develop` : qualité source Angular/Rust et invariants distribution. Aucun bundle n'est publié.

### `native-linux.yml`, `native-macos.yml`, `native-windows.yml`

Ces workflows certifient les cinq targets nativement. La certification moteurs est séparée du build Tauri :

1. construction FULL du runtime moteurs ;
2. relocation/hardening ;
3. fermeture des dépendances natives ;
4. fixtures fonctionnelles en environnement nettoyé ;
5. archive moteur immuable + SHA-256 ;
6. réutilisation de cette archive pour le build Tauri ;
7. nouvelle exécution des fixtures moteurs depuis le bundle final ;
8. smoke Angular -> Tauri ;
9. publication du payload Git-only uniquement sur `main` ou lancement manuel.

Les matrices utilisent `fail-fast: false`. Une architecture sœur en échec ne supprime pas automatiquement le package-smoke d'une cible qui possède son propre pack moteur certifié ; le téléchargement de l'artefact et le build de cette cible restent ses gates locaux.

### `engine-packs.yml`

Workflow manuel officiel de fabrication des packs moteurs. Il **ne consomme plus un candidat externe** : il construit les cinq runtimes FULL depuis le dépôt sur leurs runners natifs, les relocalise, exécute fermeture + tests fonctionnels, ré-extrait l'archive produite pour la recertifier, puis crée un draft atomique `engines-vX.Y.Z`. Une release moteur existante avec le même tag n'est jamais remplacée.

### `release-linux.yml`, `release-macos.yml`, `release-windows.yml`

Releases de production séparées par OS. Elles consomment uniquement le pack `engines-v<packVersion>` déjà publié, imposent `FILEFLOW_ENGINE_MODE=full`, construisent/signent le bundle final et rejouent les tests moteurs depuis l'artefact final. macOS effectue le codesign final puis la notarisation ; Windows effectue le hardening avant Authenticode et ne mute plus les PE ensuite.

Une cible rouge ne peut publier sa release. Les trois familles d'OS sont indépendantes.

## Gate source

`pnpm run verify` exécute :

1. Angular production build ;
2. Angular tests ;
3. `cargo fmt --check` ;
4. `cargo check --workspace --locked` ;
5. `cargo test --workspace --locked` ;
6. Clippy `--all-targets --all-features -- -D warnings`.

`python scripts/release/check-release.py` ajoute les invariants distribution : versions synchronisées, plateformes/bundles attendus, updater, workflows atomiques et manifeste moteurs.

## Smoke test du vrai bundle

`scripts/release/smoke-packaged-app.mjs` ne se contente pas de voir un PID vivant. Il lance :

- l'exécutable dans le `.app` sur macOS ;
- l'AppImage sur Linux (Xvfb en CI) ;
- le NSIS installé silencieusement dans un dossier temporaire sur Windows.

Le test positionne `FILEFLOW_SMOKE_HEALTH_FILE`. Une fois Angular initialisé, le frontend invoque `smoke_frontend_ready`; Rust écrit alors un fichier de santé atomique contenant `backend=true`, `frontend=true`, version, OS, architecture et état scheduler. Le test échoue si l'application quitte, si Angular n'atteint pas Tauri ou si le handshake dépasse le timeout.

## Packs moteurs immuables

> Pour une source privée, `fetch-engine-pack.py` accepte `FILEFLOW_ENGINE_PACK_TOKEN` ou `GITHUB_TOKEN`. Un pack moteur draft doit être publié (ou exposé via une URL authentifiée compatible) avant qu’une release applicative puisse le consommer.


Le manifeste `release/engines/manifest.json` possède un `packVersion` indépendant de la version FileFlow. Les archives ont la forme :

```text
fileflow-engines-1.0.0-aarch64-apple-darwin.tar.gz
fileflow-engines-1.0.0-x86_64-apple-darwin.tar.gz
fileflow-engines-1.0.0-x86_64-pc-windows-msvc.tar.gz
fileflow-engines-1.0.0-x86_64-unknown-linux-gnu.tar.gz
fileflow-engines-1.0.0-aarch64-unknown-linux-gnu.tar.gz
```

Chaque archive est accompagnée de `.sha256` et contient `pack-manifest.json` avec target, version, taille et SHA-256 de chaque fichier. `fetch-engine-pack.py` vérifie le checksum externe, refuse traversal/symlinks, puis exige une correspondance exacte entre inventaire déclaré et fichiers extraits.

La variable `FILEFLOW_ENGINE_PACK_URL_TEMPLATE` doit contenir **les deux placeholders** :

```text
https://github.com/ORG/REPO/releases/download/engines-v{packVersion}/fileflow-engines-{packVersion}-{target}.tar.gz
```

## Validation native des moteurs

### macOS

- `file` pour l'architecture Mach-O ;
- réécriture des dépendances internes absolues avec `install_name_tool` ;
- IDs dylib en `@rpath/...` ;
- dépendances exécutables en `@loader_path/../lib/...` ;
- rejet de `/opt/homebrew`, `/usr/local`, `/tmp` résiduels ;
- signature Developer ID des Mach-O en release ;
- `codesign --verify` sur chaque composant signé.

### Linux

- `file` / `readelf` / `ldd` ;
- RPATH `$ORIGIN` pour les ELF dynamiques ;
- rejet des bibliothèques `not found` et des dépendances provenant de `/home`, `/opt` ou `/usr/local` ;
- les ELF entièrement statiques ne sont pas forcés à recevoir un RPATH.

### Windows

- parser PE intégré pour vérifier `Machine` x64/ARM64 ;
- lecture de l'import table DLL sans dépendre de `dumpbin` ;
- toute DLL non-système importée doit être embarquée ;
- les `.exe/.dll` du pack sont signés Authenticode au moment de la release ;
- validation Authenticode après signature.

Le runtime ajoute `engines/bin` **et** `engines/lib` au PATH des processus enfants afin que Windows résolve les DLL embarquées sans modifier le PATH global de FileFlow.

## Tests fonctionnels moteurs

`functional-engine-tests.py` exécute de vraies opérations sur des fixtures temporaires : conversion image FFmpeg/ImageMagick/libvips, contrôle qpdf, création/extraction 7-Zip, round-trip zstd/lz4, metadata, OCR, Poppler, Ghostscript, Pandoc, LibreOffice, OCRmyPDF et img2pdf lorsqu'ils sont présents dans le flavor staged.

Une commande `--version` verte n'est donc plus considérée comme preuve suffisante.

## Signature et notarisation production

`generate-release-config.py --strict` refuse la release si une configuration production est absente.

### Updater

Secrets :

- `TAURI_UPDATER_PUBKEY`
- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Variable :

- `FILEFLOW_UPDATE_ENDPOINT` (typiquement `https://github.com/ORG/REPO/releases/latest/download/latest.json`)

### macOS

Secrets :

- `APPLE_CERTIFICATE` (P12 Developer ID Application en base64)
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_ID`
- `APPLE_PASSWORD` (mot de passe spécifique à l'app)
- `APPLE_TEAM_ID`

La release exige Developer ID + Hardened Runtime + notarisation/stapling. Le mode ad-hoc `-` reste réservé au package-smoke/local.

### Windows

Secrets :

- `WINDOWS_CERTIFICATE` (PFX base64)
- `WINDOWS_CERTIFICATE_PASSWORD`

Variable :

- `WINDOWS_TIMESTAMP_URL`

La release publique refuse le fallback non signé.

### Moteurs

Variables :

- `FILEFLOW_ENGINE_PACK_URL_TEMPLATE` avec `{packVersion}` + `{target}`

Le tier de production est fixé à `full` dans les workflows. Une release utilisateur partielle `core` est interdite par `release:check`.

## Updater et publication atomique

Tauri produit les artefacts updater signés. Le job final génère un `latest.json` unique couvrant :

- `darwin-aarch64`
- `darwin-x86_64`
- `windows-x86_64`
- `linux-x86_64`
- `linux-aarch64`

Les collisions de noms génériques (`FileFlow.app.tar.gz`, `FileFlow.AppImage`, etc.) sont normalisées avec le target avant publication. `SHA256SUMS` utilise ensuite les noms d'assets plats réellement visibles dans GitHub Releases.

`verify-release.mjs` exige tous les installateurs, toutes les signatures updater, la correspondance des signatures de `latest.json` et la couverture SHA-256 de **chaque** asset avant création du draft.

Le draft reste manuel au début : télécharger/tester les cinq familles d'installateurs puis cliquer **Publish**. Après plusieurs releases sans incident, ce dernier clic pourra être automatisé séparément.

## Test de transition updater

Le job final compare la dernière release publique à la nouvelle version et exécute `verify-updater-transition.mjs`. Il refuse une version qui ne progresse pas et exige les cinq URLs/signatures.

Le véritable test installé `1.0.1 -> 1.0.2` ne peut être exécuté qu'une fois les deux versions signées/publiées disponibles. L'infrastructure est prête : lorsque `v1.0.2` est construite après `v1.0.1`, la transition est automatiquement contrôlée avant le draft. Un test d'installation updater bout-en-bout sur machine propre reste le dernier gate manuel avant publication tant qu'une ancienne release installable n'existe pas dans la CI.

## Processus de release

```bash
pnpm run release:version -- 1.0.2
pnpm run release:bootstrap
# revue des changements + commit
git tag v1.0.2
git push origin v1.0.2
```

GitHub : preflight -> 5 builds -> engines -> verify -> package -> smoke -> signatures/notarisation -> artifacts privés -> manifest/checksums -> draft atomique.

Ne fusionner une branche de distribution vers `main` et ne publier le draft qu'après un passage réellement vert sur GitHub Actions. Un checkout local ne peut pas certifier Windows/macOS/Linux à la place des runners natifs.

### Runtime Windows

Le bundle Windows active `bundle.windows.staticVCRuntime=true` (clé Tauri v2) afin de ne pas exiger l’installation préalable du redistribuable Visual C++ sur la machine cible. `windows-preflight.ps1` refuse aussi l’ancienne clé invalide `bundleVCRuntime`.
