use crate::types::{TransferAck, TransferHeader, TransferStatus};
use crate::storage::db::TransferRecord;
use crate::crypto::{handshake_responder, EncryptedStream};
use log::{error, info};
use tauri::{AppHandle, Manager, Emitter};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
fn emit_receiver_progress(
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
        &crate::types::TransferProgress {
            id: id.to_string(),
            batch_name: batch_name.to_string(),
            total_size,
            transferred,
            speed,
            status,
            direction: crate::types::TransferDirection::Receive,
            device_name: device_name.to_string(),
        },
    ) {
        error!("receiver: emit transfer-progress failed: {e}");
    }
}

fn resolve_conflict_file(path: &std::path::Path) -> std::path::PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("")).to_path_buf();
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();

    let mut counter = 1;
    loop {
        let new_name = format!("{} ({}){}", stem, counter, ext);
        let new_path = dir.join(new_name);
        if !new_path.exists() {
            return new_path;
        }
        counter += 1;
    }
}

fn resolve_conflict_dir(path: &std::path::Path) -> std::path::PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("")).to_path_buf();
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

    let mut counter = 1;
    loop {
        let new_name = format!("{} ({})", name, counter);
        let new_path = dir.join(new_name);
        if !new_path.exists() {
            return new_path;
        }
        counter += 1;
    }
}

pub async fn start_receiver(app: AppHandle) -> Result<u16, String> {
    let listener = TcpListener::bind("0.0.0.0:0").await.map_err(|e| format!("bind: {e}"))?;
    let port = listener.local_addr().unwrap().port();
    info!("receiver: listening on 0.0.0.0:{port}");

    // Channel for coordinating accepted transfers among streams
    let (tx_accept, _) = broadcast::channel::<(String, bool)>(16);

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let peer_ip = addr.ip().to_string();
                    let app_c = app.clone();
                    let tx = tx_accept.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(app_c, stream, peer_ip, tx).await {
                            error!("receiver: connection error: {e}");
                        }
                    });
                }
                Err(e) => {
                    error!("receiver: accept failed: {e}");
                }
            }
        }
    });

    Ok(port)
}

async fn handle_connection(
    app: AppHandle,
    raw_stream: tokio::net::TcpStream,
    peer_ip: String,
    tx_accept: broadcast::Sender<(String, bool)>
) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let db = &state.db;

    // â”€â”€ Phase 13: ECDH handshake (responder side) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let (session_key, inner_stream) = handshake_responder(raw_stream).await?;
    let mut stream = EncryptedStream::new(inner_stream, &session_key);
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    let header_buf = stream.read_frame().await?;
    let header: TransferHeader = serde_json::from_slice(&header_buf).map_err(|e| format!("parse header: {e}"))?;

    info!("receiver: '{}' stream {}/{} (resume: {}) from {}", header.batch_name, header.stream_id, header.total_streams, header.is_resume, peer_ip);

    match handle_connection_inner(&app, db, &mut stream, &header, &peer_ip, tx_accept).await {
        Ok(()) => Ok(()),
        Err(e) => {
            if header.stream_id == 0 {
                let _ = db.update_transfer_progress(&header.transfer_id, 0, TransferStatus::Error).await;
                emit_receiver_progress(&app, &header.transfer_id, &header.batch_name, header.total_size, 0, 0, TransferStatus::Error, &peer_ip);
            }
            Err(e)
        }
    }
}

