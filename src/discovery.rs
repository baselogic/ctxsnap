use crate::config::AppConfig;
use anyhow::Result;
use ignore::WalkBuilder;
use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// Regex for the strictly excluded output files: merged_YYYYMMDD_HHMMSS.md
static MERGED_REGEX: OnceLock<Regex> = OnceLock::new();

/// Result of file discovery including any errors encountered
pub struct DiscoveryResult {
    pub files: Vec<PathBuf>,
    pub errors: Vec<String>,
}

/// Finds files to include in the snapshot.
/// `root` MUST be a canonicalized path for consistent strip_prefix behavior.
pub fn find_files(root: &Path, config: &AppConfig) -> Result<DiscoveryResult> {
    let mut files = Vec::new();
    let mut errors = Vec::new();

    // Lowercase normalization for case-insensitive matching
    let exclude_dirs: HashSet<String> = config
        .exclude_dir
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let exclude_files: HashSet<String> = config
        .exclude_file
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let exclude_exts: HashSet<String> = config
        .exclude_ext
        .iter()
        .map(|s| s.to_lowercase())
        .collect();

    // Exclude system/hidden directories
    let absolute_exclude_dirs: HashSet<&str> = [
        ".git", ".ssh", ".aws", ".gnupg", ".kube", ".cargo", ".rustup",
    ]
    .into_iter()
    .collect();

    let regex = MERGED_REGEX.get_or_init(|| Regex::new(r"^merged_\d{8}_\d{6}\.md$").unwrap());

    let walker = WalkBuilder::new(root)
        .follow_links(false)
        .max_depth(Some(config.depth))
        .hidden(false)
        .git_ignore(config.use_gitignore)
        .git_global(config.use_gitignore)
        .git_exclude(config.use_gitignore)
        .require_git(false) // Respect .gitignore even outside of a git repository
        .filter_entry({
            let exclude_dirs = exclude_dirs.clone();
            move |entry| {
                // Never prune the root itself (depth 0)
                if entry.depth() > 0 && entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy();
                    let name_lower = name.to_lowercase();
                    if exclude_dirs.contains(&name_lower)
                        || absolute_exclude_dirs.contains(name_lower.as_str())
                    {
                        return false;
                    }
                }
                true
            }
        })
        .build();

    for result in walker {
        match result {
            Ok(entry) => {
                let file_type = entry.file_type();

                if file_type.map(|ft| ft.is_symlink()).unwrap_or(false) {
                    continue;
                }

                if file_type.map(|ft| ft.is_dir()).unwrap_or(false) {
                    continue;
                }

                let path = entry.path();
                let name = entry.file_name().to_string_lossy();
                let name_lower = name.to_lowercase(); // Normalize once

                // 1. Snapshot outputs and internal config
                if regex.is_match(&name) || name_lower == "ctxsnap.toml" {
                    continue;
                }

                // 2. Lockfiles
                if !config.include_lockfiles && is_lockfile(&name) {
                    continue;
                }

                // 3. Exclude files (check lowercase)
                if exclude_files.contains(&name_lower) {
                    continue;
                }

                // Secret prefixes
                if name_lower.starts_with(".env")
                    && !name_lower.ends_with(".example")
                    && !name_lower.ends_with(".sample")
                    && !name_lower.ends_with(".template")
                    && name_lower != ".envrc"
                {
                    continue;
                }

                // 5. Exclude by extension (case-insensitive)
                if path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| exclude_exts.iter().any(|e| ext.eq_ignore_ascii_case(e)))
                {
                    continue;
                }

                files.push(path.to_path_buf());
            }
            Err(err) => {
                errors.push(format!("{}", err));
            }
        }
    }

    // Sort deterministically
    files.sort_by(|a, b| {
        let a_clean = crate::clean_path(a.strip_prefix(root).unwrap_or(a));
        let b_clean = crate::clean_path(b.strip_prefix(root).unwrap_or(b));
        a_clean.cmp(&b_clean)
    });

    Ok(DiscoveryResult { files, errors })
}

fn is_lockfile(name: &str) -> bool {
    const LOCKFILES: &[&str] = &[
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "Gemfile.lock",
        "poetry.lock",
    ];
    LOCKFILES.contains(&name)
}
