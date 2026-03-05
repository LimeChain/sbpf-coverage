use std::collections::HashMap;
use std::path::Path;

use crate::anyhow;
use crate::{Dwarf, Outcome};
use anyhow::Result;

/// Prints each traced PC alongside its disassembly (from the `.trace` file)
/// and, when DWARF info is available, the corresponding source location and code.
pub fn trace_disassemble(
    regs_path: &Path,
    vaddrs: &[u64],
    dwarf: &Dwarf,
    colorize: bool,
) -> Result<Outcome> {
    // As we read files too often introduce a cache.
    let mut file_cache = HashMap::new();
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
                let content = file_cache.entry(entry.file).or_insert_with(|| {
                    std::fs::read_to_string(entry.file).unwrap_or("".to_string())
                });
                let code = read_nth_line(content, entry.line.saturating_sub(1) as usize);
                if colorize {
                    eprintln!(
                        "[{pc_in_disassemble}] (0x{pc:x}) {disassemble}\n  \x1b[33msrc:\x1b[0m \x1b[34m{}:{}\x1b[0m\n  \x1b[36mcode:\x1b[0m \x1b[32m{}\x1b[0m",
                        entry.file,
                        entry.line,
                        code.trim(),
                    );
                } else {
                    eprintln!(
                        "[{pc_in_disassemble}] (0x{pc:x}) {disassemble}\n  src: {}:{}\n  code: {}",
                        entry.file,
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
    let pc_in_disassembly = (pc_in_trace
        - dwarf
            .loader
            .get_section_range(b".text")
            .ok_or(anyhow!("Can't get .text section begin address"))?
            .begin)
        / 8;
    Ok(pc_in_disassembly)
}