async fn handle_connection_inner(
    app: &AppHandle,
    db: &crate::storage::db::Database,
    stream: &mut EncryptedStream<tokio::net::TcpStream>,
    header: &TransferHeader,
    peer_ip: &str,
    tx_accept: broadcast::Sender<(String, bool)>
) -> Result<(), String> {
    let mut download_dir = dirs::download_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut is_accepted = false;

    if header.is_benchmark {
        download_dir = std::env::temp_dir().join(format!("bench_{}", header.transfer_id));
        is_accepted = true;

        if header.stream_id == 0 {
            tokio::fs::create_dir_all(&download_dir).await.map_err(|e| format!("create bench dir: {e}"))?;
            let _ = tx_accept.send((header.transfer_id.clone(), true));
            for entry in &header.manifest {
                let path = download_dir.join(&entry.path);
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await.unwrap_or_default();
                }
                let _ = std::fs::File::create(&path);
            }
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    } else if header.is_resume {
        if let Some(record) = db.get_transfer(&header.transfer_id).await.map_err(|e| format!("db error: {e}"))? {
            let path = std::path::PathBuf::from(&record.local_path);
            if header.file_count > 1 {
                download_dir = path;
            } else {
                download_dir = path.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf();
            }
            is_accepted = true;
        } else {
            return Err("Resume requested but transfer not found in db".to_string());
        }
    } else {
        if header.stream_id == 0 {
            // Ask user for permission for the entire transfer
            let req = crate::types::TransferRequest {
                id: header.transfer_id.clone(),
                batch_name: header.batch_name.clone(),
                total_size: header.total_size,
                file_count: header.file_count,
                sender_ip: peer_ip.to_string(),
                sender_name: "Unknown".to_string(), // In full implementation, pass sender_name in header
            };

            let (tx, rx) = tokio::sync::oneshot::channel();
            {
                let state = app.state::<crate::AppState>();
                let mut pending = state.pending_requests.lock().map_err(|e| e.to_string())?;
                pending.insert(header.transfer_id.clone(), tx);
            }

            if let Err(e) = app.emit("transfer-request", &req) {
                error!("receiver: emit transfer-request failed: {e}");
            }

            match rx.await {
                Ok(accepted) => {
                    is_accepted = accepted;
                    let _ = tx_accept.send((header.transfer_id.clone(), accepted));
                    
                    if accepted {
                        let mut local_path_str = String::new();
                        
                        if header.file_count > 1 {
                            let batch_dir = download_dir.join(&header.batch_name);
                            download_dir = resolve_conflict_dir(&batch_dir);
                            tokio::fs::create_dir_all(&download_dir).await.map_err(|e| format!("create dir: {e}"))?;
                            local_path_str = download_dir.to_string_lossy().to_string();
                            
                            for entry in &header.manifest {
                                let path = download_dir.join(&entry.path);
                                if let Some(parent) = path.parent() {
                                    tokio::fs::create_dir_all(parent).await.unwrap_or_default();
                                }
                                let _ = std::fs::File::create(&path);
                            }
                        } else if header.file_count == 1 {
                            let file_path = download_dir.join(&header.manifest[0].path);
                            let resolved_path = resolve_conflict_file(&file_path);
                            local_path_str = resolved_path.to_string_lossy().to_string();
                            
                            if let Some(parent) = resolved_path.parent() {
                                tokio::fs::create_dir_all(parent).await.unwrap_or_default();
                            }
                            let _ = std::fs::File::create(&resolved_path);
                        }

                        let record = TransferRecord {
                            id: header.transfer_id.clone(),
                            batch_name: header.batch_name.clone(),
                            total_size: header.total_size as i64,
                            file_count: header.file_count as i64,
                            transferred: 0,
                            status: "transferring".to_string(),
                            direction: "receive".to_string(),
                            device_name: "Unknown".to_string(),
                            local_path: local_path_str,
                            created_at: None,
                            updated_at: None,
                        };
                        db.insert_transfer(&record).await.map_err(|e| format!("db insert: {e}"))?;
                    }
                }
                Err(_) => {
                    is_accepted = false;
                    let _ = tx_accept.send((header.transfer_id.clone(), false));
                }
            }
        } else {
            // Wait for stream 0 to be accepted or rejected
            let mut rx = tx_accept.subscribe();
            loop {
                match tokio::time::timeout(Duration::from_secs(60), rx.recv()).await {
                    Ok(Ok((tid, accepted))) => {
                        if tid == header.transfer_id {
                            is_accepted = accepted;
                            break;
                        }
                    }
                    Ok(Err(_)) => { break; } // channel closed or lagged
                    Err(_) => { break; } // timeout
                }
            }
        }
    }
    
    // Now that acceptance is determined (either stream_id=0 or others waited for it),
    // retrieve the final download_dir (if folder) or exact local_path (if file) from DB.
    let mut exact_file_path = None;
    if is_accepted {
        if let Some(record) = db.get_transfer(&header.transfer_id).await.unwrap_or(None) {
            let path = std::path::PathBuf::from(&record.local_path);
            if header.file_count > 1 {
                download_dir = path;
            } else {
                exact_file_path = Some(path);
            }
        } else {
            return Err("Accepted but transfer not found in db".to_string());
        }
    }

    let resume_offset = if is_accepted && header.is_resume {
        db.get_stream_progress(&header.transfer_id, header.stream_id).await.unwrap_or(0)
    } else {
        0
    };

    let ack = TransferAck { accepted: is_accepted, resume_offset };
    let ack_buf = serde_json::to_vec(&ack).map_err(|e| format!("serialize ack: {e}"))?;
    stream.write_frame(&ack_buf).await?;

    if !is_accepted {
        return Ok(());
    }

    let start_global_offset = (header.total_size * header.stream_id as u64) / header.total_streams as u64;
    let end_global_offset = (header.total_size * (header.stream_id + 1) as u64) / header.total_streams as u64;

    let mut transferred = resume_offset;
    let mut hasher = blake3::Hasher::new();

    let mut current_global_offset = 0u64;

    for entry in &header.manifest {
        let file_global_start = current_global_offset;
        let file_global_end = current_global_offset + entry.size;
        current_global_offset += entry.size;

        let write_start = std::cmp::max(file_global_start, start_global_offset + transferred);
        let write_end = std::cmp::min(file_global_end, end_global_offset);

        if write_start < write_end {
            let file_path = if header.file_count == 1 {
                exact_file_path.clone().unwrap()
            } else {
                download_dir.join(&entry.path)
            };
            
            // Open for read/write without truncating, creating if needed
            let mut file = tokio::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&file_path)
                .await
                .map_err(|e| format!("open file {}: {e}", entry.path))?;
            
            let file_seek_offset = write_start - file_global_start;
            file.seek(std::io::SeekFrom::Start(file_seek_offset)).await.map_err(|e| e.to_string())?;

            let mut remaining_to_write = write_end - write_start;
            while remaining_to_write > 0 {
                let chunk = stream.read_frame().await?;
                if chunk.is_empty() {
                    return Err(format!("unexpected empty frame in {}", entry.path));
                }

                let n = chunk.len();
                file.write_all(&chunk).await.map_err(|e| format!("write file chunk: {e}"))?;
                hasher.update(&chunk);

                transferred += n as u64;
                remaining_to_write = remaining_to_write.saturating_sub(n as u64);
            }
        }
    }

    let hash_frame = stream.read_frame().await?;
    if hash_frame.len() != 32 {
        return Err(format!("invalid hash frame length: {}", hash_frame.len()));
    }
    let hash_bytes = hasher.finalize();
    if hash_bytes.as_bytes() != hash_frame.as_slice() {
        return Err("Hash mismatch!".to_string());
    }

    let _ = db.update_stream_progress(&header.transfer_id, header.stream_id, transferred).await;

    // The stream_id == 0 can wait for all other streams to finish using a countdown or just check DB,
    // but for now we assume they all finish around the same time.
    if header.stream_id == 0 {
        let _ = db.update_transfer_progress(&header.transfer_id, header.total_size, TransferStatus::Done).await;
        emit_receiver_progress(app, &header.transfer_id, &header.batch_name, header.total_size, header.total_size, 0, TransferStatus::Done, peer_ip);
    }

    Ok(())
}
