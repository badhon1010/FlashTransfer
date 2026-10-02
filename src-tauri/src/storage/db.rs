use crate::types::TransferStatus;
use log::info;
use sqlx::{sqlite::SqlitePoolOptions, FromRow, Pool, Sqlite};
use std::path::PathBuf;

// ─────────────────────────────────────────────────────────────────────────────
// Current schema version. Bump this when adding new tables or columns.
// ─────────────────────────────────────────────────────────────────────────────
#[allow(dead_code)]
const SCHEMA_VERSION: i64 = 2;

// ─────────────────────────────────────────────────────────────────────────────
// Domain types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
}

/// A row from the `transfers` table, fully serialisable to the frontend.
#[derive(FromRow, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct TransferRecord {
    pub id: String,
    pub batch_name: String,
    pub total_size: i64,
    pub file_count: i64,
    pub transferred: i64,
    pub status: String,
    pub direction: String,
    pub device_name: String,
    pub local_path: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// A row from the `device_history` table.
#[derive(FromRow, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct DeviceHistoryRecord {
    pub id: String,           // mDNS-stable device id (public-key hex)
    pub name: String,
    pub kind: String,
    pub last_ip: String,
    pub last_seen: Option<String>,
    pub transfer_count: i64,
    pub bytes_exchanged: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Database impl
// ─────────────────────────────────────────────────────────────────────────────

