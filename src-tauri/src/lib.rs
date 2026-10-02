mod commands;
mod crypto;
mod discovery;
mod transfer;
mod types;
mod storage;
mod benchmark;

use mdns_sd::ServiceDaemon;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use transfer::TransferManager;

/// Shared application state managed by Tauri.
/// All fields are `Mutex`-wrapped so they can be accessed from any command/task.
pub struct AppState {
    pub transfer_manager: Arc<Mutex<TransferManager>>,
    pub pending_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
    pub local_port: Mutex<u16>,
    pub db: Arc<storage::db::Database>,
}



#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // ── Plugins ──────────────────────────────────────────────────────────
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        // ── Setup ────────────────────────────────────────────────────────────
        .setup(|app| {
            let handle = app.handle().clone();

            // Create shared state first (dummy port initially, will update later if needed? No, wait.)
            // Wait, port is currently obtained from receiver.
            // Let's create an Arc for pending requests to pass to receiver.
            let pending_requests = Arc::new(Mutex::new(HashMap::new()));

            // Start TCP receiver on a random port; block briefly to get the port.
            let port = tauri::async_runtime::block_on(async {
                transfer::receiver::start_receiver(handle.clone())
                    .await
                    .unwrap_or_else(|e| {
                        log::error!("receiver init failed: {e}");
                        0
                    })
            });

            log::info!("setup: receiver bound on port {port}");

            // Initialize the database
            let app_data_dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            std::fs::create_dir_all(&app_data_dir).ok();
            
            let db = tauri::async_runtime::block_on(async {
                storage::db::Database::init(app_data_dir)
                    .await
                    .expect("failed to initialize sqlite database")
            });

            // Start mDNS discovery in a background thread.
            // We create the daemon here and hand it off — it must not be dropped.
            match ServiceDaemon::new() {
                Ok(mdns) => {
                    let hostname = tauri::async_runtime::block_on(async {
                        db.get_setting("device_name").await
                    }).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| discovery::get_hostname());
                    
                    discovery::start_discovery(handle, mdns, port, hostname)
                },
                Err(e)   => log::warn!("setup: mDNS daemon failed: {e}"),
            }

            // Initialise shared state
            app.manage(AppState {
                transfer_manager: Arc::new(Mutex::new(TransferManager::new())),
                pending_requests,
                local_port: Mutex::new(port),
                db: Arc::new(db),
            });

            if let Ok(size_str) = std::env::var("RUN_BENCHMARK") {
                if let Ok(size_mb) = size_str.parse::<u32>() {
                    let app_clone = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        log::info!("=== STARTING AUTOMATED BENCHMARK ({} MB) ===", size_mb);
                        let state = app_clone.state::<AppState>();
                        match benchmark::run_benchmark(app_clone.clone(), state, size_mb).await {
                            Ok(res) => {
                                log::info!("=== BENCHMARK COMPLETE ===");
                                log::info!("Size: {} MB", res.file_size_mb);
                                log::info!("Time: {} ms", res.time_ms);
                                log::info!("Speed: {:.2} MB/s", res.mb_per_sec);
                                println!("BENCHMARK_RESULT|{}|{}|{:.2}", res.file_size_mb, res.time_ms, res.mb_per_sec);
                            }
                            Err(e) => {
                                log::error!("Benchmark failed: {e}");
                            }
                        }
                        std::process::exit(0);
                    });
                }
            }

            Ok(())
        })
        // ── Commands ─────────────────────────────────────────────────────────
        .invoke_handler(tauri::generate_handler![
            commands::get_local_info,
            commands::set_device_name,
            commands::start_transfer,
            commands::pause_transfer,
            commands::resume_transfer,
            commands::cancel_transfer,
            commands::accept_transfer,
            commands::reject_transfer,
            commands::get_transfer_history,
            commands::delete_transfer,
            commands::clear_transfer_history,
            commands::get_device_history,
            commands::forget_device,
            benchmark::run_benchmark,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
