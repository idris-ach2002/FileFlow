# FileFlow — installation permanente

FileFlow utilise désormais un modèle simple : **l'application est construite par GitHub Actions, les moteurs de conversion sont installés une seule fois sur la machine**.

Le dépôt cloné ne sert qu'à lancer l'installateur. Après succès, il peut être supprimé :

- l'application reste installée dans le système ;
- les moteurs (`ffmpeg`, LibreOffice, qpdf, Tesseract, etc.) restent installés par le gestionnaire de paquets de l'OS ;
- l'icône / le widget tray FileFlow reste disponible avec l'application.

## Installation utilisateur

### macOS / Linux

```bash
./install.sh
```

Le script :

1. détecte l'OS et l'architecture ;
2. installe les moteurs localement avec plusieurs fallbacks ;
3. exécute le doctor FileFlow sans bloquer si une capacité optionnelle manque ;
4. récupère le paquet FileFlow précompilé depuis la branche `distribution/<os>-<arch>` ;
5. vérifie sa taille et son SHA-256 ;
6. installe l'application de façon permanente ;
7. lance FileFlow sauf avec `--no-launch`.

Options utiles :

```bash
./install.sh --mode dev     # diagnostic détaillé
./install.sh --force        # réinstalle le même paquet
./install.sh --skip-deps    # ne touche pas aux moteurs
./install.sh --doctor       # vérifie seulement les moteurs
```

### Windows

Depuis PowerShell :

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\install.ps1
```

Options :

```powershell
.\install.ps1 -Mode dev
.\install.ps1 -Force
.\install.ps1 -SkipDependencies
.\install.ps1 -Doctor
```

## Stratégie de fallback des dépendances

Une dépendance introuvable dans un dépôt **ne stoppe pas l'installation globale**. Le script passe à la source suivante puis continue avec le moteur suivant.

### Linux

Ordre principal selon la distribution :

- Debian/Ubuntu : `apt` ;
- Fedora/RHEL : `dnf` ;
- openSUSE : `zypper` ;
- Arch : `pacman` ;
- Homebrew Linux si déjà disponible.

Fallbacks supplémentaires :

- Homebrew pour plusieurs CLI ;
- `pipx` pour `img2pdf` et `ocrmypdf` ;
- Flatpak pour LibreOffice si Flatpak est déjà présent.

### macOS

Homebrew est utilisé pour les moteurs CLI et le cask LibreOffice. S'il manque, l'installateur tente l'installateur officiel Homebrew puis continue même en cas d'échec.

### Windows

Ordre de tentative par moteur :

1. `winget` ;
2. Chocolatey s'il est installé ;
3. Scoop s'il est installé ;
4. `pipx` pour les outils Python.

## Moteurs recherchés

FileFlow détecte : FFmpeg, libvips, ImageMagick, qpdf, img2pdf, Poppler, Ghostscript, Tesseract, OCRmyPDF, LibreOffice, Pandoc, ExifTool, 7-Zip, Zstandard et LZ4.

Une dépendance manquante désactive uniquement les actions qui en dépendent. FileFlow peut toujours démarrer.

## Où FileFlow cherche les exécutables

L'ordre est :

1. override explicite `FILEFLOW_<EXECUTABLE>_PATH` ;
2. `PATH` du processus ;
3. `FILEFLOW_ENGINE_PATH` ;
4. emplacements standards de la plateforme.

Sont notamment couverts :

- `/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin` ;
- les dossiers Python utilisateur macOS ;
- WinGet Links, WindowsApps, Scoop shims et Chocolatey ;
- les emplacements LibreOffice usuels sur macOS et Windows.

## Diagnostic

```bash
bash scripts/runtime/doctor.sh
```

ou Windows :

```powershell
.\scripts\runtime\doctor.ps1
```

Le doctor indique `[OK]` ou `[MISS]` pour chaque moteur. Avec `--strict` / `-Strict`, il renvoie une erreur si une capacité manque.

## Important

Les dépendances de **développement** (Node, pnpm, Rust, Tauri) ne sont jamais installées par l'installateur utilisateur. Elles restent uniquement nécessaires pour contribuer au projet.
