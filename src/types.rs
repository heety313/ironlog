use chrono;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct LogMessage {
    pub level: String,
    pub message: String,
    pub target: String,
    pub module_path: Option<String>,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub hash: String,
    #[serde(default = "default_timestamp")]
    pub timestamp: String,
}

// Make sure to define the default_timestamp function
pub fn default_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}
