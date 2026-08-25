# FileFlow Setup et portail de téléchargement

## Entrées utilisateur

- `FileFlowSetup` : interface native guidée install/repair/doctor/uninstall.
- `fileflow-setup-cli` : mêmes plans et même moteur transactionnel, avec `--dry-run` et `--json`.
- FileFlow > Paramètres > Mises à jour : lance la copie de maintenance avec `--mode repair|uninstall`.
- `install-fileflow-one-general.sh` et les scripts du portail : bootstrap minimal de la dernière release complète.

## Règles de sécurité

1. Le manifeste `downloads.json` doit être HTTPS, versionné et complet pour cinq cibles.
2. Taille et SHA-256 sont vérifiés avant ouverture du paquet.
3. macOS contrôle le bundle avec `codesign`; Windows contrôle Authenticode lorsqu’il existe.
4. L’application précédente est sauvegardée ou mise en quarantaine avant activation/retrait.
5. Chaque étape écrit un journal atomique et dispose d’une action de rollback.
6. Par défaut, seuls les moteurs et bibliothèques marqués `installed_by_fileflow` dans le reçu peuvent être retirés.
   Le reçu de schéma 2 conserve le gestionnaire et le paquet exacts. La désinstallation utilise
   `apt-get purge`, `dnf remove`, `zypper remove`, `pacman -Rns`, `brew uninstall`, `pipx`,
   `flatpak`, `winget`, Chocolatey ou Scoop selon l’installation réellement effectuée.
   Une dépendance déjà présente avant Setup n’est jamais revendiquée ni supprimée automatiquement.
   Un mode expert séparé permet toutefois de retirer des moteurs préexistants **explicitement sélectionnés** ;
   il est désactivé par défaut, affiche un avertissement et peut affecter d’autres applications.
7. Les fichiers produits par l’utilisateur ne font partie d’aucun plan de suppression.
8. Sous Linux en terminal, Setup valide `sudo` une seule fois puis maintient cette autorisation pendant
   l’opération. Depuis l’interface graphique, `pkexec` reste privilégié lorsqu’il est disponible.
9. Sous Windows, l’installation standard peut ajouter Git Bash comme outil de support ; s’il est ajouté
   par FileFlow, il est inscrit au reçu comme intégration et reste soumis aux mêmes règles de propriété.

## Développement local

```bash
pnpm run setup:dev
pnpm run setup:dev:local
pnpm run setup:dev:release
cargo run -p fileflow-setup --bin fileflow-setup-cli -- doctor --json
# Mode expert (jamais implicite) :
cargo run -p fileflow-setup --bin fileflow-setup-cli -- uninstall --remove-preexisting-engines --engines ffmpeg,zstd --dry-run --json
cargo test -p fileflow-setup-core -p fileflow-setup
pnpm run site:test
pnpm run site:build
pnpm run site:dev
```

Sur macOS, le contrôle avant release construit réellement FileFlow, Setup et le CLI pour ARM64 et Intel.
Le CLI embarqué dans `FileFlowSetup.app` est compilé et signé **avant** le bundling Tauri, puis sa signature
et son architecture sont revérifiées dans le bundle final :

```bash
pnpm run release:preflight-macos
```

En développement, `pnpm run setup:dev` détecte automatiquement le paquet de la version courante
dans `target/<cible>/release/bundle`. Aucun chemin ne doit être copié manuellement. Pour exiger
une source locale (et échouer immédiatement lorsqu’elle manque) :

```bash
pnpm run setup:dev:local
```

`pnpm run setup:dev:release` force au contraire le test du canal public. La source locale est
absente des builds release : un Setup distribué utilise exclusivement le manifeste validé.

Les workflows de release exécutent ensuite deux handshakes empaquetés séparés : l’application principale et FileFlow Setup sont lancés hors écran, leur frontend doit joindre Tauri par IPC, puis tout leur groupe de processus est fermé. La publication atomique reste bloquée si l’un des deux échoue.

Les bundles sont eux aussi physiquement isolés : FileFlow utilise `target/<cible>/release/bundle`,
tandis que Setup utilise `target/fileflow-setup/<cible>/release/bundle`. Cette séparation empêche
un second appel à Tauri de nettoyer ou remplacer les DMG, AppImage ou installateurs déjà produits
par le premier. Le collecteur ne fusionne les deux ensembles qu’après leur validation indépendante.

## Promotion automatique

Une version n’est pas publiée à chaque commit. Après une modification volontaire de version avec
`pnpm run release:version X.Y.Z`, le push sur `main` exécute `FileFlow Common Quality`.
Lorsque — et seulement lorsque — ce workflow est vert, `FileFlow Automatic Promotion` compare la
version avec la dernière release et déclenche le workflow atomique sur le SHA exactement testé.
Le tag `vX.Y.Z` et la GitHub Release ne sont créés qu’après la réussite et la vérification des cinq
cibles. Il ne faut donc plus créer le tag manuellement.

## Cloudflare Pages

Le projet se trouve dans `website/`. La Function `/api/downloads` récupère le manifeste GitHub, valide les URLs/checksums/tailles et le met en cache. La page détecte la plateforme, propose Setup et peut recalculer localement le SHA-256 d’un fichier choisi.

`pnpm run site:dev` lance Wrangler depuis le bon paquet workspace : les ressources statiques et
`website/functions/` sont alors servies ensemble. La détection du système est indépendante du
réseau et une réponse HTML ne peut jamais être interprétée comme `downloads.json`.
Sur `localhost`, la Function fournit un manifeste de prévisualisation sans URL factice : les cinq
cartes et la détection peuvent être testées avant la publication. En production, le portail active
les téléchargements uniquement à partir de la release stable complète la plus récente.

Le workflow nécessite :

- `CLOUDFLARE_API_TOKEN` ;
- `CLOUDFLARE_ACCOUNT_ID`.

Le jeton doit avoir la permission `Account > Cloudflare Pages > Edit`. Le projet Pages est créé une seule fois avant le premier déploiement :

```bash
cd website
pnpm dlx wrangler@4 pages project create fileflow-downloads --production-branch=main
```

Ensuite, `.github/workflows/site-cloudflare.yml` construit le portail et exécute Wrangler depuis `website/` afin d’envoyer ensemble `dist/` et `functions/`. Un domaine personnalisé peut être associé au projet `fileflow-downloads` sans modifier le code.
