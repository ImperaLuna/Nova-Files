//! The sandboxed thumbnail helper. It renders one batch of thumbnails, then
//! exits.
//!
//! `nova-files-thumbnailer <thumbnails folder> (<file> <uri> <mtime> <name>)...`
//!
//! The core works out each thumbnail's name and the file's URI and mtime
//! (the freedesktop thumbnail spec); the helper only decodes, scales and
//! writes. Before it reads any file it locks itself in (see `sandbox`):
//! read anywhere, write only inside the thumbnails folder. For each file it
//! prints one line, in order: `ok`, `failed` or `unsupported`. Formats it
//! can't decode go to the system's thumbnailers, inside the same sandbox.

#![forbid(unsafe_code)]

mod batch;
mod data_dirs;
mod mime;
mod png_text;
mod render;
mod sandbox;
mod system;

use std::io::{self, Write as _};
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(batch) = batch::Batch::from_args(std::env::args_os().skip(1)) else {
        writeln!(io::stderr(), "{}", batch::USAGE).ok();
        return ExitCode::FAILURE;
    };
    if let Err(error) = sandbox::lock_in(&batch.folder) {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return ExitCode::FAILURE;
    }
    let dirs = data_dirs::data_dirs();
    let system = render::System {
        mime: mime::MimeTypes::load(&dirs),
        thumbnailers: system::Thumbnailers::load(&dirs),
    };
    let mut out = io::stdout().lock();
    for job in &batch.jobs {
        let outcome = render::render(&batch.folder, job, &system);
        if writeln!(out, "{}", outcome.word()).is_err() {
            // Nobody is reading the answers any more.
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
