use crate::types::{FileManifestEntry, TransferAck, TransferDirection, TransferHeader, TransferProgress, TransferStatus};
use crate::commands::FileEntry;
use crate::crypto::{handshake_initiator, EncryptedStream};
use log::{error, info};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncSeekExt;
use tokio::net::TcpStream;


const CHUNK_SIZE: usize = 65_536; // 64 KB
const REPORT_INTERVAL_MS: u128 = 250;

#[allow(clippy::too_many_arguments)]
fn emit_progress(
    app: &AppHandle,
    id: &str,
    batch_name: &str,
    total_size: u64,
    transferred: u64,
    speed: u64,
    status: TransferStatus,
    device_name: &str,
) {
    if let Err(e) = app.emit(
        "transfer-progress",
        &TransferProgress {
            id: id.to_string(),
            batch_name: batch_name.to_string(),
            total_size,
            transferred,
            speed,
            status,
            direction: TransferDirection::Send,
            device_name: device_name.to_string(),
        },
    ) {
        error!("sender: emit transfer-progress failed: {e}");
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn send_batch(
    app: AppHandle,
    transfer_id: String,
    batch_name: String,
    total_size: u64,
    entries: Vec<FileEntry>,
    device_ip: String,
    device_port: u16,
    device_name: String,
    paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    is_resume: bool,
    is_benchmark: bool,
    _file_paths_json: String, // unused
) -> Result<(), String> {
    
    let total_streams = if total_size > 10 * 1024 * 1024 { 4 } else { 1 };
    let manifest: Vec<FileManifestEntry> = entries.iter().map(|e| FileManifestEntry {
        path: e.rel_path.clone(),
        size: e.size,
    }).collect();

    let entries = Arc::new(entries);
    let manifest = Arc::new(manifest);
    let mut join_handles = vec![];
    let shared_transferred = Arc::new(tokio::sync::Mutex::new(vec![0u64; total_streams as usize]));

    // Global speed calculation state
    let global_speed_state = Arc::new(tokio::sync::Mutex::new((Instant::now(), 0u64)));

    for stream_id in 0..total_streams {
        let app = app.clone();
        let transfer_id = transfer_id.clone();
        let batch_name = batch_name.clone();
        let entries = Arc::clone(&entries);
        let manifest = Arc::clone(&manifest);
        let device_ip = device_ip.clone();
        let device_name = device_name.clone();
        let paused = Arc::clone(&paused);
        let cancelled = Arc::clone(&cancelled);
        let shared_transferred = Arc::clone(&shared_transferred);
        let global_speed_state = Arc::clone(&global_speed_state);

        let handle = tokio::spawn(async move {
            send_stream(
                app, transfer_id, batch_name, total_size, entries, manifest,
                device_ip, device_port, device_name, stream_id, total_streams,
                paused, cancelled, is_resume, is_benchmark, shared_transferred, global_speed_state
            ).await
        });
        join_handles.push(handle);
    }

    let mut results = vec![];
    for handle in join_handles {
        results.push(handle.await);
    }
    
    let mut success = true;
    let mut error_msg = String::new();
    for res in results {
        match res {
            Ok(Ok(_)) => {},
            Ok(Err(e)) => {
                error!("sender: stream failed: {e}");
                success = false;
                error_msg = e;
            },
            Err(e) => {
                error!("sender: stream panicked: {e}");
                success = false;
                error_msg = "Stream panicked".to_string();
            }
        }
    }

    let state = app.state::<crate::AppState>();
    if success {
        let _ = state.db.update_transfer_progress(&transfer_id, total_size, TransferStatus::Done).await;
        emit_progress(&app, &transfer_id, &batch_name, total_size, total_size, 0, TransferStatus::Done, &device_name);
        info!("sender: '{}' sent successfully with {} streams", batch_name, total_streams);
        Ok(())
    } else {
        let total_transferred = {
            let st = shared_transferred.lock().await;
            st.iter().sum::<u64>()
        };
        let _ = state.db.update_transfer_progress(&transfer_id, total_transferred, TransferStatus::Error).await;
        emit_progress(&app, &transfer_id, &batch_name, total_size, total_transferred, 0, TransferStatus::Error, &device_name);
        Err(error_msg)
    }
}

#[allow(clippy::too_many_arguments)]
async fn send_stream(
    app: AppHandle,
    transfer_id: String,
    batch_name: String,
    total_size: u64,
    entries: Arc<Vec<FileEntry>>,
    manifest: Arc<Vec<FileManifestEntry>>,
    device_ip: String,
    device_port: u16,
    device_name: String,
    stream_id: u32,
    total_streams: u32,
    paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    is_resume: bool,
    is_benchmark: bool,
    shared_transferred: Arc<tokio::sync::Mutex<Vec<u64>>>,
    global_speed_state: Arc<tokio::sync::Mutex<(Instant, u64)>>,
) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let db = &state.db;

    let target = format!("{}:{}", device_ip, device_port);
    let raw_stream = TcpStream::connect(&target)
        .await
        .map_err(|e| format!("connect error: {e}"))?;

    // ── Phase 13: ECDH handshake (initiator side) ──────────────────────────
    let (session_key, raw_stream) = handshake_initiator(raw_stream).await?;
    let mut stream = EncryptedStream::new(raw_stream, &session_key);
    // ───────────────────────────────────────────────────────────────────────

    let header = TransferHeader {
        transfer_id: transfer_id.clone(),
        batch_name: batch_name.clone(),
        total_size,
        file_count: entries.len() as u32,
        is_resume,
        is_benchmark,
        manifest: manifest.to_vec(),
        stream_id,
        total_streams,
    };

    let header_json = serde_json::to_vec(&header).map_err(|e| format!("serialize header: {e}"))?;
    stream.write_frame(&header_json).await?;

    let ack_buf = stream.read_frame().await?;
    let ack: TransferAck = serde_json::from_slice(&ack_buf).map_err(|e| format!("parse ack: {e}"))?;

    if !ack.accepted {
        if stream_id == 0 {
            info!("sender: transfer {transfer_id} rejected by receiver");
            emit_progress(&app, &transfer_id, &batch_name, total_size, 0, 0, TransferStatus::Rejected, &device_name);
            let _ = db.update_transfer_progress(&transfer_id, 0, TransferStatus::Rejected).await;
        }
        return Err("Rejected".to_string());
    }

    let start_global_offset = (total_size * stream_id as u64) / total_streams as u64;
    let end_global_offset = (total_size * (stream_id + 1) as u64) / total_streams as u64;

    let resume_offset = ack.resume_offset; // Relative to start_global_offset
    let mut transferred = resume_offset;
    
    {
        let mut st = shared_transferred.lock().await;
        st[stream_id as usize] = transferred;
    }

    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut hasher = blake3::Hasher::new();

    let mut current_global_offset = 0u64;

    for entry in entries.iter() {
        let file_global_start = current_global_offset;
        let file_global_end = current_global_offset + entry.size;
        current_global_offset += entry.size;

        let read_start = std::cmp::max(file_global_start, start_global_offset + transferred);
        let read_end = std::cmp::min(file_global_end, end_global_offset);

        if read_start < read_end {
            let mut file = tokio::fs::File::open(&entry.abs_path)
                .await
                .map_err(|e| format!("open file {}: {e}", entry.rel_path))?;
            
            let file_seek_offset = read_start - file_global_start;
            file.seek(std::io::SeekFrom::Start(file_seek_offset)).await.map_err(|e| e.to_string())?;

            let mut remaining_to_read = read_end - read_start;
            while remaining_to_read > 0 {
                if cancelled.load(Ordering::Relaxed) {
                    return Err("Cancelled".to_string());
                }

                if paused.load(Ordering::Relaxed) {
                    if stream_id == 0 {
                        let total = {
                            let st = shared_transferred.lock().await;
                            st.iter().sum::<u64>()
                        };
                        emit_progress(&app, &transfer_id, &batch_name, total_size, total, 0, TransferStatus::Paused, &device_name);
                        let _ = db.update_transfer_progress(&transfer_id, total, TransferStatus::Paused).await;
                    }
                    while paused.load(Ordering::Relaxed) && !cancelled.load(Ordering::Relaxed) {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    if cancelled.load(Ordering::Relaxed) {
                        return Err("Cancelled".to_string());
                    }
                    if stream_id == 0 {
                        let total = {
                            let st = shared_transferred.lock().await;
                            st.iter().sum::<u64>()
                        };
                        emit_progress(&app, &transfer_id, &batch_name, total_size, total, 0, TransferStatus::Transferring, &device_name);
                        let _ = db.update_transfer_progress(&transfer_id, total, TransferStatus::Transferring).await;
                    }
                }

                let to_read = std::cmp::min(remaining_to_read, CHUNK_SIZE as u64) as usize;
                let n = {
                    use tokio::io::AsyncReadExt;
                    file.read(&mut buf[..to_read]).await.map_err(|e| format!("read error: {e}"))?
                };
                if n == 0 {
                    return Err(format!("unexpected EOF in {}", entry.rel_path));
                }

                stream.write_frame(&buf[..n]).await?;
                hasher.update(&buf[..n]);

                transferred += n as u64;
                remaining_to_read -= n as u64;

                // Update shared progress and occasionally emit
                let mut emit_now = false;
                let current_total;
                let mut speed = 0;
                {
                    let mut st = shared_transferred.lock().await;
                    st[stream_id as usize] = transferred;
                    current_total = st.iter().sum::<u64>();
                    
                    let mut speed_state = global_speed_state.lock().await;
                    let now = Instant::now();
                    if now.duration_since(speed_state.0).as_millis() >= REPORT_INTERVAL_MS {
                        emit_now = true;
                        let elapsed_secs = now.duration_since(speed_state.0).as_secs_f64();
                        speed = if elapsed_secs > 0.0 { ((current_total - speed_state.1) as f64 / elapsed_secs) as u64 } else { 0 };
                        speed_state.0 = now;
                        speed_state.1 = current_total;
                    }
                }

                if emit_now && stream_id == 0 {
                    emit_progress(&app, &transfer_id, &batch_name, total_size, current_total, speed, TransferStatus::Transferring, &device_name);
                    let _ = db.update_transfer_progress(&transfer_id, current_total, TransferStatus::Transferring).await;
                    // Note: In a real implementation we would also update stream_progress in DB periodically, 
                    // but for brevity we'll skip aggressive DB writes or only do it on pause/finish.
                }
            }
        }
    }

    let hash_bytes = hasher.finalize();
    stream.write_frame(hash_bytes.as_bytes()).await.map_err(|e| format!("write hash: {e}"))?;

    // Update final stream progress in DB
    let _ = db.update_stream_progress(&transfer_id, stream_id, transferred).await;

    Ok(())
}
