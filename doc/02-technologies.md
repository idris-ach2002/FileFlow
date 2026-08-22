# Technologies et rôle dans FileFlow

## Découpage technologique

FileFlow utilise chaque technologie dans un rôle précis :

- **Angular / TypeScript** : expérience utilisateur ;
- **Tauri** : shell desktop et pont IPC ;
- **Rust** : domaine, planification, scheduling, exécution et persistance ;
- **moteurs système** : transformation effective des formats ;
- **GitHub Actions** : construction native et publication.

## Frontend

| Package | Version déclarée | Rôle |
| --- | --- | --- |
| `@angular/aria` | `22.1.0` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/build` | `22.1.2` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/cdk` | `22.1.0` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/cli` | `22.1.2` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/common` | `22.1.0` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/compiler` | `22.1.0` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/compiler-cli` | `22.1.0` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/core` | `22.1.0` | Framework de l’interface desktop : composants, DI, état de vue et rendu. |
| `@angular/forms` | `22.1.0` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/platform-browser` | `22.1.0` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@angular/router` | `22.1.0` | Navigation entre les différents espaces de l’application. |
| `@tauri-apps/api` | `2.11.1` | API TypeScript de communication avec le shell Tauri. |
| `@tauri-apps/cli` | `2.11.4` | Build, développement et packaging desktop Tauri. |
| `@tauri-apps/plugin-dialog` | `2.7.2` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@tauri-apps/plugin-notification` | `2.3.3` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@tauri-apps/plugin-opener` | `2.5.4` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@tauri-apps/plugin-process` | `2.3.1` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `@tauri-apps/plugin-updater` | `2.10.1` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `jsdom` | `30.0.1` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `rxjs` | `^7.8.2` | Flux réactifs côté frontend et composition asynchrone. |
| `tslib` | `^2.8.1` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `typescript` | `~6.0.0` | Typage statique du frontend. |
| `vitest` | `4.1.7` | Tests TypeScript/frontend. |

## Rust

| Crate externe | Version déclarée | Rôle |
| --- | --- | --- |
| `async-trait` | `workspace` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `chrono` | `workspace` | Horodatage des jobs et historiques. |
| `dashmap` | `workspace` | Structures concurrentes partagées. |
| `fileflow-adapter-archive` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-ffmpeg` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-ghostscript` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-imagemagick` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-img2pdf` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-lz4` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-metadata` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-ocr` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-office` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-pandoc` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-poppler` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-qpdf` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-tesseract` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-vips` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-adapter-zstd` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-analysis` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-core` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-domain` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-engine` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-executor` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-formats` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-intake` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-output` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-planner` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-scheduler` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-storage` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-workflows` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `fileflow-workspace` | `path` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `infer` | `workspace` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `parking_lot` | `workspace` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `rusqlite` | `workspace` | Persistance SQLite locale. |
| `serde` | `workspace` | Sérialisation des contrats métier et IPC. |
| `serde_json` | `workspace` | Paramètres dynamiques JSON des actions. |
| `sha2` | `workspace` | Vérification d’intégrité SHA-256. |
| `sysinfo` | `workspace` | Informations machine et ressources système. |
| `tauri` | `2.11.5` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `tauri-build` | `2.6.3` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `tauri-plugin-dialog` | `2.7.2` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `tauri-plugin-notification` | `2.3.3` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `tauri-plugin-opener` | `2.5.4` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `tauri-plugin-process` | `2.3.1` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `tauri-plugin-updater` | `2.10.1` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `thiserror` | `workspace` | Modélisation d’erreurs Rust typées. |
| `tokio` | `workspace` | Runtime Rust asynchrone : tâches, processus, canaux et I/O. |
| `tokio-util` | `workspace` | CancellationToken et utilitaires asynchrones. |
| `tracing` | `workspace` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `tracing-subscriber` | `workspace` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |
| `uuid` | `workspace` | Identifiants uniques pour jobs, workspaces et staging. |
| `walkdir` | `workspace` | Dépendance du projet ; son rôle précis dépend du module qui l’importe. |

## Workspace Rust

| Crate | Rôle |
| --- | --- |
| `crates/adapters/fileflow-adapter-archive` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-ffmpeg` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-ghostscript` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-imagemagick` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-img2pdf` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-lz4` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-metadata` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-ocr` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-office` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-pandoc` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-poppler` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-qpdf` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-tesseract` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-vips` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/adapters/fileflow-adapter-zstd` | Adaptateur fin entre un moteur externe et les contrats FileFlow. |
| `crates/fileflow-analysis` | Analyse d’assets et recommandations d’actions. |
| `crates/fileflow-core` | Composition des services métier FileFlow. |
| `crates/fileflow-domain` | Modèle métier : jobs, formats, politiques de sortie, ressources et identifiants. |
| `crates/fileflow-engine` | Découverte, résolution et disponibilité des moteurs système. |
| `crates/fileflow-executor` | Orchestration asynchrone des jobs et lancement des moteurs externes. |
| `crates/fileflow-formats` | Registre de formats, familles et règles liées aux types de fichiers. |
| `crates/fileflow-intake` | Ingestion, détection initiale et préparation des entrées. |
| `crates/fileflow-output` | Résolution destination/nommage/conflits et staging transactionnel. |
| `crates/fileflow-planner` | Catalogue de capacités et planification des étapes de conversion. |
| `crates/fileflow-scheduler` | Concurrence bornée et attribution de profils de ressources. |
| `crates/fileflow-storage` | Persistance locale, historique et données applicatives. |
| `crates/fileflow-workflows` | Recettes et enchaînements automatisés. |
| `crates/fileflow-workspace` | Racines de workspace et contexte des assets. |
| `src-tauri` | Frontière desktop : Tauri, commandes IPC, état applicatif et packaging. |

## Moteurs externes

| Moteur | Domaine | Responsabilité |
| --- | --- | --- |
| FFmpeg | Audio / vidéo | Transcodage et transformations média. |
| libvips | Image | Conversions rapides et traitements image. |
| ImageMagick | Image | Opérations image polyvalentes et fallback. |
| qpdf | PDF | Validation, fusion, découpage, protection et structure PDF. |
| img2pdf | Image → PDF | Encapsulation d’images dans un PDF. |
| Poppler | PDF → image | Rendu et extraction visuelle des pages PDF. |
| Ghostscript | PDF / PostScript | Conversion et normalisation PDF/PS. |
| Tesseract | OCR | Reconnaissance optique de caractères. |
| OCRmyPDF | OCR PDF | Ajout/normalisation d’une couche OCR dans un PDF. |
| Pandoc | Documents | Conversion de documents structurés. |
| LibreOffice | Bureautique | Conversion des formats Office. |
| 7-Zip | Archives | Création et extraction d’archives. |
| zstd | Compression | Compression Zstandard. |
| LZ4 | Compression | Compression/décompression LZ4. |
| ExifTool | Métadonnées | Lecture et nettoyage des métadonnées. |

## Pourquoi des moteurs system-managed ?

Les outils restent installés sur l’hôte plutôt que d’être dupliqués dans chaque bundle.

Avantages :
- packages FileFlow plus légers ;
- mises à jour des moteurs indépendantes ;
- réutilisation des paquets natifs de l’OS ;
- meilleure séparation orchestrateur / moteur.

Contraintes :
- installation multi-plateforme plus complexe ;
- besoin d’un **runtime doctor** ;
- résolution de PATH et de versions ;
- gestion rigoureuse de l’environnement des processus externes.
