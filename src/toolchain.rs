use crate::{DebugPath, util::execute_cmd};
use std::path::PathBuf;

/// Resolves the rustc sysroot for the toolchain that compiled the binary.
/// Uses the DW_AT_producer DWARF attribute to identify the toolchain,
/// then calls `rustc +<toolchain> --print sysroot` to get the local path.
/// The sysroot contains stdlib sources needed to remap DWARF file paths.
pub fn get_toolchain_sysroot(debug_path: &DebugPath) -> Option<String> {
    if debug_path.lang == Some("DW_LANG_Rust".into())
        && let Some(producer) = debug_path.producer.as_ref()
    {
        let (toolchain, platform_tools) =
            rustc_toolchain_from_producer(producer).or_else(|| {
                eprintln!("Failed to extract toolchain from DW_AT_producer");
                None
            })?;
        let file_name = debug_path
            .path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let pt = platform_tools
            .as_ref()
            .map(|v| format!(", platform-tools {v}"))
            .unwrap_or_default();
        eprintln!("{file_name} likely: compiler {producer}{pt}, toolchain {toolchain}");
        let sysroot = execute_cmd(
            &PathBuf::from("rustc"),
            [&format!("+{toolchain}"), "--print", "sysroot"],
        )
        .or_else(|| {
            eprintln!("Failed to extract sysroot for toolchain {toolchain}");
            None
        });

        // Stdlib sources live under <sysroot>/lib/rustlib/src/rust,
        // append that so the returned path is directly usable for remapping.
        return sysroot.map(|s| format!("{}/lib/rustlib/src/rust", s.trim()));
    }

    None
}

/// Extracts the rustc toolchain specifier from the DW_AT_producer string.
/// Returns (toolchain, optional platform-tools version),
/// e.g. ("1.89.0-sbpf-solana-v1.53", Some("v1.53")) or ("nightly-2026-03-01", None).
pub fn rustc_toolchain_from_producer(producer: &str) -> Option<(String, Option<String>)> {
    let after = producer.split("rustc version ").nth(1)?;

    // Till now there are two scenarios:
    // - the toolchain used is the Solana's fork
    // - for upstream eBPF it's nightly that's used
    if !after.contains("-dev") {
        // Handle upstream eBPF here
        // "1.96.0-nightly (80381278a 2026-03-01))" -> "nightly-2026-03-01"
        // TODO: Unfortunately the date may differ from the commit hash and be on the next day.
        let date = after
            .split('(')
            .nth(1)?
            .split(')')
            .next()?
            .split_whitespace()
            .nth(1)?;
        Some((format!("nightly-{date}"), None))
    } else {
        // Handle Solana's fork here, two possible cases:
        // - DW_AT_producer ("clang LLVM (rustc version 1.89.0-dev)")
        // - DW_AT_producer ("clang LLVM (rustc version 1.89.0-dev (daa3af4 2026-03-03))")
        //   as https://github.com/anza-xyz/rust/pull/148 got merged
        // "1.89.0-dev)" or "1.89.0-dev (daa3af4 2026-03-03))"
        let version_dev = after.split(')').next()?;
        let rustc_version = version_dev.split('-').next()?;
        let producer_rustc_commit_hash = version_dev
            .split('(')
            .nth(1)
            .and_then(|inner| inner.split_whitespace().next())
            .map(String::from);
        let platform_tools_version =
            get_platform_tools_version(rustc_version, producer_rustc_commit_hash.as_deref())?;
        Some((
            // "1.89.0-sbpf-solana-v1.53"
            format!("{rustc_version}-sbpf-solana-{platform_tools_version}"),
            Some(platform_tools_version),
        ))
    }
}

/// Returns the Cargo home directory, where registry sources and caches are stored.
/// Respects the CARGO_HOME environment variable, defaulting to ~/.cargo.
pub fn cargo_home() -> String {
    std::env::var("CARGO_HOME")
        .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap()))
}

/// Scans locally installed platform-tools in ~/.cache/solana/ to find which version
/// contains a rustc matching the given version string (e.g. "1.89.0").
/// Returns the version directory name (e.g. "v1.53") if found, starting from the latest.
pub fn get_platform_tools_version(
    binary_rustc_version: &str,
    producer_rustc_commit_hash: Option<&str>,
) -> Option<String> {
    let home_dir = std::env::var("HOME").ok()?;
    let base_line = format!("{}/.cache/solana", home_dir);
    let paths = std::fs::read_dir(&base_line).ok()?;

    let mut platform_tools_dirs = Vec::new();
    for path in paths {
        let Ok(path) = path else { continue };
        // Filter only directories
        let Ok(file_type) = path.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let dir_name = path.file_name();
        // Typically installed versions start with vX.Z
        if !dir_name.to_string_lossy().starts_with("v") {
            continue;
        }
        platform_tools_dirs.push(dir_name.to_string_lossy().to_string());
    }

    platform_tools_dirs.sort();

    // Iterate backwards to start with the latest toolchain.
    // If the producer includes a rustc commit hash, use it to narrow the match;
    // otherwise fall back to version-string matching.
    platform_tools_dirs
        .iter()
        .rev()
        .map(|ver| {
            (
                ver.clone(),
                format!("{}/{}/platform-tools/rust/bin/rustc", base_line, ver),
            )
        })
        .filter(|(ver, rustc_path)| {
            if !PathBuf::from(&rustc_path).is_file() {
                return false;
            }
            let Some(platform_tools_rustc_version) =
                execute_cmd(&PathBuf::from(rustc_path), ["--version"])
            else {
                return false;
            };
            let rustc_commit_hash =
                std::fs::read_to_string(format!("{base_line}/{ver}/platform-tools/version.md"))
                    .ok()
                    .and_then(|version_file_content| {
                        version_file_content
                            .lines()
                            .find(|line| line.contains("rust.git"))
                            .and_then(|line| line.split(' ').next())
                            .map(String::from)
                    });
            if let (Some(rch), Some(ch)) = (rustc_commit_hash, producer_rustc_commit_hash)
                && !rch.starts_with(ch)
            {
                return false;
            }

            platform_tools_rustc_version.contains(binary_rustc_version)
        })
        .map(|(ver, _)| ver)
        .next()
}

/// Maps a DWARF-recorded source path to a local filesystem path.
/// DWARF paths from CI/build environments use absolute paths (e.g. /home/runner/...).
/// If a rust source root is available, paths containing `/library/` are remapped to the local toolchain sysroot.
pub fn map_dwarf_path(dwarf_path: &str, rust_src_root: Option<&str>, cargo_root: &str) -> String {
    if let (Some(rust_src_root), Some(pos)) = (rust_src_root, dwarf_path.find("/library/")) {
        let suffix = &dwarf_path[pos..];
        format!("{}/{}", rust_src_root, suffix)
    } else if let Some(pos) = dwarf_path
        .find(".cargo/registry/")
        .or_else(|| dwarf_path.find(".cargo/git/"))
    {
        let suffix = &dwarf_path[pos + ".cargo/".len()..];
        format!("{}/{}", cargo_root, suffix)
    } else {
        // fallback: path as-is
        dwarf_path.to_string()
    }
}
