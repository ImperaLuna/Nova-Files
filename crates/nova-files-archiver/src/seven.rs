//! Reads and makes archives through the installed 7-Zip: 7z, and rar when
//! that 7-Zip reads it. 7-Zip runs as the helper's child, locked in with
//! it, with its errors on its output, which the helper turns into its own
//! answers. A password goes on 7-Zip's standard input.
//!
//! 7-Zip's listing is one `key = value` line per field, an entry starting
//! at its `Path` line; a name with a line break in it reads wrong.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead as _, BufReader, Write};
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::ending::Ending;
use crate::list::{FILE_MODE, FOLDER_MODE};
use crate::progress::{Told, Unheard};
use crate::wire::{self, Clock, Entry, Kind};

const LOCALE: &str = "LC_ALL";
const UTF8: &str = "C.UTF-8";
/// Links stay links, each entry on its own line, errors with the rest,
/// no percentages, yes to every question.
const SWITCHES: [&str; 6] = ["-snl", "-bb1", "-bso1", "-bse1", "-bsp0", "-y"];
const LISTING: [&str; 6] = ["l", "-slt", "-ba", "-bso1", "-bse1", "-bsp0"];
const PATH: &[u8] = b"Path = ";
const SIZE: &[u8] = b"Size = ";
const MODIFIED: &[u8] = b"Modified = ";
const ATTRIBUTES: &[u8] = b"Attributes = ";
const ENCRYPTED: &[u8] = b"Encrypted = +";
const UNPACKED: &[u8] = b"- ";
const PACKED: &[u8] = b"+ ";
const PART: u8 = b'/';
const TIME: &str = "%Y-%m-%d %H:%M:%S";
const TIME_LEN: usize = 19;
/// `ls -l` permission letters, three groups of read, write and run: each
/// letter's bit, and the bit an `s` or `t` in its place adds.
const PERMISSIONS: [(u32, u32); 9] = [
    (0o400, 0),
    (0o200, 0),
    (0o100, 0o4000),
    (0o40, 0),
    (0o20, 0),
    (0o10, 0o2000),
    (0o4, 0),
    (0o2, 0),
    (0o1, 0o1000),
];
const PERMISSION_LETTERS: usize = 10;
/// 7-Zip's exit code for warnings: something was left out.
const WARNING: i32 = 1;

/// How hard 7z compresses, 1 to 9, as 7-Zip's own `-mx` levels.
pub type Level = u32;

/// The installed 7-Zip, when there is one, and the archive it reads.
#[derive(Debug, Clone, Copy)]
pub struct Seven<'a> {
    pub program: Option<&'a Path>,
    pub archive: &'a Path,
    pub password: Option<&'a [u8]>,
}

impl Seven<'_> {
    /// Writes every entry of the archive to `out`; unsupported without
    /// 7-Zip.
    pub fn list(self, out: &mut impl Write) -> Ending {
        self.program.map_or(Ending::Unsupported, |program| {
            list(program, self.archive, self.password, out)
        })
    }

    /// Unpacks the `picked` entries of the archive, with all inside them,
    /// or every entry when none are, into `folder`; unsupported without
    /// 7-Zip.
    pub fn extract(self, folder: &Path, picked: &[OsString], out: &mut impl Write) -> Ending {
        self.program.map_or(Ending::Unsupported, |program| {
            extract(program, self, folder, picked, out)
        })
    }
}

/// What 7-Zip said besides its entries, most telling first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Said {
    WrongPassword,
    NoSpace,
    ReadOnly,
    Denied,
    NotArchive,
    AskedPassword,
}

const SAYINGS: [(&[u8], Said); 7] = [
    (b"Wrong password", Said::WrongPassword),
    (b"No space left", Said::NoSpace),
    (b"not enough space", Said::NoSpace),
    (b"Read-only file system", Said::ReadOnly),
    (b"Permission denied", Said::Denied),
    (b"Cannot open the file as archive", Said::NotArchive),
    (b"Enter password", Said::AskedPassword),
];

#[derive(Debug, Default)]
struct Heard(BTreeSet<Said>);

impl Heard {
    fn hear(&mut self, line: &[u8]) {
        for (text, said) in SAYINGS {
            if line.windows(text.len()).any(|window| window == text) {
                self.0.insert(said);
            }
        }
    }

