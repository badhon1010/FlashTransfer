pub mod receiver;
pub mod sender;

use crate::types::DeviceInfo;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// A live handle to a running transfer, used to pause/resume/cancel it.
pub struct TransferHandle {
    pub paused: Arc<AtomicBool>,
    pub cancelled: Arc<AtomicBool>,
}

/// Central manager — holds all in-flight transfer handles and known devices.
/// Stored in `tauri::State` as `Mutex<TransferManager>`.
pub struct TransferManager {
    pub handles: HashMap<String, TransferHandle>,
    pub devices: HashMap<String, DeviceInfo>,
}

impl TransferManager {
    pub fn new() -> Self {
        Self {
            handles: HashMap::new(),
            devices: HashMap::new(),
        }
    }

    /// Register a new transfer and return its control atomics.
    pub fn register(&mut self, id: &str) -> (Arc<AtomicBool>, Arc<AtomicBool>) {
        let paused    = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        self.handles.insert(id.to_string(), TransferHandle {
            paused:    Arc::clone(&paused),
            cancelled: Arc::clone(&cancelled),
        });
        (paused, cancelled)
    }

    pub fn pause(&self, id: &str) {
        if let Some(h) = self.handles.get(id) {
            h.paused.store(true, Ordering::Relaxed);
        }
    }

    pub fn resume(&self, id: &str) {
        if let Some(h) = self.handles.get(id) {
            h.paused.store(false, Ordering::Relaxed);
        }
    }

    pub fn cancel(&self, id: &str) {
        if let Some(h) = self.handles.get(id) {
            h.cancelled.store(true, Ordering::Relaxed);
            h.paused.store(false, Ordering::Relaxed); // unblock any pause wait
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.handles.remove(id);
    }
}
