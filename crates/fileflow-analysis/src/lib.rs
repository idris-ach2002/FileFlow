//! Native, read-only analysis helpers for large workspaces.
//!
//! Duplicate confirmation is intentionally staged: file size first, then a
//! bounded first/last-block fingerprint, and only then a complete SHA-256
//! digest. The expensive full read is therefore limited to credible matches.

use fileflow_domain::AssetId;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const QUICK_BLOCK_BYTES: usize = 64 * 1024;
const FULL_HASH_BUFFER_BYTES: usize = 1024 * 1024;

type DigestBytes = [u8; 32];

#[derive(Debug, Clone)]
pub struct DuplicateInput {
    pub asset_id: AssetId,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateAsset {
    pub asset_id: AssetId,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub hash: String,
    pub size_bytes: u64,
    pub reclaimable_bytes: u64,
    pub assets: Vec<DuplicateAsset>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisWarning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateReport {
    pub input_files: usize,
    pub size_candidate_files: usize,
    pub quick_candidate_files: usize,
    pub fully_hashed_files: usize,
    pub confirmed_groups: Vec<DuplicateGroup>,
    pub reclaimable_bytes: u64,
    pub warnings: Vec<AnalysisWarning>,
}

pub fn confirm_duplicates(inputs: Vec<DuplicateInput>, threads: usize) -> DuplicateReport {
    confirm_duplicates_inner(inputs, threads.clamp(1, 32))
}

fn confirm_duplicates_inner(inputs: Vec<DuplicateInput>, threads: usize) -> DuplicateReport {
    let input_files = inputs.len();
    let mut by_size = HashMap::<u64, Vec<DuplicateInput>>::new();
    for input in inputs {
        by_size.entry(input.size_bytes).or_default().push(input);
    }
    let size_candidates = by_size
        .into_values()
        .filter(|group| group.len() > 1)
        .flatten()
        .collect::<Vec<_>>();
    let size_candidate_files = size_candidates.len();

    let quick_results = parallel_hash(&size_candidates, threads, |input| {
        quick_hash(&input.path, input.size_bytes)
    });
    let mut warnings = Vec::new();
    let mut by_quick = HashMap::<(u64, DigestBytes), Vec<DuplicateInput>>::new();
    for (input, result) in quick_results {
        match result {
            Ok(hash) => by_quick
                .entry((input.size_bytes, hash))
                .or_default()
                .push(input),
            Err(error) => warnings.push(AnalysisWarning {
                path: input.path,
                message: error.to_string(),
            }),
        }
    }
    let quick_candidates = by_quick
        .into_values()
        .filter(|group| group.len() > 1)
        .flatten()
        .collect::<Vec<_>>();
    let quick_candidate_files = quick_candidates.len();

    let full_results = parallel_hash(&quick_candidates, threads, |input| full_hash(&input.path));
    let fully_hashed_files = full_results.len();
    let mut by_full = HashMap::<(u64, DigestBytes), Vec<DuplicateInput>>::new();
    for (input, result) in full_results {
        match result {
            Ok(hash) => by_full
                .entry((input.size_bytes, hash))
                .or_default()
                .push(input),
            Err(error) => warnings.push(AnalysisWarning {
                path: input.path,
                message: error.to_string(),
            }),
        }
    }

    let mut confirmed_groups = by_full
        .into_iter()
        .filter_map(|((size_bytes, hash), group)| {
            (group.len() > 1).then(|| DuplicateGroup {
                hash: digest_hex(&hash),
                size_bytes,
                reclaimable_bytes: size_bytes.saturating_mul(group.len().saturating_sub(1) as u64),
                assets: group
                    .into_iter()
                    .map(|input| DuplicateAsset {
                        asset_id: input.asset_id,
                        path: input.path,
                        size_bytes: input.size_bytes,
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    confirmed_groups.sort_by_key(|group| std::cmp::Reverse(group.reclaimable_bytes));
    let reclaimable_bytes = confirmed_groups.iter().fold(0_u64, |sum, group| {
        sum.saturating_add(group.reclaimable_bytes)
    });

    DuplicateReport {
        input_files,
        size_candidate_files,
        quick_candidate_files,
        fully_hashed_files,
        confirmed_groups,
        reclaimable_bytes,
        warnings,
    }
}

fn parallel_hash<F>(
    inputs: &[DuplicateInput],
    threads: usize,
    hash: F,
) -> Vec<(DuplicateInput, std::io::Result<DigestBytes>)>
where
    F: Fn(&DuplicateInput) -> std::io::Result<DigestBytes> + Sync,
{
    if inputs.is_empty() {
        return Vec::new();
    }
    let worker_count = threads.min(inputs.len()).max(1);
    let chunk_size = inputs.len().div_ceil(worker_count);
    std::thread::scope(|scope| {
        let handles = inputs
            .chunks(chunk_size)
            .map(|chunk| {
                let hash = &hash;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|input| (input.clone(), hash(input)))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("duplicate hash worker panicked"))
            .collect()
    })
}

fn quick_hash(path: &Path, size_bytes: u64) -> std::io::Result<DigestBytes> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    hasher.update(size_bytes.to_le_bytes());
    let first_len = size_bytes.min(QUICK_BLOCK_BYTES as u64) as usize;
    let mut first = vec![0_u8; first_len];
    let first_read = file.read(&mut first)?;
    hasher.update(&first[..first_read]);

    if size_bytes > QUICK_BLOCK_BYTES as u64 {
        let tail_len = size_bytes.min(QUICK_BLOCK_BYTES as u64) as usize;
        file.seek(SeekFrom::End(-(tail_len as i64)))?;
        let mut tail = vec![0_u8; tail_len];
        let tail_read = file.read(&mut tail)?;
        hasher.update(&tail[..tail_read]);
    }
    Ok(hasher.finalize().into())
}

fn full_hash(path: &Path) -> std::io::Result<DigestBytes> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; FULL_HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn digest_hex(hash: &DigestBytes) -> String {
    let mut text = String::with_capacity(hash.len() * 2);
    for byte in hash {
        use std::fmt::Write as _;
        let _ = write!(&mut text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirms_only_byte_identical_files() {
        let root = std::env::temp_dir().join(format!("fileflow-dupes-{}", uuid_for_test()));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("a.bin");
        let b = root.join("b.bin");
        let c = root.join("c.bin");
        std::fs::write(&a, b"identical-content").unwrap();
        std::fs::write(&b, b"identical-content").unwrap();
        std::fs::write(&c, b"different-content").unwrap();
        let report = confirm_duplicates(
            vec![input_with_size(a), input_with_size(b), input_with_size(c)],
            2,
        );
        assert_eq!(report.confirmed_groups.len(), 1);
        assert_eq!(report.confirmed_groups[0].assets.len(), 2);
        assert_eq!(report.confirmed_groups[0].hash.len(), 64);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parallel_scan_handles_empty_input() {
        let report = confirm_duplicates(Vec::new(), 8);
        assert!(report.confirmed_groups.is_empty());
        assert_eq!(report.fully_hashed_files, 0);
    }

    fn input_with_size(path: PathBuf) -> DuplicateInput {
        let size_bytes = std::fs::metadata(&path).unwrap().len();
        DuplicateInput {
            asset_id: AssetId::new(),
            path,
            size_bytes,
        }
    }

    fn uuid_for_test() -> String {
        format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }
}
