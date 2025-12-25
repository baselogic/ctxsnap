use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub exclude_ext: Vec<String>,
    pub exclude_dir: Vec<String>,
    pub exclude_file: Vec<String>,
    pub max_file_mb: u64,
    pub max_total_mb: u64,
    pub use_gitignore: bool,
    pub include_lockfiles: bool,
    pub remove_comments: bool,
    pub depth: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            exclude_ext: vec![
                "exe", "dll", "so", "dylib", "jpg", "jpeg", "png", "gif", "svg", "webp", "ico",
                "zip", "tar", "gz", "7z", "rar", "pdf", "db", "sqlite", "sqlite3", "pyc", "pem",
                "key",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            exclude_dir: vec![
                ".git",
                "node_modules",
                "target",
                "dist",
                "build",
                ".venv",
                "venv",
                ".idea",
                ".vscode",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            exclude_file: vec![".DS_Store", "Thumbs.db", ".gitignore", ".gitattributes"]
                .into_iter()
                .map(String::from)
                .collect(),
            max_file_mb: 10,
            max_total_mb: 200,
            use_gitignore: true,
            include_lockfiles: false,
            remove_comments: false,
            depth: 50,
        }
    }
}

impl AppConfig {
    /// Load global config from next to the executable.
    /// Creates it with defaults if it doesn't exist.
    pub fn load_global() -> Result<Self> {
        let exe_path = std::env::current_exe().context("Failed to get executable path")?;
        let global_path = exe_path
            .parent()
            .context("Executable has no parent directory")?
            .join("ctxsnap.toml");

        if global_path.exists() {
            let content = fs::read_to_string(&global_path)
                .context(format!("Failed to read global config: {:?}", global_path))?;
            toml::from_str(&content).context("Global config is corrupted")
        } else {
            let config = Self::default();
            let content = toml::to_string_pretty(&config)?;
            
            // Report write failure but allow process to continue (soft fail)
            if let Err(e) = fs::write(&global_path, content) {
                eprintln!("Warning: Could not create global config at {:?}: {}", global_path, e);
            }
            Ok(config)
        }
    }

    /// Load local config from project root. Returns None if it doesn't exist.
    pub fn load_local(root: &Path) -> Result<Option<Self>> {
        let local_path = root.join("ctxsnap.toml");
        if local_path.exists() {
            let content = fs::read_to_string(&local_path)
                .context(format!("Failed to read local config: {:?}", local_path))?;
            let config = toml::from_str(&content).context("Local config is corrupted")?;
            Ok(Some(config))
        } else {
            Ok(None)
        }
    }

    /// Save config to local project root.
    pub fn save_local(&self, root: &Path) -> Result<()> {
        let local_path = root.join("ctxsnap.toml");
        let content = toml::to_string_pretty(self)?;
        fs::write(&local_path, content)
            .context(format!("Failed to write local config: {:?}", local_path))?;
        Ok(())
    }
}
