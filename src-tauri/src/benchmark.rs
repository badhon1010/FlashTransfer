use crate::AppState;
use tauri::{AppHandle, State};
use std::path::PathBuf;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use crate::commands::FileEntry;
use crate::transfer::sender;

#[derive(serde::Serialize)]
pub struct BenchmarkResult {
    pub file_size_mb: u32,
    pub time_ms: u64,
    pub mb_per_sec: f64,
}

#[tauri::command]
pub async fn run_benchmark(
    app: AppHandle,
    state: State<'_, AppState>,
    size_mb: u32,
) -> Result<BenchmarkResult, String> {
    let temp_dir = std::env::temp_dir().join("flashtransfer_bench");
    tokio::fs::create_dir_all(&temp_dir).await.map_err(|e| e.to_string())?;
    
    let file_path = temp_dir.join(format!("bench_{}MB.bin", size_mb));
    let total_size = (size_mb as u64) * 1024 * 1024;

    // 1. Create dummy file
    {
        let mut file = tokio::fs::File::create(&file_path).await.map_err(|e| e.to_string())?;
        // Write in 1MB chunks to avoid large memory allocations
        let chunk = vec![0x42u8; 1024 * 1024];
        let mut written = 0;
        while written < total_size {
            let to_write = std::cmp::min(total_size - written, chunk.len() as u64);
            file.write_all(&chunk[..to_write as usize]).await.map_err(|e| e.to_string())?;
            written += to_write;
        }
        file.sync_all().await.map_err(|e| e.to_string())?;
    }

    let entries = vec![FileEntry {
        abs_path: file_path.clone(),
        rel_path: file_path.file_name().unwrap().to_string_lossy().into_owned(),
        size: total_size,
    }];

    // Get local receiver port
    let local_port = *state.local_port.lock().unwrap();

    let transfer_id = format!("bench-{}", uuid::Uuid::new_v4());
    
    let paused = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));

    let start_time = Instant::now();

    // 2. Run transfer to localhost loopback
    sender::send_batch(
        app.clone(),
        transfer_id,
        format!("Benchmark {}MB", size_mb),
        total_size,
        entries,
        "127.0.0.1".to_string(),
        local_port,
        "Localhost".to_string(),
        paused,
        cancelled,
        false, // is_resume
        true,  // is_benchmark
        "".to_string(),
    ).await?;

    let elapsed = start_time.elapsed();
    let time_ms = elapsed.as_millis() as u64;
    let mb_per_sec = (size_mb as f64) / (elapsed.as_secs_f64().max(0.001));

    // Cleanup
    let _ = tokio::fs::remove_file(&file_path).await;
    
    Ok(BenchmarkResult {
        file_size_mb: size_mb,
        time_ms,
        mb_per_sec,
    })
}
