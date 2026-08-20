# FileFlow native engine packs

Les moteurs natifs sont versionnés indépendamment de FileFlow. `release/engines/manifest.json` fixe `packVersion` et le contrat des exécutables.

## Format immuable

Un pack cible contient :

```text
fileflow-engines-<packVersion>-<target>/
├── bin/
├── lib/
├── share/
├── licenses/
└── pack-manifest.json
```

`pack-manifest.json` contient la version, le target et l'inventaire exact (`path`, `size`, `sha256`). L'archive possède aussi un fichier `<archive>.sha256` externe.

Les targets certifiés sont :

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

## Flavors

- `optional` : aucun moteur embarqué obligatoire, utilisé pour développement/package-smoke ;
- `core` : FFmpeg/FFprobe, ImageMagick, libvips, qpdf, 7-Zip, zstd et lz4 obligatoires ;
- `full` : tous les moteurs du manifeste obligatoires.

Ne promouvoir `full` qu'après audit des licences de redistribution de chaque build exact.

## Construction locale

À partir d'un dossier avec `bin/lib/share` :

```bash
python scripts/release/make-engine-pack.py \
  --target aarch64-apple-darwin \
  --source /path/to/engines \
  --licenses /path/to/licenses
```

Le résultat est :

```text
release/engines/out/fileflow-engines-1.0.0-aarch64-apple-darwin.tar.gz
release/engines/out/fileflow-engines-1.0.0-aarch64-apple-darwin.tar.gz.sha256
```

## Téléchargement CI

`FILEFLOW_ENGINE_PACK_URL_TEMPLATE` doit contenir `{packVersion}` et `{target}` :

```text
https://github.com/ORG/REPO/releases/download/engines-v{packVersion}/fileflow-engines-{packVersion}-{target}.tar.gz
```

`fetch-engine-pack.py` refuse : checksum incorrect, version/target incorrect, traversal, symlink/hardlink, fichier absent, fichier supplémentaire ou hash/taille interne différent.

## Promotion

`.github/workflows/engine-packs.yml` prend une URL de candidats, certifie les cinq targets et crée seulement ensuite un draft atomique `engines-v<packVersion>`. Le draft doit être publié avant qu'une release FileFlow `core/full` puisse utiliser son URL publique.

Le durcissement/signature finale est target-native : `install_name_tool`/codesign sur macOS, RPATH `$ORIGIN` sur Linux, PE/DLL + Authenticode sur Windows.

### Dépôts privés

Le téléchargeur accepte `FILEFLOW_ENGINE_PACK_TOKEN` (prioritaire) ou `GITHUB_TOKEN` pour authentifier les téléchargements HTTPS. Les releases applicatives utilisent automatiquement le token GitHub en lecture ; un token dédié reste possible pour une source de packs située dans un autre dépôt privé.
