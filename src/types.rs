use rocket::http::ContentType;
use rocket::form::FromForm;
use rocket::serde::json::Json;
use include_dir::{include_dir, Dir};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use std::fs;
use std::sync::Arc;
use chrono;

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
