use crate::args::Args;
use crate::processing::FileStatus;
use anyhow::{Context, Result};
use chrono::Local;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tempfile::SpooledTempFile;

pub struct SnapshotStats {
    pub output_path: Option<PathBuf>,
    pub total_files: usize,
    pub total_bytes: u64,
    pub total_lines: usize,
    pub omitted_count: usize,
    pub stats_by_extension: HashMap<String, (usize, u64)>,
    pub top_offenders: Vec<(PathBuf, u64)>, // (Path, Size)
}

pub struct SnapshotWriter {
    // Stores body content (files code)
    body_writer: BufWriter<SpooledTempFile>,
    // Track included files for TOC
    included_paths: Vec<PathBuf>,
    // Track omitted files for report
    omitted: Vec<(PathBuf, String, u64)>,
    // Stats
    total_bytes: u64,
    total_lines: usize,
    stats_by_extension: HashMap<String, (usize, u64)>,
    top_offenders: Vec<(PathBuf, u64)>,

    root: PathBuf,
    timestamp: String,
    timestamp_file_fmt: String,
}

impl SnapshotWriter {
    pub fn new(root: PathBuf) -> Self {
        let now = Local::now();
        Self {
            // Buffer up to 2MB in RAM before spilling to disk for the temp body
            body_writer: BufWriter::new(SpooledTempFile::new(2 * 1024 * 1024)),
            included_paths: Vec::new(),
            omitted: Vec::new(),
            total_bytes: 0,
            total_lines: 0,
            stats_by_extension: HashMap::new(),
            top_offenders: Vec::new(),
            root,
            timestamp: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            timestamp_file_fmt: now.format("%Y%m%d_%H%M%S").to_string(),
        }
    }

    pub fn process_status(&mut self, status: FileStatus) -> Result<()> {
        match status {
            FileStatus::Included {
                path,
                content,
                size,
            } => {
                // Update stats
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("(no-ext)")
                    .to_string();

                let entry = self.stats_by_extension.entry(ext).or_default();
                entry.0 += 1; // count
                entry.1 += size; // bytes

                // Track for top offenders (sorted once at finalize)
                self.top_offenders.push((path.clone(), size));

                self.write_file_content(&path, &content)?;
                self.included_paths.push(path);
                self.total_bytes += size;
                self.total_lines += content.lines().count();
            }
            FileStatus::Omitted { path, reason, size } => {
                self.omitted.push((path, reason, size));
            }
        }
        Ok(())
    }

    fn write_file_content(&mut self, path: &Path, content: &str) -> Result<()> {
        let rel_path = path.strip_prefix(&self.root).unwrap_or(path);
        let rel_path_str = rel_path.to_string_lossy().replace('\\', "/");
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        writeln!(self.body_writer, "## {}\n", rel_path_str)?;
        let fence = crate::processing::fence_for(content);
        writeln!(self.body_writer, "{}{}", fence, ext)?;

        // Write content and ensure it ends with a newline
        write!(self.body_writer, "{}", content)?;
        if !content.ends_with('\n') {
            writeln!(self.body_writer)?;
        }

        writeln!(self.body_writer, "{}\n", fence)?;

        Ok(())
    }

    pub fn finalize(mut self, args: &Args, discovery_errors: &[String]) -> Result<SnapshotStats> {
        // Sort top offenders
        self.top_offenders.sort_by(|a, b| b.1.cmp(&a.1));
        self.top_offenders.truncate(5);

        let (mut final_writer, output_path): (Box<dyn Write>, Option<PathBuf>) = if args.dry_run {
            (Box::new(BufWriter::new(io::stdout())), None)
        } else {
            let output_path = args.output.clone().unwrap_or_else(|| {
                self.root
                    .join(format!("merged_{}.md", self.timestamp_file_fmt))
            });

            let file = if args.force {
                File::create(&output_path).context("Failed to create output file")?
            } else {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&output_path)
                    .context(format!(
                        "Output file exists: {:?}. Use --force.",
                        output_path
                    ))?
            };

            (
                Box::new(BufWriter::with_capacity(64 * 1024, file)),
                Some(output_path),
            )
        };

        let display_root = self
            .root
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/");

        writeln!(final_writer, "# Project Snapshot\n")?;
        writeln!(final_writer, "**Base path:** `{}`", display_root)?;
        writeln!(final_writer, "**Timestamp:** {}\n", self.timestamp)?;

        writeln!(final_writer, "## Table of Contents\n")?;
        for path in &self.included_paths {
            let rel = path.strip_prefix(&self.root).unwrap_or(path);
            writeln!(
                final_writer,
                "- {}",
                rel.to_string_lossy().replace('\\', "/")
            )?;
        }
        writeln!(final_writer)?;

        self.body_writer.flush()?;
        let mut temp_file = self.body_writer.into_inner()?;
        temp_file.seek(SeekFrom::Start(0))?;
        io::copy(&mut temp_file, &mut final_writer)?;

        if !discovery_errors.is_empty() {
            writeln!(final_writer, "## Discovery Errors\n")?;
            for error in discovery_errors {
                writeln!(final_writer, "- {}", error)?;
            }
            writeln!(final_writer)?;
        }

        writeln!(final_writer, "## Omitted\n")?;
        if self.omitted.is_empty() {
            writeln!(final_writer, "_None._\n")?;
        } else {
            writeln!(final_writer, "| Path | Size (MB) | Reason |")?;
            writeln!(final_writer, "|---|---:|---|")?;
            for (path, reason, size) in &self.omitted {
                let rel = path.strip_prefix(&self.root).unwrap_or(path);
                let mb = (*size as f64) / 1024.0 / 1024.0;
                let clean_reason = reason.replace('|', "\\|");
                writeln!(
                    final_writer,
                    "| {} | {:.2} | {} |",
                    rel.to_string_lossy().replace('\\', "/"),
                    mb,
                    clean_reason
                )?;
            }
            writeln!(final_writer)?;
        }

        writeln!(final_writer, "---\n")?;
        writeln!(final_writer, "## Summary\n")?;
        writeln!(
            final_writer,
            "- **Files included:** {}",
            self.included_paths.len()
        )?;
        writeln!(final_writer, "- **Files omitted:** {}", self.omitted.len())?;
        writeln!(
            final_writer,
            "- **Total size included:** {:.2} MB",
            self.total_bytes as f64 / 1024.0 / 1024.0
        )?;
        writeln!(final_writer, "- **Total lines:** {}", self.total_lines)?;

        // Composition breakdown
        writeln!(final_writer, "\n### Composition\n")?;
        writeln!(final_writer, "| Extension | Files | Size (MB) |")?;
        writeln!(final_writer, "|---|---:|---:|")?;
        let mut sorted_stats: Vec<_> = self.stats_by_extension.iter().collect();
        sorted_stats.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));

        for (ext, (count, size)) in sorted_stats {
            let mb = *size as f64 / 1024.0 / 1024.0;
            writeln!(final_writer, "| .{} | {} | {:.2} |", ext, count, mb)?;
        }

        final_writer.flush()?;

        Ok(SnapshotStats {
            output_path,
            total_files: self.included_paths.len(),
            total_bytes: self.total_bytes,
            total_lines: self.total_lines,
            omitted_count: self.omitted.len(),
            stats_by_extension: self.stats_by_extension,
            top_offenders: self.top_offenders,
        })
    }
}
