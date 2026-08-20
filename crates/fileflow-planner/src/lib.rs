//! Capability catalog, recommendations and conversion-path planning.
//!
//! This crate intentionally contains no Tauri/UI knowledge. It describes what
//! FileFlow can do, which engine is needed and how a conversion can be routed
//! through intermediate formats when there is no direct edge.

use fileflow_domain::{
    ActionDescriptor, ActionRecommendation, ActionScope, FormatFamily, OperationCategory,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionEdge {
    pub from: String,
    pub to: String,
    pub engine_id: String,
    pub cost: u16,
    pub lossy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionStep {
    pub from: String,
    pub to: String,
    pub engine_id: String,
    pub lossy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionPlan {
    pub input: String,
    pub output: String,
    pub total_cost: u16,
    pub steps: Vec<ConversionStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCatalog {
    pub actions: Vec<ActionDescriptor>,
    pub conversions: Vec<ConversionEdge>,
}

impl Default for CapabilityCatalog {
    fn default() -> Self {
        Self {
            actions: default_actions(),
            conversions: default_conversion_edges(),
        }
    }
}

impl CapabilityCatalog {
    pub fn action(&self, id: &str) -> Option<&ActionDescriptor> {
        self.actions.iter().find(|action| action.id == id)
    }

    pub fn actions_for_family(&self, family: FormatFamily) -> Vec<&ActionDescriptor> {
        self.actions
            .iter()
            .filter(|action| action.accepts.is_empty() || action.accepts.contains(&family))
            .collect()
    }

    pub fn conversion_plan(&self, input: &str, output: &str) -> Option<ConversionPlan> {
        if input.eq_ignore_ascii_case(output) {
            return Some(ConversionPlan {
                input: input.to_ascii_lowercase(),
                output: output.to_ascii_lowercase(),
                total_cost: 0,
                steps: Vec::new(),
            });
        }

        let input = input.to_ascii_lowercase();
        let output = output.to_ascii_lowercase();
        let mut distances: HashMap<String, u16> = HashMap::new();
        let mut previous: HashMap<String, (String, usize)> = HashMap::new();
        let mut queue = BinaryHeap::new();

        distances.insert(input.clone(), 0);
        queue.push((Reverse(0_u16), input.clone()));

        while let Some((Reverse(cost), current)) = queue.pop() {
            if current == output {
                break;
            }
            if distances.get(&current).is_some_and(|known| cost > *known) {
                continue;
            }

            for (edge_index, edge) in self
                .conversions
                .iter()
                .enumerate()
                .filter(|(_, edge)| edge.from == current)
            {
                let next_cost = cost.saturating_add(edge.cost);
                let should_update = distances
                    .get(&edge.to)
                    .is_none_or(|known| next_cost < *known);
                if should_update {
                    distances.insert(edge.to.clone(), next_cost);
                    previous.insert(edge.to.clone(), (current.clone(), edge_index));
                    queue.push((Reverse(next_cost), edge.to.clone()));
                }
            }
        }

        let total_cost = *distances.get(&output)?;
        let mut cursor = output.clone();
        let mut steps = Vec::new();
        while cursor != input {
            let (prior, edge_index) = previous.get(&cursor)?.clone();
            let edge = &self.conversions[edge_index];
            steps.push(ConversionStep {
                from: edge.from.clone(),
                to: edge.to.clone(),
                engine_id: edge.engine_id.clone(),
                lossy: edge.lossy,
            });
            cursor = prior;
        }
        steps.reverse();

        Some(ConversionPlan {
            input,
            output,
            total_cost,
            steps,
        })
    }

    pub fn recommendations(
        &self,
        family_counts: &HashMap<FormatFamily, u64>,
        available_engines: &HashSet<String>,
    ) -> Vec<ActionRecommendation> {
        let mut recommendations = Vec::new();

        let mut push = |id: &str, score: u16, reason: &str, affected_assets: u64| {
            let Some(action) = self.action(id) else {
                return;
            };
            if affected_assets == 0 {
                return;
            }
            let missing_engines = action
                .required_engines
                .iter()
                .filter(|engine| !available_engines.contains(engine.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            recommendations.push(ActionRecommendation {
                action_id: action.id.clone(),
                score,
                reason: reason.into(),
                affected_assets,
                ready: missing_engines.is_empty(),
                missing_engines,
            });
        };

        let count = |family| family_counts.get(&family).copied().unwrap_or(0);
        let images = count(FormatFamily::Image);
        let pdfs = count(FormatFamily::Pdf);
        let documents = count(FormatFamily::Document)
            + count(FormatFamily::Spreadsheet)
            + count(FormatFamily::Presentation);
        let media = count(FormatFamily::Audio) + count(FormatFamily::Video);
        let archives = count(FormatFamily::Archive);

        if images > 1 {
            push(
                "images-to-pdf",
                100,
                "Assembler ces images dans un PDF prêt à partager.",
                images,
            );
            push(
                "image-batch-convert",
                94,
                "Uniformiser rapidement les formats de toutes les images.",
                images,
            );
        } else if images == 1 {
            push(
                "image-convert",
                92,
                "Convertir l’image vers un format plus compatible.",
                images,
            );
        }

        if images > 0 {
            push(
                "strip-metadata",
                81,
                "Retirer GPS et métadonnées avant un partage.",
                images,
            );
            push(
                "image-optimize",
                88,
                "Réduire le poids des images sans compliquer les réglages.",
                images,
            );
        }

        if pdfs > 1 {
            push(
                "pdf-merge",
                99,
                "Plusieurs PDF sont présents : ils peuvent être réunis.",
                pdfs,
            );
        }
        if pdfs > 0 {
            push(
                "pdf-compress",
                89,
                "Alléger les PDF pour l’envoi et l’archivage.",
                pdfs,
            );
            push(
                "pdf-ocr",
                78,
                "Rendre les scans PDF recherchables et sélectionnables.",
                pdfs,
            );
        }

        if documents > 0 {
            push(
                "office-to-pdf",
                96,
                "Créer des PDF faciles à ouvrir et imprimer.",
                documents,
            );
        }

        if media > 0 {
            push(
                "media-compatible",
                90,
                "Créer des fichiers médias compatibles avec la plupart des appareils.",
                media,
            );
            push(
                "media-compress",
                84,
                "Réduire la taille des médias pour les partager plus facilement.",
                media,
            );
        }

        if archives > 0 {
            push(
                "archive-extract",
                98,
                "Examiner ou extraire le contenu des archives.",
                archives,
            );
        }

        let total: u64 = family_counts.values().sum();
        if total >= 8 {
            push(
                "organize-by-type",
                86,
                "Classer automatiquement cette sélection par type de fichier.",
                total,
            );
            push(
                "batch-rename",
                72,
                "Renommer proprement plusieurs éléments en une seule opération.",
                total,
            );
        }

        recommendations.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.affected_assets.cmp(&a.affected_assets))
        });
        recommendations.truncate(10);
        recommendations
    }
}

// Declarative catalog factory: parameters mirror ActionDescriptor fields.
#[allow(clippy::too_many_arguments)]
fn action(
    id: &str,
    title: &str,
    description: &str,
    category: OperationCategory,
    accepts: &[FormatFamily],
    required_engines: &[&str],
    output_format: Option<&str>,
    batchable: bool,
    destructive: bool,
    featured: bool,
) -> ActionDescriptor {
    ActionDescriptor {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        category,
        scopes: if batchable {
            vec![ActionScope::Single, ActionScope::Batch]
        } else {
            vec![ActionScope::Single]
        },
        accepts: accepts.to_vec(),
        output_format: output_format.map(str::to_owned),
        required_engines: required_engines
            .iter()
            .map(|value| (*value).into())
            .collect(),
        batchable,
        destructive,
        featured,
    }
}

fn default_actions() -> Vec<ActionDescriptor> {
    use FormatFamily::*;

    vec![
        action(
            "images-to-pdf",
            "Images vers PDF",
            "Assembler plusieurs images dans un seul PDF.",
            OperationCategory::Pdf,
            &[Image],
            &["img2pdf"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "office-to-pdf",
            "Document vers PDF",
            "Créer un PDF à partir de Word, Excel, PowerPoint ou OpenDocument.",
            OperationCategory::Pdf,
            &[Document, Spreadsheet, Presentation],
            &["office"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "text-to-pdf",
            "Texte vers PDF",
            "Transformer texte, Markdown ou HTML en PDF.",
            OperationCategory::Pdf,
            &[Text],
            &["pandoc"],
            Some("pdf"),
            true,
            false,
            false,
        ),
        action(
            "pdf-merge",
            "Fusionner des PDF",
            "Réunir plusieurs PDF dans l’ordre choisi.",
            OperationCategory::Pdf,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "pdf-split",
            "Séparer un PDF",
            "Extraire des pages ou découper un document en plusieurs PDF.",
            OperationCategory::Pdf,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            false,
            false,
            true,
        ),
        action(
            "pdf-reorder",
            "Réorganiser les pages",
            "Déplacer, supprimer et faire pivoter des pages.",
            OperationCategory::Pdf,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            false,
            false,
            false,
        ),
        action(
            "pdf-compress",
            "Compresser un PDF",
            "Réduire le poids avec des profils simples et sûrs.",
            OperationCategory::Optimize,
            &[Pdf],
            &["ghostscript"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "pdf-ocr",
            "OCR d’un PDF",
            "Rendre un scan recherchable et sélectionnable.",
            OperationCategory::Extract,
            &[Pdf],
            &["ocr"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "pdf-to-images",
            "PDF vers images",
            "Exporter chaque page sous forme d’image.",
            OperationCategory::Convert,
            &[Pdf],
            &["poppler"],
            Some("png"),
            true,
            false,
            false,
        ),
        action(
            "pdf-extract-text",
            "Extraire le texte",
            "Récupérer le texte contenu dans un PDF.",
            OperationCategory::Extract,
            &[Pdf],
            &["poppler"],
            Some("txt"),
            true,
            false,
            true,
        ),
        action(
            "image-convert",
            "Convertir une image",
            "JPG, PNG, WebP, HEIC, AVIF, TIFF et autres formats.",
            OperationCategory::Image,
            &[Image],
            &["vips"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-batch-convert",
            "Convertir toutes les images",
            "Uniformiser un lot entier en un seul format.",
            OperationCategory::Image,
            &[Image],
            &["vips"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-optimize",
            "Optimiser les images",
            "Réduire dimensions et poids avec un profil d’usage.",
            OperationCategory::Optimize,
            &[Image],
            &["vips"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-resize",
            "Redimensionner",
            "Changer les dimensions sans modifier les originaux.",
            OperationCategory::Image,
            &[Image],
            &["vips"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-rotate",
            "Rotation et orientation",
            "Corriger l’orientation ou faire pivoter un lot.",
            OperationCategory::Image,
            &[Image],
            &["vips"],
            None,
            true,
            false,
            false,
        ),
        action(
            "strip-metadata",
            "Retirer les données privées",
            "Supprimer GPS, appareil et métadonnées inutiles.",
            OperationCategory::Privacy,
            &[Image, Pdf, Audio, Video],
            &["metadata"],
            None,
            true,
            true,
            true,
        ),
        action(
            "extract-metadata",
            "Voir les métadonnées",
            "Inspecter EXIF, dimensions, codecs et informations techniques.",
            OperationCategory::Extract,
            &[Image, Pdf, Audio, Video],
            &["metadata"],
            None,
            true,
            false,
            false,
        ),
        action(
            "archive-extract",
            "Extraire une archive",
            "ZIP, 7Z, RAR, TAR, GZ et formats associés.",
            OperationCategory::Archive,
            &[Archive],
            &["archive"],
            None,
            true,
            false,
            true,
        ),
        action(
            "archive-create",
            "Créer une archive",
            "Créer un ZIP ou 7Z à partir de fichiers et dossiers.",
            OperationCategory::Archive,
            &[],
            &["archive"],
            Some("zip"),
            true,
            false,
            true,
        ),
        action(
            "archive-repack",
            "Recompresser une archive",
            "Changer de format ou de niveau de compression.",
            OperationCategory::Archive,
            &[Archive],
            &["archive"],
            None,
            true,
            false,
            false,
        ),
        action(
            "tar-zstd-create",
            "Dossier ou lot → TAR.ZST",
            "Regrouper plusieurs éléments puis les compresser avec Zstandard en une seule action.",
            OperationCategory::Archive,
            &[],
            &["archive", "zstd"],
            Some("tar.zst"),
            true,
            false,
            true,
        ),
        action(
            "tar-lz4-create",
            "Dossier ou lot → TAR.LZ4",
            "Regrouper plusieurs éléments puis privilégier une compression/décompression extrêmement rapide avec LZ4.",
            OperationCategory::Archive,
            &[],
            &["archive", "lz4"],
            Some("tar.lz4"),
            true,
            false,
            true,
        ),
        action(
            "zstd-compress",
            "Compresser avec Zstandard",
            "Créer un fichier .zst très rapidement, idéal pour les gros fichiers et les sauvegardes.",
            OperationCategory::Optimize,
            &[
                Image,
                Pdf,
                Document,
                Spreadsheet,
                Presentation,
                Audio,
                Video,
                Archive,
                Ebook,
                Text,
                Unknown,
            ],
            &["zstd"],
            Some("zst"),
            true,
            false,
            true,
        ),
        action(
            "zstd-decompress",
            "Décompresser Zstandard",
            "Décompresser un fichier .zst ou .zstd en conservant l’original.",
            OperationCategory::Archive,
            &[Archive],
            &["zstd"],
            None,
            true,
            false,
            true,
        ),
        action(
            "lz4-compress",
            "Compresser très vite avec LZ4",
            "Créer un fichier .lz4 avec un algorithme lossless conçu pour la vitesse maximale.",
            OperationCategory::Optimize,
            &[
                Image,
                Pdf,
                Document,
                Spreadsheet,
                Presentation,
                Audio,
                Video,
                Archive,
                Ebook,
                Text,
                Unknown,
            ],
            &["lz4"],
            Some("lz4"),
            true,
            false,
            true,
        ),
        action(
            "lz4-decompress",
            "Décompresser LZ4",
            "Restaurer un fichier .lz4 très rapidement sans supprimer l’archive d’origine.",
            OperationCategory::Archive,
            &[Archive],
            &["lz4"],
            None,
            true,
            false,
            true,
        ),
        action(
            "media-compatible",
            "Rendre compatible",
            "Transcoder vers des formats faciles à lire sur téléphone, TV et web.",
            OperationCategory::Media,
            &[Audio, Video],
            &["ffmpeg"],
            None,
            true,
            false,
            true,
        ),
        action(
            "video-convert",
            "Convertir une vidéo",
            "Convertir vers MP4, WebM, MKV ou MOV avec un profil compatible.",
            OperationCategory::Convert,
            &[Video],
            &["ffmpeg"],
            None,
            true,
            false,
            true,
        ),
        action(
            "media-compress",
            "Compresser un média",
            "Réduire la taille d’une vidéo ou d’un fichier audio.",
            OperationCategory::Optimize,
            &[Audio, Video],
            &["ffmpeg"],
            None,
            true,
            false,
            true,
        ),
        action(
            "extract-audio",
            "Extraire l’audio",
            "Créer un fichier audio à partir d’une vidéo.",
            OperationCategory::Extract,
            &[Video],
            &["ffmpeg"],
            Some("m4a"),
            true,
            false,
            false,
        ),
        action(
            "video-to-gif",
            "Créer un GIF",
            "Créer une animation courte à partir d’une vidéo.",
            OperationCategory::Convert,
            &[Video],
            &["ffmpeg"],
            Some("gif"),
            true,
            false,
            false,
        ),
        action(
            "audio-convert",
            "Convertir l’audio",
            "MP3, M4A, WAV, FLAC, OGG et Opus.",
            OperationCategory::Convert,
            &[Audio],
            &["ffmpeg"],
            None,
            true,
            false,
            false,
        ),
        action(
            "text-convert",
            "Convertir un texte ou document léger",
            "Convertir Markdown, HTML, RST et texte vers HTML, Markdown, DOCX ou EPUB.",
            OperationCategory::Convert,
            &[Text],
            &["pandoc"],
            None,
            true,
            false,
            false,
        ),
        action(
            "ebook-convert",
            "Convertir un livre",
            "Convertir EPUB ou FB2 vers un format de lecture ou de document courant.",
            OperationCategory::Convert,
            &[Ebook],
            &["pandoc"],
            None,
            true,
            false,
            false,
        ),
        action(
            "ocr-image",
            "Lire un document photographié",
            "Extraire le texte d’une image ou d’un scan.",
            OperationCategory::Extract,
            &[Image],
            &["tesseract"],
            Some("txt"),
            true,
            false,
            true,
        ),
        action(
            "batch-rename",
            "Renommer en masse",
            "Aperçu avant/après avec numéro, date et modèle de nom.",
            OperationCategory::Organize,
            &[],
            &[],
            None,
            true,
            true,
            true,
        ),
        action(
            "organize-by-type",
            "Classer par type",
            "Créer automatiquement Images, PDF, Documents, Vidéos et autres dossiers.",
            OperationCategory::Organize,
            &[],
            &[],
            None,
            true,
            true,
            true,
        ),
        action(
            "duplicate-scan",
            "Trouver les doublons",
            "Repérer les fichiers identiques sans supprimer automatiquement.",
            OperationCategory::Organize,
            &[],
            &[],
            None,
            true,
            false,
            true,
        ),
    ]
}

fn edge(from: &str, to: &str, engine: &str, cost: u16, lossy: bool) -> ConversionEdge {
    ConversionEdge {
        from: from.into(),
        to: to.into(),
        engine_id: engine.into(),
        cost,
        lossy,
    }
}

fn default_conversion_edges() -> Vec<ConversionEdge> {
    let mut edges = Vec::new();

    for from in [
        "jpeg", "png", "webp", "heic", "heif", "avif", "jxl", "tiff", "bmp", "gif", "raw",
    ] {
        for to in ["jpeg", "png", "webp", "avif", "jxl", "tiff"] {
            if from != to {
                edges.push(edge(
                    from,
                    to,
                    "vips",
                    if to == "jpeg" || to == "webp" { 2 } else { 1 },
                    to == "jpeg",
                ));
            }
        }
        edges.push(edge(from, "pdf", "vips", 2, false));
    }

    for from in [
        "doc", "docx", "odt", "rtf", "pages", "wpd", "xls", "xlsx", "ods", "numbers", "ppt",
        "pptx", "odp", "keynote",
    ] {
        edges.push(edge(from, "pdf", "office", 1, false));
    }

    for from in ["txt", "text", "md", "html", "rst", "markdown"] {
        edges.push(edge(from, "html", "pandoc", 1, false));
        edges.push(edge(from, "md", "pandoc", 1, false));
        edges.push(edge(from, "docx", "pandoc", 2, false));
        edges.push(edge(from, "epub", "pandoc", 2, false));
    }

    edges.extend([
        edge("pdf", "png", "poppler", 2, true),
        edge("pdf", "jpeg", "poppler", 3, true),
        edge("mov", "mp4", "ffmpeg", 2, true),
        edge("mkv", "mp4", "ffmpeg", 2, true),
        edge("avi", "mp4", "ffmpeg", 3, true),
        edge("webm", "mp4", "ffmpeg", 3, true),
        edge("mpeg", "mp4", "ffmpeg", 3, true),
        edge("wmv", "mp4", "ffmpeg", 3, true),
        edge("flv", "mp4", "ffmpeg", 3, true),
        edge("mpeg-ts", "mp4", "ffmpeg", 3, true),
        edge("mp4", "webm", "ffmpeg", 3, true),
        edge("mp4", "mkv", "ffmpeg", 2, false),
        edge("mov", "mkv", "ffmpeg", 2, false),
        edge("wav", "mp3", "ffmpeg", 2, true),
        edge("flac", "mp3", "ffmpeg", 2, true),
        edge("m4a", "mp3", "ffmpeg", 2, true),
        edge("ogg", "mp3", "ffmpeg", 2, true),
        edge("mp3", "wav", "ffmpeg", 2, false),
        edge("flac", "wav", "ffmpeg", 1, false),
        edge("aac", "mp3", "ffmpeg", 2, true),
        edge("aiff", "flac", "ffmpeg", 1, false),
        edge("wav", "flac", "ffmpeg", 1, false),
        edge("ape", "flac", "ffmpeg", 2, false),
    ]);

    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_multi_step_conversion_path() {
        let catalog = CapabilityCatalog {
            actions: Vec::new(),
            conversions: vec![
                edge("docx", "pdf", "office", 1, false),
                edge("pdf", "png", "poppler", 2, true),
            ],
        };

        let plan = catalog.conversion_plan("docx", "png").unwrap();
        assert_eq!(plan.total_cost, 3);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].engine_id, "office");
        assert_eq!(plan.steps[1].engine_id, "poppler");
    }

    #[test]
    fn exposes_fast_lossless_archive_workflows() {
        let catalog = CapabilityCatalog::default();
        let zstd = catalog.action("tar-zstd-create").expect("TAR.ZST action");
        let lz4 = catalog.action("tar-lz4-create").expect("TAR.LZ4 action");
        assert_eq!(zstd.output_format.as_deref(), Some("tar.zst"));
        assert_eq!(lz4.output_format.as_deref(), Some("tar.lz4"));
        assert_eq!(
            zstd.required_engines,
            vec!["archive".to_string(), "zstd".to_string()]
        );
        assert_eq!(
            lz4.required_engines,
            vec!["archive".to_string(), "lz4".to_string()]
        );
    }

    #[test]
    fn recommends_pdf_merge_for_multiple_pdfs() {
        let catalog = CapabilityCatalog::default();
        let counts = HashMap::from([(FormatFamily::Pdf, 3)]);
        let engines = HashSet::from([
            "qpdf".to_string(),
            "ghostscript".to_string(),
            "ocr".to_string(),
        ]);

        let recommendations = catalog.recommendations(&counts, &engines);
        assert!(
            recommendations
                .iter()
                .any(|item| item.action_id == "pdf-merge" && item.ready)
        );
    }
}
