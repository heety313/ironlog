#[macro_use]
extern crate rocket;

use ironlog::client_handler;
use ironlog::config::Config;
use ironlog::types::LogMessage;

use chrono::{Duration, Utc};
use clap::Parser;
use include_dir::{include_dir, Dir};
use rocket::form::FromForm;
use rocket::http::ContentType;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

static STATIC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/static");

fn default_timestamp() -> String {
    Utc::now().to_rfc3339()
}

async fn optimize_sqlite(pool: &SqlitePool) {
    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(pool)
        .await
        .expect("Failed to set journal_mode");

    sqlx::query("PRAGMA synchronous = NORMAL;")
        .execute(pool)
        .await
        .expect("Failed to set synchronous mode");

    sqlx::query("PRAGMA cache_size = -64000;") // 64MB cache
        .execute(pool)
        .await
        .expect("Failed to set cache size");
}

#[rocket::main]
async fn main() {
    let config = Config::parse();
    let config_arc = Arc::new(config.clone()); // Create an Arc<Config> for sharing

    // Database file path
    let db_path = &config.log_db;

    // Check if the database file exists
    if !Path::new(db_path).exists() {
        // Create the database file by establishing a connection
        fs::File::create(db_path).expect("Failed to create database file.");
    }

    // Initialize the database connection pool
    let db_url = format!("sqlite://{}", db_path);
    let db_pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to create pool.");

    // Ensure the logs table exists
    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            level TEXT,
            message TEXT,
            target TEXT,
            module_path TEXT,
            file TEXT,
            line INTEGER,
            hash TEXT,
            timestamp TEXT
        )
    ",
    )
    .execute(&db_pool)
    .await
    .expect("Failed to create logs table.");

    // Optimize SQLite for performance
    optimize_sqlite(&db_pool).await;

    // Start the log handler in a separate task
    let db_pool_clone = db_pool.clone();
    tokio::spawn(async move {
        client_handler::start_log_handler(db_pool_clone, config_arc).await;
    });

    // Launch the Rocket server
    let api_server_ip = config
        .api_server_ip
        .parse::<std::net::IpAddr>()
        .expect("Invalid IP address for API server");
    let figment = rocket::Config::figment()
        .merge(("address", api_server_ip))
        .merge(("port", config.api_server_port));

    rocket::custom(figment)
        .manage(db_pool)
        .manage(config) // Manage the original Config, not the Arc<Config>
        .mount(
            "/api",
            routes![
                get_hashes,
                get_logs,
                get_log_info,
                purge_logs,
                insert_log,
                delete_logs_by_hash,
                delete_logs_by_time_range,
            ],
        )
        .mount("/", routes![index, serve_file])
        .launch()
        .await
        .unwrap();
}

pub fn truncate_string(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let mut end = max_bytes;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        s[..end].to_string()
    }
}

#[get("/")]
fn index() -> Option<(ContentType, Vec<u8>)> {
    let file = STATIC_DIR.get_file("index.html")?;

    let content_type = ContentType::from_extension(file.path().extension()?.to_str()?)
        .unwrap_or(ContentType::HTML);

    Some((content_type, file.contents().to_vec()))
}

#[get("/hashes")]
async fn get_hashes(db_pool: &rocket::State<SqlitePool>) -> Json<Vec<String>> {
    let rows = sqlx::query("SELECT DISTINCT hash FROM logs")
        .fetch_all(db_pool.inner())
        .await
        .expect("Failed to fetch hashes.");

    let hashes = rows
        .into_iter()
        .map(|row| row.get::<String, _>("hash"))
        .collect();

    Json(hashes)
}

#[derive(FromForm)]
struct LogQuery {
    count: Option<i64>,
    start: Option<String>,
    end: Option<String>,
}