    /// How a run that exited with `code` ended, given a password or not.
    /// A warning ends a run that only reads well.
    fn ending(&self, code: Option<i32>, password: bool, writing: bool) -> Ending {
        match (code, self.0.first()) {
            (Some(0), _) => Ending::Done,
            (_, Some(Said::WrongPassword)) if password => Ending::WrongPassword,
            (_, Some(Said::WrongPassword)) => Ending::NeedsPassword,
            (_, Some(Said::NoSpace)) => Ending::NoSpace,
            (_, Some(Said::ReadOnly)) => Ending::ReadOnly,
            (_, Some(Said::Denied)) => Ending::Denied,
            (_, Some(Said::NotArchive)) => Ending::Unsupported,
            (_, Some(Said::AskedPassword)) if !password => Ending::NeedsPassword,
            (Some(WARNING), _) if !writing => Ending::Done,
            _ if writing => Ending::NotWritten,
            _ => Ending::Broken,
        }
    }
}

/// Writes every entry of `archive` to `out`.
fn list(program: &Path, archive: &Path, password: Option<&[u8]>, out: &mut impl Write) -> Ending {
    let mut unheard = false;
    let ending = listing(program, archive, password, |entry| {
        unheard |= wire::write(out, &entry.as_entry()).is_err();
    });
    if unheard {
        return Ending::Usage;
    }
    ending
}

/// Unpacks the `picked` entries of `seven`'s archive, or all, into
/// `folder`.
fn extract(
    program: &Path,
    seven: Seven<'_>,
    folder: &Path,
    picked: &[OsString],
    out: &mut impl Write,
) -> Ending {
    let (archive, password) = (seven.archive, seven.password);
    let mut sizes = HashMap::new();
    let listed = listing(program, archive, password, |entry| {
        sizes.insert(entry.path.clone(), entry.size);
    });
    if listed != Ending::Done {
        return listed;
    }
    let into = [b"-o".as_slice(), folder.as_os_str().as_bytes()].concat();
    let mut args: Vec<&OsStr> = vec!["x".as_ref(), archive.as_os_str(), OsStr::from_bytes(&into)];
    args.extend(SWITCHES.iter().map(OsStr::new));
    args.push("--".as_ref());
    args.extend(picked.iter().map(OsString::as_os_str));
    let mut told = Told::new(out);
    follow(program, &args, password, false, |path| {
        let path = path.strip_suffix(b"/").unwrap_or(path);
        told.done(sizes.get(path).copied().unwrap_or_default())
    })
}

/// Makes `archive`, a new 7z, of `sources`, each under its own name;
/// with a password, its names are hidden too.
pub fn compress(
    program: &Path,
    archive: &Path,
    password: Option<&[u8]>,
    (sources, level): (&[&Path], Level),
    out: &mut impl Write,
) -> Ending {
    let level = format!("-mx={level}");
    let mut args: Vec<&OsStr> = vec!["a".as_ref(), "-t7z".as_ref(), level.as_ref()];
    if password.is_some() {
        args.extend(["-mhe=on", "-p"].map(OsStr::new));
    }
    args.extend(SWITCHES.iter().map(OsStr::new));
    args.extend([archive.as_os_str(), "--".as_ref()]);
    args.extend(sources.iter().map(|source| source.as_os_str()));
    let mut told = Told::new(out);
    let mut folders = HashSet::new();
    follow(program, &args, password, true, |name| {
        for (at, _) in name.iter().enumerate().filter(|&(_, &byte)| byte == PART) {
            if folders.insert(name[..at].to_vec()) {
                told.done(0)?;
            }
        }
        told.done(packed_size(sources, name))
    })
}

/// The bytes the file `name` in the archive held, found among `sources`
/// by its first part; 0 for anything else.
fn packed_size(sources: &[&Path], name: &[u8]) -> u64 {
    let (top, rest) = name
        .iter()
        .position(|&byte| byte == PART)
        .map_or((name, None), |at| {
            (&name[..at], name.get(at + 1..).map(OsStr::from_bytes))
        });
    sources
        .iter()
        .find(|source| source.file_name().map(OsStr::as_bytes) == Some(top))
        .map(|source| rest.map_or_else(|| source.to_path_buf(), |rest| source.join(rest)))
        .and_then(|path| path.symlink_metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .map_or(0, |metadata| metadata.len())
}

/// One entry of 7-Zip's listing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Found {
    path: Vec<u8>,
    size: u64,
    modified: i64,
    attributes: Vec<u8>,
    locked: bool,
}

impl Found {
    fn take(&mut self, line: &[u8]) {
        if let Some(size) = line.strip_prefix(SIZE) {
            self.size = decimal(size).unwrap_or_default();
        } else if let Some(time) = line.strip_prefix(MODIFIED) {
            self.modified = wall_time(time).unwrap_or_default();
        } else if let Some(attributes) = line.strip_prefix(ATTRIBUTES) {
            self.attributes = attributes.to_vec();
        } else {
            self.locked |= line == ENCRYPTED;
        }
    }

