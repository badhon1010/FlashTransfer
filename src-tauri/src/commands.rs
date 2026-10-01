use crate::{
    discovery::{get_hostname, get_local_ip},
    transfer::sender,
    types::LocalInfo,
    AppState,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub abs_path: PathBuf,
    pub rel_path: String,
    pub size: u64,
}
use tauri::{AppHandle, State};

type CommandResult<T> = Result<T, String>;

/// Return local device name, IP, and receiver port for the status bar.
#[tauri::command]
pub async fn get_local_info(state: State<'_, AppState>) -> CommandResult<LocalInfo> {
    let port = *state.local_port.lock().map_err(|e| e.to_string())?;
    Ok(LocalInfo {
        name: get_hostname(),
        ip:   get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string()),
        port,
    })
}

pub fn build_batch_manifest(file_paths: &[String]) -> Result<(String, u64, Vec<FileEntry>), String> {
    let mut entries = Vec::new();
    let mut total_size = 0u64;

    for path_str in file_paths {
        let root_path = Path::new(path_str);
        if !root_path.exists() {
            return Err(format!("Path does not exist: {}", path_str));
        }

        let root_name = root_path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let parent_dir = root_path.parent().unwrap_or(Path::new(""));

        for entry in walkdir::WalkDir::new(root_path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                let metadata = entry.metadata().map_err(|e| e.to_string())?;
                let size = metadata.len();
                let abs_path = entry.path().to_path_buf();
                
                let rel_path = if file_paths.len() == 1 && root_path.is_file() {
                    root_name.clone()
                } else {
                    let stripped = abs_path.strip_prefix(parent_dir).unwrap_or(&abs_path);
                    stripped.to_string_lossy().replace('\\', "/")
                };

                entries.push(FileEntry {
                    abs_path,
                    rel_path,
                    size,
                });
                total_size += size;
            }
        }
    }

    let batch_name = if file_paths.len() == 1 {
        let p = Path::new(&file_paths[0]);
        p.file_name().unwrap_or_default().to_string_lossy().to_string()
    } else {
        format!("{} files", entries.len())
    };

    Ok((batch_name, total_size, entries))
}

/// Kick off one transfer task per file in `file_paths` to the given device.
/// Returns the list of transfer IDs that were created.
#[tauri::command]
pub async fn start_transfer(
    app: AppHandle,
    state: State<'_, AppState>,
    device_id: String,
    file_paths: Vec<String>,
) -> CommandResult<Vec<String>> {
    // Look up the target device
    let device = {
        let mgr = state.transfer_manager.lock().map_err(|e| e.to_string())?;
        mgr.devices
            .get(&device_id)
            .cloned()
            .ok_or_else(|| format!("unknown device: {device_id}"))?
    };

    let file_paths_json = serde_json::to_string(&file_paths).unwrap_or_default();

    let (batch_name, total_size, entries) = tokio::task::spawn_blocking(move || {
        build_batch_manifest(&file_paths)
    })
    .await
    .map_err(|e| e.to_string())??;

    if entries.is_empty() {
        return Err("No files to transfer".to_string());
    }

    let id = uuid::Uuid::new_v4().to_string();

    let (paused, cancelled) = {
        let mut mgr = state.transfer_manager.lock().map_err(|e| e.to_string())?;
        mgr.register(&id)
    };

    let app_clone      = app.clone();
    let transfer_id    = id.clone();
    let device_ip      = device.ip.clone();
    let device_port    = device.port;
    let device_name    = device.name.clone();
    let tm_arc         = std::sync::Arc::clone(&state.transfer_manager);

    tokio::spawn(async move {
        let result = sender::send_batch(
            app_clone,
            transfer_id.clone(),
            batch_name,
            total_size,
            entries,
            device_ip,
            device_port,
            device_name,
            paused,
            cancelled,
            false,
            file_paths_json,
        )
        .await;

        if let Err(e) = result {
            log::error!("transfer {transfer_id} failed: {e}");
        }

        if let Ok(mut mgr) = tm_arc.lock() {
            mgr.remove(&transfer_id);
        }
    });

    Ok(vec![id])
}

#[tauri::command]
pub async fn pause_transfer(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    state
        .transfer_manager
        .lock()
        .map_err(|e| e.to_string())?
        .pause(&id);
    Ok(())
}

#[tauri::command]
pub async fn resume_transfer(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    // 1. Try a hot resume (transfer task is currently suspended in memory)
    {
        let mut mgr = state.transfer_manager.lock().map_err(|e| e.to_string())?;
        if mgr.handles.contains_key(&id) {
            mgr.resume(&id);
            return Ok(());
        }
    }

    // 2. Cold resume (transfer task is dead, we must restart it from DB state)
    let record = state.db.get_transfer(&id).await.map_err(|e| e.to_string())?
        .ok_or_else(|| "transfer not found in db".to_string())?;
    
    if record.direction == "receive" {
        return Err("Cannot initiate resume for a received transfer (sender must initiate)".into());
    }

    let file_paths: Vec<String> = serde_json::from_str(&record.local_path).map_err(|e| format!("parse file_paths: {e}"))?;
    
    let (batch_name, total_size, entries) = tokio::task::spawn_blocking(move || {
        build_batch_manifest(&file_paths)
    })
    .await
    .map_err(|e| e.to_string())??;

    if entries.is_empty() {
        return Err("No files to transfer".to_string());
    }

    let (paused, cancelled) = {
        let mut mgr = state.transfer_manager.lock().map_err(|e| e.to_string())?;
        mgr.register(&id)
    };

    // We need the target device's current IP/Port. 
    // Usually, we would look it up by device_name or it needs to be discovered again.
    // For now, we will try to find it in the current discovery map by name.
    let device = {
        let mgr = state.transfer_manager.lock().map_err(|e| e.to_string())?;
        mgr.devices.values()
            .find(|d| d.name == record.device_name)
            .cloned()
    };

    let device = match device {
        Some(d) => d,
        None => return Err(format!("Device '{}' is not currently online", record.device_name)),
    };

    let app_clone      = app.clone();
    let transfer_id    = id.clone();
    let device_ip      = device.ip.clone();
    let device_port    = device.port;
    let device_name    = device.name.clone();
    let tm_arc         = std::sync::Arc::clone(&state.transfer_manager);
    let file_paths_json = record.local_path.clone();

    tokio::spawn(async move {
        let result = sender::send_batch(
            app_clone,
            transfer_id.clone(),
            batch_name,
            total_size,
            entries,
            device_ip,
            device_port,
            device_name,
            paused,
            cancelled,
            true, // is_resume
            file_paths_json,
        )
        .await;

        if let Err(e) = result {
            log::error!("transfer {transfer_id} failed: {e}");
        }

        if let Ok(mut mgr) = tm_arc.lock() {
            mgr.remove(&transfer_id);
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn get_transfer_history(state: State<'_, AppState>) -> CommandResult<Vec<crate::storage::db::TransferRecord>> {
    state.db.get_all_transfers().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_transfer(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    state
        .transfer_manager
        .lock()
        .map_err(|e| e.to_string())?
        .cancel(&id);
    Ok(())
}

#[tauri::command]
pub async fn accept_transfer(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let mut pending = state.pending_requests.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = pending.remove(&id) {
        let _ = tx.send(true);
    }
    Ok(())
}

#[tauri::command]
pub async fn reject_transfer(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let mut pending = state.pending_requests.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = pending.remove(&id) {
        let _ = tx.send(false);
    }
    Ok(())
}
