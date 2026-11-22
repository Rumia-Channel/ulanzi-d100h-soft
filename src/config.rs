//! Config file location and persistence.
//! Schema and matching live in dial-core; this module only owns the path.
use std::path::PathBuf;

use dial_core::Config;

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ulanzi-d100h-soft")
        .join("config.json")
}

pub fn load() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match Config::from_json(&text) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("config: parse error in {}: {error}; using defaults", path.display());
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

pub fn save(config: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = config.to_json().map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}
