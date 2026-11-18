//! The sandboxed archive helper. It reads one archive for the core, then
//! exits.
//!
//! `nova-files-archiver list <archive>`
//!
//! It opens the archive, then locks itself in before it reads a byte (see
//! `sandbox`): no other file, no network, no signal outside itself. It finds the format from the file's first bytes, never its
//! name (see `format`), and writes each entry to its standard output (see
//! `wire`). How it ended is its exit code (see `Ending`), which the core
//! checks with the same numbers.

#![forbid(unsafe_code)]

mod decompress;
mod ending;
mod format;
mod list;
mod sandbox;
mod wire;

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write as _};
use std::path::Path;
use std::process::ExitCode;

use ending::Ending;

/// The argument that asks for a listing.
const LIST: &str = "list";
const USAGE: &str = "usage: nova-files-archiver list <archive>";

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let ending = match args.as_slice() {
        [mode, archive] if mode == LIST => run_list(Path::new(archive)),
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
