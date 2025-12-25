use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper to get the command - avoids deprecated cargo_bin
fn cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ctxsnap"))
}

#[test]
fn test_help_shown_without_run_flag() {
    let mut cmd = cmd();
    cmd.arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("--run"));
}

#[test]
fn test_integration_basic_output() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("a.txt"), "Hello A").unwrap();
    fs::write(root.join("b.rs"), "fn main() {}").unwrap();
    fs::write(
        root.join("doc.md"),
        "Here is a code block:\n```rust\nfn foo() {}\n```\nEnd.",
    )
    .unwrap();

    let output_file = root.join("test_output.md");

    let mut cmd = cmd();
    cmd.arg(root)
        .arg("--run")
        .arg("--output")
        .arg(&output_file)
        .assert()
        .success();

    assert!(output_file.exists());
    let content = fs::read_to_string(&output_file).unwrap();

    // Check content
    assert!(content.contains("Hello A"));
    assert!(content.contains("fn main() {}"));
    // Check that doc.md is fenced with 4 backticks because it contains 3
    assert!(content.contains("````"));
    assert!(content.contains("Here is a code block:"));
}

#[test]
fn test_exclusion_of_previous_output() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("normal.txt"), "I am normal").unwrap();
    fs::write(root.join("merged_20240101_120000.md"), "OLD CONTENT").unwrap();

    let output_file = root.join("snapshot.md");

    let mut cmd = cmd();
    cmd.arg(root)
        .arg("--run")
        .arg("--output")
        .arg(&output_file)
        .assert()
        .success();

    let content = fs::read_to_string(&output_file).unwrap();

    assert!(content.contains("I am normal"));
    assert!(!content.contains("OLD CONTENT"));
}

#[test]
fn test_binary_exclusion() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("visible.txt"), "Text").unwrap();
    fs::write(root.join("binary.bin"), [0u8, 1, 2, 3, 4]).unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Visible text should be in content section
    assert!(stdout.contains("Text"));

    // Binary should be in Omitted section with reason
    assert!(stdout.contains("## Omitted"));
    assert!(stdout.contains("binary.bin"));
    assert!(stdout.contains("Binary detected"));

    // But binary file should NOT have its own content section header
    assert!(!stdout.contains("## binary.bin"));
}

#[test]
fn test_determinism() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("z.txt"), "Z").unwrap();
    fs::write(root.join("a.txt"), "A").unwrap();
    fs::create_dir(root.join("subdir")).unwrap();
    fs::write(root.join("subdir/m.txt"), "M").unwrap();

    // Run 1
    let mut cmd1 = cmd();
    let output1 = cmd1.arg(root).arg("--dry-run").output().unwrap();
    let stdout1 = String::from_utf8(output1.stdout).unwrap();

    // Extract only file content sections (not Table of Contents, Omitted, Summary)
    let extract_file_sections = |s: &str| -> Vec<String> {
        s.lines()
            .filter(|l| {
                l.starts_with("## ")
                    && !l.contains("Table of Contents")
                    && !l.contains("Omitted")
                    && !l.contains("Summary")
                    && !l.contains("Discovery Errors")
            })
            .map(|l| l.trim_start_matches("## ").to_string())
            .collect()
    };

    let order1 = extract_file_sections(&stdout1);

    // Run 2
    let mut cmd2 = cmd();
    let output2 = cmd2.arg(root).arg("--dry-run").output().unwrap();
    let stdout2 = String::from_utf8(output2.stdout).unwrap();
    let order2 = extract_file_sections(&stdout2);

    assert_eq!(
        order1, order2,
        "File order should be deterministic across runs"
    );

    // Verify alphabetical sort (a.txt < subdir/m.txt < z.txt)
    assert_eq!(order1.len(), 3);
    assert!(order1[0].contains("a.txt"));
    assert!(order1[1].contains("subdir") && order1[1].contains("m.txt"));
    assert!(order1[2].contains("z.txt"));
}

#[test]
fn test_hidden_files_included_by_default() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join(".hidden.txt"), "Hidden content").unwrap();
    fs::write(root.join("visible.txt"), "Visible").unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // With hidden(false), hidden files should be processed
    assert!(
        stdout.contains("## .hidden.txt") || stdout.contains(".hidden.txt"),
        "Hidden files should be included by default"
    );
    assert!(stdout.contains("Hidden content"));
}

#[test]
fn test_manual_exclusion() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("include.txt"), "Include me").unwrap();
    fs::write(root.join("exclude.txt"), "Exclude me").unwrap();
    fs::create_dir(root.join("exclude_dir")).unwrap();
    fs::write(root.join("exclude_dir/file.txt"), "Deep excluded").unwrap();

    let mut cmd = cmd();
    let output = cmd
        .arg(root)
        .arg("--dry-run")
        .arg("--exclude-file")
        .arg("exclude.txt")
        .arg("--exclude-dir")
        .arg("exclude_dir")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("Include me"));
    assert!(!stdout.contains("Exclude me"));
    assert!(!stdout.contains("Deep excluded"));
}

