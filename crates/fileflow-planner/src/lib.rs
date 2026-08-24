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
    pub lossy_steps: usize,
    pub intermediates: Vec<String>,
    pub steps: Vec<ConversionStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCapabilityProfile {
    pub id: String,
    pub label: String,
    pub family: FormatFamily,
    pub extensions: Vec<String>,
    pub preview: bool,
    pub readable: bool,
    pub writable: bool,
    pub metadata: bool,
    pub thumbnail: bool,
    pub extractable: bool,
    pub streamable: bool,
    pub capabilities: Vec<String>,
    pub actions: Vec<String>,
    pub convert_to: Vec<String>,
    pub compress_to: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionUiKind {
    Conversion,
    Image,
    Pdf,
    Media,
    Archive,
    Extract,
    Organization,
    Privacy,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionInputMode {
    Files,
    Directories,
    FilesOrDirectories,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionParameterKind {
    Select,
    Number,
    Range,
    Toggle,
    Text,
    Password,
    Color,
    Time,
    PageRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionParameterOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionParameterDescriptor {
    pub key: String,
    pub label: String,
    pub description: String,
    pub kind: ActionParameterKind,
    pub default_value: Option<String>,
    pub minimum: Option<String>,
    pub maximum: Option<String>,
    pub step: Option<String>,
    pub required: bool,
    pub advanced: bool,
    pub options: Vec<ActionParameterOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionUiSpec {
    pub action_id: String,
    pub kind: ActionUiKind,
    pub input_mode: ActionInputMode,
    pub source_formats: Vec<String>,
    pub target_formats: Vec<String>,
    pub default_target: Option<String>,
    pub parameters: Vec<ActionParameterDescriptor>,
    pub supports_preview: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCatalog {
    pub actions: Vec<ActionDescriptor>,
    pub conversions: Vec<ConversionEdge>,
    pub formats: Vec<FormatCapabilityProfile>,
    pub action_ui: Vec<ActionUiSpec>,
}

impl Default for CapabilityCatalog {
    fn default() -> Self {
        let actions = default_actions();
        let formats = default_format_capabilities();
        Self {
            action_ui: default_action_ui(&actions, &formats),
            actions,
            conversions: default_conversion_edges(),
            formats,
        }
    }
}

impl CapabilityCatalog {
    pub fn action(&self, id: &str) -> Option<&ActionDescriptor> {
        self.actions.iter().find(|action| action.id == id)
    }

    pub fn action_ui(&self, id: &str) -> Option<&ActionUiSpec> {
        self.action_ui.iter().find(|spec| spec.action_id == id)
    }

    pub fn format(&self, id: &str) -> Option<&FormatCapabilityProfile> {
        let normalized = id.to_ascii_lowercase();
        self.formats.iter().find(|profile| {
            profile.id == normalized
                || profile
                    .extensions
                    .iter()
                    .any(|extension| extension == &normalized)
        })
    }

    pub fn actions_for_family(&self, family: FormatFamily) -> Vec<&ActionDescriptor> {
        self.actions
            .iter()
            .filter(|action| action.accepts.is_empty() || action.accepts.contains(&family))
            .collect()
    }

    pub fn conversion_plan(&self, input: &str, output: &str) -> Option<ConversionPlan> {
        self.conversion_plan_filtered(input, output, |_| true)
    }

    pub fn conversion_plan_with_engines(
        &self,
        input: &str,
        output: &str,
        available_engines: &HashSet<String>,
    ) -> Option<ConversionPlan> {
        self.conversion_plan_filtered(input, output, |edge| {
            available_engines.contains(&edge.engine_id)
        })
    }

    fn conversion_plan_filtered<F>(
        &self,
        input: &str,
        output: &str,
        allowed: F,
    ) -> Option<ConversionPlan>
    where
        F: Fn(&ConversionEdge) -> bool,
    {
        if input.eq_ignore_ascii_case(output) {
            return Some(ConversionPlan {
                input: input.to_ascii_lowercase(),
                output: output.to_ascii_lowercase(),
                total_cost: 0,
                lossy_steps: 0,
                intermediates: Vec::new(),
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
                .filter(|(_, edge)| edge.from == current && allowed(edge))
            {
                // FileFlow prefers fidelity over a superficially shorter path. A
                // lossy intermediate is deliberately expensive, and every extra
                // intermediate adds a small stability/latency cost.
                let fidelity_penalty = if edge.lossy { 24 } else { 0 };
                let intermediate_penalty = if edge.to == output { 0 } else { 2 };
                let next_cost = cost
                    .saturating_add(edge.cost)
                    .saturating_add(fidelity_penalty)
                    .saturating_add(intermediate_penalty);
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
        let lossy_steps = steps.iter().filter(|step| step.lossy).count();
        let intermediates = steps
            .iter()
            .take(steps.len().saturating_sub(1))
            .map(|step| step.to.clone())
            .collect();

        Some(ConversionPlan {
            input,
            output,
            total_cost,
            lossy_steps,
            intermediates,
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

fn parameter(
    key: &str,
    label: &str,
    description: &str,
    kind: ActionParameterKind,
    default_value: Option<&str>,
    bounds: Option<(&str, &str, &str)>,
) -> ActionParameterDescriptor {
    let (minimum, maximum, step) = bounds.unwrap_or(("", "", ""));
    ActionParameterDescriptor {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind,
        default_value: default_value.map(str::to_owned),
        minimum: (!minimum.is_empty()).then(|| minimum.into()),
        maximum: (!maximum.is_empty()).then(|| maximum.into()),
        step: (!step.is_empty()).then(|| step.into()),
        required: false,
        advanced: false,
        options: Vec::new(),
    }
}

fn select_parameter(
    key: &str,
    label: &str,
    description: &str,
    default_value: &str,
    values: &[(&str, &str)],
) -> ActionParameterDescriptor {
    ActionParameterDescriptor {
        options: values
            .iter()
            .map(|(value, label)| ActionParameterOption {
                value: (*value).into(),
                label: (*label).into(),
            })
            .collect(),
        ..parameter(
            key,
            label,
            description,
            ActionParameterKind::Select,
            Some(default_value),
            None,
        )
    }
}

fn action_parameters(id: &str) -> Vec<ActionParameterDescriptor> {
    use ActionParameterKind::*;
    let number = |key, label, default, min, max, step| {
        parameter(
            key,
            label,
            "Le moteur valide cette valeur avant le traitement.",
            Number,
            Some(default),
            Some((min, max, step)),
        )
    };
    let range = |key, label, default, min, max, step| {
        parameter(
            key,
            label,
            "Ajustement appliqué sans modifier le fichier original.",
            Range,
            Some(default),
            Some((min, max, step)),
        )
    };

    match id {
        "image-rotate" => vec![range("angle", "Angle", "90", "-360", "360", "1")],
        "image-sepia" => vec![range("strength", "Intensité", "80", "0", "100", "1")],
        "image-adjust" => vec![
            range("brightness", "Luminosité", "0", "-100", "100", "1"),
            range("contrast", "Contraste", "0", "-100", "100", "1"),
            range("saturation", "Saturation", "100", "0", "200", "1"),
            range("gamma", "Gamma", "1", "0.1", "5", "0.1"),
        ],
        "image-sharpen" => vec![range("amount", "Netteté", "1", "0.1", "5", "0.1")],
        "image-blur" => vec![range("radius", "Rayon", "2", "0.1", "20", "0.1")],
        "image-threshold" => vec![range("threshold", "Seuil", "50", "0", "100", "1")],
        "image-posterize" => vec![range("levels", "Niveaux", "6", "2", "32", "1")],
        "image-pixelate" => vec![range(
            "pixelPercent",
            "Taille des pixels",
            "8",
            "1",
            "50",
            "1",
        )],
        "image-flatten" => vec![parameter(
            "background",
            "Arrière-plan",
            "Couleur utilisée pour remplacer la transparence.",
            Color,
            Some("#ffffff"),
            None,
        )],
        "image-crop-center" => vec![
            number("width", "Largeur", "1200", "1", "20000", "1"),
            number("height", "Hauteur", "1200", "1", "20000", "1"),
        ],
        "image-resize-exact" => vec![
            number("width", "Largeur", "1920", "1", "20000", "1"),
            number("height", "Hauteur", "1080", "1", "20000", "1"),
            select_parameter(
                "fit",
                "Ajustement",
                "Conserver, remplir ou étirer l’image.",
                "contain",
                &[
                    ("contain", "Contenir"),
                    ("fill", "Remplir"),
                    ("stretch", "Étirer"),
                ],
            ),
        ],
        "image-crop-custom" => vec![
            number("width", "Largeur", "1200", "1", "20000", "1"),
            number("height", "Hauteur", "1200", "1", "20000", "1"),
            number("x", "Position X", "0", "0", "20000", "1"),
            number("y", "Position Y", "0", "0", "20000", "1"),
        ],
        "image-canvas" => vec![
            number("width", "Largeur", "1920", "1", "20000", "1"),
            number("height", "Hauteur", "1080", "1", "20000", "1"),
            parameter(
                "background",
                "Arrière-plan",
                "Couleur de la nouvelle toile.",
                Color,
                Some("#ffffff"),
                None,
            ),
        ],
        "image-contrast-stretch" => vec![
            range("blackPoint", "Point noir", "0.5", "0", "20", "0.1"),
            range("whitePoint", "Point blanc", "0.5", "0", "20", "0.1"),
        ],
        "image-set-dpi" => vec![number("dpi", "Résolution DPI", "300", "36", "2400", "1")],
        "image-perspective" => vec![
            number("x0", "Coin 1 · X", "0", "0", "20000", "1"),
            number("y0", "Coin 1 · Y", "0", "0", "20000", "1"),
            number("x1", "Coin 2 · X", "1200", "0", "20000", "1"),
            number("y1", "Coin 2 · Y", "0", "0", "20000", "1"),
            number("x2", "Coin 3 · X", "1200", "0", "20000", "1"),
            number("y2", "Coin 3 · Y", "1200", "0", "20000", "1"),
            number("x3", "Coin 4 · X", "0", "0", "20000", "1"),
            number("y3", "Coin 4 · Y", "1200", "0", "20000", "1"),
            number("width", "Largeur finale", "1200", "1", "20000", "1"),
            number("height", "Hauteur finale", "1200", "1", "20000", "1"),
        ],
        "image-border" => vec![
            number("pixels", "Épaisseur", "16", "1", "500", "1"),
            parameter(
                "color",
                "Couleur",
                "Couleur de la bordure.",
                Color,
                Some("#ffffff"),
                None,
            ),
        ],
        "image-vignette" => vec![range("radius", "Intensité", "12", "0", "100", "1")],
        "image-watermark" => vec![
            parameter(
                "text",
                "Texte",
                "Texte placé en bas à droite.",
                Text,
                Some("FileFlow"),
                None,
            ),
            number("fontSize", "Taille", "28", "8", "200", "1"),
        ],
        "pdf-rotate-pages" => vec![
            select_parameter(
                "angle",
                "Rotation",
                "Rotation appliquée aux pages sélectionnées.",
                "90",
                &[
                    ("90", "90°"),
                    ("180", "180°"),
                    ("270", "270°"),
                    ("-90", "-90°"),
                ],
            ),
            parameter(
                "pages",
                "Pages",
                "Exemples : 1,3-5 ou 1-z.",
                PageRange,
                Some("1-z"),
                None,
            ),
        ],
        "pdf-select-pages" => vec![parameter(
            "pages",
            "Pages à conserver",
            "Exemples : 1,3-5 ou 1-z.",
            PageRange,
            Some("1-z"),
            None,
        )],
        "pdf-protect" => vec![ActionParameterDescriptor {
            required: true,
            ..parameter(
                "password",
                "Mot de passe",
                "Le mot de passe n’est jamais mémorisé.",
                Password,
                None,
                None,
            )
        }],
        "video-rotate" => vec![select_parameter(
            "direction",
            "Rotation",
            "Orientation de la vidéo finale.",
            "right",
            &[
                ("right", "90° à droite"),
                ("left", "90° à gauche"),
                ("180", "180°"),
            ],
        )],
        "video-resize" => vec![
            number("width", "Largeur", "1920", "16", "7680", "2"),
            number("height", "Hauteur", "1080", "16", "4320", "2"),
        ],
        "video-thumbnail" => vec![parameter(
            "second",
            "Position",
            "Instant de la miniature dans la vidéo.",
            Time,
            Some("1"),
            Some(("0", "86400", "0.1")),
        )],
        "media-trim" => vec![
            parameter(
                "start",
                "Début",
                "Instant de début.",
                Time,
                Some("0"),
                Some(("0", "86400", "0.1")),
            ),
            parameter(
                "duration",
                "Durée",
                "Durée conservée.",
                Time,
                Some("30"),
                Some(("0.1", "86400", "0.1")),
            ),
        ],
        "audio-gain" => vec![range("gainDb", "Gain", "0", "-30", "30", "0.5")],
        "smart-to-pdf" | "collection-to-pdf" => vec![
            select_parameter(
                "finalCompression",
                "Compression finale",
                "Optimisation appliquée au PDF final.",
                "balanced",
                &[
                    ("keep", "Conserver"),
                    ("small", "Plus léger"),
                    ("balanced", "Équilibré"),
                    ("high", "Haute qualité"),
                ],
            ),
            number("targetSizeMb", "Taille cible (Mo)", "0", "0", "4096", "1"),
            parameter(
                "improve",
                "Améliorer les scans",
                "Redressement et OCR lorsque le moteur le permet.",
                Toggle,
                Some("false"),
                None,
            ),
            parameter(
                "stripMetadata",
                "Nettoyer les métadonnées",
                "Retire les informations privées avant le partage.",
                Toggle,
                Some("false"),
                None,
            ),
            parameter(
                "signatureText",
                "Signature visuelle",
                "Texte ajouté au document final.",
                Text,
                None,
                None,
            ),
        ],
        _ => Vec::new(),
    }
}

fn action_targets(action: &ActionDescriptor) -> Vec<String> {
    let targets: &[&str] = match action.id.as_str() {
        "image-convert" | "image-batch-convert" => {
            &["jpg", "png", "webp", "avif", "tiff", "bmp", "gif"]
        }
        "office-convert" => &[
            "pdf", "docx", "odt", "rtf", "txt", "html", "xlsx", "ods", "csv", "pptx", "odp",
        ],
        "audio-convert" => &["mp3", "m4a", "aac", "wav", "flac", "ogg", "opus"],
        "extract-audio" => &["m4a", "mp3", "aac", "wav", "flac", "ogg", "opus"],
        "video-convert" => &["mp4", "webm", "mkv", "mov"],
        "text-convert" => &["html", "md", "docx", "epub", "txt"],
        "ebook-convert" => &["html", "md", "docx", "txt", "epub"],
        "pdf-to-images" => &["png", "jpg"],
        "archive-create" => &["zip", "7z", "tar"],
        "archive-package" => &["smart", "tar.zst", "zip", "tar"],
        "tar-zstd-create" => &["tar.zst"],
        "zstd-compress" => &["zst"],
        _ => &[],
    };
    if targets.is_empty() {
        action.output_format.iter().cloned().collect()
    } else {
        targets.iter().map(|value| (*value).into()).collect()
    }
}

fn action_kind(action: &ActionDescriptor) -> ActionUiKind {
    if action.id.ends_with("-convert") {
        return ActionUiKind::Conversion;
    }

    match action.category {
        OperationCategory::Convert | OperationCategory::Document => ActionUiKind::Conversion,
        OperationCategory::Image => ActionUiKind::Image,
        OperationCategory::Pdf => ActionUiKind::Pdf,
        OperationCategory::Media => ActionUiKind::Media,
        OperationCategory::Archive => ActionUiKind::Archive,
        OperationCategory::Extract => ActionUiKind::Extract,
        OperationCategory::Organize => ActionUiKind::Organization,
        OperationCategory::Privacy => ActionUiKind::Privacy,
        OperationCategory::Optimize => {
            if action.accepts.contains(&FormatFamily::Image) {
                ActionUiKind::Image
            } else if action.accepts.contains(&FormatFamily::Pdf) {
                ActionUiKind::Pdf
            } else if action
                .accepts
                .iter()
                .any(|family| matches!(*family, FormatFamily::Audio | FormatFamily::Video))
            {
                ActionUiKind::Media
            } else {
                ActionUiKind::Generic
            }
        }
    }
}

fn default_action_ui(
    actions: &[ActionDescriptor],
    formats: &[FormatCapabilityProfile],
) -> Vec<ActionUiSpec> {
    actions
        .iter()
        .map(|action| {
            let mut source_formats = formats
                .iter()
                .filter(|format| {
                    action.accepts.is_empty() || action.accepts.contains(&format.family)
                })
                .flat_map(|format| format.extensions.iter().cloned())
                .collect::<Vec<_>>();
            source_formats.sort();
            source_formats.dedup();
            let input_mode = match action.id.as_str() {
                "tar-zstd-create" | "tar-lz4-create" | "archive-create" | "archive-package"
                | "collection-to-pdf" => ActionInputMode::FilesOrDirectories,
                "organize-by-type" | "duplicate-scan" => ActionInputMode::Directories,
                _ => ActionInputMode::Files,
            };
            let target_formats = action_targets(action);
            let default_target = action
                .output_format
                .clone()
                .or_else(|| target_formats.first().cloned());
            ActionUiSpec {
                action_id: action.id.clone(),
                kind: action_kind(action),
                input_mode,
                source_formats,
                target_formats,
                default_target,
                parameters: action_parameters(&action.id),
                supports_preview: matches!(
                    action_kind(action),
                    ActionUiKind::Image | ActionUiKind::Pdf | ActionUiKind::Media
                ),
            }
        })
        .collect()
}

fn default_actions() -> Vec<ActionDescriptor> {
    use FormatFamily::*;

    vec![
        action(
            "smart-to-pdf",
            "Créer un PDF",
            "Trouver automatiquement le meilleur chemin vers PDF, même en plusieurs étapes.",
            OperationCategory::Pdf,
            &[],
            &[],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "collection-to-pdf",
            "Faire un dossier PDF",
            "Regrouper fichiers, dossier ou ZIP dans un seul PDF ordonné.",
            OperationCategory::Pdf,
            &[],
            &[],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "pdf-protect",
            "Protéger un PDF",
            "Créer une copie protégée par mot de passe sans modifier l’original.",
            OperationCategory::Privacy,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
            false,
            true,
        ),
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
            "office-convert",
            "Convertir un document Office",
            "Passer entre formats éditables compatibles : DOCX/ODT/RTF, XLSX/ODS/CSV, PPTX/ODP ou PDF.",
            OperationCategory::Document,
            &[Document, Spreadsheet, Presentation],
            &["office"],
            None,
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
            &["pandoc", "office"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "html-to-pdf",
            "HTML vers PDF",
            "Rendre la page avec JavaScript dans un navigateur isolé, puis l’imprimer en PDF.",
            OperationCategory::Pdf,
            &[Text],
            &["browser"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "email-to-pdf",
            "E-mail EML vers PDF",
            "Créer une copie PDF lisible de l’e-mail, de ses en-têtes et de son contenu texte.",
            OperationCategory::Pdf,
            &[Text],
            &["browser"],
            Some("pdf"),
            true,
            false,
            true,
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
            "pdf-rotate-pages",
            "Tourner des pages PDF",
            "Faire pivoter tout le document ou une plage de pages sans rasteriser le PDF.",
            OperationCategory::Pdf,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "pdf-select-pages",
            "Extraire / réordonner des pages",
            "Créer un nouveau PDF à partir d’une plage comme 1-3,7,10-z.",
            OperationCategory::Pdf,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "pdf-linearize",
            "Optimiser pour le web",
            "Créer un PDF Fast Web View qui peut commencer à s’afficher avant la fin du téléchargement.",
            OperationCategory::Optimize,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
            false,
            false,
        ),
        action(
            "pdf-optimize-lossless",
            "Optimisation PDF sans perte",
            "Réécrire les flux et objets du PDF sans réduire la qualité des images.",
            OperationCategory::Optimize,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
            false,
            true,
        ),
        action(
            "pdf-repair",
            "Réécrire / réparer un PDF",
            "Réécrire la structure et les tables de références d’un PDF lisible mais endommagé.",
            OperationCategory::Pdf,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
            false,
            false,
        ),
        action(
            "pdf-flatten-rotation",
            "Aplatir les rotations",
            "Appliquer physiquement les rotations de pages pour améliorer la compatibilité.",
            OperationCategory::Pdf,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
            false,
            false,
        ),
        action(
            "pdf-flatten-annotations",
            "Aplatir les annotations",
            "Intégrer les annotations visibles dans le contenu du PDF pour le partage et l’impression.",
            OperationCategory::Pdf,
            &[Pdf],
            &["qpdf"],
            Some("pdf"),
            true,
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
            "image-rotate-left",
            "Tourner à gauche",
            "Faire pivoter l’image de 90° vers la gauche avec aperçu avant traitement.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-rotate-right",
            "Tourner à droite",
            "Faire pivoter l’image de 90° vers la droite avec aperçu avant traitement.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-rotate-180",
            "Rotation 180°",
            "Retourner l’image sans modifier l’original.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-rotate",
            "Rotation libre",
            "Choisir précisément l’angle de rotation et prévisualiser le résultat.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-flip-horizontal",
            "Miroir horizontal",
            "Inverser l’image de gauche à droite.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-flip-vertical",
            "Miroir vertical",
            "Inverser l’image de haut en bas.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-auto-orient",
            "Corriger l’orientation EXIF",
            "Appliquer automatiquement l’orientation enregistrée par l’appareil photo.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-auto-enhance",
            "Amélioration automatique",
            "Corriger orientation, niveaux et contraste avec un profil prudent.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-adjust",
            "Lumière, contraste et couleurs",
            "Ajuster luminosité, contraste, saturation et gamma avec aperçu.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-grayscale",
            "Noir et blanc",
            "Créer une version en niveaux de gris.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-sepia",
            "Sépia",
            "Appliquer un rendu sépia réglable.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-sharpen",
            "Renforcer la netteté",
            "Accentuer les détails avec une intensité bornée.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-blur",
            "Flouter",
            "Appliquer un flou réglable à une image ou un lot.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-noise-reduce",
            "Réduire le bruit",
            "Nettoyer légèrement le bruit et les petits artefacts.",
            OperationCategory::Optimize,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-threshold",
            "Seuil noir / blanc",
            "Transformer un scan ou visuel en noir/blanc selon un seuil réglable.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-posterize",
            "Réduire les niveaux de couleur",
            "Créer un rendu postérisé en contrôlant le nombre de niveaux.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-pixelate",
            "Pixelliser",
            "Masquer ou styliser une image avec une pixellisation réglable.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-flatten",
            "Aplatir la transparence",
            "Remplacer la transparence par un fond choisi pour une meilleure compatibilité.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-trim",
            "Retirer les bordures",
            "Recadrer automatiquement les marges uniformes autour d’un scan ou visuel.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-crop-center",
            "Recadrage centré",
            "Recadrer au centre avec largeur et hauteur exactes.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-resize-exact",
            "Dimensions précises",
            "Choisir largeur, hauteur et comportement : contenir, remplir ou étirer.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-crop-custom",
            "Recadrage libre",
            "Définir précisément la zone de recadrage X/Y/largeur/hauteur avec aperçu avant application.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-canvas",
            "Agrandir la zone de travail",
            "Créer un canevas exact, centrer l’image et choisir la couleur de fond.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-auto-gamma",
            "Gamma automatique",
            "Corriger automatiquement une image trop sombre ou trop claire avec une transformation prudente.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-contrast-stretch",
            "Étendre le contraste",
            "Récupérer du contraste sur des scans ou photos ternes avec un seuil de noirs/blancs réglable.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-colorspace-srgb",
            "Normaliser en sRGB",
            "Convertir l’espace colorimétrique vers sRGB pour le web, les écrans et le partage.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-set-dpi",
            "Changer la résolution DPI",
            "Définir la densité d’impression sans redimensionner les pixels.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-perspective",
            "Corriger la perspective",
            "Redresser un document photographié en définissant les quatre coins de la zone utile.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
        ),
        action(
            "image-border",
            "Ajouter une bordure",
            "Ajouter une bordure avec épaisseur et couleur choisies.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-vignette",
            "Ajouter une vignette",
            "Assombrir progressivement les bords avec un rayon réglable.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            false,
        ),
        action(
            "image-watermark",
            "Ajouter un filigrane texte",
            "Ajouter un texte discret en bas à droite sans écraser l’original.",
            OperationCategory::Image,
            &[Image],
            &["imagemagick"],
            None,
            true,
            false,
            true,
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
            "archive-package",
            "Compression intelligente",
            "Choisir automatiquement TAR.ZST pour le débit maximal, ou créer un ZIP/TAR compatible à partir d’un lot ou d’un dossier.",
            OperationCategory::Archive,
            &[],
            &["archive", "zstd"],
            None,
            true,
            false,
            true,
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
            "video-rotate",
            "Tourner une vidéo",
            "Rotation 90° gauche/droite ou 180° avec réencodage contrôlé.",
            OperationCategory::Media,
            &[Video],
            &["ffmpeg"],
            None,
            true,
            false,
            true,
        ),
        action(
            "video-resize",
            "Changer la résolution vidéo",
            "Redimensionner une vidéo vers des dimensions précises en préservant le ratio si souhaité.",
            OperationCategory::Media,
            &[Video],
            &["ffmpeg"],
            None,
            true,
            false,
            true,
        ),
        action(
            "video-mute",
            "Créer une vidéo sans son",
            "Supprimer la piste audio sans modifier inutilement la vidéo.",
            OperationCategory::Media,
            &[Video],
            &["ffmpeg"],
            None,
            true,
            false,
            false,
        ),
        action(
            "video-thumbnail",
            "Extraire une miniature",
            "Créer une image JPG à l’instant choisi dans la vidéo.",
            OperationCategory::Extract,
            &[Video],
            &["ffmpeg"],
            Some("jpg"),
            true,
            false,
            true,
        ),
        action(
            "media-trim",
            "Découper un extrait",
            "Choisir un début et une durée pour créer un extrait audio ou vidéo.",
            OperationCategory::Media,
            &[Audio, Video],
            &["ffmpeg"],
            None,
            true,
            false,
            true,
        ),
        action(
            "audio-normalize",
            "Normaliser le volume",
            "Uniformiser le volume perçu avec le filtre EBU R128 loudnorm.",
            OperationCategory::Media,
            &[Audio],
            &["ffmpeg"],
            None,
            true,
            false,
            true,
        ),
        action(
            "audio-gain",
            "Modifier le volume",
            "Augmenter ou réduire le niveau sonore en décibels.",
            OperationCategory::Media,
            &[Audio],
            &["ffmpeg"],
            None,
            true,
            false,
            false,
        ),
        action(
            "audio-mono",
            "Convertir en mono",
            "Mélanger les canaux vers une piste mono compatible et légère.",
            OperationCategory::Media,
            &[Audio],
            &["ffmpeg"],
            None,
            true,
            false,
            false,
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

fn default_format_capabilities() -> Vec<FormatCapabilityProfile> {
    use FormatFamily::*;

    let image_actions = vec![
        "image-convert",
        "image-batch-convert",
        "image-optimize",
        "image-resize",
        "image-rotate-left",
        "image-rotate-right",
        "image-rotate-180",
        "image-rotate",
        "image-flip-horizontal",
        "image-flip-vertical",
        "image-auto-orient",
        "image-auto-enhance",
        "image-adjust",
        "image-grayscale",
        "image-sepia",
        "image-sharpen",
        "image-blur",
        "image-noise-reduce",
        "image-threshold",
        "image-posterize",
        "image-pixelate",
        "image-flatten",
        "image-trim",
        "image-crop-center",
        "image-resize-exact",
        "image-crop-custom",
        "image-canvas",
        "image-auto-gamma",
        "image-contrast-stretch",
        "image-colorspace-srgb",
        "image-set-dpi",
        "image-perspective",
        "image-border",
        "image-vignette",
        "image-watermark",
        "strip-metadata",
        "extract-metadata",
        "ocr-image",
        "images-to-pdf",
        "zstd-compress",
        "lz4-compress",
        "archive-package",
    ];
    let pdf_actions = vec![
        "pdf-merge",
        "pdf-split",
        "pdf-reorder",
        "pdf-rotate-pages",
        "pdf-select-pages",
        "pdf-linearize",
        "pdf-optimize-lossless",
        "pdf-repair",
        "pdf-flatten-rotation",
        "pdf-flatten-annotations",
        "pdf-compress",
        "pdf-ocr",
        "pdf-to-images",
        "pdf-extract-text",
        "strip-metadata",
        "extract-metadata",
        "zstd-compress",
        "lz4-compress",
        "archive-package",
    ];
    let video_actions = vec![
        "video-convert",
        "video-rotate",
        "video-resize",
        "video-mute",
        "video-thumbnail",
        "media-trim",
        "media-compatible",
        "media-compress",
        "extract-audio",
        "video-to-gif",
        "strip-metadata",
        "extract-metadata",
        "zstd-compress",
        "lz4-compress",
        "archive-package",
    ];
    let audio_actions = vec![
        "audio-convert",
        "audio-normalize",
        "audio-gain",
        "audio-mono",
        "media-trim",
        "media-compatible",
        "media-compress",
        "strip-metadata",
        "extract-metadata",
        "zstd-compress",
        "lz4-compress",
        "archive-package",
    ];

    vec![
        format_profile(
            "jpeg",
            "JPEG / JPG",
            Image,
            &["jpg", "jpeg"],
            true,
            &image_actions,
            &["png", "webp", "avif", "heic", "jxl", "tiff", "gif", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "png",
            "PNG",
            Image,
            &["png"],
            true,
            &image_actions,
            &["jpg", "webp", "avif", "heic", "jxl", "tiff", "gif", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "webp",
            "WebP",
            Image,
            &["webp"],
            true,
            &image_actions,
            &["jpg", "png", "avif", "tiff", "gif", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "heic",
            "HEIC / HEIF",
            Image,
            &["heic", "heif"],
            true,
            &image_actions,
            &["jpg", "png", "webp", "avif", "tiff", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "avif",
            "AVIF",
            Image,
            &["avif"],
            true,
            &image_actions,
            &["jpg", "png", "webp", "tiff", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "tiff",
            "TIFF",
            Image,
            &["tif", "tiff"],
            true,
            &image_actions,
            &["jpg", "png", "webp", "avif", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "gif",
            "GIF / image animée",
            Image,
            &["gif"],
            true,
            &image_actions,
            &["jpg", "png", "webp", "avif", "mp4", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "jxl",
            "JPEG XL",
            Image,
            &["jxl"],
            true,
            &image_actions,
            &["jpg", "png", "webp", "avif", "tiff", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "bitmap",
            "BMP / ICO",
            Image,
            &["bmp", "ico"],
            true,
            &image_actions,
            &["jpg", "png", "webp", "avif", "tiff", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "vector",
            "SVG / EPS vectoriel",
            Image,
            &["svg", "eps"],
            true,
            &[
                "image-convert",
                "image-batch-convert",
                "extract-metadata",
                "strip-metadata",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["png", "jpg", "webp", "avif", "tiff", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "photoshop",
            "Adobe Photoshop",
            Image,
            &["psd", "psb"],
            true,
            &[
                "image-convert",
                "image-batch-convert",
                "image-flatten",
                "image-resize-exact",
                "extract-metadata",
                "strip-metadata",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["png", "jpg", "webp", "tiff", "pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "raw",
            "RAW photo",
            Image,
            &[
                "dng", "cr2", "cr3", "nef", "arw", "orf", "raf", "rw2", "pef",
            ],
            true,
            &[
                "image-convert",
                "image-auto-orient",
                "image-auto-enhance",
                "image-resize-exact",
                "strip-metadata",
                "extract-metadata",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["jpg", "png", "tiff", "webp", "avif"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "extended-image",
            "Images étendues et professionnelles",
            Image,
            &[
                "apng", "jpe", "jfif", "jp2", "j2k", "jpf", "jpx", "jpm", "mj2", "tga", "icb",
                "vda", "vst", "dds", "exr", "hdr", "rgbe", "pbm", "pgm", "ppm", "pnm", "pam",
                "pcx", "dcx", "qoi", "xcf", "cur", "icns", "wmf", "emf",
            ],
            true,
            &image_actions,
            &["jpg", "png", "webp", "avif", "tiff", "pdf"],
            &["zip", "7z", "tar", "tar.gz", "tar.zst", "zst", "lz4"],
        ),
        format_profile(
            "pdf",
            "PDF",
            Pdf,
            &["pdf"],
            true,
            &pdf_actions,
            &["png", "txt"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "docx",
            "Word / DOCX",
            Document,
            &["doc", "docx", "odt", "rtf"],
            false,
            &[
                "office-to-pdf",
                "office-convert",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "xlsx",
            "Excel / tableur",
            Spreadsheet,
            &["xls", "xlsx", "xlsm", "ods", "csv", "tsv"],
            false,
            &[
                "office-to-pdf",
                "office-convert",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "pptx",
            "PowerPoint / présentation",
            Presentation,
            &["ppt", "pptx", "odp"],
            false,
            &[
                "office-to-pdf",
                "office-convert",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["pdf"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "html",
            "Page HTML dynamique",
            Text,
            &["html", "htm"],
            true,
            &[
                "html-to-pdf",
                "text-to-pdf",
                "text-convert",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["pdf", "html", "docx", "txt"],
            &["zip", "7z", "tar", "tar.gz", "tar.zst", "zst", "lz4"],
        ),
        format_profile(
            "eml",
            "E-mail EML",
            Text,
            &["eml", "mail"],
            true,
            &[
                "email-to-pdf",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["pdf"],
            &["zip", "7z", "tar", "tar.gz", "tar.zst", "zst", "lz4"],
        ),
        format_profile(
            "markup",
            "Texte / Markdown / HTML",
            Text,
            &["txt", "md", "markdown", "rst", "tex"],
            true,
            &[
                "text-to-pdf",
                "text-convert",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["pdf", "html", "md", "docx", "epub", "txt"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "structured-data",
            "JSON / XML / YAML / TOML / données",
            Text,
            &[
                "json",
                "xml",
                "yaml",
                "yml",
                "toml",
                "jsonl",
                "ndjson",
                "sql",
                "ini",
                "cfg",
                "conf",
                "properties",
                "log",
            ],
            true,
            &["zstd-compress", "lz4-compress", "archive-package"],
            &[],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "epub",
            "EPUB / FictionBook",
            Ebook,
            &["epub", "fb2"],
            false,
            &[
                "ebook-convert",
                "zstd-compress",
                "lz4-compress",
                "archive-package",
            ],
            &["html", "md", "docx", "txt", "epub"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "amazon-ebook",
            "MOBI / AZW / AZW3",
            Ebook,
            &["mobi", "azw", "azw3"],
            false,
            &["zstd-compress", "lz4-compress", "archive-package"],
            &[],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "comic-book",
            "Bandes dessinées numériques",
            Ebook,
            &["cbz", "cbr", "cb7"],
            false,
            &["zstd-compress", "lz4-compress", "archive-package"],
            &[],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "djvu",
            "DjVu",
            Ebook,
            &["djvu", "djv"],
            false,
            &["zstd-compress", "lz4-compress", "archive-package"],
            &[],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "audio",
            "Audio",
            Audio,
            &["mp3", "wav", "aac", "m4a", "flac", "ogg", "opus", "aiff"],
            false,
            &audio_actions,
            &["mp3", "m4a", "wav", "flac", "ogg", "opus"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "video",
            "Vidéo",
            Video,
            &[
                "mp4", "mov", "mkv", "avi", "webm", "mpeg", "wmv", "flv", "m2ts",
            ],
            false,
            &video_actions,
            &["mp4", "webm", "mkv", "mov", "gif", "m4a", "mp3"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4", "zst",
                "lz4",
            ],
        ),
        format_profile(
            "zstd",
            "Zstandard",
            Archive,
            &["zst", "zstd", "tzst"],
            false,
            &["zstd-decompress", "archive-extract", "archive-package"],
            &[],
            &[],
        ),
        format_profile(
            "lz4",
            "LZ4",
            Archive,
            &["lz4"],
            false,
            &["lz4-decompress", "archive-extract", "archive-package"],
            &[],
            &[],
        ),
        format_profile(
            "archive",
            "Archives et conteneurs",
            Archive,
            &[
                "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz", "tbz2", "xz", "txz", "cab",
                "arj", "cpio", "iso",
            ],
            false,
            &["archive-extract", "archive-package"],
            &[
                "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4",
            ],
            &[],
        ),
    ]
}

// Declarative capability-profile factory: parameters map directly to the format capability matrix.
#[allow(clippy::too_many_arguments)]
fn format_profile(
    id: &str,
    label: &str,
    family: FormatFamily,
    extensions: &[&str],
    preview: bool,
    actions: &[&str],
    convert_to: &[&str],
    compress_to: &[&str],
) -> FormatCapabilityProfile {
    let metadata = actions
        .iter()
        .any(|action| matches!(*action, "extract-metadata" | "strip-metadata"));
    let thumbnail = preview || matches!(family, FormatFamily::Pdf | FormatFamily::Video);
    let extractable = actions
        .iter()
        .any(|action| matches!(*action, "archive-extract" | "pdf-extract-text"));
    let streamable = matches!(family, FormatFamily::Audio | FormatFamily::Video);
    let mut capabilities = vec!["inspect".to_owned()];
    if preview {
        capabilities.push("preview".into());
    }
    if !convert_to.is_empty() {
        capabilities.push("convert".into());
    }
    if !compress_to.is_empty() {
        capabilities.push("compress".into());
    }
    if metadata {
        capabilities.push("metadata".into());
    }
    if thumbnail {
        capabilities.push("thumbnail".into());
    }
    if extractable {
        capabilities.push("extract".into());
    }
    if streamable {
        capabilities.push("stream".into());
    }
    if matches!(family, FormatFamily::Image) {
        capabilities.extend(["editPixels".into(), "batch".into(), "privacy".into()]);
    }
    if matches!(family, FormatFamily::Pdf) {
        capabilities.extend(["pages".into(), "ocr".into(), "repair".into()]);
    }
    if matches!(family, FormatFamily::Audio | FormatFamily::Video) {
        capabilities.push("transcode".into());
    }
    if matches!(
        family,
        FormatFamily::Document
            | FormatFamily::Spreadsheet
            | FormatFamily::Presentation
            | FormatFamily::Text
            | FormatFamily::Ebook
    ) {
        capabilities.push("documentTransform".into());
    }
    FormatCapabilityProfile {
        id: id.into(),
        label: label.into(),
        family,
        extensions: extensions.iter().map(|value| (*value).into()).collect(),
        preview,
        readable: true,
        writable: !matches!(
            id,
            "raw" | "amazon-ebook" | "comic-book" | "djvu" | "structured-data"
        ),
        metadata,
        thumbnail,
        extractable,
        streamable,
        capabilities,
        actions: actions.iter().map(|value| (*value).into()).collect(),
        convert_to: convert_to.iter().map(|value| (*value).into()).collect(),
        compress_to: compress_to.iter().map(|value| (*value).into()).collect(),
    }
}

fn default_conversion_edges() -> Vec<ConversionEdge> {
    let mut edges = Vec::new();

    // Lossless / minimally destructive image intermediates. JPEG/WebP are
    // penalised as intermediate formats because they can introduce generation
    // loss; PNG/TIFF are preferred when a direct PDF route is unavailable.
    for from in [
        "jpeg",
        "png",
        "webp",
        "heic",
        "heif",
        "avif",
        "jxl",
        "tiff",
        "bmp",
        "gif",
        "raw",
        "svg",
        "photoshop",
        "eps",
        "apng",
        "jpeg2000",
        "tga",
        "dds",
        "openexr",
        "radiance",
        "netpbm",
        "pcx",
        "qoi",
        "xcf",
        "cursor",
        "icns",
        "windows-metafile",
    ] {
        for to in ["jpeg", "png", "webp", "avif", "tiff"] {
            if from != to {
                edges.push(edge(
                    from,
                    to,
                    if matches!(from, "eps" | "photoshop") {
                        "imagemagick"
                    } else {
                        "vips"
                    },
                    if matches!(to, "jpeg" | "webp") { 3 } else { 1 },
                    matches!(to, "jpeg" | "webp"),
                ));
            }
        }
    }
    for from in ["jpeg", "png", "tiff"] {
        edges.push(edge(from, "pdf", "img2pdf", 1, false));
    }
    // ImageMagick is a compatibility fallback for codecs that a local libvips
    // build may not contain (notably some RAW, vector and professional formats).
    for from in [
        "heic",
        "heif",
        "avif",
        "jxl",
        "raw",
        "svg",
        "photoshop",
        "eps",
        "apng",
        "jpeg2000",
        "tga",
        "dds",
        "openexr",
        "radiance",
        "netpbm",
        "pcx",
        "qoi",
        "xcf",
        "cursor",
        "icns",
        "windows-metafile",
    ] {
        edges.push(edge(from, "png", "imagemagick", 2, false));
    }

    for from in [
        "doc", "docx", "odt", "rtf", "wpd", "xls", "xlsx", "ods", "csv", "tsv", "ppt", "pptx",
        "odp",
    ] {
        edges.push(edge(from, "pdf", "office", 1, false));
    }

    for from in ["txt", "text", "md", "html", "rst", "markdown", "tex"] {
        edges.push(edge(from, "docx", "pandoc", 1, false));
        edges.push(edge(from, "html", "pandoc", 1, false));
        edges.push(edge(from, "epub", "pandoc", 2, false));
    }
    edges.push(edge("html", "pdf", "browser", 1, false));
    edges.push(edge("eml", "pdf", "browser", 1, false));
    for from in ["epub", "fb2"] {
        edges.push(edge(from, "docx", "pandoc", 2, false));
    }

    edges.extend([
        edge("pdf", "png", "poppler", 3, true),
        edge("pdf", "jpeg", "poppler", 4, true),
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
            formats: Vec::new(),
            action_ui: Vec::new(),
        };

        let plan = catalog.conversion_plan("docx", "png").unwrap();
        assert_eq!(plan.total_cost, 29);
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
    fn format_matrix_exposes_deep_image_tooling() {
        let catalog = CapabilityCatalog::default();
        let jpeg = catalog.format("jpg").expect("JPEG capability profile");
        assert!(jpeg.preview);
        assert!(jpeg.actions.iter().any(|action| action == "image-rotate"));
        assert!(
            jpeg.actions
                .iter()
                .any(|action| action == "image-watermark")
        );
        assert!(jpeg.compress_to.iter().any(|format| format == "zst"));
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
    #[test]
    fn planner_prefers_lossless_intermediate_over_shorter_lossy_route() {
        let catalog = CapabilityCatalog {
            actions: Vec::new(),
            conversions: vec![
                edge("xyz", "jpeg", "image", 1, true),
                edge("jpeg", "pdf", "pdf", 1, false),
                edge("xyz", "png", "image", 2, false),
                edge("png", "pdf", "pdf", 2, false),
            ],
            formats: Vec::new(),
            action_ui: Vec::new(),
        };
        let plan = catalog.conversion_plan("xyz", "pdf").expect("route");
        assert_eq!(plan.intermediates, vec!["png"]);
        assert_eq!(plan.lossy_steps, 0);
    }

    #[test]
    fn planner_can_filter_routes_by_installed_engines() {
        let catalog = CapabilityCatalog::default();
        let engines = HashSet::from(["pandoc".to_string(), "office".to_string()]);
        let plan = catalog
            .conversion_plan_with_engines("md", "pdf", &engines)
            .expect("Markdown should route via DOCX");
        assert_eq!(plan.intermediates, vec!["docx"]);
        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn image_pdf_routes_never_use_libreoffice() {
        let catalog = CapabilityCatalog::default();
        let engines = HashSet::from([
            "img2pdf".to_string(),
            "vips".to_string(),
            "imagemagick".to_string(),
            "office".to_string(),
        ]);
        for input in ["jpeg", "png", "tiff", "heic", "avif", "photoshop"] {
            let plan = catalog
                .conversion_plan_with_engines(input, "pdf", &engines)
                .expect("image should have a PDF route");
            assert!(plan.steps.iter().all(|step| step.engine_id != "office"));
        }
    }

    #[test]
    fn html_and_email_use_the_browser_pdf_renderer_when_available() {
        let catalog = CapabilityCatalog::default();
        let engines = HashSet::from([
            "browser".to_string(),
            "pandoc".to_string(),
            "office".to_string(),
        ]);
        let html = catalog
            .conversion_plan_with_engines("html", "pdf", &engines)
            .expect("HTML route");
        let email = catalog
            .conversion_plan_with_engines("eml", "pdf", &engines)
            .expect("EML route");

        assert_eq!(html.steps[0].engine_id, "browser");
        assert_eq!(email.steps[0].engine_id, "browser");
    }

    #[test]
    fn exposes_action_specific_ui_contracts() {
        let catalog = CapabilityCatalog::default();
        let convert = catalog
            .action_ui("image-convert")
            .expect("image conversion UI");
        assert_eq!(convert.kind, ActionUiKind::Conversion);
        assert!(convert.target_formats.iter().any(|format| format == "webp"));

        let protect = catalog.action_ui("pdf-protect").expect("PDF protection UI");
        let password = protect
            .parameters
            .iter()
            .find(|parameter| parameter.key == "password")
            .expect("password field");
        assert!(password.required);
        assert_eq!(password.kind, ActionParameterKind::Password);

        let package = catalog
            .action_ui("archive-package")
            .expect("archive package UI");
        assert_eq!(package.input_mode, ActionInputMode::FilesOrDirectories);
        assert!(
            package
                .target_formats
                .iter()
                .any(|format| format == "tar.zst")
        );
    }
}
