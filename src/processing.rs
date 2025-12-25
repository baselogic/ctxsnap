use crate::config::AppConfig;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

const SAMPLE_SIZE: usize = 8 * 1024;
const CONTROL_CHAR_THRESHOLD: f64 = 0.02;

#[derive(Debug)]
pub enum FileStatus {
    Included {
        path: PathBuf,
        content: String,
        size: u64,
    },
    Omitted {
        path: PathBuf,
        reason: String,
        size: u64,
    },
}

pub fn process_file(path: PathBuf, config: &AppConfig) -> FileStatus {
    let metadata = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            return FileStatus::Omitted {
                path,
                reason: format!("Failed to read metadata: {}", e),
                size: 0,
            };
        }
    };

    let size = metadata.len();
    let max_bytes = config.max_file_mb.saturating_mul(1024 * 1024);

    // Initial check based on metadata
    if size > max_bytes {
        return FileStatus::Omitted {
            path,
            reason: format!(
                "Size {} MB exceeds limit of {} MB",
                size / 1024 / 1024,
                config.max_file_mb
            ),
            size,
        };
    }

    if size == 0 {
        return FileStatus::Included {
            path,
            content: String::new(),
            size: 0,
        };
    }

    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            return FileStatus::Omitted {
                path,
                reason: format!("Failed to open file: {}", e),
                size,
            };
        }
    };

    // Enforce limit during read via `take`.
    // Protect against growing files or incorrect metadata.
    // Read one extra byte to detect truncation.
    let read_limit = max_bytes.saturating_add(1);
    let mut full_buffer = Vec::with_capacity(std::cmp::min(size, max_bytes) as usize);

    if let Err(e) = file.take(read_limit).read_to_end(&mut full_buffer) {
        return FileStatus::Omitted {
            path,
            reason: format!("Failed to read file: {}", e),
            size,
        };
    }

    // Check if we actually hit the limit during physical read
    if full_buffer.len() as u64 > max_bytes {
        return FileStatus::Omitted {
            path,
            reason: format!(
                "File content exceeded limit of {} MB (detected during read)",
                config.max_file_mb
            ),
            size: full_buffer.len() as u64,
        };
    }

    // Check binary on the slice of the buffer
    let sample_len = std::cmp::min(SAMPLE_SIZE, full_buffer.len());
    if !is_mostly_text(&full_buffer[..sample_len]) {
        return FileStatus::Omitted {
            path,
            reason: "Binary detected".to_string(),
            size: full_buffer.len() as u64,
        };
    }

    // Decode
    let (cow, _encoding_used, had_errors) = encoding_rs::UTF_8.decode(&full_buffer);

    let mut content = if had_errors {
        let (cow_fallback, _, _) = encoding_rs::WINDOWS_1252.decode(&full_buffer);
        let text = cow_fallback.as_ref();

        // Fast control char check on the fallback string
        let control_count = text
            .chars()
            .filter(|c| c.is_control() && *c != '\n' && *c != '\r' && *c != '\t')
            .count();

        const FALLBACK_CONTROL_THRESHOLD: f64 = 0.01;
        let char_count = text.chars().count().max(1);
        let control_ratio = control_count as f64 / char_count as f64;

        if control_ratio > FALLBACK_CONTROL_THRESHOLD {
            return FileStatus::Omitted {
                path,
                reason: format!("Too many control chars: {:.2}%", control_ratio * 100.0),
                size: full_buffer.len() as u64,
            };
        }

        cow_fallback.into_owned()
    } else {
        cow.into_owned()
    };

    // Remove comments
    const MAX_STRIP_SIZE: u64 = 1024 * 1024;
    if config.remove_comments && (full_buffer.len() as u64) < MAX_STRIP_SIZE {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        content = strip_comments(&content, ext);
    }

    FileStatus::Included {
        path,
        content,
        size: full_buffer.len() as u64,
    }
}

static RE_C: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static RE_HASH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static RE_DASH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static RE_XML: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

