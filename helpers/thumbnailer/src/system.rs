use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use rustix::process::{Pid, PidfdFlags, pidfd_open};

const THUMBNAILERS_FOLDER: &str = "thumbnailers";
const THUMBNAILER_EXTENSION: &str = "thumbnailer";
const GROUP: &str = "[Thumbnailer Entry]";
const EXEC_KEY: &str = "Exec";
const TRY_EXEC_KEY: &str = "TryExec";
const MIME_KEY: &str = "MimeType";
const LIST_SEPARATOR: char = ';';
const MAX_FILE_BYTES: u64 = 64 * 1024;
const RUN_TIMEOUT: Duration = Duration::from_secs(10);
const READER_THREAD: &str = "thumbnailer-output";
const PATH_VAR: &str = "PATH";
const QUOTE: char = '"';
const ESCAPE: char = '\\';
const FIELD: char = '%';

pub struct Request<'a> {
    pub input: &'a Path,
    pub uri: &'a str,
    pub output: &'a Path,
    pub size: u32,
}

pub struct Thumbnailers(HashMap<String, Vec<String>>);

impl Thumbnailers {
    pub fn load(data_dirs: &[PathBuf]) -> Self {
        let mut by_mime = HashMap::new();
        for dir in data_dirs {
            let Ok(files) = fs::read_dir(dir.join(THUMBNAILERS_FOLDER)) else {
                continue;
            };
            let mut files: Vec<PathBuf> = files
                .filter_map(|file| file.ok().map(|file| file.path()))
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == THUMBNAILER_EXTENSION)
                })
                .collect();
            files.sort();
            for file in files {
                let Some((exec, mimes)) = read_entry(&file) else {
                    continue;
                };
                for mime in mimes {
                    by_mime.entry(mime).or_insert_with(|| exec.clone());
                }
            }
        }
        Self(by_mime)
    }

    pub fn run(&self, mime: &str, request: &Request) -> Option<io::Result<()>> {
        let exec = self.0.get(mime)?;
        Some(run(exec, request))
    }
}

fn read_entry(file: &Path) -> Option<(Vec<String>, Vec<String>)> {
    let text = read_capped(file)?;
    let mut in_group = false;
    let mut values: HashMap<&str, &str> = HashMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_group = line == GROUP;
        } else if in_group && let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim(), value.trim());
        }
    }
    let exec = split_exec(values.get(EXEC_KEY)?)?;
    let program = values
        .get(TRY_EXEC_KEY)
        .copied()
        .or_else(|| exec.first().map(String::as_str))?;
    if !installed(program) {
        return None;
    }
    let mimes = values
        .get(MIME_KEY)?
        .split(LIST_SEPARATOR)
        .filter(|mime| !mime.is_empty())
        .map(str::to_owned)
        .collect();
    Some((exec, mimes))
}

fn read_capped(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

fn installed(program: &str) -> bool {
    let program = Path::new(program);
    if program.is_absolute() {
        return program.is_file();
    }
    env::var_os(PATH_VAR)
        .is_some_and(|path| env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

fn split_exec(line: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            QUOTE => {
                in_quotes = !in_quotes;
                started = true;
            }
            ESCAPE if in_quotes => current.push(chars.next()?),
            ' ' if !in_quotes => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                current.push(c);
                started = true;
            }
        }
    }
    if in_quotes {
        return None;
    }
    if started {
        args.push(current);
    }
    (!args.is_empty()).then_some(args)
}

fn fill(arg: &str, request: &Request) -> OsString {
    let mut out = OsString::new();
    let mut chars = arg.chars();
    while let Some(c) = chars.next() {
        if c != FIELD {
            out.push(c.to_string());
            continue;
        }
        match chars.next() {
            Some('i') => out.push(request.input),
            Some('u') => out.push(request.uri),
            Some('o') => out.push(request.output),
            Some('s') => out.push(request.size.to_string()),
            Some(FIELD) => out.push(FIELD.to_string()),
            _ => {}
        }
    }
    out
}

fn run(exec: &[String], request: &Request) -> io::Result<()> {
    run_within(exec, request, RUN_TIMEOUT)
}