    /// What it is, and its permission bits.
    fn kind(&self) -> (Kind, u32) {
        let mut words = self.attributes.split(|&byte| byte == b' ');
        let flags = words.next().unwrap_or_default();
        let unix = words.find(|word| word.len() == PERMISSION_LETTERS);
        match unix.and_then(<[u8]>::split_first) {
            Some((b'd', letters)) => (Kind::Folder, mode(letters)),
            Some((b'l', letters)) => (Kind::Link, mode(letters)),
            Some((_, letters)) => (Kind::File, mode(letters)),
            None if flags.contains(&b'D') => (Kind::Folder, FOLDER_MODE),
            None => (Kind::File, FILE_MODE),
        }
    }

    fn as_entry(&self) -> Entry<'_> {
        let (kind, mode) = self.kind();
        Entry {
            kind,
            locked: self.locked,
            size: self.size,
            modified: self.modified,
            clock: Clock::Local,
            mode,
            path: &self.path,
            link: b"",
        }
    }
}

/// Lists `archive`, calling `each` with every entry; says how it ended.
fn listing(
    program: &Path,
    archive: &Path,
    password: Option<&[u8]>,
    mut each: impl FnMut(&Found),
) -> Ending {
    let mut args: Vec<&OsStr> = LISTING.iter().map(OsStr::new).collect();
    args.push(archive.as_os_str());
    let Ok((mut child, output)) = start(program, &args, password) else {
        return Ending::Usage;
    };
    let mut heard = Heard::default();
    let mut found: Option<Found> = None;
    for line in BufReader::new(output).split(b'\n') {
        let Ok(line) = line else {
            break;
        };
        if let Some(path) = line.strip_prefix(PATH) {
            if let Some(done) = found.replace(Found {
                path: path.to_vec(),
                ..Found::default()
            }) {
                each(&done);
            }
        } else if let Some(entry) = found.as_mut() {
            entry.take(&line);
        }
        heard.hear(&line);
    }
    let code = child.wait().ok().and_then(|status| status.code());
    let ending = heard.ending(code, password.is_some(), false);
    if let (Ending::Done, Some(last)) = (ending, &found) {
        each(last);
    }
    ending
}

/// Runs 7-Zip with `args` and calls `each` with the name on every line
/// that says an entry is done; says how it ended.
fn follow(
    program: &Path,
    args: &[&OsStr],
    password: Option<&[u8]>,
    writing: bool,
    mut each: impl FnMut(&[u8]) -> Result<(), Unheard>,
) -> Ending {
    let done = if writing { PACKED } else { UNPACKED };
    let Ok((mut child, output)) = start(program, args, password) else {
        return Ending::Usage;
    };
    let mut heard = Heard::default();
    for line in BufReader::new(output).split(b'\n') {
        let Ok(line) = line else {
            break;
        };
        if let Some(name) = line.strip_prefix(done)
            && each(name).is_err()
        {
            child.kill().ok();
            child.wait().ok();
            return Ending::Usage;
        }
        heard.hear(&line);
    }
    let code = child.wait().ok().and_then(|status| status.code());
    heard.ending(code, password.is_some(), writing)
}

/// Starts 7-Zip with `args`, `password` and a line break on its standard
/// input, then nothing more.
fn start(
    program: &Path,
    args: &[&OsStr],
    password: Option<&[u8]>,
) -> io::Result<(Child, ChildStdout)> {
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env(LOCALE, UTF8)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    if let (Some(mut input), Some(password)) = (child.stdin.take(), password) {
        // A 7-Zip that doesn't read it says why on its output.
        input.write_all(&[password, b"\n"].concat()).ok();
    }
    let output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("no output"))?;
    Ok((child, output))
}

fn decimal(text: &[u8]) -> Option<u64> {
    std::str::from_utf8(text).ok()?.parse().ok()
}

/// A wall time 7-Zip prints, as seconds since 1970 read as UTC.
fn wall_time(text: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(text.get(..TIME_LEN)?).ok()?;
    let time = chrono::NaiveDateTime::parse_from_str(text, TIME).ok()?;
    Some(time.and_utc().timestamp())
}

