# ctxsnap

![Rust CI](https://github.com/baselogic/ctxsnap/actions/workflows/ci.yml/badge.svg) ![License](https://img.shields.io/github/license/baselogic/ctxsnap) ![Version](https://img.shields.io/github/v/tag/baselogic/ctxsnap)

High-performance, streaming source code bundler for LLM context injection.

## Overview

`ctxsnap` is a command-line tool designed to consolidate an entire software repository into a single, well-structured Markdown document. While similar tools exist, most are written in interpreted languages and load the entire repository into memory, leading to significant resource exhaustion when processing large projects.

`ctxsnap` is built in Rust with a focus on systems-level efficiency. It utilizes a streaming architecture that maintains a low memory footprint (O(1) for file content via spooling, O(N) for metadata storage), ensuring it can run on resource-constrained environments (e.g., embedded systems or low-end VPS) while processing multi-gigabyte codebases.

## Key Architectural Features

### 1. Streaming Engine
Unlike tools that collect all file contents into a massive vector before writing, `ctxsnap` implements a dual-stage spooling mechanism. 
*   **Discovery**: Traverses the filesystem using a highly efficient walker (`ignore` crate) that respects `.gitignore` rules.
*   **Processing**: Reads, validates, and decodes files individually.
*   **Spooling**: Content is written to a `SpooledTempFile`. This maintains data in RAM up to a configurable threshold (2MB) before automatically spilling to disk. This ensures that only the metadata (paths and stats) stays in primary memory, while the bulk data is handled by the OS filesystem cache or disk.

### 2. Context-Aware Comment Stripping
Token limits in LLMs are a primary constraint. `ctxsnap` provides a `--remove-comments` flag that uses context-aware regular expressions based on file extensions.
*   **C-Style**: Handles `//` and `/* */` (Rust, C, C++, Java, JS, TS, etc.).
*   **Hash-Style**: Handles `#` (Python, Shell, Ruby, TOML, YAML, etc.).
*   **Dash-Style**: Handles `--` (SQL, Haskell, Lua).
*   **XML-Style**: Handles `<!-- -->` (HTML, XML, Vue, Svelte).
*   **Safety**: Context-aware regex parsing attempts to avoid stripping comments inside quoted strings. **Note:** This is a best-effort approach; complex cases like raw strings or heredocs may still be affected. Large files (>1MB) are bypassed to maintain high throughput and avoid excessive CPU usage on massive blobs.

### 3. Hierarchical Configuration
`ctxsnap` follows a deterministic configuration cascade:
1.  **Hardcoded Defaults**: Internal safety limits and common binary exclusions.
2.  **Global Config**: `ctxsnap.toml` located in the executable's directory. Created automatically on first run.
3.  **Local Config**: `ctxsnap.toml` in the project root. Allows project-specific overrides.
4.  **CLI Arguments**: Explicit flags that override all lower levels.

### 4. Robust Encoding and Binary Detection
*   **Zero-NUL Check**: Quickly identifies binary blobs by scanning for NUL bytes in the first 8KB.
*   **Fallback Decoding**: Primary attempt via UTF-8. If it fails, the engine falls back to `WINDOWS_1252`. Content is only included if the resulting control-character ratio remains below 2%, ensuring text fidelity while omitting garbage data.

## Installation

Building from source requires the Rust toolchain (MSRV 1.75+).

```bash
git clone https://github.com/baselogic/ctxsnap.git
cd ctxsnap
cargo build --release
```

The binary will be located at `target/release/ctxsnap`. Move it to a directory in your PATH.

## Usage

Generate a snapshot of the current directory:
```bash
ctxsnap --run
```

Preview what would be included without writing to disk:
```bash
ctxsnap --dry-run
```

Initialize a project-specific configuration file:
```bash
ctxsnap --init
```

### Common Flags
*   `-r, --run`: Required to perform actual file generation.
*   `-o, --output <PATH>`: Explicit path for the resulting Markdown file.
*   `--remove-comments`: Strips comments based on language syntax.
*   `--max-file-mb <UINT>`: Skip files larger than N megabytes.
*   `--max-total-mb <UINT>`: Hard limit on the cumulative size of the snapshot content.
*   `--include-lockfiles`: Force inclusion of package manager lockfiles (excluded by default).

## Telemetry and Diagnostics

At the end of every run, `ctxsnap` provides a detailed summary to `stderr`:
*   **Composition by Type**: A breakdown of extensions, file counts, and total size contribution.
*   **Top 5 Largest Files**: Identifies which files are consuming the most space in your snapshot.
*   **Path Normalization**: Automatically strips Windows UNC prefixes (`\\?\`) and normalizes backslashes to forward slashes for cross-platform compatibility.

## Logic Flow

1.  **Validate**: Sanity checks on CLI arguments and root path.
2.  **Canonicalize**: Resolves absolute paths to ensure consistent prefix stripping.
3.  **Traverse**: Discovers valid files while pruning excluded directories.
4.  **Process**: 
    *   Read metadata.
    *   Budget check (MB limits).
    *   Binary check.
    *   Decode and (optional) strip comments.
5.  **Stream**: Write content to the spooler.
6.  **Finalize**: Assemble the final document: Header -> Table of Contents -> Spooled Body -> Telemetry Tables.

## License

MIT License. See `LICENSE` for details.
