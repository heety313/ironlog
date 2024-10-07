// config.rs
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about)]
pub struct Config {
    /// Database file path
    #[clap(long, default_value = "logs.db")]
    pub log_db: String,

    /// TCP listener IP
    #[clap(long, default_value = "127.0.0.1")]
    pub tcp_listener_ip: String,

    /// TCP listener port
    #[clap(long, default_value = "5000")]
    pub tcp_listener_port: u16,

    /// API server IP
    #[clap(long, default_value = "127.0.0.1")]
    pub api_server_ip: String,

    /// API server port
    #[clap(long, default_value = "8000")]
    pub api_server_port: u16,

    /// Max number of hashes
    #[clap(long, default_value = "1000")]
    pub max_hashes: usize,

    /// Max number of logs per hash
    #[clap(long, default_value = "10000")]
    pub max_log_count: usize,
}