/// The permission bits `ls -l` letters name, without the type letter.
fn mode(letters: &[u8]) -> u32 {
    letters
        .iter()
        .zip(PERMISSIONS)
        .map(|(&letter, (bit, special))| match letter {
            b'-' => 0,
            b's' | b't' => bit + special,
            b'S' | b'T' => special,
            _ => bit,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn found(attributes: &[u8]) -> Found {
        Found {
            attributes: attributes.to_vec(),
            ..Found::default()
        }
    }

    #[test]
    fn reads_permission_letters() {
        assert_eq!(mode(b"rwxr-xr-x"), 0o755);
        assert_eq!(mode(b"rw-r-----"), 0o640);
        assert_eq!(mode(b"rwsr-Sr-T"), 0o6744 + 0o1000);
        assert_eq!(mode(b"--x--x--t"), 0o1111);
        assert_eq!(mode(b"---------"), 0);
    }

    #[test]
    fn tells_kinds_by_attributes() {
        assert_eq!(found(b"D drwxr-x---").kind(), (Kind::Folder, 0o750));
        assert_eq!(found(b"A lrwxrwxrwx").kind(), (Kind::Link, 0o777));
        assert_eq!(found(b"A -rw-------").kind(), (Kind::File, 0o600));
        assert_eq!(found(b"D").kind(), (Kind::Folder, FOLDER_MODE));
        assert_eq!(found(b"RA").kind(), (Kind::File, FILE_MODE));
        assert_eq!(found(b"").kind(), (Kind::File, FILE_MODE));
    }

    #[test]
    fn takes_fields_of_an_entry() {
        let mut entry = Found::default();
        for line in [
            &b"Size = 12"[..],
            b"Packed Size = 3",
            b"Modified = 2026-11-18 08:56:32.2102780",
            b"Attributes = A -rw-r--r--",
            b"Encrypted = +",
        ] {
            entry.take(line);
        }
        assert_eq!(entry.size, 12);
        assert_eq!(entry.modified, 1_794_992_192);
        assert_eq!(entry.attributes, b"A -rw-r--r--");
        assert!(entry.locked);
        entry.take(b"Encrypted = -");
        assert!(entry.locked);
        let mut plain = Found::default();
        plain.take(b"Encrypted = -");
        plain.take(b"Modified = ");
        plain.take(b"Size = ");
        assert_eq!(plain, Found::default());
    }

    #[test]
    fn what_7zip_said_gives_the_ending() {
        let heard = |line: &[u8]| {
            let mut heard = Heard::default();
            heard.hear(line);
            heard
        };
        let wrong = heard(b"ERROR: a.7z : Cannot open encrypted archive. Wrong password?");
        assert_eq!(wrong.ending(Some(2), true, false), Ending::WrongPassword);
        assert_eq!(wrong.ending(Some(2), false, false), Ending::NeedsPassword);
        assert_eq!(wrong.ending(Some(0), true, false), Ending::Done);
        let asked = heard(b"Enter password:");
        assert_eq!(asked.ending(Some(255), false, false), Ending::NeedsPassword);
        assert_eq!(asked.ending(Some(2), true, false), Ending::Broken);
        assert_eq!(
            heard(b"ERROR: No space left on device").ending(Some(2), false, true),
            Ending::NoSpace
        );
        assert_eq!(
            heard(b"There is not enough space on the disk").ending(Some(2), false, true),
            Ending::NoSpace
        );
        assert_eq!(
            heard(b"ERROR: Read-only file system").ending(Some(2), false, true),
            Ending::ReadOnly
        );
        assert_eq!(
            heard(b"WARNING: Permission denied : a").ending(Some(1), false, true),
            Ending::Denied
        );
        assert_eq!(
            heard(b"ERROR: x : Cannot open the file as archive").ending(Some(2), false, false),
            Ending::Unsupported
        );
        let quiet = Heard::default();
        assert_eq!(quiet.ending(Some(WARNING), false, false), Ending::Done);
        assert_eq!(quiet.ending(Some(WARNING), false, true), Ending::NotWritten);
        assert_eq!(quiet.ending(Some(2), false, false), Ending::Broken);
        assert_eq!(quiet.ending(None, false, true), Ending::NotWritten);
    }

    #[test]
    fn finds_a_packed_files_size_among_the_sources() {
        let root = std::env::temp_dir().join(format!("nova-seven-size-{}", std::process::id()));
        std::fs::create_dir_all(root.join("docs/in")).unwrap();
        std::fs::write(root.join("docs/in/a.txt"), b"abc").unwrap();
        std::fs::write(root.join("one.txt"), b"12345").unwrap();
        let sources = [root.join("docs"), root.join("one.txt")];
        let sources: Vec<&Path> = sources.iter().map(PathBuf::as_path).collect();
        assert_eq!(packed_size(&sources, b"docs/in/a.txt"), 3);
        assert_eq!(packed_size(&sources, b"one.txt"), 5);
        assert_eq!(packed_size(&sources, b"docs/in"), 0);
        assert_eq!(packed_size(&sources, b"other/a.txt"), 0);
        std::fs::remove_dir_all(&root).ok();
    }
}