#[test]
fn test_limits() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // File 1: 1MB
    let megabyte = vec![b'a'; 1024 * 1024];
    fs::write(root.join("big1.txt"), &megabyte).unwrap();
    fs::write(root.join("big2.txt"), &megabyte).unwrap();

    let mut cmd = cmd();
    let output = cmd
        .arg(root)
        .arg("--dry-run")
        .arg("--max-total-mb")
        .arg("1")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // First file should be included (sorts first alphabetically)
    assert!(stdout.contains("## big1.txt"));

    // Second file should be omitted due to budget
    assert!(stdout.contains("## Omitted"));
    assert!(stdout.contains("big2.txt"));
    assert!(stdout.contains("Budget exceeded"));
}

#[test]
fn test_fence_escaping() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // File with many consecutive backticks
    let backticks = "`".repeat(10);
    let content = format!("Normal text\n{}\nMore text", backticks);
    fs::write(root.join("backticks.txt"), &content).unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Should use 11 backticks for fence (10 + 1)
    assert!(
        stdout.contains(&"`".repeat(11)),
        "Fence should be 11 backticks for content with 10"
    );
    assert!(
        stdout.contains(&backticks),
        "Original backtick content should be preserved"
    );
}

#[test]
fn test_no_overwrite_without_force() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("test.txt"), "Content").unwrap();

    let output_file = root.join("output.md");

    // First run - create the file
    let mut cmd1 = cmd();
    cmd1.arg(root)
        .arg("--run")
        .arg("--output")
        .arg(&output_file)
        .assert()
        .success();

    // Second run without --force should fail
    let mut cmd2 = cmd();
    cmd2.arg(root)
        .arg("--run")
        .arg("--output")
        .arg(&output_file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Output file exists"));
}

#[test]
fn test_force_overwrite() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("test.txt"), "Content").unwrap();

    let output_file = root.join("output.md");

    // First run
    let mut cmd1 = cmd();
    cmd1.arg(root)
        .arg("--run")
        .arg("--output")
        .arg(&output_file)
        .assert()
        .success();

    // Second run WITH --force should succeed
    let mut cmd2 = cmd();
    cmd2.arg(root)
        .arg("--run")
        .arg("--output")
        .arg(&output_file)
        .arg("--force")
        .assert()
        .success();
}

#[test]
fn test_invalid_max_file_mb() {
    let mut cmd = cmd();
    cmd.arg(".")
        .arg("--run")
        .arg("--max-file-mb")
        .arg("0")
        .assert()
        .failure()
        .stderr(predicate::str::contains("must be positive"));
}

#[test]
fn test_nonexistent_root() {
    let temp = TempDir::new().unwrap();
    let nonexistent = temp.path().join("does_not_exist_xyz123");

    let mut cmd = cmd();
    cmd.arg(&nonexistent)
        .arg("--run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn test_windows_1252_fallback() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create a file with Windows-1252 encoding (ï = 0xEF in Windows-1252)
    let windows1252_content = b"This is valid Windows-1252: na\xEFve";
    fs::write(root.join("win1252.txt"), windows1252_content).unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should be included (Windows-1252 is valid text)
    assert!(stdout.contains("## win1252.txt"));
    assert!(stdout.contains("This is valid Windows-1252"));
}

#[test]
fn test_utf8_with_bom() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // UTF-8 BOM + content
    let bom = b"\xEF\xBB\xBF";
    let content = b"Hello World";
    let mut full = bom.to_vec();
    full.extend_from_slice(content);

    fs::write(root.join("bom.txt"), full).unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Should be included (UTF-8 BOM is valid)
    assert!(stdout.contains("## bom.txt"));
    assert!(stdout.contains("Hello World"));
}

#[test]
fn test_mixed_line_endings() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Mix of \n, \r\n, \r
    fs::write(root.join("mixed.txt"), "Line1\nLine2\r\nLine3\rLine4").unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("## mixed.txt"));
    assert!(stdout.contains("Line1"));
}

#[test]
fn test_dry_run_no_file_created() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("test.txt"), "Content").unwrap();

    let mut cmd = cmd();
    cmd.arg(root).arg("--dry-run").assert().success();

    // Verify no merged file was created
    let merged_files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("merged_"))
        .collect();

    assert!(merged_files.is_empty(), "Dry run should not create files");
}

#[test]
fn test_empty_file_included() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("empty.txt"), "").unwrap();
    fs::write(root.join("normal.txt"), "content").unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Empty file should be included (not omitted)
    assert!(stdout.contains("## empty.txt"));
    // Normal file should also be there
    assert!(stdout.contains("## normal.txt"));
}

