use std::path::{Path, PathBuf};

use crate::{DebugPath, util::execute_cmd};

pub fn get_toolchain_source_path(debug_path: &DebugPath) -> Option<String> {
    if debug_path.lang == Some("DW_LANG_Rust".into()) {
        if let Some(producer) = debug_path.producer.as_ref() {
            let toolchain = rustc_toolchain_from_producer(&producer)?;
            let sysroot = execute_cmd(
                &PathBuf::from("rustc"),
                [&format!("+{toolchain}"), "--print", "sysroot"],
            )?;
        }
    }

    todo!()
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
        eprintln!("BLABLA");
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
        let version = after.split(['-', ' ', ')']).next()?;
        todo!() // TODO
    }
}
