use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::toolchain::{cargo_home, get_toolchain_sysroot};
use crate::{Dwarf, Outcome};
use anyhow::Result;

/// Prints each traced PC alongside its disassembly (from the `.trace` file)
/// and, when DWARF info is available, the corresponding source location and code.
pub fn trace_disassemble(
    src_paths: &HashSet<PathBuf>,
    regs_path: &Path,
    vaddrs: &[u64],
    dwarf: &Dwarf,
    colorize: bool,
) -> Result<Outcome> {
    // As we read files too often introduce a cache.
    let mut file_cache = HashMap::new();

    // Get toolchain path used for this debug object
    let toolchain_path = get_toolchain_sysroot(&dwarf.debug_path);

    // Take advantage of the `SBF_TRACE_DISASSEMBLE` generated trace
    // that is dumped into `.trace` (if requested). We can't generate it here because
    // we need an Executable/Analysis/etc.. What we do here instead is the
    // mapping from PC to code complementing the assembly.
    // Read it once and keep it.
    let disassemble_content =
        std::fs::read_to_string(regs_path.with_extension("trace").display().to_string())
            .unwrap_or("".into());

    // Wide instructions like lddw occupy two 8-byte slots, but we don't need to
    // skip the extra slot here. The PCs come from an execution trace recorded by
    // the VM, which already advanced past both slots during execution.
    for (idx, pc) in vaddrs.iter().enumerate() {
        // Vaddrs indices must map exactly to the output of disassemble content.
        let disassemble = read_nth_line(&disassemble_content, idx);
        // We actually print this one to be able to match with what `solana-sbpf`
        // prints in the disassemble.
        let pc_in_disassemble = pc_in_disassemble(*pc, dwarf)?;
        match dwarf.vaddr_entry_map.iter().find(|(vaddr, _)| *vaddr == pc) {
            None => {
                eprintln!("[{pc_in_disassemble}] (0x{pc:x}) {disassemble}");
            }
            Some((_, entry)) => {
                let (content, file_path) = file_cache.entry(entry.file).or_insert_with(|| {
                    // If we can't find the file try to remap the path directing to the toolchain sources.
                    if let Ok(content) = std::fs::read_to_string(entry.file) {
                        (content, entry.file.to_string())
                    } else {
                        let mapped_dwarf_path =
                            map_dwarf_path(&entry.file, toolchain_path.as_deref(), &cargo_home());
                        if mapped_dwarf_path != entry.file {
                            if let Ok(content) = std::fs::read_to_string(&mapped_dwarf_path) {
                                // Remapping did the trick, we can use the source from the toolchain path.
                                return (content, mapped_dwarf_path.to_string());
                            }
                        }
                        // Fill still not found.
                        ("".into(), entry.file.to_string())
                    }
                });
                let code = read_nth_line(content, entry.line.saturating_sub(1) as usize);
                if colorize {
                    let is_user_src = src_paths
                        .iter()
                        .any(|path| file_path.contains(&path.to_string_lossy().to_string()));
                    // Highlight user source files in purple, other files (e.g. dependencies) in blue.
                    let file_color = if is_user_src { "\x1b[35m" } else { "\x1b[34m" };
                    eprintln!(
                        "[{pc_in_disassemble}] (0x{pc:x}) {disassemble}\n  \x1b[33msrc:\x1b[0m {file_color}{}:{}\x1b[0m\n  \x1b[36mcode:\x1b[0m \x1b[32m{}\x1b[0m",
                        file_path,
                        entry.line,
                        code.trim(),
                    );
                } else {
                    eprintln!(
                        "[{pc_in_disassemble}] (0x{pc:x}) {disassemble}\n  src: {}:{}\n  code: {}",
                        file_path,
                        entry.line,
                        code.trim(),
                    );
                }
            }
        };
    }

    Ok(Outcome::TraceDisassemble)
}

/// Returns the nth line from the given string, or empty if out of bounds.
pub fn read_nth_line(file_content: &str, line_number: usize) -> String {
    file_content
        .lines()
        .nth(line_number)
        .unwrap_or("")
        .to_string()
}

/// The pc we can observe in `.trace` doesn't take into account
/// the start of the `.text` section start address as we do.
/// I believe it's a good idea to reconcile these two in some
/// follow-ups.
pub fn pc_in_disassemble(pc_in_trace: u64, dwarf: &Dwarf) -> Result<u64> {
    let pc_in_disassembly = (pc_in_trace - dwarf.text_section_offset) / 8;
    Ok(pc_in_disassembly)
}

/// Maps a DWARF-recorded source path to a local filesystem path.
/// DWARF paths from platform-tools builds use the CI runner's absolute paths (e.g. /home/runner/...).
/// If a rust source root is available, paths containing `/library/` are remapped to the local toolchain sysroot.
fn map_dwarf_path(dwarf_path: &str, rust_src_root: Option<&str>, cargo_root: &str) -> String {
    if let (Some(rust_src_root), Some(pos)) = (rust_src_root, dwarf_path.find("/library/")) {
        let suffix = &dwarf_path[pos..];
        return format!("{}/{}", rust_src_root, suffix);
    } else if let Some(pos) = dwarf_path.find(".cargo/registry/") {
        let suffix = &dwarf_path[pos + ".cargo/".len()..];
        return format!("{}/{}", cargo_root, suffix);
    } else {
        // fallback: path as-is
        return dwarf_path.to_string();
    }
}