/// Removes comments based on file extension to avoid cross-language syntax corruption.
fn strip_comments(content: &str, ext: &str) -> String {
    let ext_lower = ext.to_lowercase();

    enum Style {
        C,
        Hash,
        Dash,
        Xml,
        None,
    }

    let style = match ext_lower.as_str() {
        "rs" | "c" | "cpp" | "h" | "hpp" | "js" | "ts" | "java" | "go" | "kt" | "swift" | "css"
        | "cs" | "php" => Style::C,
        "py" | "sh" | "rb" | "yaml" | "yml" | "toml" | "dockerfile" | "pl" | "ps1" => Style::Hash,
        "sql" | "lua" | "hs" => Style::Dash,
        "html" | "xml" | "vue" | "svelte" => Style::Xml,
        _ => Style::None,
    };

    match style {
        Style::C => {
            // Match strings (double, single) OR comments (block, line)
            // Groups: 1=double quote string, 2=single quote string, 3=block comment, 4=line comment
            let re = RE_C.get_or_init(|| {
                regex::Regex::new(r#"(?m)"(\\.|[^"\\])*"|'(\\.|[^'\\])*'|(/\*[\s\S]*?\*/)|(//.*)$"#)
                    .unwrap()
            });
            re.replace_all(content, |caps: &regex::Captures| {
                // Check if captured groups are comments (group 3 or 4)
                if caps.get(3).is_some() || caps.get(4).is_some() {
                    "".to_string()
                } else {
                    caps.get(0).unwrap().as_str().to_string()
                }
            })
            .into_owned()
        }
        Style::Hash => {
            // Groups: 1=double quote string, 2=single quote string, 3=hash comment
            let re = RE_HASH.get_or_init(|| {
                regex::Regex::new(r#"(?m)"(\\.|[^"\\])*"|'(\\.|[^'\\])*'|(#.*)$"#).unwrap()
            });
            re.replace_all(content, |caps: &regex::Captures| {
                if caps.get(3).is_some() {
                    "".to_string()
                } else {
                    caps.get(0).unwrap().as_str().to_string()
                }
            })
            .into_owned()
        }
        Style::Dash => {
            // Groups: 1=double quote string, 2=single quote string, 3=dash comment
            let re = RE_DASH.get_or_init(|| {
                regex::Regex::new(r#"(?m)"(\\.|[^"\\])*"|'(\\.|[^'\\])*'|(--.*)$"#).unwrap()
            });
            re.replace_all(content, |caps: &regex::Captures| {
                if caps.get(3).is_some() {
                    "".to_string()
                } else {
                    caps.get(0).unwrap().as_str().to_string()
                }
            })
            .into_owned()
        }
        Style::Xml => {
            let re = RE_XML.get_or_init(|| regex::Regex::new(r#"(?s)<!--.*?-->"#).unwrap());
            re.replace_all(content, |_caps: &regex::Captures| "".to_string())
                .into_owned()
        }
        Style::None => content.to_string(),
    }
}

fn is_mostly_text(sample: &[u8]) -> bool {
    if sample.is_empty() {
        return true;
    }
    if sample.contains(&0) {
        return false;
    }

    if let Ok(s) = std::str::from_utf8(sample) {
        if s.is_empty() {
            return true;
        }
        let (total, control_count) = s.chars().fold((0, 0), |(t, c), ch| {
            let is_control = ch.is_control() && ch != '\n' && ch != '\r' && ch != '\t';
            (t + 1, if is_control { c + 1 } else { c })
        });
        return (control_count as f64 / total as f64) < CONTROL_CHAR_THRESHOLD;
    }

    let mut control = 0usize;
    for &b in sample {
        if (b < 0x09) || (b > 0x0D && b < 0x20 && b != 0x1B) || b == 0x7F {
            control += 1;
        }
    }
    (control as f64 / sample.len() as f64) < CONTROL_CHAR_THRESHOLD
}

pub fn fence_for(content: &str) -> String {
    let mut max_run = 0usize;
    let mut run = 0usize;
    for ch in content.chars() {
        if ch == '`' {
            run += 1;
            max_run = max_run.max(run);
        } else {
            run = 0;
        }
    }
    let len = std::cmp::max(3, max_run + 1);
    "`".repeat(len)
}
