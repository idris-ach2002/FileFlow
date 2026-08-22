# Installation générale

La racine du dépôt contient :

```text
install-fileflow-one-general.sh
```

Il fournit un point d’entrée unique pour Windows, macOS et Linux.

## Télécharger

Depuis GitHub :

```text
https://github.com/idris-ach2002/FileFlow/blob/main/install-fileflow-one-general.sh
```

Télécharger le fichier brut puis l’exécuter.

## macOS / Linux

```bash
chmod +x install-fileflow-one-general.sh
./install-fileflow-one-general.sh
```

## Windows

Installer Git for Windows si nécessaire, ouvrir **Git Bash**, puis :

```bash
bash install-fileflow-one-general.sh
```

## Fonctionnement interne

Le script :

1. détecte l’OS ;
2. détecte `x64` ou `arm64` ;
3. mappe la machine vers la target FileFlow ;
4. vérifie que le canal natif `distribution/<target>` existe ;
5. clone temporairement `main` ;
6. lit `manifest.env` du dernier payload vert ;
7. affiche version, commit source, package et SHA-256 ;
8. sous Windows, nettoie un éventuel `install.env` devenu orphelin ;
9. appelle `install.sh --force` ou `install.ps1 -Force` ;
10. laisse l’installateur officiel installer/réutiliser les moteurs système ;
11. vérifie que l’application est réellement installée ;
12. supprime le clone temporaire.

## Pourquoi ne pas compiler sur le poste utilisateur ?

L’utilisateur ne doit pas installer toute la toolchain Rust/Node pour obtenir FileFlow. La CI construit le package natif ; l’installateur récupère le dernier payload déjà validé.

## Moteurs système

FileFlow s’appuie sur :
- FFmpeg ;
- libvips ;
- ImageMagick ;
- qpdf ;
- img2pdf ;
- Poppler ;
- Ghostscript ;
- Tesseract ;
- OCRmyPDF ;
- Pandoc ;
- LibreOffice ;
- 7-Zip ;
- zstd ;
- LZ4 ;
- ExifTool.

## Pourquoi les branches `distribution/*` existent encore ?

Le système d’installation actuel les utilise comme registre du dernier package vert par target.

Pour avoir **réellement une seule branche `main`**, il faut d’abord migrer :
- les packages natifs ;
- `manifest.env` ou son équivalent ;
- les SHA-256 ;
- la sélection du dernier build vert

vers **GitHub Releases** (ou un autre registre d’artefacts). Une fois cette migration validée, les branches `distribution/*` deviennent supprimables.
