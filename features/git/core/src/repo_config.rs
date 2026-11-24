use std::fs::File;
use std::io::{self, Read as _};
use std::path::Path;

use crate::git_dir::common_dir;

const FILES: [&str; 2] = ["config", "config.worktree"];
const LARGEST: u64 = 1 << 20;
const RUNNING_SECTIONS: [&str; 3] = ["filter", "include", "includeif"];
const CORE: &str = "core";
const RUNNING_CORE_KEYS: [&str; 2] = ["fsmonitor", "attributesfile"];

pub fn runs_programs(git_dir: &Path) -> bool {
    let dirs = [Some(git_dir.to_path_buf()), common_dir(git_dir)];
    dirs.iter()
        .flatten()
        .flat_map(|dir| FILES.iter().map(move |name| dir.join(name)))
        .any(|file| match read_capped(&file) {
            Ok(Some(text)) => names_a_program(&text),
            Ok(None) => false,
            Err(error) => {
                log::info!("{}: {error}", file.display());
                true
            }
        })
}

fn read_capped(file: &Path) -> io::Result<Option<String>> {
    let opened = match File::open(file) {
        Ok(opened) => opened,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    opened.take(LARGEST + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LARGEST {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

pub fn names_a_program(text: &str) -> bool {
    let mut section = String::new();
    for line in text.lines().map(str::trim_start) {
        let entry = if let Some(header) = line.strip_prefix('[') {
            let (name, rest) = header.split_once(']').unwrap_or((header, ""));
            section = first_word(name, &['"', '.']);
            if RUNNING_SECTIONS.contains(&section.as_str()) {
                return true;
            }
            rest.trim_start()
        } else {
            line
        };
        let key = first_word(entry, &['=', ';', '#']);
        if section == CORE && RUNNING_CORE_KEYS.contains(&key.as_str()) {
            return true;
        }
    }
    false
}

fn first_word(text: &str, ends: &[char]) -> String {
    text.split(|c: char| c.is_whitespace() || ends.contains(&c))
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}
