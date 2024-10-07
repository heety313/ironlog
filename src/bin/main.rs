#[macro_use]
extern crate rocket;

use ironlog::config::Config;

use rocket::http::ContentType;
use rocket::form::FromForm;
use rocket::serde::json::Json;
use include_dir::{include_dir, Dir};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, BufReader};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use std::fs;
use chrono::{Utc, Duration};
use clap::Parser;

static STATIC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/static");

#[derive(Serialize, Deserialize, Clone, sqlx::FromRow)]
struct LogMessage {
    level: String,
    message: String,
    target: String,
    module_path: Option<String>,
    file: Option<String>,
    line: Option<i64>, // SQLite INTEGER maps to i64
    hash: String,
    #[serde(default = "default_timestamp")]
    timestamp: String,
}

fn default_timestamp() -> String {
    Utc::now().to_rfc3339()
}

#[rocket::main]
async fn main() {
    let config = Config::parse();

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
    sqlx::query("
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
    ")
    .execute(&db_pool)
    .await
    .expect("Failed to create logs table.");

    // Start TCP listener in a separate task
    let db_pool_clone = db_pool.clone();
    let config_clone = config.clone();
    tokio::spawn(async move {
        let listener_addr = format!("{}:{}", config_clone.tcp_listener_ip, config_clone.tcp_listener_port);
        let listener = TcpListener::bind(&listener_addr).await.unwrap();
        println!("Log server is running on {}", listener_addr);

        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let db_pool = db_pool_clone.clone();
            let config = config_clone.clone();
            tokio::spawn(handle_client(socket, db_pool, config));
        }
    });

    // Launch the Rocket server
    let api_server_ip = config.api_server_ip.parse::<std::net::IpAddr>().expect("Invalid IP address for API server");
    let figment = rocket::Config::figment()
        .merge(("address", api_server_ip))
        .merge(("port", config.api_server_port));

    rocket::custom(figment)
        .manage(db_pool)
        .mount(
            "/api",
            routes![
                get_hashes,
                get_logs,
                list_files,
                get_date_range,
            ],
        )
        .mount("/", routes![index, serve_file])
        .launch()
        .await.unwrap();
}

fn truncate_string(s: &str, max_bytes: usize) -> String {
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

async fn handle_client(socket: tokio::net::TcpStream, db_pool: SqlitePool, config: Config) {
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

    let hashes = rows.into_iter()
        .map(|row| row.get::<String, _>("hash"))
        .collect();

    Json(hashes)
}

#[derive(Serialize)]
struct DateRange {
    min_date: String,
    max_date: String,
}

#[get("/date_range")]
async fn get_date_range(db_pool: &rocket::State<SqlitePool>) -> Json<DateRange> {
    let min_date: String = sqlx::query_scalar("SELECT MIN(timestamp) FROM logs")
        .fetch_one(db_pool.inner())
        .await
        .unwrap_or_else(|_| Utc::now().to_rfc3339());

    let max_date: String = sqlx::query_scalar("SELECT MAX(timestamp) FROM logs")
        .fetch_one(db_pool.inner())
        .await
        .unwrap_or_else(|_| Utc::now().to_rfc3339());

    if max_date.is_empty() {
        return Json(DateRange {
            min_date: (Utc::now() - Duration::days(7)).to_rfc3339(),
            max_date: Utc::now().to_rfc3339(),
        });
    }

    Json(DateRange {
        min_date,
        max_date,
    })
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

    // Initialize the QueryBuilder
    let mut builder = QueryBuilder::<sqlx::Sqlite>::new("
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
        WHERE hash = ");
    builder.push_bind(hash);

    let mut count = None;

    if let Some(ref query_params) = q {
        if let Some(ref s) = query_params.start {
            builder.push(" AND strftime('%s', timestamp) >= ");
            builder.push_bind(s);
        }
        if let Some(ref e) = query_params.end {
            builder.push(" AND strftime('%s', timestamp) <= ");
            builder.push_bind(e);
        }
        // `count` is Copy, so no need to clone
        count = query_params.count;
    }

    builder.push(" ORDER BY timestamp DESC");

    // Add LIMIT if `count` is provided
    if let Some(c) = count {
        builder.push(" LIMIT ");
        builder.push_bind(c);
    }

    // Build and execute the query
    let query = builder.build_query_as::<LogMessage>();

    let logs = query
        .fetch_all(db_pool.inner())
        .await
        .ok()?;

    Some(Json(logs))
}

#[get("/list_files")]
fn list_files() -> String {
    let files: Vec<_> = STATIC_DIR.files()
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
