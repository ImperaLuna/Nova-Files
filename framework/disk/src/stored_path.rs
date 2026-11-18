use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

pub const END: u8 = 0;
pub const MAX_PATH: usize = 4096;

pub fn fits(folder: &Path) -> bool {
    let bytes = folder.as_os_str().as_bytes();
    folder.is_absolute() && bytes.len() <= MAX_PATH && !bytes.contains(&END)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn only_an_absolute_path_short_enough_and_without_the_end_byte_fits() {
        assert!(fits(Path::new("/a")));
        assert!(fits(&PathBuf::from(format!(
            "/{}",
            "a".repeat(MAX_PATH - 1)
        ))));
        assert!(!fits(&PathBuf::from(format!("/{}", "a".repeat(MAX_PATH)))));
        assert!(!fits(Path::new("relative")));
        assert!(!fits(Path::new("/a\0b")));
    }
}
