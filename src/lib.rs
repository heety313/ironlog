use log::{Metadata, Record};
use serde::Serialize;
use std::io::Write;
use std::net::TcpStream;
use std::sync::Mutex;
use chrono::Utc; // Add this import

#[derive(Serialize)]
struct LogMessage<'a> {
    timestamp: String, // Add this field
    level: String,
    message: String,
    target: &'a str,
    module_path: Option<&'a str>,
    file: Option<&'a str>,
    line: Option<u32>,
    hash: String,
}

pub struct TcpLogger {
    server_addr: String,
    hash: String,
    stream: Mutex<TcpStream>,
}

impl TcpLogger {
    pub fn init(server_addr: &str, hash: &str, level: log::LevelFilter) -> Result<(), log::SetLoggerError> {
        let stream = TcpStream::connect(server_addr).expect("Could not connect to log server");
        let logger = TcpLogger {
            server_addr: server_addr.to_string(),
            hash: hash.to_string(),
            stream: Mutex::new(stream),
        };
        log::set_boxed_logger(Box::new(logger))?;
        log::set_max_level(level);
        Ok(())
    }
}

impl log::Log for TcpLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let log_message = LogMessage {
                timestamp: Utc::now().to_rfc3339(), // Add this line
                level: record.level().to_string(),
                message: record.args().to_string(),
                target: record.target(),
                module_path: record.module_path_static(),
                file: record.file_static(),
                line: record.line(),
                hash: self.hash.clone(),
            };
            if let Ok(json) = serde_json::to_string(&log_message) {
                let mut stream = self.stream.lock().unwrap();
                if let Err(e) = writeln!(stream, "{}", json) {
                    eprintln!("Failed to send log: {}", e);
                }
            }
        }
    }

    fn flush(&self) {}
}

pub mod config;