#[test]
fn test_include_lockfiles_flag() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("Cargo.lock"), "# Lockfile content").unwrap();
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();

    // Without flag - lockfile excluded
    let mut cmd1 = cmd();
    let output1 = cmd1.arg(root).arg("--dry-run").output().unwrap();
    let stdout1 = String::from_utf8(output1.stdout).unwrap();
    assert!(
        !stdout1.contains("Cargo.lock"),
        "Cargo.lock should be excluded by default"
    );

    // With flag - lockfile included
    let mut cmd2 = cmd();
    let output2 = cmd2
        .arg(root)
        .arg("--dry-run")
        .arg("--include-lockfiles")
        .output()
        .unwrap();
    let stdout2 = String::from_utf8(output2.stdout).unwrap();
    assert!(
        stdout2.contains("Cargo.lock"),
        "Cargo.lock should be included with --include-lockfiles"
    );
}

#[test]
fn test_base_path_format() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("test.txt"), "Content").unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();

    // Path should NOT contain \\?\
    assert!(
        !stdout.contains(r"\\?\"),
        "Base path should not contain \\\\?\\"
    );
    // Path should use forward slashes
    assert!(stdout.contains("**Base path:**"));
}

#[test]
fn test_env_prefix_exclusion() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create various .env variants
    fs::write(root.join(".env"), "SECRET=abc").unwrap();
    fs::write(root.join(".env.local"), "SECRET=local").unwrap();
    fs::write(root.join(".env.production"), "SECRET=prod").unwrap();
    fs::write(root.join("regular.txt"), "Normal content").unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // regular.txt should be included
    assert!(stdout.contains("## regular.txt"));
    assert!(stdout.contains("Normal content"));

    // All .env* variants should be excluded
    assert!(
        !stdout.contains(".env"),
        "All .env* files should be excluded"
    );
}

#[test]
fn test_remove_comments_context_aware() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // 1. Rust file: Should remove // but NOT #
    let rust_code = r#"
// Rust comment
#[derive(Debug)]
struct Foo; // comment
"#;
    fs::write(root.join("main.rs"), rust_code).unwrap();

    // 2. Python file: Should remove # but NOT //
    let py_code = r#"
# Python comment
val = 10 // 2
"#;
    fs::write(root.join("app.py"), py_code).unwrap();

    let mut cmd = cmd();
    let output = cmd
        .arg(root)
        .arg("--dry-run")
        .arg("--remove-comments")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Rust checks
    assert!(!stdout.contains("Rust comment"));
    assert!(
        stdout.contains("#[derive(Debug)]"),
        "Rust attributes should be PRESERVED"
    );

    // Python checks
    assert!(!stdout.contains("Python comment"));
    assert!(
        stdout.contains("10 // 2"),
        "Python integer division should be PRESERVED"
    );
}

#[test]
fn test_init_creates_local_config() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create a dummy file so the directory is valid
    fs::write(root.join("dummy.txt"), "content").unwrap();

    let config_path = root.join("ctxsnap.toml");
    assert!(
        !config_path.exists(),
        "Config should not exist before --init"
    );

    let mut cmd = cmd();
    cmd.arg(root)
        .arg("--init")
        .assert()
        .success()
        .stderr(predicate::str::contains("Initialized local config"));

    assert!(config_path.exists(), "Config should exist after --init");

    // Verify it's valid TOML with expected keys
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("exclude_ext"));
    assert!(content.contains("max_file_mb"));
    assert!(content.contains("depth"));
}

#[test]
fn test_secret_filtering_variants() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join(".env"), "SECRET").unwrap();
    fs::write(root.join(".env.local"), "SECRET").unwrap();
    fs::write(root.join(".env.example"), "TEMPLATE").unwrap();
    fs::write(root.join(".env.sample"), "TEMPLATE").unwrap();
    fs::write(root.join(".env.template"), "TEMPLATE").unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(!stdout.contains("## .env\n"));
    assert!(!stdout.contains("## .env.local\n"));
    assert!(stdout.contains("## .env.example\n"));
    assert!(stdout.contains("## .env.sample\n"));
    assert!(stdout.contains("## .env.template\n"));
}

#[test]
fn test_fence_extreme() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    let backticks = "`".repeat(110);
    fs::write(root.join("extreme.txt"), &backticks).unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    let expected_fence = "`".repeat(111);
    assert!(stdout.contains(&expected_fence));
}

#[test]
fn test_absolute_security_excludes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir(root.join(".ssh")).unwrap();
    fs::write(root.join(".ssh/id_rsa"), "PRIVATE KEY").unwrap();
    fs::create_dir(root.join(".aws")).unwrap();
    fs::write(root.join(".aws/credentials"), "KEYS").unwrap();
    fs::write(root.join("regular.txt"), "OK").unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("regular.txt"));
    assert!(!stdout.contains("PRIVATE KEY"));
    assert!(!stdout.contains(".ssh"));
    assert!(!stdout.contains(".aws"));
}

// Unix-only symlink test
#[cfg(unix)]
#[test]
fn test_symlink_not_followed() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("real.txt"), "Real content").unwrap();
    symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

    let mut cmd = cmd();
    let output = cmd.arg(root).arg("--dry-run").output().unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();

    // Content should appear exactly once (from real.txt only)
    let count = stdout.matches("Real content").count();
    assert_eq!(count, 1, "Content should appear exactly once");

    // link.txt should not have its own section
    assert!(!stdout.contains("## link.txt"));
}
