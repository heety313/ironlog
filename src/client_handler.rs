// client_handler.rs

use crate::config::Config;
use crate::types::LogMessage;
use serde_json;
use sqlx::SqlitePool;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, Duration};

struct LogStats {
    hash_set: HashMap<String, usize>,
    total_hashes: usize,
}

struct LogQueue {
    queue: VecDeque<LogMessage>,
    max_size: usize,
}

impl LogQueue {
    fn new(max_size: usize) -> Self {
        LogQueue {
            queue: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    fn push(&mut self, log: LogMessage) -> Option<LogMessage> {
        let dropped = if self.queue.len() >= self.max_size {
            self.queue.pop_front()
        } else {
            None
        };
        self.queue.push_back(log);
        dropped
    }
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

pub async fn start_log_handler(db_pool: SqlitePool, config: Arc<Config>) {
    let log_stats = Arc::new(Mutex::new(LogStats {
        hash_set: HashMap::new(),
        total_hashes: 0,
    }));

    // Initialize log_stats with existing data from the database
    {
        let mut stats = log_stats.lock().await;
        let hashes: Vec<(String, i64)> = sqlx::query_as(
            "
            SELECT hash, COUNT(*) as count
            FROM logs
            GROUP BY hash
            ORDER BY count DESC
            LIMIT ?
        ",
        )
        .bind(config.max_hashes as i64)
        .fetch_all(&db_pool)
        .await
        .expect("Failed to fetch initial hashes from database");

        for (hash, count) in hashes {
            stats.hash_set.insert(hash, count as usize);
            stats.total_hashes += 1;
        }
    }

    // Create a channel for log messages
    let (log_sender, log_receiver) = mpsc::channel(10000);

    // Spawn a background task to write logs to the database
    let db_writer_pool = db_pool.clone();
    let db_writer_config = Arc::clone(&config);
    tokio::spawn(async move {
        database_writer(log_receiver, db_writer_pool, db_writer_config).await;
    });

    // Spawn a background task to periodically update the database and perform log count checks
    let stats_clone = Arc::clone(&log_stats);
    let pool_clone = db_pool.clone();
    let config_clone = Arc::clone(&config);
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(60)); // Run every 60 seconds
        loop {
            interval.tick().await;
            update_database(&pool_clone, &stats_clone, &config_clone).await;
            perform_log_count_checks(&pool_clone, &config_clone).await;
        }
    });

    // Start TCP listener
    let listener_addr = format!("{}:{}", config.tcp_listener_ip, config.tcp_listener_port);
    let listener = TcpListener::bind(&listener_addr)
        .await
        .expect("Failed to bind TCP listener");
    println!("Log server is running on {}", listener_addr);

    loop {
        let (socket, _) = listener
            .accept()
            .await
            .expect("Failed to accept connection");
        let db_pool = db_pool.clone();
        let config = Arc::clone(&config);
        let log_stats = Arc::clone(&log_stats);
        let log_sender = log_sender.clone();
        tokio::spawn(async move {
            handle_client(socket, db_pool, config, log_stats, log_sender).await;
        });
    }
}

pub async fn handle_client(
    socket: TcpStream,
    db_pool: SqlitePool,
    config: Arc<Config>,
    log_stats: Arc<Mutex<LogStats>>,
    log_sender: mpsc::Sender<LogMessage>,
) {
    let reader = BufReader::new(socket);
    let mut lines = reader.lines();

    static LOG_INDEX: AtomicUsize = AtomicUsize::new(0);

    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(mut log_message) = serde_json::from_str::<LogMessage>(&line) {
            let index = LOG_INDEX.fetch_add(1, Ordering::SeqCst);

            log_message.message = truncate_string(&log_message.message, config.max_log_length);

            let should_log = {
                let mut stats = log_stats.lock().await;
                if !stats.hash_set.contains_key(&log_message.hash) {
                    if stats.total_hashes < config.max_hashes {
                        stats.hash_set.insert(log_message.hash.clone(), 1);
                        stats.total_hashes += 1;
                        true
                    } else {
                        false
                    }
                } else {
                    *stats.hash_set.get_mut(&log_message.hash).unwrap() += 1;
                    true
                }
            };

            if should_log {
                // Send the log message to the database writer
                if let Err(e) = log_sender.send(log_message.clone()).await {
                    eprintln!("Failed to send log message to database writer: {}", e);
                }
            }
        }
    }
}

async fn database_writer(
    mut log_receiver: mpsc::Receiver<LogMessage>,
    db_pool: SqlitePool,
    config: Arc<Config>,
) {
    let log_queue = LogQueue::new(10000);
    let mut batch = Vec::with_capacity(1000);

    while let Some(log_message) = log_receiver.recv().await {
        batch.push(log_message);

        // If the batch is full or we haven't received a message for a while, flush the batch
        if batch.len() >= 1000 || log_receiver.is_empty() {
            write_logs_to_database(&batch, &db_pool, &config).await;
            batch.clear();
        }
    }
}

async fn write_logs_to_database(logs: &[LogMessage], db_pool: &SqlitePool, config: &Config) {
    let mut transaction = db_pool.begin().await.expect("Failed to begin transaction");

    for log in logs {
        sqlx::query(
            "
            INSERT INTO logs (level, message, target, module_path, file, line, hash, timestamp)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ",
        )
        .bind(&log.level)
        .bind(&log.message)
        .bind(&log.target)
        .bind(&log.module_path)
        .bind(&log.file)
        .bind(log.line)
        .bind(&log.hash)
        .bind(&log.timestamp)
        .execute(&mut *transaction)
        .await
        .expect("Failed to insert log into database.");
    }

    transaction
        .commit()
        .await
        .expect("Failed to commit transaction");
}

async fn update_database(db_pool: &SqlitePool, log_stats: &Arc<Mutex<LogStats>>, config: &Config) {
    let mut stats = log_stats.lock().await;

    // Print when writing to the database
    // Reset the hash set if it's too large
    if stats.total_hashes > config.max_hashes {
        stats.hash_set.clear();
        stats.total_hashes = 0;

        // Repopulate with current hashes from the database
        let hashes: Vec<(String, i64)> = sqlx::query_as(
            "
            SELECT hash, COUNT(*) as count
            FROM logs
            GROUP BY hash
            ORDER BY count DESC
            LIMIT ?
        ",
        )
        .bind(config.max_hashes as i64)
        .fetch_all(db_pool)
        .await
        .expect("Failed to fetch hashes from database");

        for (hash, count) in hashes {
            stats.hash_set.insert(hash, count as usize);
            stats.total_hashes += 1;
        }
    }
}

async fn perform_log_count_checks(db_pool: &SqlitePool, config: &Config) {
    let hashes: Vec<String> = sqlx::query_scalar("SELECT DISTINCT hash FROM logs")
        .fetch_all(db_pool)
        .await
        .expect("Failed to fetch hashes");

    for hash in hashes {
        let log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logs WHERE hash = ?")
            .bind(&hash)
            .fetch_one(db_pool)
            .await
            .unwrap_or(0);

        if log_count > config.max_log_count as i64 {
            let logs_to_delete = log_count - config.max_log_count as i64;

            sqlx::query(
                "
                DELETE FROM logs
                WHERE id IN (
                    SELECT id FROM logs
                    WHERE hash = ?
                    ORDER BY timestamp ASC
                    LIMIT ?
                )
            ",
            )
            .bind(&hash)
            .bind(logs_to_delete)
            .execute(db_pool)
            .await
            .expect("Failed to delete old logs.");
        }
    }
}
