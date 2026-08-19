use chrono::{DateTime, Utc};
use fileflow_domain::{
    ArchiveAsset, Asset, AssetCommon, AssetId, AssetKind, DirectoryAsset, FileAsset, FormatFamily,
    IntakeRequestId, SymlinkAsset,
};
use fileflow_formats::FormatRegistry;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    time::SystemTime,
};
use thiserror::Error;
use tokio::{fs, io::AsyncReadExt, sync::mpsc};

const DEFAULT_BATCH_SIZE: usize = 64;
const DEFAULT_SAMPLE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ScanOptions {
    pub recursive: bool,
    pub follow_symlinks: bool,
    pub include_hidden: bool,
    pub max_depth: Option<usize>,
    pub batch_size: usize,
    pub sample_bytes: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            follow_symlinks: false,
            include_hidden: true,
            max_depth: None,
            batch_size: DEFAULT_BATCH_SIZE,
            sample_bytes: DEFAULT_SAMPLE_BYTES,
        }
    }
}

impl ScanOptions {
    fn normalized(&self) -> Self {
        Self {
            batch_size: self.batch_size.clamp(1, 512),
            sample_bytes: self.sample_bytes.clamp(512, 64 * 1024),
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeStats {
    pub discovered: u64,
    pub files: u64,
    pub directories: u64,
    pub archives: u64,
    pub symlinks: u64,
    pub total_bytes: u64,
    pub warnings: u64,
}

impl IntakeStats {
    fn record(&mut self, asset: &Asset) {
        self.discovered += 1;
        self.total_bytes = self.total_bytes.saturating_add(asset.size_bytes());

        match asset.kind() {
            AssetKind::File => self.files += 1,
            AssetKind::Directory => self.directories += 1,
            AssetKind::Archive => self.archives += 1,
            AssetKind::Symlink => self.symlinks += 1,
            AssetKind::Other => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeWarning {
    pub path: PathBuf,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeReport {
    pub request_id: IntakeRequestId,
    pub roots: usize,
    pub stats: IntakeStats,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
pub enum IntakeEvent {
    Started {
        request_id: IntakeRequestId,
        roots: usize,
    },
    Batch {
        request_id: IntakeRequestId,
        assets: Vec<Asset>,
        stats: IntakeStats,
    },
    Progress {
        request_id: IntakeRequestId,
        stats: IntakeStats,
    },
    Warning {
        request_id: IntakeRequestId,
        warning: IntakeWarning,
        stats: IntakeStats,
    },
    Finished {
        report: IntakeReport,
    },
}

#[derive(Debug, Error)]
pub enum IntakeError {
    #[error("no input paths were provided")]
    EmptyInput,
    #[error("intake event consumer disconnected")]
    ConsumerDisconnected,
}

#[derive(Debug, Clone, Default)]
pub struct IntakeScanner {
    formats: FormatRegistry,
}

impl IntakeScanner {
    pub async fn scan(
        &self,
        roots: Vec<PathBuf>,
        options: ScanOptions,
        events: mpsc::Sender<IntakeEvent>,
    ) -> Result<IntakeReport, IntakeError> {
        if roots.is_empty() {
            return Err(IntakeError::EmptyInput);
        }

        let options = options.normalized();
        let request_id = IntakeRequestId::new();
        let mut stats = IntakeStats::default();
        let mut queue = VecDeque::new();
        let mut visited_directories = HashSet::new();
        let mut batch = Vec::with_capacity(options.batch_size);

        send(
            &events,
            IntakeEvent::Started {
                request_id,
                roots: roots.len(),
            },
        )
        .await?;

        for (root_index, root) in roots.iter().enumerate() {
            queue.push_back(PendingPath {
                path: root.clone(),
                root: root.clone(),
                root_index,
                depth: 0,
            });
        }

        while let Some(pending) = queue.pop_front() {
            let symlink_metadata = match fs::symlink_metadata(&pending.path).await {
                Ok(metadata) => metadata,
                Err(error) => {
                    warning(
                        &events,
                        request_id,
                        &mut stats,
                        &pending.path,
                        "metadata",
                        error.to_string(),
                    )
                    .await?;
                    continue;
                }
            };

            let file_type = symlink_metadata.file_type();
            if file_type.is_symlink() && !options.follow_symlinks {
                let target = fs::read_link(&pending.path).await.ok();
                let asset = Asset::Symlink(SymlinkAsset {
                    common: common(&pending, symlink_metadata.modified().ok()),
                    target,
                });
                push_asset(
                    asset,
                    request_id,
                    &events,
                    &mut stats,
                    &mut batch,
                    options.batch_size,
                )
                .await?;
                continue;
            }

            let metadata = if file_type.is_symlink() {
                match fs::metadata(&pending.path).await {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        warning(
                            &events,
                            request_id,
                            &mut stats,
                            &pending.path,
                            "symlinkTarget",
                            error.to_string(),
                        )
                        .await?;
                        continue;
                    }
                }
            } else {
                symlink_metadata
            };

            if metadata.is_dir() {
                if !options.include_hidden && is_hidden(&pending.path) && pending.depth > 0 {
                    continue;
                }

                if options.follow_symlinks
                    && let Ok(canonical) = fs::canonicalize(&pending.path).await
                    && !visited_directories.insert(canonical)
                {
                    continue;
                }

                let asset = Asset::Directory(DirectoryAsset {
                    common: common(&pending, metadata.modified().ok()),
                });
                push_asset(
                    asset,
                    request_id,
                    &events,
                    &mut stats,
                    &mut batch,
                    options.batch_size,
                )
                .await?;

                if options.recursive && options.max_depth.is_none_or(|max| pending.depth < max) {
                    match fs::read_dir(&pending.path).await {
                        Ok(mut entries) => loop {
                            match entries.next_entry().await {
                                Ok(Some(entry)) => {
                                    if !options.include_hidden && is_hidden(&entry.path()) {
                                        continue;
                                    }
                                    queue.push_back(PendingPath {
                                        path: entry.path(),
                                        root: pending.root.clone(),
                                        root_index: pending.root_index,
                                        depth: pending.depth + 1,
                                    });
                                }
                                Ok(None) => break,
                                Err(error) => {
                                    warning(
                                        &events,
                                        request_id,
                                        &mut stats,
                                        &pending.path,
                                        "readDirectory",
                                        error.to_string(),
                                    )
                                    .await?;
                                    break;
                                }
                            }
                        },
                        Err(error) => {
                            warning(
                                &events,
                                request_id,
                                &mut stats,
                                &pending.path,
                                "openDirectory",
                                error.to_string(),
                            )
                            .await?;
                        }
                    }
                }

                continue;
            }

            if metadata.is_file() {
                if !options.include_hidden && is_hidden(&pending.path) {
                    continue;
                }

                let sample = match read_sample(&pending.path, options.sample_bytes).await {
                    Ok(sample) => sample,
                    Err(error) => {
                        warning(
                            &events,
                            request_id,
                            &mut stats,
                            &pending.path,
                            "readSample",
                            error.to_string(),
                        )
                        .await?;
                        Vec::new()
                    }
                };
                let format = self.formats.detect(&pending.path, &sample);
                let common = common(&pending, metadata.modified().ok());
                let size_bytes = metadata.len();

                let asset = if format.family == FormatFamily::Archive {
                    Asset::Archive(ArchiveAsset {
                        common,
                        size_bytes,
                        format,
                    })
                } else {
                    Asset::File(FileAsset {
                        common,
                        size_bytes,
                        format,
                    })
                };

                push_asset(
                    asset,
                    request_id,
                    &events,
                    &mut stats,
                    &mut batch,
                    options.batch_size,
                )
                .await?;
            }
        }

        flush_batch(&events, request_id, &stats, &mut batch).await?;
        send(
            &events,
            IntakeEvent::Progress {
                request_id,
                stats: stats.clone(),
            },
        )
        .await?;

        let report = IntakeReport {
            request_id,
            roots: roots.len(),
            stats,
        };
        send(
            &events,
            IntakeEvent::Finished {
                report: report.clone(),
            },
        )
        .await?;

        Ok(report)
    }
}

#[derive(Debug)]
struct PendingPath {
    path: PathBuf,
    root: PathBuf,
    root_index: usize,
    depth: usize,
}

fn common(pending: &PendingPath, modified: Option<SystemTime>) -> AssetCommon {
    let relative_path = if pending.path == pending.root {
        pending
            .path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_default()
    } else {
        pending
            .path
            .strip_prefix(&pending.root)
            .map(PathBuf::from)
            .unwrap_or_else(|_| pending.path.clone())
    };

    AssetCommon {
        id: AssetId::new(),
        root_index: pending.root_index,
        path: pending.path.clone(),
        relative_path,
        name: display_name(&pending.path),
        hidden: is_hidden(&pending.path),
        modified_at: modified.map(DateTime::<Utc>::from),
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') && name != "." && name != "..")
}

async fn read_sample(path: &Path, sample_bytes: usize) -> std::io::Result<Vec<u8>> {
    let mut file = fs::File::open(path).await?;
    let mut sample = vec![0_u8; sample_bytes];
    let read = file.read(&mut sample).await?;
    sample.truncate(read);
    Ok(sample)
}

async fn push_asset(
    asset: Asset,
    request_id: IntakeRequestId,
    events: &mpsc::Sender<IntakeEvent>,
    stats: &mut IntakeStats,
    batch: &mut Vec<Asset>,
    batch_size: usize,
) -> Result<(), IntakeError> {
    stats.record(&asset);
    batch.push(asset);

    if batch.len() >= batch_size {
        flush_batch(events, request_id, stats, batch).await?;
    }

    Ok(())
}

async fn flush_batch(
    events: &mpsc::Sender<IntakeEvent>,
    request_id: IntakeRequestId,
    stats: &IntakeStats,
    batch: &mut Vec<Asset>,
) -> Result<(), IntakeError> {
    if batch.is_empty() {
        return Ok(());
    }

    let assets = std::mem::take(batch);
    send(
        events,
        IntakeEvent::Batch {
            request_id,
            assets,
            stats: stats.clone(),
        },
    )
    .await
}

async fn warning(
    events: &mpsc::Sender<IntakeEvent>,
    request_id: IntakeRequestId,
    stats: &mut IntakeStats,
    path: &Path,
    code: &str,
    message: String,
) -> Result<(), IntakeError> {
    stats.warnings += 1;
    tracing::warn!(path = %path.display(), %code, %message, "file intake warning");
    send(
        events,
        IntakeEvent::Warning {
            request_id,
            warning: IntakeWarning {
                path: path.to_path_buf(),
                code: code.into(),
                message,
            },
            stats: stats.clone(),
        },
    )
    .await
}

async fn send(events: &mpsc::Sender<IntakeEvent>, event: IntakeEvent) -> Result<(), IntakeError> {
    events
        .send(event)
        .await
        .map_err(|_| IntakeError::ConsumerDisconnected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use uuid::Uuid;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("fileflow-intake-test-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn scans_nested_files_and_classifies_archives() {
        let root = temp_root();
        let nested = root.join("nested");
        stdfs::create_dir_all(&nested).unwrap();
        stdfs::write(root.join("notes.txt"), "hello").unwrap();
        stdfs::write(nested.join("archive.zip"), [b'P', b'K', 3, 4, 20, 0]).unwrap();
        stdfs::write(
            root.join("image.png"),
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        )
        .unwrap();

        let (tx, mut rx) = mpsc::channel(32);
        let report = IntakeScanner::default()
            .scan(vec![root.clone()], ScanOptions::default(), tx)
            .await
            .unwrap();

        let mut assets = Vec::new();
        while let Some(event) = rx.recv().await {
            if let IntakeEvent::Batch { assets: batch, .. } = event {
                assets.extend(batch);
            }
        }

        assert_eq!(report.stats.archives, 1);
        assert_eq!(report.stats.files, 2);
        assert_eq!(report.stats.directories, 2);
        assert!(
            assets
                .iter()
                .any(|asset| asset.family() == FormatFamily::Image)
        );
        assert!(
            assets
                .iter()
                .any(|asset| asset.kind() == AssetKind::Archive)
        );

        stdfs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn does_not_follow_symlinks_by_default() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        stdfs::create_dir_all(&root).unwrap();
        stdfs::write(root.join("target.txt"), "target").unwrap();
        symlink(root.join("target.txt"), root.join("link.txt")).unwrap();

        let (tx, mut rx) = mpsc::channel(32);
        let report = IntakeScanner::default()
            .scan(vec![root.clone()], ScanOptions::default(), tx)
            .await
            .unwrap();
        while rx.recv().await.is_some() {}

        assert_eq!(report.stats.symlinks, 1);
        assert_eq!(report.stats.files, 1);

        stdfs::remove_dir_all(root).unwrap();
    }
}
