use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

fn main() {
    let static_dir = Path::new("static/resources");
    fs::create_dir_all(static_dir).expect("Failed to create static directory");

    download_file(
        "https://cdnjs.cloudflare.com/ajax/libs/jquery/3.6.0/jquery.min.js",
        static_dir.join("jquery.min.js"),
    );

    download_file(
        "https://cdnjs.cloudflare.com/ajax/libs/jqueryui/1.12.1/jquery-ui.min.js",
        static_dir.join("jquery-ui.min.js"),
    );

    download_file(
        "https://code.jquery.com/ui/1.12.1/themes/base/jquery-ui.css",
        static_dir.join("jquery-ui.css"),
    );
}

fn download_file(url: &str, output_path: impl AsRef<Path>) {
    let output_path = output_path.as_ref();

    if !output_path.exists() {
        println!("Downloading {} to {:?}", url, output_path);
        let output = Command::new("curl")
            .arg("-sSL")
            .arg(url)
            .output()
            .expect("Failed to execute curl");

        if output.status.success() {
            let mut file = fs::File::create(output_path)
                .expect("Failed to create file for download");
            file.write_all(&output.stdout)
                .expect("Failed to write downloaded content");
        } else {
            eprintln!(
                "Failed to download {}: {}",
                url,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    } else {
        println!("File already exists: {:?}", output_path);
    }
}