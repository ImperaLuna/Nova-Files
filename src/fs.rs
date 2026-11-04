//! Everything the UI reads from disk: folder entries, drives and recent files.
//!
//! All of it is synchronous and runs on the UI thread for now. Moving folder
//! loading off the UI thread is the first real speed work.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::SharedString;

pub struct Entry {
    pub name: SharedString,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

/// Visible entries of `path`: folders first, then files, each sorted by
/// name, case-insensitive. Hidden entries (starting with a dot) are skipped.
pub fn list_entries(path: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            // `fs::metadata` follows symlinks, so a link to a folder opens like one.
            let meta = std::fs::metadata(&path).ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            Some(Entry {
                name: name.into(),
                size: meta.as_ref().filter(|_| !is_dir).map(|m| m.len()),
                modified: meta.as_ref().and_then(|m| m.modified().ok()),
                path,
                is_dir,
            })
        })
        .collect();
    entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
    entries
}

/// Number of visible entries in a folder, for the Home cards.
pub fn count_entries(path: &Path) -> usize {
    std::fs::read_dir(path)
        .map(|r| {
            r.flatten()
                .filter(|e| !e.file_name().as_bytes().starts_with(b"."))
                .count()
        })
        .unwrap_or(0)
}

pub struct Drive {
    pub name: SharedString,
    pub mount: PathBuf,
    pub total: u64,
    pub free: u64,
}

impl Drive {
    pub fn used_fraction(&self) -> f32 {
        if self.total == 0 {
            0.
        } else {
            1. - self.free as f32 / self.total as f32
        }
    }
}

/// Mounted block devices from /proc/mounts, one per device, root first.
pub fn drives() -> Vec<Drive> {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut seen = Vec::new();
    let mut drives = Vec::new();
    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let (Some(device), Some(mount)) = (parts.next(), parts.next()) else {
            continue;
        };
        if !device.starts_with("/dev/") || mount.starts_with("/boot") || seen.contains(&device) {
            continue;
        }
        // /proc/mounts escapes spaces as \040.
        let mount = PathBuf::from(mount.replace("\\040", " "));
        let Some((total, free)) = space(&mount) else {
            continue;
        };
        seen.push(device);
        let name = if mount == Path::new("/") {
            "System".to_owned()
        } else {
            mount
                .file_name()
                .map_or("Drive".into(), |n| n.to_string_lossy().into_owned())
        };
        drives.push(Drive {
            name: name.into(),
            mount,
            total,
            free,
        });
    }
    drives.sort_by_key(|d| d.mount != Path::new("/"));
    drives
}

/// Total and available bytes on the filesystem holding `path`.
fn space(path: &Path) -> Option<(u64, u64)> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a valid C string and `stat` is a writable statvfs.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let block = stat.f_frsize as u64;
    Some((stat.f_blocks as u64 * block, stat.f_bavail as u64 * block))
}

/// The most recently used files, newest first, from the freedesktop
/// recently-used list that GTK and most desktops write.
pub fn recent_files(home: &Path, limit: usize) -> Vec<PathBuf> {
    let xbel =
        std::fs::read_to_string(home.join(".local/share/recently-used.xbel")).unwrap_or_default();
    let mut files: Vec<(String, PathBuf)> = xbel
        .split("<bookmark ")
        .skip(1)
        .filter_map(|b| {
            let href = attr(b, "href")?.strip_prefix("file://")?;
            let modified = attr(b, "modified").unwrap_or_default().to_owned();
            let path = PathBuf::from(percent_decode(href));
            path.is_file().then_some((modified, path))
        })
        .collect();
    // ISO 8601 timestamps sort as text.
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().map(|(_, p)| p).take(limit).collect()
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        if bytes[i] == b'%'
            && let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// "1.2 MB" style sizes, in powers of 1000 like most Linux file managers.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000. && unit < UNITS.len() - 1 {
        value /= 1000.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// "4 Nov 2026, 14:05", in UTC for now. Local time zones come later.
pub fn format_time(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{day} {} {year}, {:02}:{:02}",
        MONTHS[month as usize - 1],
        rem / 3600,
        rem % 3600 / 60
    )
}

/// Days since 1970-01-01 to a (year, month, day) date. Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A human name for what an entry is, from its extension.
pub fn kind(entry_name: &str, is_dir: bool) -> String {
    if is_dir {
        return "Folder".into();
    }
    let Some((_, ext)) = entry_name.rsplit_once('.') else {
        return "File".into();
    };
    let known = match ext.to_ascii_lowercase().as_str() {
        "md" => "Markdown",
        "rs" => "Rust source",
        "nix" => "Nix file",
        "toml" => "TOML file",
        "json" => "JSON file",
        "lock" => "Lock file",
        "txt" => "Text file",
        "png" => "PNG image",
        "jpg" | "jpeg" => "JPEG image",
        "svg" => "SVG image",
        "pdf" => "PDF document",
        "iso" => "Disk image",
        "zip" | "tar" | "gz" | "xz" | "zst" => "Archive",
        "mp3" | "flac" | "ogg" => "Audio",
        "mp4" | "mkv" | "webm" => "Video",
        _ => return format!("{} file", ext.to_uppercase()),
    };
    known.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_396), (2025, 11, 4));
        assert_eq!(
            format_time(UNIX_EPOCH + std::time::Duration::from_secs(90_061)),
            "2 Jan 1970, 01:01"
        );
    }

    #[test]
    fn sizes_and_decoding() {
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1_500_000), "1.5 MB");
        assert_eq!(percent_decode("/home/a%20b/%C3%A9.txt"), "/home/a b/é.txt");
        assert_eq!(percent_decode("/x%2"), "/x%2");
    }
}
