use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const HOME: &str = "~";

pub fn name_of(path: &Path) -> String {
    name_os(path).to_string_lossy().into_owned()
}

pub fn name_os(path: &Path) -> &OsStr {
    path.file_name().unwrap_or(path.as_os_str())
}

pub fn where_is(path: &Path, home: &Path) -> String {
    let parent = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    match parent.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => HOME.to_owned(),
        Ok(rest) => format!("{HOME}/{}", rest.to_string_lossy()),
        Err(_) => parent.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_a_folder_is_reads_home_as_a_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(
            where_is(Path::new("/home/me/Projects/nova"), home),
            "~/Projects"
        );
        assert_eq!(where_is(Path::new("/home/me/Projects"), home), "~");
        assert_eq!(where_is(Path::new("/mnt/games/steam"), home), "/mnt/games");
        assert_eq!(where_is(Path::new("/home/meg/x"), home), "/home/meg");
        assert_eq!(where_is(Path::new("/"), home), "");
        assert_eq!(name_of(Path::new("/")), "/");
        assert_eq!(name_of(Path::new("/home/me/Projects")), "Projects");
    }
}
