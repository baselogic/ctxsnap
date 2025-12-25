mod args;
mod config;
mod discovery;
mod output;
mod processing;

use anyhow::{Context, Result};
use args::Args;
use clap::Parser;
use std::path::Path;
use std::time::Instant;

/// Strips Windows extended-length path prefix and normalizes to forward slashes.
pub fn clean_path(p: &Path) -> String {
    p.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
}

fn main() -> Result<()> {
    let start_time = Instant::now();
    let args = Args::parse();
    args.validate()?;

    let root = std::fs::canonicalize(&args.root).context(format!(
        "Failed to canonicalize root: {}",
        args.root.display()
    ))?;

    // Load global and local config
    let mut config = config::AppConfig::load_global()?;

    if let Some(local) = config::AppConfig::load_local(&root)? {
        config = local;
    }

    if let Some(v) = args.max_file_mb {
        config.max_file_mb = v;
    }
    if let Some(v) = args.max_total_mb {
        config.max_total_mb = v;
    }
    if let Some(v) = args.depth {
        config.depth = v;
    }
    if args.remove_comments {
        config.remove_comments = true;
    }
    if args.include_lockfiles {
        config.include_lockfiles = true;
    }
    if args.no_gitignore {
        config.use_gitignore = false;
    }
    config.exclude_ext.extend(args.exclude_ext.clone());
    config.exclude_dir.extend(args.exclude_dir.clone());
    config.exclude_file.extend(args.exclude_file.clone());

    // Handle --init
    if args.init {
        config.save_local(&root)?;
        eprintln!(
            "Initialized local config: {}/ctxsnap.toml",
            clean_path(&root)
        );
        return Ok(());
    }

    // Handle no action flags
    if !args.run && !args.dry_run {
        use clap::CommandFactory;
        Args::command().print_help()?;
        println!("\n\nUse --run or -r to generate the snapshot, or --dry-run to preview.");
        return Ok(());
    }

    eprintln!("Scanning: {}", clean_path(&root));

    // Discovery
    let discovery = discovery::find_files(&root, &config)?;
    let total_found = discovery.files.len();

    eprintln!("Found:    {} files", total_found);

    // Processing
    let max_total_bytes = config.max_total_mb.saturating_mul(1024 * 1024);
    let mut used: u64 = 0;

    let mut writer = output::SnapshotWriter::new(root.clone());

    for path in discovery.files {
        let size = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(e) => {
                writer.process_status(processing::FileStatus::Omitted {
                    path,
                    reason: format!("Metadata error: {}", e),
                    size: 0,
                })?;
                continue;
            }
        };

        if used.saturating_add(size) > max_total_bytes {
            writer.process_status(processing::FileStatus::Omitted {
                path,
                reason: format!("Budget exceeded (limit={} MB)", config.max_total_mb),
                size,
            })?;
            continue;
        }

        let status = processing::process_file(path, &config);

        if let processing::FileStatus::Included { size, .. } = &status {
            used = used.saturating_add(*size);
        }

        writer.process_status(status)?;
    }

    // Finalize
    let stats = writer.finalize(&args, &discovery.errors)?;
    let duration = start_time.elapsed();

    // Final Report
    eprintln!("\n--- Snapshot Summary ---");
    if let Some(path) = &stats.output_path {
        eprintln!("Output:   {}", clean_path(path));
    } else {
        eprintln!("Output:   (Dry Run - Stdout)");
    }

    eprintln!(
        "Stats:    {} included, {} omitted",
        stats.total_files, stats.omitted_count
    );
    eprintln!(
        "Content:  {:.2} MB ({} lines)",
        stats.total_bytes as f64 / 1024.0 / 1024.0,
        stats.total_lines
    );

    if !stats.stats_by_extension.is_empty() {
        eprintln!("\nComposition by Type:");
        let mut breakdown: Vec<_> = stats.stats_by_extension.iter().collect();
        breakdown.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
        for (ext, (count, size)) in breakdown {
            let mb = *size as f64 / 1024.0 / 1024.0;
            eprintln!("  .{:<8} {:>10.2} MB ({:>4} files)", ext, mb, count);
        }
    }

    if !stats.top_offenders.is_empty() {
        eprintln!("\nTop 5 Largest Files:");
        for (path, size) in stats.top_offenders {
            let mb = size as f64 / 1024.0 / 1024.0;
            let rel_path = path.strip_prefix(&root).unwrap_or(&path);
            eprintln!("  {:>10.2} MB  {}", mb, clean_path(rel_path));
        }
    }

    if !discovery.errors.is_empty() {
        eprintln!("\nErrors:   {} access errors", discovery.errors.len());
    }

    eprintln!("\nTime:     {:.3}s", duration.as_secs_f64());
    eprintln!("------------------------");

    Ok(())
}
