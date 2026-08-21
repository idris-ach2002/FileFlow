//! Destination policies, safe naming and atomic finalization.

use chrono::Local;
use fileflow_domain::{ConflictStrategy, DestinationPolicy, NamingStrategy, OutputPolicy};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};
use thiserror::Error;
use uuid::Uuid;

const MAX_CONFLICT_ATTEMPTS: u32 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRequest {
    pub source: PathBuf,
    pub source_root: Option<PathBuf>,
    pub desired_extension: Option<String>,
    pub operation_suffix: Option<String>,
    pub policy: OutputPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputPlan {
    pub destination_directory: PathBuf,
    pub final_path: PathBuf,
    pub temporary_path: PathBuf,
    pub replaces_existing: bool,
    pub skipped: bool,
}

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("source has no parent directory: {0}")]
    MissingParent(PathBuf),
    #[error("custom destination is required for custom-folder policy")]
    MissingCustomDirectory,
    #[error("destination already exists and conflict policy is ask: {0}")]
    ConflictRequiresDecision(PathBuf),
    #[error("could not find a free output name after {MAX_CONFLICT_ATTEMPTS} attempts")]
    ConflictLimit,
    #[error("refusing to overwrite the source file: {0}")]
    SourceOverwrite(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct OutputResolver;

impl OutputResolver {
    pub fn plan(&self, request: &OutputRequest) -> Result<OutputPlan, OutputError> {
        let source_parent = request
            .source
            .parent()
            .ok_or_else(|| OutputError::MissingParent(request.source.clone()))?;
        let destination_directory = match request.policy.destination {
            DestinationPolicy::SameFolder | DestinationPolicy::AskEveryTime => {
                source_parent.to_path_buf()
            }
            DestinationPolicy::Subfolder => {
                source_parent.join(sanitize_component(&request.policy.subfolder_name))
            }
            DestinationPolicy::CustomFolder => request
                .policy
                .custom_directory
                .clone()
                .ok_or(OutputError::MissingCustomDirectory)?,
        };

        let destination_directory = if request.policy.preserve_tree {
            relative_parent(request.source_root.as_deref(), &request.source)
                .map_or(destination_directory.clone(), |relative| {
                    destination_directory.join(relative)
                })
        } else {
            destination_directory
        };

        let stem = request
            .source
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("resultat");
        let stem = output_stem(
            stem,
            request.policy.naming,
            request.operation_suffix.as_deref(),
        );
        let extension = request
            .desired_extension
            .as_deref()
            .or_else(|| request.source.extension().and_then(|value| value.to_str()));
        let mut candidate = destination_directory.join(file_name(&stem, extension));
        if !request.policy.overwrite_original && same_path(&candidate, &request.source) {
            let safe_suffix = request.operation_suffix.as_deref().unwrap_or("fileflow");
            candidate = destination_directory.join(file_name(
                &format!("{stem}_{}", sanitize_component(safe_suffix)),
                extension,
            ));
        }
        let (final_path, replaces_existing, skipped) =
            resolve_conflict(&candidate, request.policy.conflict)?;

        if !request.policy.overwrite_original && same_path(&final_path, &request.source) {
            return Err(OutputError::SourceOverwrite(final_path));
        }

        let final_stem = final_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("result");
        let token = Uuid::new_v4().simple();
        let temporary_name = match final_path.extension().and_then(|value| value.to_str()) {
            Some(extension) => format!(".{final_stem}.fileflow-{token}.tmp.{extension}"),
            None => format!(".{final_stem}.fileflow-{token}.tmp"),
        };
        let temporary_path = destination_directory.join(temporary_name);

        Ok(OutputPlan {
            destination_directory,
            final_path,
            temporary_path,
            replaces_existing,
            skipped,
        })
    }

    pub fn plan_named(
        &self,
        request: &OutputRequest,
        preferred_file_name: &str,
    ) -> Result<OutputPlan, OutputError> {
        let source_parent = request
            .source
            .parent()
            .ok_or_else(|| OutputError::MissingParent(request.source.clone()))?;
        let destination_directory = match request.policy.destination {
            DestinationPolicy::SameFolder | DestinationPolicy::AskEveryTime => {
                source_parent.to_path_buf()
            }
            DestinationPolicy::Subfolder => {
                source_parent.join(sanitize_component(&request.policy.subfolder_name))
            }
            DestinationPolicy::CustomFolder => request
                .policy
                .custom_directory
                .clone()
                .ok_or(OutputError::MissingCustomDirectory)?,
        };
        let destination_directory = if request.policy.preserve_tree {
            relative_parent(request.source_root.as_deref(), &request.source)
                .map_or(destination_directory.clone(), |relative| {
                    destination_directory.join(relative)
                })
        } else {
            destination_directory
        };

        let safe_name = Path::new(preferred_file_name)
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .map(sanitize_file_name)
            .unwrap_or_else(|| "resultat".into());
        let candidate = destination_directory.join(safe_name);
        let (final_path, replaces_existing, skipped) =
            resolve_conflict(&candidate, request.policy.conflict)?;
        if !request.policy.overwrite_original && same_path(&final_path, &request.source) {
            return Err(OutputError::SourceOverwrite(final_path));
        }

        let final_stem = final_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("result");
        let token = Uuid::new_v4().simple();
        let temporary_name = match final_path.extension().and_then(|value| value.to_str()) {
            Some(extension) => format!(".{final_stem}.fileflow-{token}.tmp.{extension}"),
            None => format!(".{final_stem}.fileflow-{token}.tmp"),
        };
        let temporary_path = destination_directory.join(temporary_name);

        Ok(OutputPlan {
            destination_directory,
            final_path,
            temporary_path,
            replaces_existing,
            skipped,
        })
    }

    pub async fn prepare(&self, plan: &OutputPlan) -> Result<(), OutputError> {
        if plan.skipped {
            return Ok(());
        }
        tokio::fs::create_dir_all(&plan.destination_directory).await?;
        Ok(())
    }

    pub async fn finalize(&self, plan: &OutputPlan) -> Result<(), OutputError> {
        if plan.skipped {
            return Ok(());
        }
        if plan.replaces_existing && plan.final_path.exists() {
            tokio::fs::remove_file(&plan.final_path).await?;
        }

        match tokio::fs::rename(&plan.temporary_path, &plan.final_path).await {
            Ok(()) => Ok(()),
            Err(rename_error) => {
                match tokio::fs::copy(&plan.temporary_path, &plan.final_path).await {
                    Ok(_) => {
                        tokio::fs::remove_file(&plan.temporary_path).await?;
                        Ok(())
                    }
                    Err(_) => Err(OutputError::Io(rename_error)),
                }
            }
        }
    }

    pub async fn cleanup(&self, plan: &OutputPlan) {
        let _ = tokio::fs::remove_file(&plan.temporary_path).await;
    }
}

fn output_stem(
    source_stem: &str,
    naming: NamingStrategy,
    operation_suffix: Option<&str>,
) -> String {
    match naming {
        NamingStrategy::Original => source_stem.to_owned(),
        NamingStrategy::OperationSuffix => operation_suffix
            .filter(|value| !value.trim().is_empty())
            .map_or_else(
                || source_stem.to_owned(),
                |suffix| format!("{source_stem}_{}", sanitize_component(suffix)),
            ),
        NamingStrategy::DateSuffix => format!("{source_stem}_{}", Local::now().format("%Y-%m-%d")),
    }
}

fn resolve_conflict(
    candidate: &Path,
    strategy: ConflictStrategy,
) -> Result<(PathBuf, bool, bool), OutputError> {
    if !candidate.exists() {
        return Ok((candidate.to_path_buf(), false, false));
    }

    match strategy {
        ConflictStrategy::Replace => Ok((candidate.to_path_buf(), true, false)),
        ConflictStrategy::Skip => Ok((candidate.to_path_buf(), false, true)),
        ConflictStrategy::Ask => Err(OutputError::ConflictRequiresDecision(
            candidate.to_path_buf(),
        )),
        ConflictStrategy::Increment => {
            let parent = candidate.parent().unwrap_or_else(|| Path::new("."));
            let stem = candidate
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("resultat");
            let extension = candidate.extension().and_then(|value| value.to_str());
            for index in 1..=MAX_CONFLICT_ATTEMPTS {
                let next = parent.join(file_name(&format!("{stem} ({index})"), extension));
                if !next.exists() {
                    return Ok((next, false, false));
                }
            }
            Err(OutputError::ConflictLimit)
        }
    }
}

fn file_name(stem: &str, extension: Option<&str>) -> String {
    match extension.map(str::trim).filter(|value| !value.is_empty()) {
        Some(extension) => format!("{stem}.{}", extension.trim_start_matches('.')),
        None => stem.to_owned(),
    }
}

fn relative_parent(root: Option<&Path>, source: &Path) -> Option<PathBuf> {
    let root = root?;
    let relative = source.strip_prefix(root).ok()?;
    relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn sanitize_component(value: &str) -> String {
    let cleaned = value
        .trim()
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '\0' => '-',
            other => other,
        })
        .collect::<String>();
    if cleaned.is_empty() {
        "FileFlow".into()
    } else {
        cleaned
    }
}

