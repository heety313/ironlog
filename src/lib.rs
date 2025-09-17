use chrono::Utc;
use log::{Metadata, Record};
use serde::Serialize;
use std::io::Write;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

#[derive(Serialize)]
struct LogMessage<'a> {
    timestamp: String,
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
    stream: Arc<Mutex<TcpStream>>,
}

impl TcpLogger {
    pub fn init(
        server_addr: &str,
        hash: &str,
        level: log::LevelFilter,
    ) -> Result<(), log::SetLoggerError> {
        let stream = TcpStream::connect(server_addr).expect("Could not connect to log server");
        let logger = TcpLogger {
            server_addr: server_addr.to_string(),
            hash: hash.to_string(),
            stream: Arc::new(Mutex::new(stream)),
        };
        log::set_boxed_logger(Box::new(logger))?;
        log::set_max_level(level);
        Ok(())
    }

    pub fn new(
        server_addr: &str,
        hash: &str,
        _use_system_logger: bool,
    ) -> Result<Self, std::io::Error> {
        let stream = TcpStream::connect(server_addr)?;
        Ok(TcpLogger {
            server_addr: server_addr.to_string(),
            hash: hash.to_string(),
            stream: Arc::new(Mutex::new(stream)),
        })
    }

    pub fn info(&self, message: &str) {
        self.log_message(log::Level::Info, message);
    }

    pub fn error(&self, message: &str) {
        self.log_message(log::Level::Error, message);
    }

    pub fn debug(&self, message: &str) {
        self.log_message(log::Level::Debug, message);
    }

    pub fn warn(&self, message: &str) {
        self.log_message(log::Level::Warn, message);
    }

    fn log_message(&self, level: log::Level, message: &str) {
        let log_message = LogMessage {
            timestamp: Utc::now().to_rfc3339(),
            level: level.to_string(),
            message: message.to_string(),
            target: "independent_logger",
            module_path: None,
            file: None,
            line: None,
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

impl log::Log for TcpLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let log_message = LogMessage {
                timestamp: Utc::now().to_rfc3339(),
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

pub mod client_handler;
pub mod config;
pub mod types;
