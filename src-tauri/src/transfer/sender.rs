use crate::types::{TransferAck, TransferDirection, TransferProgress, TransferStatus};
use log::{error, info};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
    entries: Vec<crate::commands::FileEntry>,
    device_ip: String,
    device_port: u16,
    device_name: String,
    paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    is_resume: bool,
    file_paths_json: String,
) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let db = &state.db;

    if !is_resume {
        let record = crate::storage::db::TransferRecord {
            id: transfer_id.clone(),
            batch_name: batch_name.clone(),
            total_size: total_size as i64,
            file_count: entries.len() as i64,
            transferred: 0,
            status: "pending".to_string(),
            direction: "send".to_string(),
            device_name: device_name.clone(),
            local_path: file_paths_json, // We store the JSON list of root file paths here
        };
        db.insert_transfer(&record).await.map_err(|e| format!("db insert: {e}"))?;
    }

    emit_progress(&app, &transfer_id, &batch_name, total_size, 0, 0, TransferStatus::Pending, &device_name);

    info!("sender: connecting to {device_ip}:{device_port} (resume={is_resume})");
    let mut stream = TcpStream::connect(format!("{device_ip}:{device_port}"))
        .await
        .map_err(|e| format!("connect: {e}"))?;

    let header = crate::types::TransferHeader {
        transfer_id: transfer_id.clone(),
        batch_name: batch_name.clone(),
        total_size,
        file_count: entries.len() as u32,
        is_resume,
    };
    let header_json = serde_json::to_vec(&header).map_err(|e| format!("header serialize: {e}"))?;
    let header_len = (header_json.len() as u32).to_be_bytes();
    stream.write_all(&header_len).await.map_err(|e| format!("write header len: {e}"))?;
    stream.write_all(&header_json).await.map_err(|e| format!("write header: {e}"))?;

    // Read TransferAck
    let mut ack_len_bytes = [0u8; 4];
    stream.read_exact(&mut ack_len_bytes).await.map_err(|e| format!("read ack len: {e}"))?;
    let ack_len = u32::from_be_bytes(ack_len_bytes) as usize;
    
    let mut ack_buf = vec![0u8; ack_len];
    stream.read_exact(&mut ack_buf).await.map_err(|e| format!("read ack: {e}"))?;
    
    let ack: TransferAck = serde_json::from_slice(&ack_buf).map_err(|e| format!("parse ack: {e}"))?;

    if !ack.accepted {
        info!("sender: transfer {transfer_id} rejected by receiver");
        emit_progress(&app, &transfer_id, &batch_name, total_size, 0, 0, TransferStatus::Rejected, &device_name);
        let _ = db.update_transfer_progress(&transfer_id, 0, TransferStatus::Rejected).await;
        return Ok(());
    }

    let resume_offset = ack.resume_offset;
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut transferred = resume_offset;
    let mut last_report = Instant::now();
    let mut last_bytes = transferred;
    let mut hasher = blake3::Hasher::new();
    let mut stream_offset = 0u64;

    emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Transferring, &device_name);
    let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Transferring).await;

    for entry in entries {
        let path_bytes = entry.rel_path.as_bytes();
        let path_len = (path_bytes.len() as u32).to_be_bytes();
        let size_bytes = entry.size.to_be_bytes();

        stream.write_all(&path_len).await.map_err(|e| format!("write path len: {e}"))?;
        stream.write_all(path_bytes).await.map_err(|e| format!("write path: {e}"))?;
        stream.write_all(&size_bytes).await.map_err(|e| format!("write size: {e}"))?;

        hasher.update(&path_len);
        hasher.update(path_bytes);
        hasher.update(&size_bytes);

        if stream_offset + entry.size <= resume_offset {
            if let Ok(mut file) = tokio::fs::File::open(&entry.abs_path).await {
                let mut f_buf = vec![0u8; CHUNK_SIZE];
                while let Ok(n) = file.read(&mut f_buf).await {
                    if n == 0 { break; }
                    hasher.update(&f_buf[..n]);
                }
            } else {
                let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Error).await;
                emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Error, &device_name);
                return Err(format!("resume failed: could not read file {} for hashing", entry.rel_path));
            }
            stream_offset += entry.size;
            continue;
        }

        let file_resume_offset = if stream_offset < resume_offset {
            resume_offset - stream_offset
        } else { 0 };

        let mut file = tokio::fs::File::open(&entry.abs_path)
            .await
            .map_err(|e| format!("open file {}: {e}", entry.rel_path))?;

        if file_resume_offset > 0 {
            let mut f_buf = vec![0u8; CHUNK_SIZE];
            let mut read_so_far = 0;
            while read_so_far < file_resume_offset {
                let to_read = std::cmp::min(CHUNK_SIZE as u64, file_resume_offset - read_so_far) as usize;
                if let Ok(n) = file.read(&mut f_buf[..to_read]).await {
                    if n == 0 { break; }
                    hasher.update(&f_buf[..n]);
                    read_so_far += n as u64;
                } else { break; }
            }
        }

        let mut file_transferred = file_resume_offset;
        while file_transferred < entry.size {
            if cancelled.load(Ordering::Relaxed) {
                info!("sender: transfer {transfer_id} cancelled");
                emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Error, &device_name);
                let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Error).await;
                return Ok(());
            }

            if paused.load(Ordering::Relaxed) {
                emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Paused, &device_name);
                let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Paused).await;
                while paused.load(Ordering::Relaxed) && !cancelled.load(Ordering::Relaxed) {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                if cancelled.load(Ordering::Relaxed) {
                    emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Error, &device_name);
                    let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Error).await;
                    return Ok(());
                }
                last_report = Instant::now();
                last_bytes = transferred;
                emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Transferring, &device_name);
                let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Transferring).await;
            }

            let remaining = entry.size - file_transferred;
            #[allow(clippy::cast_possible_truncation)]
            let to_read = std::cmp::min(remaining, CHUNK_SIZE as u64) as usize;

            let n = match file.read(&mut buf[..to_read]).await {
                Ok(n) => n,
                Err(e) => {
                    let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Error).await;
                    emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Error, &device_name);
                    return Err(format!("read chunk: {e}"));
                }
            };

            if n == 0 {
                let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Error).await;
                emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Error, &device_name);
                return Err(format!("unexpected EOF in file {}", entry.rel_path));
            }

            if let Err(e) = stream.write_all(&buf[..n]).await {
                let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Error).await;
                emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Error, &device_name);
                return Err(format!("write chunk: {e}"));
            }
            
            hasher.update(&buf[..n]);

            file_transferred += n as u64;
            transferred += n as u64;

            let now = Instant::now();
            if now.duration_since(last_report).as_millis() >= REPORT_INTERVAL_MS {
                let elapsed_secs = now.duration_since(last_report).as_secs_f64();
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                let speed = if elapsed_secs > 0.0 { ((transferred - last_bytes) as f64 / elapsed_secs) as u64 } else { 0 };
                last_bytes = transferred;
                last_report = now;
                emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, speed, TransferStatus::Transferring, &device_name);
                let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Transferring).await;
            }
        }
        stream_offset += entry.size;
    }

    let hash_bytes = hasher.finalize();
    if let Err(e) = stream.write_all(hash_bytes.as_bytes()).await {
        let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Error).await;
        emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Error, &device_name);
        return Err(format!("write hash trailer: {e}"));
    }

    info!("sender: '{}' sent ({} bytes total)", batch_name, transferred);
    emit_progress(&app, &transfer_id, &batch_name, total_size, transferred, 0, TransferStatus::Done, &device_name);
    let _ = db.update_transfer_progress(&transfer_id, transferred, TransferStatus::Done).await;
    
    Ok(())
}
