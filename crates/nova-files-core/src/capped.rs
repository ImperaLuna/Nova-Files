//! Reading a small text file whole, up to a cap, for the system and
//! settings files the core reads (the MIME database, icon themes, toolkit
//! settings). Bytes that aren't UTF-8 become U+FFFD, so one odd line (an
//! old Latin-1 translation, say) never hides the rest of the file.

use std::fs::File;
use std::io::Read as _;
use std::path::Path;

/// The first `max_bytes` of the file at `path` as text; `None` when it
/// can't be read.
pub fn text(path: &Path, max_bytes: u64) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(max_bytes)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::*;

    #[test]
    fn reads_up_to_the_cap_and_keeps_lines_that_are_not_utf8() {
        let path = env::temp_dir().join(format!("nova-files-capped-{}", std::process::id()));
        fs::write(&path, b"Comment=Gr\xfcn\nName=Mine\n").unwrap();

        assert_eq!(
            text(&path, 1024).as_deref(),
            Some("Comment=Gr\u{fffd}n\nName=Mine\n")
        );
        assert_eq!(text(&path, 3).as_deref(), Some("Com"));
        fs::remove_file(&path).unwrap();
        assert_eq!(text(&path, 1024), None);
    }
}
