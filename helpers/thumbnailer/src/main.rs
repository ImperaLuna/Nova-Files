#![forbid(unsafe_code)]

mod batch;
mod data_dirs;
mod document;
mod figure;
mod mime;
mod moving;
mod png_text;
mod preview;
mod render;
mod sandbox;
mod system;
mod vector;

use std::io::{self, Write as _};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1).peekable();
    if args.next_if(|arg| arg == preview::FLAG).is_some() {
        let Some(request) = preview::Request::from_args(args) else {
            writeln!(io::stderr(), "{}", preview::USAGE).ok();
            return ExitCode::FAILURE;
        };
        return preview::run(&request);
    }
    if args.next_if(|arg| arg == figure::FLAG).is_some() {
        let Some(request) = figure::Request::from_args(args) else {
            writeln!(io::stderr(), "{}", figure::USAGE).ok();
            return ExitCode::FAILURE;
        };
        return figure::run(&request);
    }
    if args.next_if(|arg| arg == document::FLAG).is_some() {
        let Some(file) = document::file_from_args(args) else {
            writeln!(io::stderr(), "{}", document::USAGE).ok();
            return ExitCode::FAILURE;
        };
        return document::run(&file);
    }
    let Some(batch) = batch::Batch::from_args(args) else {
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
        let outcome = render::render(&batch.folder, batch.size, job, &system);
        if writeln!(out, "{}", outcome.word()).is_err() {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
