# FileFlow — installation utilisateur

FileFlow utilise deux circuits distincts :

- **CI / développeur** : `fileflow.sh` et les artifacts GitHub Actions.
- **Production / utilisateur final** : `install.sh` (macOS/Linux) et `install.ps1` (Windows), basés uniquement sur les GitHub Releases publiques.

Aucun environnement de développement n'est requis sur la machine utilisateur :
Node.js, pnpm, Rust, Cargo, Python, Tauri et GitHub CLI ne sont pas nécessaires.

## macOS

```bash
./install.sh
```

L'installateur :

1. détecte Apple Silicon ou Intel ;
2. télécharge le DMG officiel correspondant ;
3. vérifie `SHA256SUMS-macos` ;
4. vérifie la signature de code et Gatekeeper ;
5. installe `FileFlow.app` dans `/Applications` si possible, sinon `~/Applications` ;
6. lance FileFlow.

FileFlow apparaît ensuite dans Applications, Launchpad et Spotlight.

## Linux

```bash
./install.sh
```

Comportement automatique :

- Debian/Ubuntu : `.deb` via `apt`, avec résolution des dépendances runtime ;
- Fedora/RHEL et dérivés : `.rpm` ;
- autre distribution ou absence de privilèges : installation AppImage locale sous `~/.local/opt/fileflow`, création du `.desktop`, de l'icône et du lanceur `~/.local/bin/fileflow`.

Le fallback AppImage utilise `APPIMAGE_EXTRACT_AND_RUN=1`, donc il ne dépend pas de FUSE/libfuse2 pour démarrer.

Pour forcer l'installation sans sudo :

```bash
./install.sh --linux-user
```

## Windows

Depuis PowerShell :

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\install.ps1
```

L'installateur :

1. télécharge `FileFlow-Windows-x64-Setup.exe` ;
2. vérifie `SHA256SUMS-windows` ;
3. vérifie Authenticode ;
4. exécute l'installation NSIS en mode utilisateur ;
5. FileFlow apparaît dans le menu Démarrer / recherche Windows ;
6. tente de lancer l'application.

## Diagnostic utilisateur / développeur

Mode utilisateur, par défaut :

```bash
./install.sh
```

Le message contient uniquement :

- un code stable, par exemple `FF-I-004` ;
- une explication compréhensible ;
- l'action à effectuer.

Mode développeur :

```bash
./install.sh --mode dev
```

ou :

```powershell
.\install.ps1 -Mode dev
```

Le diagnostic ajoute :

- étape exacte ;
- OS et architecture ;
- version/tag ;
- asset et URL ;
- erreur système ;
- chemin du log.

Logs :

- macOS : `~/Library/Logs/FileFlow/`
- Linux : `~/.local/state/fileflow/` ou `$XDG_STATE_HOME/fileflow/`
- Windows : `%LOCALAPPDATA%\FileFlow\Logs\`

## Codes d'erreur installateur

| Code | Catégorie |
|---|---|
| `FF-I-001` | OS / architecture non supporté |
| `FF-I-002` | réseau / serveur inaccessible |
| `FF-I-003` | release ou asset absent |
| `FF-I-004` | checksum absent ou invalide |
| `FF-I-005` | permissions / écriture |
| `FF-I-006` | signature / confiance du système |
| `FF-I-007` | DMG / archive / extraction invalide |
| `FF-I-008` | installation du paquet échouée |
| `FF-I-009` | application installée mais lancement automatique échoué |
| `FF-I-010` | outil système indispensable absent |
| `FF-I-011` | version invalide / introuvable |
| `FF-I-999` | erreur système inattendue |

## Assets canoniques de production

Les workflows de release produisent toujours ces noms :

### macOS

- `FileFlow-macOS-arm64.dmg`
- `FileFlow-macOS-x64.dmg`

### Linux

- `FileFlow-Linux-x64.AppImage`
- `FileFlow-Linux-x64.deb`
- `FileFlow-Linux-x64.rpm`
- `FileFlow-Linux-arm64.AppImage`
- `FileFlow-Linux-arm64.deb`
- `FileFlow-Linux-arm64.rpm`

### Windows

- `FileFlow-Windows-x64-Setup.exe`
- `FileFlow-Windows-x64.msi`

Les installateurs utilisent les tags indépendants :

- `macos-vX.Y.Z`
- `linux-vX.Y.Z`
- `windows-vX.Y.Z`

L'échec ou l'absence d'une plateforme n'empêche pas l'installation des autres.