fn run_within(exec: &[String], request: &Request, timeout: Duration) -> io::Result<()> {
    let (program, args) = exec
        .split_first()
        .ok_or_else(|| io::Error::other("empty Exec"))?;
    let mut child = Command::new(program)
        .args(args.iter().map(|arg| fill(arg, request)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut output = child.stdout.take();
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            if let Some(output) = &mut output {
                io::copy(output, &mut io::sink()).ok();
            }
        })
        .ok();
    if !exits_within(&child, timeout)? {
        child.kill().ok();
        child.wait()?;
        return Err(io::ErrorKind::TimedOut.into());
    }
    let status = child.wait()?;
    let kind = match status.code() {
        Some(0) => return Ok(()),
        Some(_) => io::ErrorKind::InvalidData,
        None => io::ErrorKind::Interrupted,
    };
    Err(io::Error::new(
        kind,
        format!("thumbnailer exited with {status}"),
    ))
}

fn exits_within(child: &Child, timeout: Duration) -> io::Result<bool> {
    let exited = pidfd_open(Pid::from_child(child), PidfdFlags::empty())?;
    let mut watched = [PollFd::new(&exited, PollFlags::IN)];
    let timeout = Timespec::try_from(timeout).map_err(|_| io::ErrorKind::InvalidInput)?;
    loop {
        match poll(&mut watched, Some(&timeout)) {
            Ok(ready) => return Ok(ready > 0),
            Err(Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_lines_split_like_desktop_entries() {
        assert_eq!(
            split_exec("ffmpegthumbnailer -i %i -o %o -s %s -f").unwrap(),
            [
                "ffmpegthumbnailer",
                "-i",
                "%i",
                "-o",
                "%o",
                "-s",
                "%s",
                "-f"
            ]
        );
        assert_eq!(
            split_exec(r#"run  "a b" "c\"d" """#).unwrap(),
            ["run", "a b", "c\"d", ""]
        );
        assert_eq!(
            split_exec(r"run a\b"),
            Some(vec!["run".into(), r"a\b".into()])
        );
        assert_eq!(split_exec(r#"run "open"#), None);
        assert_eq!(split_exec("   "), None);
    }

    #[test]
    fn a_thumbnailer_that_closes_its_output_and_keeps_running_is_stopped_in_time() {
        let request = Request {
            input: Path::new("/in/a.mp4"),
            uri: "file:///in/a.mp4",
            output: Path::new("/out/a.png"),
            size: 128,
        };
        let exec = ["sh", "-c", "exec >&-; exec sleep 30"].map(str::to_owned);
        let started = std::time::Instant::now();

        let ran = run_within(&exec, &request, Duration::from_millis(200));

        assert_eq!(
            ran.map_err(|error| error.kind()),
            Err(io::ErrorKind::TimedOut)
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn field_codes_are_filled_in() {
        let request = Request {
            input: Path::new("/in/a b.mp4"),
            uri: "file:///in/a%20b.mp4",
            output: Path::new("/out/x.png"),
            size: 128,
        };
        assert_eq!(fill("%i", &request), "/in/a b.mp4");
        assert_eq!(fill("--uri=%u", &request), "--uri=file:///in/a%20b.mp4");
        assert_eq!(fill("%o", &request), "/out/x.png");
        assert_eq!(fill("-s%s", &request), "-s128");
        assert_eq!(fill("100%%%x", &request), "100%");
    }

    #[test]
    fn only_installed_complete_thumbnailers_are_loaded() {
        let root = env::temp_dir().join(format!("nova-thumbnailers-{}", std::process::id()));
        let folder = root.join(THUMBNAILERS_FOLDER);
        fs::create_dir_all(&folder).unwrap();
        let program = env::current_exe().unwrap();
        let entry = |mime: &str, program: &Path| {
            format!(
                "{GROUP}\nExec={} %i %o\nMimeType={mime};\n",
                program.display()
            )
        };
        let padding = "# padding\n".repeat(500);
        fs::write(
            folder.join("long.thumbnailer"),
            padding + &entry("video/long", &program),
        )
        .unwrap();
        fs::write(
            folder.join("missing.thumbnailer"),
            entry("video/missing", &root.join("not-installed")),
        )
        .unwrap();

        let thumbnailers = Thumbnailers::load(std::slice::from_ref(&root));
        fs::remove_dir_all(&root).ok();

        assert!(thumbnailers.0.contains_key("video/long"));
        assert!(!thumbnailers.0.contains_key("video/missing"));
    }
}