#[get("/logs/<hash>?<q..>")]
async fn get_logs(
    hash: &str,
    q: Option<LogQuery>,
    db_pool: &rocket::State<SqlitePool>,
) -> Option<Json<Vec<LogMessage>>> {
    use sqlx::QueryBuilder;

    let mut builder = QueryBuilder::<sqlx::Sqlite>::new(
        "
        SELECT
            level,
            message,
            target,
            module_path,
            file,
            line,
            hash,
            timestamp
        FROM logs
        WHERE hash = ",
    );
    builder.push_bind(hash);

    let mut count = None;

    if let Some(ref query_params) = q {
        if let Some(ref s) = query_params.start {
            builder.push(" AND timestamp >= ");
            builder.push_bind(s);
        }
        if let Some(ref e) = query_params.end {
            builder.push(" AND timestamp <= ");
            builder.push_bind(e);
        }
        count = query_params.count;
    }

    builder.push(" ORDER BY timestamp DESC");

    if let Some(c) = count {
        builder.push(" LIMIT ");
        builder.push_bind(c);
    }

    let query = builder.build_query_as::<LogMessage>();

    let logs = query.fetch_all(db_pool.inner()).await.ok()?;

    Some(Json(logs))
}

#[get("/list_files")]
fn list_files() -> String {
    let files: Vec<_> = STATIC_DIR
        .files()
        .map(|f| f.path().display().to_string())
        .collect();
    format!("Files in static dir: {:?}", files)
}

#[get("/<file..>")]
fn serve_file(file: PathBuf) -> Option<(ContentType, Vec<u8>)> {
    let file = STATIC_DIR.get_file(file.to_str()?)?;

    let content_type = ContentType::from_extension(file.path().extension()?.to_str()?)
        .unwrap_or(ContentType::Bytes);

    Some((content_type, file.contents().to_vec()))
}

// New Struct for LogInfo
#[derive(Serialize)]
struct LogInfo {
    db_size: u64, // in bytes
    total_log_count: i64,
    number_of_hashes: i64,
    min_date: String,
    max_date: String,
    hash_list: Vec<String>,
}

// Endpoint to get log info
#[get("/log_info")]
async fn get_log_info(
    db_pool: &rocket::State<SqlitePool>,
    config: &rocket::State<Config>,
) -> Option<Json<LogInfo>> {
    // Get the size of the db file
    let db_path = &config.log_db;
    let metadata = fs::metadata(db_path).ok()?;
    let db_size = metadata.len();

    // Get total log count
    let total_log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logs")
        .fetch_one(db_pool.inner())
        .await
        .ok()?;

    // Get number of hashes
    let number_of_hashes: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT hash) FROM logs")
        .fetch_one(db_pool.inner())
        .await
        .ok()?;

    // Get date range
    let min_date: String = sqlx::query_scalar("SELECT MIN(timestamp) FROM logs")
        .fetch_one(db_pool.inner())
        .await
        .unwrap_or_else(|_| Utc::now().to_rfc3339());

    let max_date: String = sqlx::query_scalar("SELECT MAX(timestamp) FROM logs")
        .fetch_one(db_pool.inner())
        .await
        .unwrap_or_else(|_| Utc::now().to_rfc3339());

    if max_date.is_empty() {
        return Some(Json(LogInfo {
            db_size,
            total_log_count,
            number_of_hashes,
            min_date: (Utc::now() - Duration::days(7)).to_rfc3339(),
            max_date: Utc::now().to_rfc3339(),
            hash_list: vec![],
        }));
    }

    // Get list of hash names
    let rows = sqlx::query("SELECT DISTINCT hash FROM logs")
        .fetch_all(db_pool.inner())
        .await
        .ok()?;

    let hash_list = rows
        .into_iter()
        .map(|row| row.get::<String, _>("hash"))
        .collect();

    Some(Json(LogInfo {
        db_size,
        total_log_count,
        number_of_hashes,
        min_date,
        max_date,
        hash_list,
    }))
}

// Endpoint to purge all logs
#[post("/purge_logs")]
async fn purge_logs(db_pool: &rocket::State<SqlitePool>) -> Json<String> {
    let result = sqlx::query("DELETE FROM logs")
        .execute(db_pool.inner())
        .await;

    match result {
        Ok(_) => Json("Logs purged successfully.".to_string()),
        Err(e) => Json(format!("Failed to purge logs: {}", e)),
    }
}

// New endpoint to delete logs by hash
#[derive(Serialize)]
struct DeleteResponse {
    success: bool,
    message: String,
    deleted_count: Option<i64>,
}

