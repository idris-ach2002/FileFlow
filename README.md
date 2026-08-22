<p align="center">
  <img src="doc/assets/branding/fileflow-emblem.png" alt="FileFlow" width="190">
</p>

<h1 align="center">FileFlow</h1>

<p align="center">
  <strong>Transformer, organiser et automatiser les fichiers depuis une application desktop native.</strong>
</p>

<p align="center">
FileFlow orchestre des moteurs de conversion système éprouvés derrière une interface unifiée.
Le logiciel privilégie l’exécution locale, la traçabilité, les sorties déterministes et un packaging natif Windows, macOS et Linux.
</p>

---

## Qu’est-ce que FileFlow ?

**FileFlow est une station de transformation de fichiers multiplateforme.**

L’utilisateur sélectionne des fichiers, FileFlow détecte leurs formats, détermine les actions compatibles, planifie les moteurs nécessaires, contrôle la concurrence, exécute les conversions puis finalise les résultats selon une politique de destination et de conflit.

Version du workspace documentée : **1.0.1**.

## Ce que FileFlow sait faire

- convertir des images individuellement ou par lot ;
- créer des PDF à partir d’images ou de collections ;
- fusionner, découper, valider et transformer des PDF ;
- effectuer de l’OCR ;
- convertir des documents Office et des formats structurés ;
- traiter des médias avec FFmpeg ;
- créer et extraire des archives ;
- compresser/décompresser avec zstd et LZ4 ;
- gérer les métadonnées ;
- organiser les sorties, l’historique, les favoris et les automatisations ;
- fonctionner avec des moteurs système installés sur Windows, macOS et Linux.

## Documentation

| Ressource | Objet |
| --- | --- |
| [Index documentaire](doc/INDEX.md) | Navigation globale |
| [Architecture](doc/01-architecture.md) | Couches, crates, IPC, moteurs |
| [Technologies](doc/02-technologies.md) | Technologie → responsabilité |
| [Algorithmes & complexité](doc/03-algorithms.md) | Implémentations et coûts |
| [Graphe vers PDF](doc/04-pdf-transformation-graph.md) | Pipeline Smart-to-PDF |
| [Fonctionnalités](doc/05-features.md) | Catalogue fonctionnel |
| [Interface](doc/06-ui.md) | Parcours 5 étapes + espaces avancés |
| [CI/CD](doc/07-ci-cd.md) | Workflows et distribution |
| [Difficultés](doc/08-difficulties.md) | Incidents et solutions |
| [Exécution](doc/09-execution.md) | Local + GitHub Actions |
| [Installation](doc/10-installation.md) | Installateur général |

## Installation rapide

Télécharger `install-fileflow-one-general.sh` depuis la racine du dépôt.

macOS / Linux :

```bash
chmod +x install-fileflow-one-general.sh
./install-fileflow-one-general.sh
```

Windows : ouvrir **Git Bash**, puis :

```bash
bash install-fileflow-one-general.sh
```

Le script détecte la plateforme, sélectionne le payload natif vert correspondant, appelle l’installateur officiel de FileFlow et vérifie l’installation.

Voir [Installation générale](doc/10-installation.md).

## Architecture en un coup d’œil

```mermaid
flowchart LR
    UI["Angular / TypeScript"] --> IPC["Tauri IPC"]
    IPC --> CORE["Rust core"]
    CORE --> INTAKE["Intake + Formats"]
    CORE --> PLAN["Planner"]
    PLAN --> SCHED["Resource Scheduler"]
    SCHED --> EXEC["Executor"]
    EXEC --> ENGINES["Moteurs système"]
    EXEC --> OUTPUT["Output Resolver"]
    OUTPUT --> STORE["Historique / SQLite"]
```

## Principe d’ingénierie

FileFlow ne réimplémente pas FFmpeg, qpdf, Tesseract ou LibreOffice. Il construit au-dessus d’eux une couche produit : détection, planification, gestion des chemins, contrôle des ressources, annulation, staging, validation, historique et interface utilisateur.

---

**Commencer par [`doc/INDEX.md`](doc/INDEX.md).**
