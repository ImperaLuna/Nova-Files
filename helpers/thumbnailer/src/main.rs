#![forbid(unsafe_code)]

mod data_dirs;
mod jobs;
mod mime;
mod png_text;
mod preview;
mod render;
mod sandbox;
mod system;

use std::io::{self, BufRead, Write};
use std::path::Path;
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
    let Some(folder) = jobs::folder_from_args(args) else {
        writeln!(io::stderr(), "{}", jobs::USAGE).ok();
        return ExitCode::FAILURE;
    };
    if let Err(error) = sandbox::lock_in(&folder) {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return ExitCode::FAILURE;
    }
    let dirs = data_dirs::data_dirs();
    let system = render::System {
        mime: mime::MimeTypes::load(&dirs),
        thumbnailers: system::Thumbnailers::load(&dirs),
    };
    serve(
        &folder,
        &system,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
    )
}

fn serve(
    folder: &Path,
    system: &render::System,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> ExitCode {
    loop {
        let job = match jobs::next_job(input) {
            Ok(Some(job)) => job,
            Ok(None) => return ExitCode::SUCCESS,
            Err(error) => {
                writeln!(io::stderr(), "{error}\n{}", jobs::USAGE).ok();
                return ExitCode::FAILURE;
            }
        };
        let outcome = render::render(folder, &job, system);
        if writeln!(out, "{}", outcome.word())
            .and_then(|()| out.flush())
            .is_err()
        {
            return ExitCode::FAILURE;
        }
    }
}
