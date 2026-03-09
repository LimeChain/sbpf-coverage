use std::{
    env::current_dir,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::Loader;
use crate::addr2line::gimli::{self, DW_AT_language, DW_AT_producer, DwAt, DwTag, LittleEndian};
use crate::{Object, ObjectSection};
use anyhow::anyhow;
use sha2::{Digest, Sha256};

pub trait StripCurrentDir {
    fn strip_current_dir(&self) -> &Self;
}

impl StripCurrentDir for Path {
    fn strip_current_dir(&self) -> &Self {
        let Ok(current_dir) = current_dir() else {
            return self;
        };
        self.strip_prefix(current_dir).unwrap_or(self)
    }
}

pub fn find_files_with_extension(dirs: &[PathBuf], extension: &str) -> Vec<PathBuf> {
    let mut so_files = Vec::new();

    for dir in dirs {
        if dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == extension) {
                    so_files.push(path);
                }
            }
        }
    }

    so_files
}

pub fn compute_hash(slice: &[u8]) -> String {
    hex::encode(Sha256::digest(slice).as_slice())
}

pub fn get_section_start_address(loader: &Loader, section: &str) -> anyhow::Result<u64> {
    Ok(loader
        .get_section_range(section.as_bytes())
        .ok_or(anyhow!("Can't get {} section begin address", section))?
        .begin)
}

pub fn get_dwarf_attribute(
    object: &object::File,
    tag: DwTag,
    attribute: DwAt,
) -> anyhow::Result<String> {
    let load_section = |id: gimli::SectionId| -> Result<_, LittleEndian> {
        let data = object
            .section_by_name(id.name())
            .map(|s| s.data().unwrap_or(&[]))
            .unwrap_or(&[]);
        Ok(gimli::EndianSlice::new(data, LittleEndian))
    };

    let dwarf = addr2line::gimli::Dwarf::load(&load_section)
        .map_err(|_| anyhow!("Failed to load DWARF sections"))?;
    let mut iter = dwarf.units();
    while let Ok(Some(header)) = iter.next() {
        let Ok(unit) = dwarf.unit(header) else {
            continue;
        };
        let mut entries = unit.entries();
        while let Ok(Some(entry)) = entries.next_dfs() {
            if let Some(val) = entry.attr_value(attribute)
                && entry.tag() == tag
            {
                match attribute {
                    a if a == DW_AT_producer => {
                        if let Ok(s) = dwarf.attr_string(&unit, val) {
                            return Ok(s.to_string_lossy().to_string());
                        }
                    }
                    a if a == DW_AT_language => {
                        if let gimli::AttributeValue::Language(lang) = val {
                            return Ok(lang.to_string());
                        }
                    }
                    _ => continue,
                }
            }
        }
    }
    Err(anyhow!(
        "No DWARF entry found for {:?} with attribute {:?}",
        tag,
        attribute
    ))
}

pub fn execute_cmd<I, S>(program: &Path, args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| {
            eprintln!("Failed to execute {}: {}", program.display(), e);
        })
        .ok()?;

    let output = child
        .wait_with_output()
        .map_err(|e| eprintln!("failed to wait on child: {}", e))
        .ok()?;
    Some(
        output
            .stdout
            .as_slice()
            .iter()
            .map(|&c| c as char)
            .collect::<String>()
            .trim()
            .into(),
    )
}
