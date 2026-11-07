//! A file's MIME type from its name, using the shared MIME database's
//! `mime/globs2` files (freedesktop shared-mime-info).
//!
//! Only `*.ext` globs are used, matched without case; the highest weight
//! wins, and on a tie the folder found first. That covers the formats
//! thumbnailers handle, without reading the file.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

const GLOBS_FILE: &str = "mime/globs2";
/// The real file is a few hundred KB.
const MAX_GLOBS_BYTES: u64 = 4 * 1024 * 1024;
const EXTENSION_GLOB: &str = "*.";
const COMMENT: char = '#';
const SEPARATOR: char = ':';

/// Extension (lower case) to MIME type.
pub struct MimeTypes(HashMap<String, (u32, String)>);

impl MimeTypes {
    /// Reads `mime/globs2` in each of `data_dirs`.
    pub fn load(data_dirs: &[PathBuf]) -> Self {
        let mut types = HashMap::new();
        for dir in data_dirs {
            let Some(text) = read_capped(&dir.join(GLOBS_FILE)) else {
                continue;
            };
            for line in text.lines().filter(|line| !line.starts_with(COMMENT)) {
                let mut fields = line.split(SEPARATOR);
                let (Some(weight), Some(mime), Some(glob)) =
                    (fields.next(), fields.next(), fields.next())
                else {
                    continue;
                };
                let (Ok(weight), Some(extension)) =
                    (weight.parse::<u32>(), glob.strip_prefix(EXTENSION_GLOB))
                else {
                    continue;
                };
                if extension.contains(['*', '?', '[']) {
                    continue;
                }
                let better = types
                    .get(&extension.to_lowercase())
                    .is_none_or(|(best, _): &(u32, String)| weight > *best);
                if better {
                    types.insert(extension.to_lowercase(), (weight, mime.to_owned()));
                }
            }
        }
        Self(types)
    }

    pub fn of(&self, path: &Path) -> Option<&str> {
        let extension = path.extension()?.to_string_lossy().to_lowercase();
        self.0.get(&extension).map(|(_, mime)| mime.as_str())
    }
}

fn read_capped(path: &Path) -> Option<String> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_GLOBS_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn extension_globs_by_weight_and_order() {
        let root = std::env::temp_dir().join(format!("nova-mime-{}", std::process::id()));
        let (first, second) = (root.join("a"), root.join("b"));
        fs::create_dir_all(first.join("mime")).unwrap();
        fs::create_dir_all(second.join("mime")).unwrap();
        fs::write(
            first.join(GLOBS_FILE),
            "# comment\n50:video/mp4:*.mp4\n50:image/heif:*.heic\n10:text/x-readme:README*\n",
        )
        .unwrap();
        fs::write(
            second.join(GLOBS_FILE),
            "50:video/other:*.mp4\n80:image/heic:*.HEIC\n",
        )
        .unwrap();

        let types = MimeTypes::load(&[first, second]);
        fs::remove_dir_all(&root).ok();

        assert_eq!(types.of(Path::new("/x/clip.MP4")), Some("video/mp4"));
        assert_eq!(types.of(Path::new("/x/photo.heic")), Some("image/heic"));
        assert_eq!(types.of(Path::new("/x/README")), None);
        assert_eq!(types.of(Path::new("/x/notes.zzz")), None);
    }

    #[test]
    fn a_large_database_is_read_to_its_end() {
        let root = std::env::temp_dir().join(format!("nova-mime-large-{}", std::process::id()));
        fs::create_dir_all(root.join("mime")).unwrap();
        // Bigger than any real globs2 file, still under the limit.
        let mut text = "# padding\n".repeat(200_000);
        text.push_str("50:video/mp4:*.mp4\n");
        fs::write(root.join(GLOBS_FILE), text).unwrap();

        let types = MimeTypes::load(std::slice::from_ref(&root));
        fs::remove_dir_all(&root).ok();

        assert_eq!(types.of(Path::new("/x/clip.mp4")), Some("video/mp4"));
    }
}