#[delete("/delete_logs/<hash>")]
async fn delete_logs_by_hash(
    hash: &str,
    db_pool: &rocket::State<SqlitePool>,
) -> Json<DeleteResponse> {
    let result = sqlx::query("DELETE FROM logs WHERE hash = ?")
        .bind(hash)
        .execute(db_pool.inner())
        .await;

    match result {
        Ok(query_result) => Json(DeleteResponse {
            success: true,
            message: format!("Successfully deleted logs for hash: {}", hash),
            deleted_count: Some(query_result.rows_affected() as i64),
        }),
        Err(e) => Json(DeleteResponse {
            success: false,
            message: format!("Failed to delete logs for hash {}: {}", hash, e),
            deleted_count: None,
        }),
    }
}

// New endpoint to delete logs by time range
#[derive(Deserialize)]
struct TimeRangeDelete {
    start_time: String,
    end_time: String,
}

#[delete("/delete_time_range", data = "<time_range>")]
async fn delete_logs_by_time_range(
    time_range: Json<TimeRangeDelete>,
    db_pool: &rocket::State<SqlitePool>,
) -> Json<DeleteResponse> {
    let result = sqlx::query("DELETE FROM logs WHERE timestamp >= ? AND timestamp <= ?")
        .bind(&time_range.start_time)
        .bind(&time_range.end_time)
        .execute(db_pool.inner())
        .await;

    match result {
        Ok(query_result) => Json(DeleteResponse {
            success: true,
            message: format!(
                "Successfully deleted logs between {} and {}",
                time_range.start_time, time_range.end_time
            ),
            deleted_count: Some(query_result.rows_affected() as i64),
        }),
        Err(e) => Json(DeleteResponse {
            success: false,
            message: format!("Failed to delete logs in time range: {}", e),
            deleted_count: None,
        }),
    }
}

// Endpoint to insert a log
#[post("/insert_log", data = "<log_message>")]
async fn insert_log(
    log_message: Json<LogMessage>,
    db_pool: &rocket::State<SqlitePool>,
    config: &rocket::State<Config>,
) -> Json<String> {
    let config = config.inner();
    let db_pool = db_pool.inner();

    let mut log_message = log_message.into_inner(); // Consume Json wrapper

    // Truncate the message if it exceeds max_log_length
    log_message.message = truncate_string(&log_message.message, config.max_log_length);

    // Check if the hash exists
    let hash_exists: bool =
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM logs WHERE hash = ?)")
            .bind(&log_message.hash)
            .fetch_one(db_pool)
            .await
            .unwrap_or(0)
            != 0;

    if !hash_exists {
        // Get the total number of distinct hashes
        let num_hashes: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT hash) FROM logs")
            .fetch_one(db_pool)
            .await
            .unwrap_or(0);

        if num_hashes >= config.max_hashes as i64 {
            // Do not log this message
            return Json("Maximum number of hashes reached. Log not inserted.".to_string());
        }
    }

    // Insert the log_message into the database
    let result = sqlx::query(
        "
        INSERT INTO logs (level, message, target, module_path, file, line, hash, timestamp)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    ",
    )
    .bind(&log_message.level)
    .bind(&log_message.message)
    .bind(&log_message.target)
    .bind(&log_message.module_path)
    .bind(&log_message.file)
    .bind(log_message.line)
    .bind(&log_message.hash)
    .bind(&log_message.timestamp)
    .execute(db_pool)
    .await;

    match result {
        Ok(_) => (),
        Err(e) => return Json(format!("Failed to insert log into database: {}", e)),
    }

    // Now check if the number of logs for this hash exceeds max_log_count + 50
    let log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logs WHERE hash = ?")
        .bind(&log_message.hash)
        .fetch_one(db_pool)
        .await
        .unwrap_or(0);

    if log_count > (config.max_log_count + 50) as i64 {
        // Delete the oldest 50 logs for this hash
        let result = sqlx::query(
            "
            DELETE FROM logs
            WHERE id IN (
                SELECT id FROM logs
                WHERE hash = ?
                ORDER BY timestamp ASC
                LIMIT 50
            )
        ",
        )
        .bind(&log_message.hash)
        .execute(db_pool)
        .await;

        match result {
            Ok(_) => (),
            Err(e) => return Json(format!("Failed to delete old logs: {}", e)),
        }
    }

    Json("Log inserted successfully.".to_string())
}
