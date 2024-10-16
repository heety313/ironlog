// client_handler.rs

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use sqlx::{SqlitePool, Row};
use serde_json;
use chrono::Utc;
use crate::{LogMessage, Config, truncate_string};

pub async fn handle_client(socket: TcpStream, db_pool: SqlitePool, config: Config) {
    let reader = BufReader::new(socket);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(log_message) = serde_json::from_str::<LogMessage>(&line) {
            let mut log_message = log_message; // Make it mutable

            // Truncate the message if it exceeds max_log_length
            log_message.message = truncate_string(&log_message.message, config.max_log_length);

            let hash_exists: bool = sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM logs WHERE hash = ?)")
                .bind(&log_message.hash)
                .fetch_one(&db_pool)
                .await
                .unwrap_or(0) != 0;

            if !hash_exists {
                // Get the total number of distinct hashes
                let num_hashes: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT hash) FROM logs")
                    .fetch_one(&db_pool)
                    .await
                    .unwrap_or(0);

                if num_hashes >= config.max_hashes as i64 {
                    // Do not log this message
                    continue;
                }
            }

            // Insert the log_message into the database
            sqlx::query("
                INSERT INTO logs (level, message, target, module_path, file, line, hash, timestamp)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ")
            .bind(&log_message.level)
            .bind(&log_message.message)
            .bind(&log_message.target)
            .bind(&log_message.module_path)
            .bind(&log_message.file)
            .bind(log_message.line)
            .bind(&log_message.hash)
            .bind(&log_message.timestamp)
            .execute(&db_pool)
            .await
            .expect("Failed to insert log into database.");

            // Now check if the number of logs for this hash exceeds max_log_count + 50
            let log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logs WHERE hash = ?")
                .bind(&log_message.hash)
                .fetch_one(&db_pool)
                .await
                .unwrap_or(0);

            if log_count > (config.max_log_count + 50) as i64 {
                // Delete the oldest 50 logs for this hash
                sqlx::query("
                    DELETE FROM logs 
                    WHERE id IN (
                        SELECT id FROM logs 
                        WHERE hash = ? 
                        ORDER BY timestamp ASC 
                        LIMIT 50
                    )
                ")
                .bind(&log_message.hash)
                .execute(&db_pool)
                .await
                .expect("Failed to delete old logs.");
            }
        }
    }
}
