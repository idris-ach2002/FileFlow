# Fonctionnalités FileFlow

Modules frontend détectés : `advanced`, `automations`, `favorites`, `formats`, `help`, `history`, `home`, `organize`, `settings`, `welcome`, `workspace`.

## Images

- conversion de formats ;
- traitement par lot ;
- images vers PDF ;
- prise en charge étendue des images, dont HEIC/HEIF, RAW, JPEG 2000, OpenEXR et formats Netpbm ;
- intégration libvips et ImageMagick ;
- politique de sortie commune avec les autres opérations.

## PDF

- Smart-to-PDF ;
- collection/dossier vers PDF ;
- images vers PDF ;
- HTML dynamique vers PDF avec navigateur isolé ;
- EML vers PDF après décodage MIME et neutralisation des scripts ;
- fusion de PDF ;
- découpage ;
- PDF vers images ;
- validation de signature ;
- validation qpdf ;
- protection lorsqu’elle est demandée ;
- OCR et traitements complémentaires selon paramètres.

## OCR

- reconnaissance via Tesseract ;
- traitement PDF OCR via OCRmyPDF lorsque le pipeline l’exige.

## Documents bureautiques

- LibreOffice pour les formats Office ;
- Pandoc pour les formats documentaires compatibles.

## Médias

FFmpeg fournit les capacités audio/vidéo disponibles dans le catalogue FileFlow.

## Archives et compression

- création et extraction d’archives ;
- 7-Zip ;
- zstd ;
- LZ4 ;
- expansion d’archives dans certains workflows intelligents.

## Métadonnées

ExifTool est utilisé pour les opérations de métadonnées et les nettoyages associés.

## Organisation des sorties

Les politiques comprennent notamment :
- même dossier ;
- sous-dossier ;
- dossier personnalisé ;
- demande interactive ;
- conservation d’arborescence ;
- stratégie de conflit ;
- stratégie de nommage ;
- protection contre l’écrasement de la source.

## Workspace

Le workspace conserve la relation entre chaque asset et sa racine source, afin que les opérations puissent reconstruire ou ignorer l’arborescence selon la politique choisie.

Les fichiers qui ne sont pas directement affichables par le WebView reçoivent une prévisualisation locale générée et mise en cache : images rares et HEIC, documents Office, HTML, EML, textes, EPUB/FB2 et miniatures vidéo. Une destination choisie explicitement dans le workspace est toujours prioritaire sur le dossier guidé configuré auparavant.

## Automatisations

Les recettes et automatisations réutilisent les contrats métier de FileFlow : actions, paramètres et `OutputPolicy`.

## Historique et favoris

L’application expose des espaces dédiés à l’historique et aux favoris pour faciliter la réutilisation des opérations et la lecture des résultats précédents.

## Actions détectées dans `fileflow-executor`

| Action ID | Catégorie |
| --- | --- |
| `archive` | Archive / compression |
| `archive-create` | Archive / compression |
| `archive-package` | Archive / compression |
| `audio-convert` | Média |
| `audio-gain` | Média |
| `audio-mono` | Média |
| `audio-normalize` | Média |
| `avif` | Autre |
| `bmp` | Autre |
| `collection-to-pdf` | PDF |
| `date` | Autre |
| `docx` | Autre |
| `ebook-convert` | Autre |
| `epub` | Autre |
| `extract-audio` | Média |
| `extract-metadata` | Autre |
| `ffmpeg` | Autre |
| `fill` | Autre |
| `ghostscript` | Autre |
| `gif` | Autre |
| `html` | Autre |
| `image-adjust` | Image |
| `image-auto-enhance` | Image |
| `image-auto-gamma` | Image |
| `image-auto-orient` | Image |
| `image-batch-convert` | Image |
| `image-blur` | Image |
| `image-border` | Image |
| `image-canvas` | Image |
| `image-colorspace-srgb` | Image |
| `image-contrast-stretch` | Image |
| `image-convert` | Image |
| `image-crop-center` | Image |
| `image-crop-custom` | Image |
| `image-flatten` | Image |
| `image-flip-horizontal` | Image |
| `image-flip-vertical` | Image |
| `image-grayscale` | Image |
| `image-noise-reduce` | Image |
| `image-optimize` | Image |
| `image-perspective` | Image |
| `image-pixelate` | Image |
| `image-posterize` | Image |
| `image-resize` | Image |
| `image-resize-exact` | Image |
| `image-rotate` | Image |
| `image-rotate-180` | Image |
| `image-rotate-left` | Image |
| `image-rotate-right` | Image |
| `image-sepia` | Image |
| `image-set-dpi` | Image |
| `image-sharpen` | Image |
| `image-threshold` | Image |
| `image-trim` | Image |
| `image-vignette` | Image |
| `image-watermark` | Image |
| `imagemagick` | Image |
| `images-to-pdf` | PDF |
| `img2pdf` | PDF |
| `jpeg` | Autre |
| `jpg` | Autre |
| `left` | Autre |
| `lz4` | Archive / compression |
| `lz4-compress` | Archive / compression |
| `media-compatible` | Média |
| `media-compress` | Média |
| `media-trim` | Média |
| `ocr` | OCR |
| `ocr-image` | Image |
| `odt` | Autre |
| `office` | Document |
| `office-convert` | Document |
| `office-to-pdf` | PDF |
| `pandoc` | Autre |
| `pdf` | PDF |
| `pdf-compress` | PDF |
| `pdf-extract-text` | PDF |
| `pdf-flatten-annotations` | PDF |
| `pdf-flatten-rotation` | PDF |
| `pdf-linearize` | PDF |
| `pdf-merge` | PDF |
| `pdf-ocr` | PDF |
| `pdf-optimize-lossless` | PDF |
| `pdf-protect` | PDF |
| `pdf-repair` | PDF |
| `pdf-rotate-pages` | PDF |
| `pdf-select-pages` | PDF |
| `png` | Autre |
| `poppler` | Autre |
| `qpdf` | PDF |
| `rtf` | Autre |
| `selection` | Autre |
| `smart-to-pdf` | PDF |
| `stretch` | Autre |
| `strip-metadata` | Autre |
| `tar-lz4-create` | Archive / compression |
| `tar-zstd-create` | Archive / compression |
| `tesseract` | Autre |
| `text` | Autre |
| `text-convert` | Autre |
| `text-to-pdf` | PDF |
| `tif` | Autre |
| `tiff` | Autre |
| `txt` | Autre |
| `video-convert` | Média |
| `video-mute` | Média |
| `video-resize` | Média |
| `video-rotate` | Média |
| `video-thumbnail` | Média |
| `video-to-gif` | Média |
| `vips` | Autre |
| `webp` | Autre |
| `zip` | Autre |
| `zstd` | Archive / compression |
| `zstd-compress` | Archive / compression |

> Le tableau est produit automatiquement à partir des branches d’exécution présentes dans le code au moment de la génération.
