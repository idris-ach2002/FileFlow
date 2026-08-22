# Exécuter FileFlow : local ou workflow

Ce fichier est l’unique guide développeur pour :
1. récupérer/puller le projet ;
2. exécuter les validations en local ;
3. lancer un build via GitHub Actions.

## 1. Premier clone

```bash
git clone https://github.com/idris-ach2002/FileFlow.git
cd FileFlow
git switch main
git pull --ff-only origin main
```

## 2. Projet déjà présent

```bash
cd /chemin/vers/FileFlow
git switch main
git pull --ff-only origin main
git status
git rev-parse --short=12 HEAD
```

## 3. Dépendances JavaScript

```bash
pnpm install --frozen-lockfile
```

## 4. Moteurs système

macOS / Linux :

```bash
bash scripts/runtime/install-dependencies.sh --no-update
```

Windows PowerShell :

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/runtime/install-dependencies.ps1
```

## 5. Validation locale

```bash
pnpm run ci:preflight
cargo check --workspace --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

## 6. Frontend

```bash
pnpm --filter @fileflow/frontend build
```

Pour le développement, utiliser le script `dev` déclaré dans les `package.json` du workspace.

## 7. Bundle desktop local

```bash
pnpm exec tauri build
```

Le package produit correspond uniquement à l’OS courant.

## 8. GitHub Actions

Workflows présents :

- `ci.yml` — FileFlow Common Quality
- `native-linux.yml` — FileFlow Native Linux
- `native-macos.yml` — FileFlow Native macOS
- `native-windows.yml` — FileFlow Native Windows
- `release-linux.yml` — FileFlow Release Linux
- `release-macos.yml` — FileFlow Release macOS
- `release-windows.yml` — FileFlow Release Windows

Lister :

```bash
gh workflow list
```

Lancer Windows uniquement :

```bash
gh workflow run native-windows.yml --ref main
```

Lister ses runs :

```bash
gh run list --workflow native-windows.yml --limit 10
```

Suivre un run :

```bash
gh run watch <RUN_ID> --exit-status
```

Utiliser le fichier YAML macOS/Linux correspondant pour les autres plateformes.

## 9. Choisir local ou workflow

| Besoin | Local | Workflow |
| --- | ---: | ---: |
| test Rust rapide | oui | optionnel |
| développement UI | oui | optionnel |
| bundle natif de la machine | oui | oui |
| build Windows depuis un Mac | non | oui |
| publication du payload de distribution | non | oui |
| test réel utilisateur | oui sur l’OS cible | non suffisant seul |

## 10. Commits documentaires

Pour éviter les builds natifs lors d’un changement purement documentaire :

```bash
git commit -m "docs: update documentation [skip actions]"
```
