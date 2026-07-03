use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Tell Cargo to rerun this build script if these files or directories change
    println!("cargo:rerun-if-changed=css/input.css");
    
    println!("cargo:rerun-if-changed=.env");
    
    // Watch templates directory and its contents
    if Path::new("templates").exists() {
        println!("cargo:rerun-if-changed=templates");
        if let Ok(entries) = std::fs::read_dir("templates") {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if let Some(path_str) = path.to_str() {
                        println!("cargo:rerun-if-changed={}", path_str);
                    }
                }
            }
        }
    }

    let mut tailwind_bin = env::var("TAILWIND_CLI_PATH").ok();

    // Try parsing .env file for TAILWIND_CLI_PATH if not already set in process env
    if tailwind_bin.is_none() {
        if let Ok(content) = std::fs::read_to_string(".env") {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    if key.trim() == "TAILWIND_CLI_PATH" {
                        tailwind_bin = Some(value.trim().to_string());
                        break;
                    }
                }
            }
        }
    }

    // Get Tailwind executable path or fallback to "tailwindcss"
    let mut tailwind_bin = tailwind_bin.unwrap_or_else(|| "tailwindcss".to_string());

    if tailwind_bin.starts_with("~/") {
        if let Ok(home) = env::var("HOME") {
            tailwind_bin = tailwind_bin.replacen("~", &home, 1);
        }
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("output.css");

    // Execute the Tailwind compiler
    let status = Command::new(&tailwind_bin)
        .args([
            "-i",
            "css/input.css",
            "-o",
            dest_path.to_str().unwrap(),
            "--minify",
        ])
        .status();

    match status {
        Ok(status) => {
            if !status.success() {
                panic!("Tailwind CSS compilation failed with exit code: {}", status);
            }
        }
        Err(e) => {
            panic!(
                "Failed to run Tailwind executable '{}'. Make sure it's installed and the TAILWIND_CLI_PATH environment variable is set correctly. Error: {}",
                tailwind_bin, e
            );
        }
    }
}