impl Database {
    /// Connect and migrate (idempotent).
    pub async fn init(app_data_dir: PathBuf) -> Result<Self, String> {
        let db_path = app_data_dir.join("flashtransfer.db");
        let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

        info!("db: connecting to {}", db_url);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .map_err(|e| format!("failed to connect to sqlite: {e}"))?;

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    // ── Migration ─────────────────────────────────────────────────────────────

    async fn migrate(&self) -> Result<(), String> {
        // Ensure the schema_version table exists first.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("create schema_version: {e}"))?;

        let current: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_version")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

        if current < 1 {
            self.migrate_v1().await?;
        }
        if current < 2 {
            self.migrate_v2().await?;
        }

        Ok(())
    }

    /// V1 — initial schema (transfers, settings, stream_progress).
    async fn migrate_v1(&self) -> Result<(), String> {
        info!("db: applying migration v1");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS transfers (
                id          TEXT    PRIMARY KEY,
                batch_name  TEXT    NOT NULL,
                total_size  INTEGER NOT NULL,
                file_count  INTEGER NOT NULL,
                transferred INTEGER NOT NULL DEFAULT 0,
                status      TEXT    NOT NULL,
                direction   TEXT    NOT NULL,
                device_name TEXT    NOT NULL,
                local_path  TEXT    NOT NULL,
                created_at  TEXT    DEFAULT (datetime('now')),
                updated_at  TEXT    DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS stream_progress (
                transfer_id TEXT    NOT NULL,
                stream_id   INTEGER NOT NULL,
                transferred INTEGER NOT NULL,
                PRIMARY KEY (transfer_id, stream_id)
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("migrate v1: {e}"))?;

        // Reset any in-flight transfers from a previous crash
        sqlx::query(
            "UPDATE transfers SET status = 'paused' WHERE status IN ('transferring', 'pending');"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("migrate v1 reset: {e}"))?;

        sqlx::query("INSERT INTO schema_version (version) VALUES (1);")
            .execute(&self.pool)
            .await
            .map_err(|e| format!("migrate v1 stamp: {e}"))?;

        Ok(())
    }

    /// V2 — adds `device_history` table.
    async fn migrate_v2(&self) -> Result<(), String> {
        info!("db: applying migration v2");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS device_history (
                id              TEXT    PRIMARY KEY,
                name            TEXT    NOT NULL,
                kind            TEXT    NOT NULL DEFAULT 'unknown',
                last_ip         TEXT    NOT NULL DEFAULT '',
                last_seen       TEXT    DEFAULT (datetime('now')),
                transfer_count  INTEGER NOT NULL DEFAULT 0,
                bytes_exchanged INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("migrate v2: {e}"))?;

        sqlx::query("INSERT INTO schema_version (version) VALUES (2);")
            .execute(&self.pool)
            .await
            .map_err(|e| format!("migrate v2 stamp: {e}"))?;

        Ok(())
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    pub async fn get_setting(&self, key: &str) -> Option<String> {
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten()
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value"
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Stream progress (parallel transfer resume) ────────────────────────────

    pub async fn get_stream_progress(&self, transfer_id: &str, stream_id: u32) -> Option<u64> {
        sqlx::query_scalar::<_, i64>(
            "SELECT transferred FROM stream_progress WHERE transfer_id = ? AND stream_id = ?"
        )
        .bind(transfer_id)
        .bind(stream_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .map(|v| v as u64)
    }

    pub async fn update_stream_progress(
        &self,
        transfer_id: &str,
        stream_id: u32,
        transferred: u64,
    ) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO stream_progress (transfer_id, stream_id, transferred)
             VALUES (?, ?, ?)
             ON CONFLICT(transfer_id, stream_id) DO UPDATE SET transferred = excluded.transferred"
        )
        .bind(transfer_id)
        .bind(stream_id)
        .bind(transferred as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Transfers ─────────────────────────────────────────────────────────────

    pub async fn insert_transfer(&self, record: &TransferRecord) -> Result<(), String> {
        sqlx::query(
            r#"
            INSERT INTO transfers
                (id, batch_name, total_size, file_count, transferred, status,
                 direction, device_name, local_path)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                transferred = excluded.transferred,
                status      = excluded.status,
                updated_at  = datetime('now')
            "#,
        )
        .bind(&record.id)
        .bind(&record.batch_name)
        .bind(record.total_size)
        .bind(record.file_count)
        .bind(record.transferred)
        .bind(&record.status)
        .bind(&record.direction)
        .bind(&record.device_name)
        .bind(&record.local_path)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("insert_transfer: {e}"))?;
        Ok(())
    }

    pub async fn update_transfer_progress(
        &self,
        id: &str,
        transferred: u64,
        status: TransferStatus,
    ) -> Result<(), String> {
        let status_str = match status {
            TransferStatus::Pending      => "pending",
            TransferStatus::Transferring => "transferring",
            TransferStatus::Paused       => "paused",
            TransferStatus::Done         => "done",
            TransferStatus::Error        => "error",
            TransferStatus::Rejected     => "rejected",
        };
        sqlx::query(
            "UPDATE transfers
             SET transferred = ?1, status = ?2, updated_at = datetime('now')
             WHERE id = ?3"
        )
        .bind(transferred as i64)
        .bind(status_str)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("update_transfer_progress: {e}"))?;
        Ok(())
    }

    pub async fn get_transfer(&self, id: &str) -> Result<Option<TransferRecord>, String> {
        sqlx::query_as::<_, TransferRecord>(
            r#"SELECT id, batch_name, total_size, file_count, transferred, status,
                      direction, device_name, local_path, created_at, updated_at
               FROM transfers WHERE id = ?1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("get_transfer: {e}"))
    }

    pub async fn get_all_transfers(&self) -> Result<Vec<TransferRecord>, String> {
        sqlx::query_as::<_, TransferRecord>(
            r#"SELECT id, batch_name, total_size, file_count, transferred, status,
                      direction, device_name, local_path, created_at, updated_at
               FROM transfers ORDER BY created_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("get_all_transfers: {e}"))
    }

    pub async fn delete_transfer(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM transfers WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("delete_transfer: {e}"))?;
        sqlx::query("DELETE FROM stream_progress WHERE transfer_id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("delete stream_progress: {e}"))?;
        Ok(())
    }

    pub async fn clear_transfer_history(&self) -> Result<(), String> {
        // Only clear completed / errored transfers; leave active ones.
        sqlx::query(
            "DELETE FROM transfers WHERE status IN ('done', 'error', 'rejected')"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("clear_history: {e}"))?;
        Ok(())
    }

    // ── Device History ────────────────────────────────────────────────────────

    /// Upsert a device into history and bump its stats.
    pub async fn upsert_device(
        &self,
        id: &str,
        name: &str,
        kind: &str,
        ip: &str,
        bytes_delta: i64,
    ) -> Result<(), String> {
        sqlx::query(
            r#"
            INSERT INTO device_history (id, name, kind, last_ip, last_seen, transfer_count, bytes_exchanged)
            VALUES (?1, ?2, ?3, ?4, datetime('now'), 1, ?5)
            ON CONFLICT(id) DO UPDATE SET
                name            = excluded.name,
                kind            = excluded.kind,
                last_ip         = excluded.last_ip,
                last_seen       = datetime('now'),
                transfer_count  = device_history.transfer_count + 1,
                bytes_exchanged = device_history.bytes_exchanged + excluded.bytes_exchanged
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(kind)
        .bind(ip)
        .bind(bytes_delta)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("upsert_device: {e}"))?;
        Ok(())
    }

    pub async fn get_device_history(&self) -> Result<Vec<DeviceHistoryRecord>, String> {
        sqlx::query_as::<_, DeviceHistoryRecord>(
            r#"SELECT id, name, kind, last_ip, last_seen, transfer_count, bytes_exchanged
               FROM device_history ORDER BY last_seen DESC"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("get_device_history: {e}"))
    }

    pub async fn get_device(&self, id: &str) -> Result<Option<DeviceHistoryRecord>, String> {
        sqlx::query_as::<_, DeviceHistoryRecord>(
            r#"SELECT id, name, kind, last_ip, last_seen, transfer_count, bytes_exchanged
               FROM device_history WHERE id = ?1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("get_device: {e}"))
    }

    pub async fn forget_device(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM device_history WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("forget_device: {e}"))?;
        Ok(())
    }
}
