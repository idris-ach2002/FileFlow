use fileflow_intake::{IntakeEvent, IntakeScanner, ScanOptions};
use std::{fs, path::PathBuf, time::Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

fn stress_root() -> PathBuf {
    std::env::temp_dir().join(format!("fileflow-intake-stress-{}", Uuid::new_v4()))
}

#[tokio::test]
#[ignore = "manual stress harness; run with --ignored --nocapture"]
async fn scans_large_directory_without_unbounded_ipc_buffering() {
    let file_count = std::env::var("FILEFLOW_STRESS_FILES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20_000);
    let root = stress_root();
    fs::create_dir_all(&root).unwrap();

    for index in 0..file_count {
        fs::write(root.join(format!("item-{index:06}.txt")), b"FileFlow stress fixture").unwrap();
    }

    let (tx, mut rx) = mpsc::channel(4);
    let consumer = tokio::spawn(async move {
        let mut batches = 0_usize;
        let mut streamed_assets = 0_usize;
        while let Some(event) = rx.recv().await {
            if let IntakeEvent::Batch { assets, .. } = event {
                batches += 1;
                streamed_assets += assets.len();
            }
        }
        (batches, streamed_assets)
    });

    let started = Instant::now();
    let report = IntakeScanner::default()
        .scan(vec![root.clone()], ScanOptions::default(), tx)
        .await
        .unwrap();
    let elapsed = started.elapsed();
    let (batches, streamed_assets) = consumer.await.unwrap();

    println!(
        "FileFlow intake stress: {file_count} files, {streamed_assets} assets, {batches} batches, {elapsed:?}"
    );

    assert_eq!(report.stats.files as usize, file_count);
    assert_eq!(streamed_assets, file_count + 1); // root directory + files
    assert!(batches > 1);

    fs::remove_dir_all(root).unwrap();
}