fn sanitize_file_name(value: &str) -> String {
    let cleaned = value
        .trim()
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '\0' => '-',
            other if other.is_control() => '-',
            other => other,
        })
        .collect::<String>();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "resultat".into()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_non_destructive_subfolder_destination() {
        let resolver = OutputResolver;
        let request = OutputRequest {
            source: PathBuf::from("/tmp/photos/photo.heic"),
            source_root: None,
            desired_extension: Some("jpg".into()),
            operation_suffix: None,
            policy: OutputPolicy::default(),
        };
        let plan = resolver.plan(&request).unwrap();
        assert_eq!(
            plan.final_path,
            PathBuf::from("/tmp/photos/FileFlow/photo.jpg")
        );
        assert_ne!(plan.final_path, request.source);
    }

    #[test]
    fn preserves_relative_tree_in_custom_destination() {
        let resolver = OutputResolver;
        let request = OutputRequest {
            source: PathBuf::from("/input/trip/day1/photo.heic"),
            source_root: Some(PathBuf::from("/input")),
            desired_extension: Some("jpg".into()),
            operation_suffix: None,
            policy: OutputPolicy {
                destination: DestinationPolicy::CustomFolder,
                custom_directory: Some(PathBuf::from("/output")),
                ..OutputPolicy::default()
            },
        };
        let plan = resolver.plan(&request).unwrap();
        assert_eq!(
            plan.final_path,
            PathBuf::from("/output/trip/day1/photo.jpg")
        );
    }
    #[test]
    fn temporary_file_keeps_final_extension_for_format_aware_engines() {
        let directory =
            std::env::temp_dir().join(format!("fileflow-output-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("photo.heic");
        std::fs::write(&source, b"fixture").unwrap();
        let resolver = OutputResolver;
        let plan = resolver
            .plan(&OutputRequest {
                source,
                source_root: None,
                desired_extension: Some("webp".into()),
                operation_suffix: Some("converti".into()),
                policy: OutputPolicy::default(),
            })
            .unwrap();
        assert_eq!(
            plan.temporary_path
                .extension()
                .and_then(|value| value.to_str()),
            Some("webp")
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn named_plan_supports_extension_stripping_for_decompression() {
        let resolver = OutputResolver;
        let request = OutputRequest {
            source: PathBuf::from("/tmp/archive.tar.zst"),
            source_root: None,
            desired_extension: None,
            operation_suffix: Some("decompresse".into()),
            policy: OutputPolicy::default(),
        };
        let plan = resolver.plan_named(&request, "archive.tar").unwrap();
        assert_eq!(plan.final_path, PathBuf::from("/tmp/FileFlow/archive.tar"));
        assert_eq!(
            plan.temporary_path
                .extension()
                .and_then(|value| value.to_str()),
            Some("tar")
        );
    }

    #[test]
    fn same_folder_same_extension_falls_back_to_operation_suffix() {
        let directory =
            std::env::temp_dir().join(format!("fileflow-output-same-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("photo.jpg");
        std::fs::write(&source, b"fixture").unwrap();
        let resolver = OutputResolver;
        let plan = resolver
            .plan(&OutputRequest {
                source: source.clone(),
                source_root: None,
                desired_extension: Some("jpg".into()),
                operation_suffix: Some("optimise".into()),
                policy: OutputPolicy {
                    destination: DestinationPolicy::SameFolder,
                    naming: NamingStrategy::Original,
                    ..OutputPolicy::default()
                },
            })
            .unwrap();
        assert_eq!(plan.final_path, directory.join("photo_optimise.jpg"));
        assert_ne!(plan.final_path, source);
        let _ = std::fs::remove_dir_all(directory);
    }
}
