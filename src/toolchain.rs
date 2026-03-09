use crate::{DebugPath, util::execute_cmd};
use std::path::PathBuf;

/// Resolves the rustc sysroot for the toolchain that compiled the binary.
/// Uses the DW_AT_producer DWARF attribute to identify the toolchain,
/// then calls `rustc +<toolchain> --print sysroot` to get the local path.
/// The sysroot contains stdlib sources needed to remap DWARF file paths.
pub fn get_toolchain_sysroot(debug_path: &DebugPath) -> Option<String> {
    if debug_path.lang == Some("DW_LANG_Rust".into()) {
        if let Some(producer) = debug_path.producer.as_ref() {
            let toolchain = rustc_toolchain_from_producer(&producer).or_else(|| {
                eprintln!("Failed to extract toolchain from DW_AT_producer");
                None
            })?;
            let sysroot = execute_cmd(
                &PathBuf::from("rustc"),
                [&format!("+{toolchain}"), "--print", "sysroot"],
            )
            .or_else(|| {
                eprintln!("Failed to extract sysroot for toolchain {toolchain}");
                None
            });

            return sysroot;
        }
    }

    None
}

/// Extracts the rustc toolchain specifier from the DW_AT_producer string.
/// Returns e.g. "1.89.0-sbpf-solana-v1.53" or "nightly-2026-03-01".
pub fn rustc_toolchain_from_producer(producer: &str) -> Option<String> {
    let after = producer.split("rustc version ").nth(1)?;

    // Till now there are two scenarios:
    // - the toolchain used is the Solana's fork
    // - for upstream eBPF it's nightly that's used
    if !after.contains("-dev") {
        // Handle upstream eBPF here
        // "1.96.0-nightly (80381278a 2026-03-01))" -> "nightly-2026-03-01"
        let date = after
            .split('(')
            .nth(1)?
            .split(')')
            .next()?
            .split_whitespace()
            .nth(1)?;
        Some(format!("nightly-{date}"))
    } else {
        // Handle Solana's fork here
        // "1.89.0-dev)" -> "1.89.0-sbpf-solana-v1.53"
        let _version = after.split(['-', ' ', ')']).next()?;
        todo!() // TODO
    }
}

/// Returns the Cargo home directory, where registry sources and caches are stored.
/// Respects the CARGO_HOME environment variable, defaulting to ~/.cargo.
pub fn cargo_home() -> String {
    std::env::var("CARGO_HOME")
        .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap()))
}
