#[macro_use] extern crate rocket;

use rocket::http::ContentType;
use include_dir::{include_dir, Dir};
use std::path::PathBuf;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, BufReader};
use once_cell::sync::Lazy;

static LOG_STORAGE: Lazy<Mutex<HashMap<String, Vec<LogMessage>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

// Embedding the static files
static STATIC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/static");

#[derive(Serialize, Deserialize, Clone)]
struct LogMessage {
    level: String,
    message: String,
    target: String,
    module_path: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    hash: String,
    timestamp: String,
}

// TCP Listener Task
#[rocket::main]
async fn main() {
    // Start TCP listener in a separate task
    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:5000").await.unwrap();
        println!("Log server is running on 127.0.0.1:5000");

        loop {
            let (socket, _) = listener.accept().await.unwrap();
            tokio::spawn(handle_client(socket));
        }
    });

    // Launch the Rocket server
    rocket::build()
        .mount("/api", routes![get_hashes, get_logs, list_files])
        .mount("/", routes![index, serve_file])  // Serve static files from root, including index.html
        .launch()
        .await.unwrap();
}

async fn handle_client(socket: tokio::net::TcpStream) {
    let reader = BufReader::new(socket);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(log_message) = serde_json::from_str::<LogMessage>(&line) {
            let mut storage = LOG_STORAGE.lock().unwrap();
            storage.entry(log_message.hash.clone())
                .or_insert_with(Vec::new)
                .push(log_message);
        }
    }
}

// Serve the index.html file when querying the root
#[get("/")]
fn index() -> Option<(ContentType, Vec<u8>)> {
    // Get the index.html file from the embedded directory
    let file = STATIC_DIR.get_file("index.html")?;

    // Guess the MIME type based on the file extension (in this case, it's HTML)
    let content_type = ContentType::from_extension(file.path().extension()?.to_str()?)
        .unwrap_or(ContentType::HTML);

    // Return the file contents with the appropriate content type
    Some((content_type, file.contents().to_vec()))
}

// HTTP Endpoints
#[get("/hashes")]
fn get_hashes() -> Json<Vec<String>> {
    let storage = LOG_STORAGE.lock().unwrap();
    Json(storage.keys().cloned().collect())
}

#[get("/logs/<hash>")]
fn get_logs(hash: &str) -> Option<Json<Vec<LogMessage>>> {
    let storage = LOG_STORAGE.lock().unwrap();
    storage.get(hash).cloned().map(Json)
}

// Route for listing files (for debugging purposes)
#[get("/list_files")]
fn list_files() -> String {
    let files: Vec<_> = STATIC_DIR.files()
        .map(|f| f.path().display().to_string())
        .collect();
    format!("Files in static dir: {:?}", files)
}

// Route for serving embedded static files
#[get("/<file..>")]
fn serve_file(file: PathBuf) -> Option<(ContentType, Vec<u8>)> {
    // Try to get the file from the embedded directory
    let file = STATIC_DIR.get_file(file.to_str()?)?;

    // Guess the MIME type based on the file extension
    let content_type = ContentType::from_extension(file.path().extension()?.to_str()?)
        .unwrap_or(ContentType::Bytes);

    // Return the file contents with the appropriate content type
    Some((content_type, file.contents().to_vec()))
}
