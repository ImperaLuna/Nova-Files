//! The sandboxed archive helper. It reads one archive for the core, then
//! exits.
//!
//! `nova-files-archiver [--seven <7-zip>] list <archive>`
//! `nova-files-archiver [--seven <7-zip>] extract <archive> <folder> [<entry>...]`
//! `nova-files-archiver [--seven <7-zip>] compress <packing> <level> <archive> <source>...`
//!
//! `extract` unpacks into `folder`, which it may write inside and nowhere
//! else: the entries named, with all inside them, or every one. `compress` makes `archive`, a new file, of the sources, which it
//! may read and nothing else. A password, when one is needed, comes on
//! standard input. Given the installed 7-Zip, it reads 7z and rar through
//! it (see `seven`), and makes 7z with it (packing `7z`).
//!
//! It opens the archive, then locks itself in before it reads a byte (see
//! `sandbox`): no other file, no network, no signal outside itself. It finds the format from the file's first bytes, never its
//! name (see `format`), and writes each entry to its standard output (see
//! `wire`). How it ended is its exit code (see `Ending`), which the core
//! checks with the same numbers.

#![forbid(unsafe_code)]

mod compress;
mod decompress;
mod ending;
mod extract;
mod format;
mod list;
mod progress;
mod sandbox;
mod seven;
mod unpack;
mod wire;

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::process::ExitCode;

use ending::Ending;
use rustix::fs::{Mode, OFlags};
use sandbox::Grant;
use seven::Seven;
use unpack::Unpacking;

const LIST: &str = "list";
const EXTRACT: &str = "extract";
const COMPRESS: &str = "compress";
const SEVEN: &str = "--seven";
const SEVEN_ZIP: &str = "7z";
const USAGE: &str = "usage: nova-files-archiver [--seven <7-zip>] list <archive>\n       nova-files-archiver [--seven <7-zip>] extract <archive> <folder> [<entry>...]\n       nova-files-archiver [--seven <7-zip>] compress <packing> <level> <archive> <source>...";
/// A source to compress, held open only to name it to the sandbox.
const SOURCE: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
/// The folder a 7z is made in, held open only to name it to the sandbox.
const MADE_IN: OFlags = OFlags::PATH.union(OFlags::DIRECTORY).union(OFlags::CLOEXEC);
/// The longest password read.
const MAX_PASSWORD: u64 = 4_096;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (seven, args) = match args.as_slice() {
        [flag, program, rest @ ..] if flag == SEVEN => (Some(Path::new(program)), rest),
        rest => (None, rest),
    };
    let ending = match args {
        [mode, archive] if mode == LIST => run_list(Path::new(archive), seven),
        [mode, archive, folder, picked @ ..] if mode == EXTRACT => {
            run_extract(Path::new(archive), Path::new(folder), picked, seven)
        }
        [mode, packing, level, archive, sources @ ..]
            if mode == COMPRESS && !sources.is_empty() =>
        {
            let packing = match packing.to_str() {
                Some(SEVEN_ZIP) => seven.map(Packing::Seven),
                _ => compress::Packing::of(packing).map(Packing::Own),
            };
            match (packing, level.to_str().and_then(|level| level.parse().ok())) {
                (Some(packing), Some(level)) => {
                    run_compress(packing, level, Path::new(archive), sources, seven)
                }
                _ => Ending::Usage,
            }
        }
        _ => {
            writeln!(io::stderr(), "{USAGE}").ok();
            Ending::Usage
        }
    };
    ending.code()
}

/// How a compress packs: the helper's own way, or 7-Zip's 7z.
#[derive(Clone, Copy)]
enum Packing<'a> {
    Own(compress::Packing),
    Seven(&'a Path),
}

/// A compress under way: the helper's own into the file it made, or 7-Zip
/// making its own.
enum Making<'a> {
    Own(compress::Packing, File),
    Seven(&'a Path),
}

fn run_list(path: &Path, seven: Option<&Path>) -> Ending {
    let archive = match File::open(path) {
        Ok(archive) => archive,
        Err(error) => return Ending::of_open(&error),
    };
    let Some(password) = read_password() else {
        return Ending::Usage;
    };
    let Ok(grants) = seven_grants(seven, &archive) else {
        return Ending::Usage;
    };
    if let Err(error) = sandbox::lock_in(grants) {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return Ending::Usage;
    }
    let mut out = io::BufWriter::new(io::stdout().lock());
    let seven = Seven {
        program: seven,
        archive: path,
        password: (!password.is_empty()).then_some(password.as_slice()),
    };
    let ending = list::list(archive, seven, &mut out);
    if out.flush().is_err() {
        // Nobody is reading the answers any more.
        return Ending::Usage;
    }
    ending
}

