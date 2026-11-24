use std::env;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub const GIT: &str = "git";
const GIT_ENV: &[u8] = b"GIT_";
const READ_WAIT: Duration = Duration::from_secs(10);
const READER_THREAD: &str = "nova-files-git-read";

pub fn git() -> Command {
    let mut command = Command::new(GIT);
    for (name, _) in env::vars_os() {
        if name.as_bytes().starts_with(GIT_ENV) {
            command.env_remove(name);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

pub fn read<T: Send + 'static>(
    mut command: Command,
    parse: impl FnOnce(ChildStdout) -> T + Send + 'static,
) -> io::Result<T> {
    let mut child = command.spawn()?;
    let answer = read_output(&mut child, parse);
    child.kill()?;
    child.wait()?;
    answer
}

fn read_output<T: Send + 'static>(
    child: &mut Child,
    parse: impl FnOnce(ChildStdout) -> T + Send + 'static,
) -> io::Result<T> {
    let output = child.stdout.take().ok_or(io::ErrorKind::BrokenPipe)?;
    let (done, read) = mpsc::channel();
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            done.send(parse(output)).ok();
        })?;
    read.recv_timeout(READ_WAIT)
        .map_err(|_| io::ErrorKind::TimedOut.into())
}
