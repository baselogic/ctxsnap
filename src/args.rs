use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "ctxsnap", version, about = "Concatenates project files into a single Markdown snapshot.", long_about = None)]
pub struct Args {
    /// Root directory to scan. Defaults to current directory.
    #[arg(default_value = ".")]
    pub root: PathBuf,

    /// Actually run the snapshot generation. Without this, shows help.
    #[arg(long, short = 'r')]
    pub run: bool,

    /// Explicit output file path.
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Force overwrite if the output file already exists.
    #[arg(long)]
    pub force: bool,

    /// Dry run: print snapshot to stdout instead of writing to file.
    #[arg(long)]
    pub dry_run: bool,

    /// Maximum size per file in MB. Files larger than this are skipped.
    #[arg(long)]
    pub max_file_mb: Option<u64>,

    /// Maximum total size of included content in MB.
    #[arg(long)]
    pub max_total_mb: Option<u64>,

    /// Disable .gitignore usage.
    #[arg(long)]
    pub no_gitignore: bool,

    /// Include lock files (Cargo.lock, package-lock.json, etc.).
    #[arg(long)]
    pub include_lockfiles: bool,

    /// Additional file extensions to exclude (comma separated).
    #[arg(long, value_delimiter = ',')]
    pub exclude_ext: Vec<String>,

    /// Additional directories to exclude.
    #[arg(long, value_delimiter = ',')]
    pub exclude_dir: Vec<String>,

    /// Additional filenames to exclude.
    #[arg(long, value_delimiter = ',')]
    pub exclude_file: Vec<String>,

    /// Remove comments from supported file types.
    #[arg(long)]
    pub remove_comments: bool,

    /// Maximum depth to scan.
    #[arg(long)]
    pub depth: Option<usize>,

    /// Create a local ctxsnap.toml in the root directory.
    #[arg(long)]
    pub init: bool,
}

impl Args {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(m) = self.max_file_mb {
            anyhow::ensure!(m > 0, "max_file_mb must be positive");
            anyhow::ensure!(m <= 1024, "max_file_mb cannot exceed 1GB");
        }
        if let Some(m) = self.max_total_mb {
            anyhow::ensure!(m > 0, "max_total_mb must be positive");
            anyhow::ensure!(m <= 10240, "max_total_mb cannot exceed 10GB");
        }
        if let Some(d) = self.depth {
            anyhow::ensure!(d > 0 && d < 1000, "depth must be between 1 and 999");
        }
        anyhow::ensure!(
            self.root.exists(),
            "Root path does not exist: {:?}",
            self.root
        );
        Ok(())
    }
}
