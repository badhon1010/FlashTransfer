use crate::types::{TransferDirection, TransferStatus};
use log::{error, info};
use sqlx::{sqlite::SqlitePoolOptions, FromRow, Pool, Sqlite};
use std::path::PathBuf;

#[derive(Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
}

#[derive(FromRow, serde::Serialize)]
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
}

impl Database {
    /// Initialize the SQLite database connection and create tables if they don't exist
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
        db.create_tables().await?;

        Ok(db)
    }

    async fn create_tables(&self) -> Result<(), String> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS transfers (
                id TEXT PRIMARY KEY,
                batch_name TEXT NOT NULL,
                total_size INTEGER NOT NULL,
                file_count INTEGER NOT NULL,
                transferred INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                direction TEXT NOT NULL,
                device_name TEXT NOT NULL,
                local_path TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("failed to create transfers table: {e}"))?;

        // Reset transferring statuses to paused on startup
        sqlx::query(
            r#"
            UPDATE transfers 
            SET status = 'paused' 
            WHERE status = 'transferring' OR status = 'pending'
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("failed to reset statuses: {e}"))?;

        Ok(())
    }

    pub async fn insert_transfer(&self, record: &TransferRecord) -> Result<(), String> {
        sqlx::query(
            r#"
            INSERT INTO transfers (id, batch_name, total_size, file_count, transferred, status, direction, device_name, local_path)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                transferred = excluded.transferred,
                status = excluded.status,
                updated_at = CURRENT_TIMESTAMP
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
        .map_err(|e| format!("failed to insert transfer: {e}"))?;

        Ok(())
    }

    pub async fn update_transfer_progress(&self, id: &str, transferred: u64, status: TransferStatus) -> Result<(), String> {
        let status_str = match status {
            TransferStatus::Pending => "pending",
            TransferStatus::Transferring => "transferring",
            TransferStatus::Paused => "paused",
            TransferStatus::Done => "done",
            TransferStatus::Error => "error",
            TransferStatus::Rejected => "rejected",
        };

        sqlx::query(
            r#"
            UPDATE transfers 
            SET transferred = ?1, status = ?2, updated_at = CURRENT_TIMESTAMP 
            WHERE id = ?3
            "#,
        )
        .bind(transferred as i64)
        .bind(status_str)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("failed to update progress: {e}"))?;

        Ok(())
    }

    pub async fn get_transfer(&self, id: &str) -> Result<Option<TransferRecord>, String> {
        let row = sqlx::query_as::<_, TransferRecord>(
            r#"
            SELECT id, batch_name, total_size, file_count, transferred, status, direction, device_name, local_path
            FROM transfers
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("failed to get transfer: {e}"))?;

        Ok(row)
    }

    pub async fn get_all_transfers(&self) -> Result<Vec<TransferRecord>, String> {
        let rows = sqlx::query_as::<_, TransferRecord>(
            r#"
            SELECT id, batch_name, total_size, file_count, transferred, status, direction, device_name, local_path
            FROM transfers
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("failed to fetch transfers: {e}"))?;

        Ok(rows)
    }
}
