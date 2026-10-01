use crate::types::{TransferDirection, TransferHeader, TransferProgress, TransferRequest, TransferStatus, TransferAck};
use log::{error, info, warn};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const CHUNK_SIZE: usize = 65_536; // 64 KB
const REPORT_INTERVAL_MS: u128 = 250;

/// Bind a TCP listener on an OS-assigned port and start accepting incoming
/// file transfers in the background. Returns the bound port.
pub async fn start_receiver(
    app: AppHandle,
    pending_requests: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
) -> Result<u16, String> {
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("bind receiver: {e}"))?;

    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();

    info!("receiver: listening on port {port}");

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    info!("receiver: incoming connection from {peer}");
                    let app_clone = app.clone();
                    let pending_clone = std::sync::Arc::clone(&pending_requests);
                    tokio::spawn(async move {
                        if let Err(e) = handle_incoming(app_clone, pending_clone, stream, peer.ip().to_string()).await {
                            error!("receiver: transfer error from {peer}: {e}");
                        }
                    });
                }
                Err(e) => error!("receiver: accept error: {e}"),
            }
        }
    });

    Ok(port)
}

async fn handle_incoming(
    app: AppHandle,
    pending_requests: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
    mut stream: TcpStream,
    peer_ip: String,
) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let db = &state.db;

    // --- Read length-prefixed JSON header ---
    let mut len_bytes = [0u8; 4];
    stream.read_exact(&mut len_bytes).await.map_err(|e| format!("read header len: {e}"))?;
    let header_len = u32::from_be_bytes(len_bytes) as usize;

    let mut header_buf = vec![0u8; header_len];
    stream.read_exact(&mut header_buf).await.map_err(|e| format!("read header: {e}"))?;

    let header: TransferHeader = serde_json::from_slice(&header_buf).map_err(|e| format!("parse header: {e}"))?;

    info!("receiver: '{}' (resume: {}) from {}", header.batch_name, header.is_resume, peer_ip);

    let mut resume_offset = 0u64;
    let mut download_dir = dirs::download_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut single_file_unique_path = None;

    if header.is_resume {
        if let Some(record) = db.get_transfer(&header.transfer_id).await.map_err(|e| format!("db error: {e}"))? {
            resume_offset = record.transferred as u64;
            let path = std::path::PathBuf::from(&record.local_path);
            
            if header.file_count > 1 {
                download_dir = path;
            } else {
                single_file_unique_path = Some(path.clone());
                download_dir = path.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf();
            }

            let ack = TransferAck { accepted: true, resume_offset };
            let ack_buf = serde_json::to_vec(&ack).unwrap();
            let ack_len = (ack_buf.len() as u32).to_be_bytes();
            stream.write_all(&ack_len).await.map_err(|e| format!("write ack len: {e}"))?;
            stream.write_all(&ack_buf).await.map_err(|e| format!("write ack: {e}"))?;
            
            let _ = db.update_transfer_progress(&header.transfer_id, resume_offset, TransferStatus::Transferring).await;
        } else {
            let ack = TransferAck { accepted: false, resume_offset: 0 };
            let ack_buf = serde_json::to_vec(&ack).unwrap();
            let ack_len = (ack_buf.len() as u32).to_be_bytes();
            let _ = stream.write_all(&ack_len).await;
            let _ = stream.write_all(&ack_buf).await;
            return Err("resume failed: unknown transfer id".into());
        }
    } else {
        app.emit("transfer-request", &TransferRequest {
            id: header.transfer_id.clone(),
            batch_name: header.batch_name.clone(),
            total_size: header.total_size,
            file_count: header.file_count,
            sender_ip: peer_ip.clone(),
            sender_name: peer_ip.clone(),
        }).ok();

        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut pending = pending_requests.lock().map_err(|e| e.to_string())?;
            pending.insert(header.transfer_id.clone(), tx);
        }

        let approved = match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
            Ok(Ok(res)) => res,
            _ => false,
        };

        if !approved {
            let ack = TransferAck { accepted: false, resume_offset: 0 };
            let ack_buf = serde_json::to_vec(&ack).unwrap();
            let ack_len = (ack_buf.len() as u32).to_be_bytes();
            let _ = stream.write_all(&ack_len).await;
            let _ = stream.write_all(&ack_buf).await;
            
            info!("receiver: transfer {} rejected", header.transfer_id);
            emit_progress(&app, &header, 0, 0, TransferStatus::Rejected, &peer_ip);
            if let Ok(mut pending) = pending_requests.lock() {
                pending.remove(&header.transfer_id);
            }
            return Ok(());
        }

        let ack = TransferAck { accepted: true, resume_offset: 0 };
        let ack_buf = serde_json::to_vec(&ack).unwrap();
        let ack_len = (ack_buf.len() as u32).to_be_bytes();
        stream.write_all(&ack_len).await.map_err(|e| format!("write ack len: {e}"))?;
        stream.write_all(&ack_buf).await.map_err(|e| format!("write ack: {e}"))?;

        if header.file_count > 1 {
            let safe_dir_name = header.batch_name.replace(|c: char| !c.is_alphanumeric() && c != ' ' && c != '-', "_");
            download_dir = unique_path(&download_dir, &safe_dir_name);
            tokio::fs::create_dir_all(&download_dir).await.map_err(|e| format!("create batch dir: {e}"))?;
        } else {
            let path = unique_path(&download_dir, &header.batch_name);
            single_file_unique_path = Some(path.clone());
            download_dir = path.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf();
        }

        let final_path = if header.file_count > 1 {
            download_dir.clone()
        } else {
            single_file_unique_path.clone().unwrap()
        };

        let record = crate::storage::db::TransferRecord {
            id: header.transfer_id.clone(),
            batch_name: header.batch_name.clone(),
            total_size: header.total_size as i64,
            file_count: header.file_count as i64,
            transferred: 0,
            status: "transferring".to_string(),
            direction: "receive".to_string(),
            device_name: peer_ip.clone(),
            local_path: final_path.to_string_lossy().to_string(),
        };
        db.insert_transfer(&record).await.map_err(|e| format!("db insert: {e}"))?;
    }

    emit_progress(&app, &header, resume_offset, 0, TransferStatus::Transferring, &peer_ip);

    let mut hasher = blake3::Hasher::new();
    let mut transferred = resume_offset;
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut last_report = Instant::now();
    let mut last_bytes = transferred;
    
    let is_single_file = header.file_count == 1;
    let mut stream_offset = 0u64;

    for _ in 0..header.file_count {
        let mut path_len_bytes = [0u8; 4];
        stream.read_exact(&mut path_len_bytes).await.map_err(|e| format!("read path len: {e}"))?;
        let path_len = u32::from_be_bytes(path_len_bytes) as usize;

        let mut path_bytes = vec![0u8; path_len];
        stream.read_exact(&mut path_bytes).await.map_err(|e| format!("read path: {e}"))?;

        let mut size_bytes = [0u8; 8];
        stream.read_exact(&mut size_bytes).await.map_err(|e| format!("read size: {e}"))?;
        let file_size = u64::from_be_bytes(size_bytes);

        hasher.update(&path_len_bytes);
        hasher.update(&path_bytes);
        hasher.update(&size_bytes);

        let rel_path_str = String::from_utf8(path_bytes).map_err(|e| format!("invalid path string: {e}"))?;
        
        let safe_rel_path = std::path::Path::new(&rel_path_str)
            .components()
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .collect::<std::path::PathBuf>();

        let out_path = if is_single_file {
            single_file_unique_path.clone().unwrap()
        } else {
            download_dir.join(safe_rel_path)
        };

        if stream_offset + file_size <= resume_offset {
            // Entire file is skipped, hash it from disk
            if let Ok(mut file) = tokio::fs::File::open(&out_path).await {
                let mut f_buf = vec![0u8; CHUNK_SIZE];
                while let Ok(n) = file.read(&mut f_buf).await {
                    if n == 0 { break; }
                    hasher.update(&f_buf[..n]);
                }
            } else {
                let _ = db.update_transfer_progress(&header.transfer_id, transferred, TransferStatus::Error).await;
                emit_progress(&app, &header, transferred, 0, TransferStatus::Error, &peer_ip);
                return Err("resume failed: could not read skipped file for hashing".into());
            }
            stream_offset += file_size;
            continue;
        }

        let file_resume_offset = if stream_offset < resume_offset {
            resume_offset - stream_offset
        } else { 0 };

        if let Some(parent) = out_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        let mut file = if file_resume_offset > 0 {
            let f = tokio::fs::OpenOptions::new().append(true).open(&out_path).await.map_err(|e| format!("open append: {e}"))?;
            if let Ok(mut read_f) = tokio::fs::File::open(&out_path).await {
                let mut f_buf = vec![0u8; CHUNK_SIZE];
                let mut read_so_far = 0;
                while read_so_far < file_resume_offset {
                    let to_read = std::cmp::min(CHUNK_SIZE as u64, file_resume_offset - read_so_far) as usize;
                    if let Ok(n) = read_f.read(&mut f_buf[..to_read]).await {
                        if n == 0 { break; }
                        hasher.update(&f_buf[..n]);
                        read_so_far += n as u64;
                    } else { break; }
                }
            }
            f
        } else {
            tokio::fs::File::create(&out_path).await.map_err(|e| format!("create file: {e}"))?
        };

        let mut file_transferred = file_resume_offset;
        while file_transferred < file_size {
            let remaining = file_size - file_transferred;
            #[allow(clippy::cast_possible_truncation)]
            let to_read = std::cmp::min(remaining, CHUNK_SIZE as u64) as usize;

            let n = match stream.read(&mut buf[..to_read]).await {
                Ok(n) => n,
                Err(e) => {
                    let _ = db.update_transfer_progress(&header.transfer_id, transferred, TransferStatus::Error).await;
                    emit_progress(&app, &header, transferred, 0, TransferStatus::Error, &peer_ip);
                    return Err(format!("read chunk: {e}"));
                }
            };
            if n == 0 {
                let _ = db.update_transfer_progress(&header.transfer_id, transferred, TransferStatus::Error).await;
                emit_progress(&app, &header, transferred, 0, TransferStatus::Error, &peer_ip);
                return Err("unexpected EOF during file stream".into());
            }

            if let Err(e) = file.write_all(&buf[..n]).await {
                let _ = db.update_transfer_progress(&header.transfer_id, transferred, TransferStatus::Error).await;
                emit_progress(&app, &header, transferred, 0, TransferStatus::Error, &peer_ip);
                return Err(format!("write file chunk: {e}"));
            }
            hasher.update(&buf[..n]);

            file_transferred += n as u64;
            transferred += n as u64;

            let now = Instant::now();
            if now.duration_since(last_report).as_millis() >= REPORT_INTERVAL_MS {
                let elapsed = now.duration_since(last_report).as_secs_f64();
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                let speed = if elapsed > 0.0 { ((transferred - last_bytes) as f64 / elapsed) as u64 } else { 0 };
                last_bytes = transferred;
                last_report = now;
                emit_progress(&app, &header, transferred, speed, TransferStatus::Transferring, &peer_ip);
                let _ = db.update_transfer_progress(&header.transfer_id, transferred, TransferStatus::Transferring).await;
            }
        }
        stream_offset += file_size;
    }

    let mut trailer = [0u8; 32];
    if let Err(e) = stream.read_exact(&mut trailer).await {
        let _ = db.update_transfer_progress(&header.transfer_id, transferred, TransferStatus::Error).await;
        emit_progress(&app, &header, transferred, 0, TransferStatus::Error, &peer_ip);
        return Err(format!("read hash trailer: {e}"));
    }

    let actual_hash = hasher.finalize();
    if actual_hash.as_bytes() == &trailer {
        info!("receiver: '{}' verified OK", header.batch_name);
        emit_progress(&app, &header, transferred, 0, TransferStatus::Done, &peer_ip);
        let _ = db.update_transfer_progress(&header.transfer_id, transferred, TransferStatus::Done).await;
    } else {
        warn!("receiver: hash mismatch for '{}'", header.batch_name);
        emit_progress(&app, &header, transferred, 0, TransferStatus::Error, &peer_ip);
        let _ = db.update_transfer_progress(&header.transfer_id, transferred, TransferStatus::Error).await;
        // In resumable transfer, keeping the corrupted part allows resume to just overwrite if we tracked it per-file,
        // but since we hash the whole stream, a mismatch means the *whole* stream is invalid. 
        // We could just mark it Error and let them resume (which would be broken unless we rewind).
        // Since we don't support rewinding safely right now, if a hash mismatch happens, it's pretty fatal.
        // Let's not delete the file automatically. The user can retry.
    }

    Ok(())
}

fn emit_progress(
    app: &AppHandle,
    header: &TransferHeader,
    transferred: u64,
    speed: u64,
    status: TransferStatus,
    device_name: &str,
) {
    if let Err(e) = app.emit(
        "transfer-progress",
        &TransferProgress {
            id: header.transfer_id.clone(),
            batch_name: header.batch_name.clone(),
            total_size: header.total_size,
            transferred,
            speed,
            status,
            direction: TransferDirection::Receive,
            device_name: device_name.to_string(),
        },
    ) {
        error!("receiver: emit failed: {e}");
    }
}

/// Build a non-conflicting output path by appending (1), (2), … if needed.
fn unique_path(dir: &std::path::Path, file_name: &str) -> std::path::PathBuf {
    let base = dir.join(file_name);
    if !base.exists() { return base; }

    let stem = std::path::Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string());
    let ext = std::path::Path::new(file_name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    for i in 1u32.. {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() { return candidate; }
    }

    base
}
