use serde::{Deserialize, Serialize};

// ── Device ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Laptop,
    Desktop,
    Phone,
    Tablet,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalInfo {
    pub name: String,
    pub ip: String,
    pub port: u16,
}

// ── Transfer ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferStatus {
    Pending,
    Transferring,
    Paused,
    Done,
    Error,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Send,
    Receive,
}

/// Emitted to the frontend on every progress tick and status change.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub id: String,
    pub batch_name: String,
    pub total_size: u64,
    pub transferred: u64,
    /// Bytes per second, smoothed over the last report interval.
    pub speed: u64,
    pub status: TransferStatus,
    pub direction: TransferDirection,
    pub device_name: String,
}

/// Length-prefixed JSON header sent before file bytes over TCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferHeader {
    pub transfer_id: String,
    pub batch_name: String,
    pub total_size: u64,
    pub file_count: u32,
    pub is_resume: bool,
}

/// Acknowledgment sent from receiver to sender.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferAck {
    pub accepted: bool,
    pub resume_offset: u64,
}

/// Emitted when an incoming file request is received, before the user accepts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    pub id: String,
    pub batch_name: String,
    pub total_size: u64,
    pub file_count: u32,
    pub sender_ip: String,
    pub sender_name: String,
}