fn run_extract(
    path: &Path,
    folder_path: &Path,
    picked: &[OsString],
    seven: Option<&Path>,
) -> Ending {
    let archive = match File::open(path) {
        Ok(archive) => archive,
        Err(error) => return Ending::of_open(&error),
    };
    let folder = match rustix::fs::open(folder_path, unpack::FOLDER, Mode::empty()) {
        Ok(folder) => folder,
        Err(error) => return Ending::of_write(&error.into()),
    };
    let Some(password) = read_password() else {
        return Ending::Usage;
    };
    let umask = rustix::process::umask(Mode::empty());
    rustix::process::umask(umask);
    // Reads the user's time zone while files can still be read.
    chrono::Local::now();
    let locked_in = folder.try_clone().map_err(Into::into).and_then(|unpack| {
        let mut grants = seven_grants(seven, &archive)?;
        grants.push(sandbox::unpacking(unpack));
        sandbox::lock_in(grants)
    });
    if let Err(error) = locked_in {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return Ending::Usage;
    }
    let into = Unpacking::new(folder, umask.bits());
    let seven = Seven {
        program: seven,
        archive: path,
        password: (!password.is_empty()).then_some(password.as_slice()),
    };
    let mut out = io::BufWriter::new(io::stdout().lock());
    let target = extract::Target {
        into,
        folder: folder_path,
        picked,
    };
    let ending = extract::extract(archive, target, seven, &mut out);
    if out.flush().is_err() {
        return Ending::Usage;
    }
    ending
}

/// What the sandbox leaves open for 7-Zip, when there is one, to read
/// `archive`.
fn seven_grants(seven: Option<&Path>, archive: &File) -> io::Result<Vec<Grant>> {
    let Some(program) = seven else {
        return Ok(Vec::new());
    };
    let mut grants = sandbox::running(program);
    grants.push(sandbox::reading(OwnedFd::from(archive.try_clone()?), false));
    Ok(grants)
}

fn run_compress(
    packing: Packing<'_>,
    level: compress::Level,
    archive: &Path,
    sources: &[OsString],
    seven: Option<&Path>,
) -> Ending {
    let sources: Vec<&Path> = sources.iter().map(Path::new).collect();
    let mut grants = Vec::with_capacity(sources.len());
    for source in &sources {
        let opened = rustix::fs::open(*source, SOURCE, Mode::empty());
        let metadata = source.symlink_metadata();
        match (opened, metadata) {
            (Ok(fd), Ok(metadata)) => grants.push(sandbox::reading(fd, metadata.is_dir())),
            (Err(error), _) => return Ending::of_open(&error.into()),
            (_, Err(error)) => return Ending::of_open(&error),
        }
    }
    grants.extend(seven.map(sandbox::running).unwrap_or_default());
    let making = match packing {
        Packing::Own(packing) => match File::options().write(true).create_new(true).open(archive) {
            Ok(output) => Making::Own(packing, output),
            Err(error) => return Ending::of_write(&error),
        },
        Packing::Seven(program) => {
            let made_in = archive.parent().unwrap_or_else(|| Path::new("/"));
            match rustix::fs::open(made_in, MADE_IN, Mode::empty()) {
                Ok(folder) => grants.push(sandbox::making_in(folder)),
                Err(error) => return Ending::of_write(&error.into()),
            }
            Making::Seven(program)
        }
    };
    let Some(password) = read_password() else {
        return Ending::Usage;
    };
    let Ok(password) = String::from_utf8(password) else {
        return Ending::Usage;
    };
    let password = (!password.is_empty()).then_some(password);
    // Reads the user's time zone while files can still be read.
    chrono::Local::now();
    if let Err(error) = sandbox::lock_in(grants) {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return Ending::Usage;
    }
    let mut out = io::BufWriter::new(io::stdout().lock());
    match making {
        Making::Own(packing, output) => {
            let recipe = compress::Recipe {
                packing,
                level,
                password: password.as_deref(),
            };
            compress::compress(output, &sources, &recipe, &mut out)
        }
        Making::Seven(program) => seven::compress(
            program,
            archive,
            password.as_deref().map(str::as_bytes),
            (&sources, level),
            &mut out,
        ),
    }
}

/// What comes on standard input, up to [`MAX_PASSWORD`] bytes.
fn read_password() -> Option<Vec<u8>> {
    let mut password = Vec::new();
    io::stdin()
        .take(MAX_PASSWORD)
        .read_to_end(&mut password)
        .ok()?;
    Some(password)
}
