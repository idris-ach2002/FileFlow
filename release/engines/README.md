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

- `optional` : développement local uniquement ;
- `core` : tier interne partiel, non distribuable aux utilisateurs ;
- `full` : tous les moteurs du manifeste obligatoires.

Les workflows natifs, `engine-packs.yml`, les releases et `publish-git-payload.py` imposent `full` pour toute distribution utilisateur.

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

`.github/workflows/engine-packs.yml` est désormais la fabrique officielle : il construit directement les cinq packs FULL sur leurs runners natifs, les relocalise, vérifie leur fermeture de dépendances, exécute les fixtures, recrée une archive déterministe puis la ré-extrait pour une seconde certification. Il crée ensuite seulement un draft atomique `engines-v<packVersion>`.

Le draft doit être publié avant les releases applicatives. Le même tag moteur ne peut pas être remplacé, ce qui rend le pack consommé par les releases immuable. Le codesign Developer ID / Authenticode de production est appliqué dans les workflows de release après toute mutation du runtime.

### Dépôts privés

Le téléchargeur accepte `FILEFLOW_ENGINE_PACK_TOKEN` (prioritaire) ou `GITHUB_TOKEN` pour authentifier les téléchargements HTTPS. Les releases applicatives utilisent automatiquement le token GitHub en lecture ; un token dédié reste possible pour une source de packs située dans un autre dépôt privé.

## Contrat machine cliente

Les packs sont conçus pour le contrat FileFlow **Git-only** : Node, pnpm, Rust, Cargo, Python système, Conda, FFmpeg, ImageMagick, Ghostscript, LibreOffice, qpdf, Tesseract, Docker et GitHub CLI ne sont jamais des prérequis client. Les wrappers moteurs neutralisent les variables de build et utilisent uniquement le runtime privé embarqué, plus les composants fondamentaux de l'OS explicitement considérés comme ABI de plateforme.

`pack-manifest.json` contient l'inventaire de fichiers, `contentSha256` et la provenance exacte des paquets Conda/Python copiés. `make-engine-pack.py` produit une archive déterministe et affiche la taille totale, les plus gros fichiers et dossiers ; `FILEFLOW_ENGINE_PACK_MAX_BYTES` peut imposer une limite.
