//! The sandboxed archive helper. It reads one archive for the core, then
//! exits.
//!
//! `nova-files-archiver list <archive>`
//! `nova-files-archiver extract <archive> <folder>`
//! `nova-files-archiver compress <packing> <level> <archive> <source>...`
//!
//! `extract` unpacks into `folder`, which it may write inside and nowhere
//! else. `compress` makes `archive`, a new file, of the sources, which it
//! may read and nothing else. A password, when one is needed, comes on
//! standard input.
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
mod unpack;
mod wire;

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::path::Path;
use std::process::ExitCode;

use ending::Ending;
use rustix::fs::{Mode, OFlags};
use unpack::Unpacking;

const LIST: &str = "list";
const EXTRACT: &str = "extract";
const COMPRESS: &str = "compress";
const USAGE: &str = "usage: nova-files-archiver list <archive>\n       nova-files-archiver extract <archive> <folder>\n       nova-files-archiver compress <packing> <level> <archive> <source>...";
/// A source to compress, held open only to name it to the sandbox.
const SOURCE: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
/// The longest password read.
const MAX_PASSWORD: u64 = 4_096;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let ending = match args.as_slice() {
        [mode, archive] if mode == LIST => run_list(Path::new(archive)),
        [mode, archive, folder] if mode == EXTRACT => {
            run_extract(Path::new(archive), Path::new(folder))
        }
        [mode, packing, level, archive, sources @ ..]
            if mode == COMPRESS && !sources.is_empty() =>
        {
            match (
                compress::Packing::of(packing),
                level.to_str().and_then(|level| level.parse().ok()),
            ) {
                (Some(packing), Some(level)) => {
                    run_compress(packing, level, Path::new(archive), sources)
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

fn run_list(archive: &Path) -> Ending {
    let archive = match File::open(archive) {
        Ok(archive) => archive,
        Err(error) => return Ending::of_open(&error),
    };
    if let Err(error) = sandbox::lock_in() {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return Ending::Usage;
    }
    let mut out = io::BufWriter::new(io::stdout().lock());
    let ending = list::list(archive, &mut out);
    if out.flush().is_err() {
        // Nobody is reading the answers any more.
        return Ending::Usage;
    }
    ending
}

fn run_extract(archive: &Path, folder: &Path) -> Ending {
    let archive = match File::open(archive) {
        Ok(archive) => archive,
        Err(error) => return Ending::of_open(&error),
    };
    let folder = match rustix::fs::open(folder, unpack::FOLDER, Mode::empty()) {
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
    let locked_in = folder
        .try_clone()
        .map_err(Into::into)
        .and_then(sandbox::lock_in_unpacking);
    if let Err(error) = locked_in {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return Ending::Usage;
    }
    let into = Unpacking::new(folder, umask.bits());
    let password = (!password.is_empty()).then_some(password.as_slice());
    let mut out = io::BufWriter::new(io::stdout().lock());
    let ending = extract::extract(archive, into, password, &mut out);
    if out.flush().is_err() {
        return Ending::Usage;
    }
    ending
}

fn run_compress(
    packing: compress::Packing,
    level: compress::Level,
    archive: &Path,
    sources: &[OsString],
) -> Ending {
    let sources: Vec<&Path> = sources.iter().map(Path::new).collect();
    let mut readable = Vec::with_capacity(sources.len());
    for source in &sources {
        let opened = rustix::fs::open(*source, SOURCE, Mode::empty());
        let metadata = source.symlink_metadata();
        match (opened, metadata) {
            (Ok(fd), Ok(metadata)) => readable.push((fd, metadata.is_dir())),
            (Err(error), _) => return Ending::of_open(&error.into()),
            (_, Err(error)) => return Ending::of_open(&error),
        }
    }
    let output = match File::options().write(true).create_new(true).open(archive) {
        Ok(output) => output,
        Err(error) => return Ending::of_write(&error),
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
    if let Err(error) = sandbox::lock_in_reading(&readable) {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return Ending::Usage;
    }
    let mut out = io::BufWriter::new(io::stdout().lock());
    let recipe = compress::Recipe {
        packing,
        level,
        password: password.as_deref(),
    };
    compress::compress(output, &sources, &recipe, &mut out)
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
