//! Start-up phase times and frame times, for the benchmark.
//!
//! Each trace is off unless its variable names a file. [`start_traces`]
//! opens the files that are named and installs one logger that writes GPUI's
//! timing records to them; with neither variable set nothing is installed
//! and nothing is written.
//!
//! - [`STARTUP_TRACE_VARIABLE`]: `<unix ms> process-start` first, then one
//!   `<unix ms> <label>` line for each start-up phase GPUI logs under its
//!   `gpui::startup` target (vulkan-instance, gpu-device, fonts-ready,
//!   first-frame).
//! - [`FRAME_TRACE_VARIABLE`]: one `<start ns> <end ns>` line for each frame
//!   GPUI draws, both on `CLOCK_MONOTONIC` (the compositor's presentation
//!   clock). GPUI logs a frame under `gpui::frame` with its duration right
//!   after present returns, so the end is when the record arrives and the
//!   start is that minus the duration. A frame that only re-presents, or
//!   is skipped, writes nothing.
//!
//! Every other log record is ignored. Each line is one unbuffered write, so
//! lines from different threads never interleave and a killed process loses
//! none. The files are appended to, never truncated, so a reader removes
//! them before each run.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};
use rustix::time::{ClockId, clock_gettime};

/// Names the file the start-up phase times go to.
pub const STARTUP_TRACE_VARIABLE: &str = "NOVA_FILES_STARTUP_TRACE";
/// Names the file the frame times go to.
pub const FRAME_TRACE_VARIABLE: &str = "NOVA_FILES_FRAME_TRACE";

/// `gpui_wgpu::STARTUP_LOG_TARGET` in the ImperaLuna/zed fork. Nova Files
/// doesn't depend on `gpui_wgpu` directly, so the name is repeated here.
const GPUI_STARTUP_TARGET: &str = "gpui::startup";
/// `gpui::FRAME_LOG_TARGET` in the ImperaLuna/zed fork; the core never
/// depends on GPUI, so the name is repeated here.
const GPUI_FRAME_TARGET: &str = "gpui::frame";
/// GPUI logs its phases and frames at debug level.
const TRACE_LEVEL: LevelFilter = LevelFilter::Debug;
const PROCESS_START: &str = "process-start";
const NANOS_PER_SECOND: u128 = 1_000_000_000;

const STARTUP_TRACE: &str = "start-up trace";
const FRAME_TRACE: &str = "frame trace";
const TRACES: &str = "traces";

/// A trace file that couldn't be opened or written, or a logger that was
/// already installed.
#[derive(Debug)]
pub struct TraceError {
    trace: &'static str,
    error: io::Error,
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.trace, self.error)
    }
}

impl std::error::Error for TraceError {}

/// Starts the traces whose variables are set. Call it first in `main`, so
/// `process-start` is as early as it can be.
///
/// A trace that fails is left off and reported; the others still start.
pub fn start_traces() -> Vec<TraceError> {
    let (traces, mut errors) = open_traces(
        env::var_os(STARTUP_TRACE_VARIABLE),
        env::var_os(FRAME_TRACE_VARIABLE),
    );
    let Some(traces) = traces else {
        return errors;
    };
    match log::set_boxed_logger(Box::new(traces)) {
        Ok(()) => log::set_max_level(TRACE_LEVEL),
        Err(error) => errors.push(TraceError {
            trace: TRACES,
            error: io::Error::other(error),
        }),
    }
    errors
}

/// Opens the trace files the variables name (`startup`, `frames`) and
/// writes `process-start`. `None` when neither trace is on.
fn open_traces(
    startup: Option<OsString>,
    frames: Option<OsString>,
) -> (Option<Traces>, Vec<TraceError>) {
    let mut errors = Vec::new();
    let mut open = |path, trace| {
        open_named(path).unwrap_or_else(|error| {
            errors.push(TraceError { trace, error });
            None
        })
    };
    let startup = open(startup, STARTUP_TRACE);
    let frames = open(frames, FRAME_TRACE);
    if let Some(file) = &startup
        && let Err(error) = file.startup_phase(PROCESS_START)
    {
        errors.push(TraceError {
            trace: STARTUP_TRACE,
            error,
        });
    }
    let traces = (startup.is_some() || frames.is_some()).then_some(Traces { startup, frames });
    (traces, errors)
}

/// The file `path` names, if it names one.
fn open_named(path: Option<OsString>) -> io::Result<Option<TraceFile>> {
    path.filter(|path| !path.is_empty())
        .map(|path| TraceFile::open(Path::new(&path)))
        .transpose()
}

struct TraceFile(Mutex<File>);

impl TraceFile {
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self(Mutex::new(file)))
    }

    /// Appends `line` in a single write.
    fn append(&self, line: &str) -> io::Result<()> {
        let mut file = self
            .0
            .lock()
            .map_err(|_| io::Error::other("trace lock poisoned"))?;
        file.write_all(line.as_bytes())
    }

    fn startup_phase(&self, label: &str) -> io::Result<()> {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        self.append(&format!("{} {label}\n", since_epoch.as_millis()))
    }

    /// A frame that took `duration` nanoseconds and ended just now.
    fn frame(&self, duration: &str) -> io::Result<()> {
        let duration: u128 = duration
            .trim()
            .parse()
            .map_err(|_| io::Error::other(format!("not a frame duration: {duration:?}")))?;
        let end = monotonic_nanos();
        let start = end.saturating_sub(duration);
        self.append(&format!("{start} {end}\n"))
    }
}

/// Now on `CLOCK_MONOTONIC`, in nanoseconds.
fn monotonic_nanos() -> u128 {
    let now = clock_gettime(ClockId::Monotonic);
    let seconds = u128::try_from(now.tv_sec).unwrap_or_default();
    let nanos = u128::try_from(now.tv_nsec).unwrap_or_default();
    seconds * NANOS_PER_SECOND + nanos
}

struct Traces {
    startup: Option<TraceFile>,
    frames: Option<TraceFile>,
}

impl Traces {
    /// The file for records logged under `target`, if that trace is on.
    fn file(&self, target: &str) -> Option<&TraceFile> {
        match target {
            GPUI_STARTUP_TARGET => self.startup.as_ref(),
            GPUI_FRAME_TARGET => self.frames.as_ref(),
            _ => None,
        }
    }
}

impl Log for Traces {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.file(metadata.target()).is_some()
    }

    fn log(&self, record: &Record) {
        // The log macros don't ask `enabled` first.
        let Some(file) = self.file(record.target()) else {
            return;
        };
        let message = record.args().to_string();
        let (written, trace) = if record.target() == GPUI_FRAME_TARGET {
            (file.frame(&message), FRAME_TRACE)
        } else {
            (file.startup_phase(&message), STARTUP_TRACE)
        };
        if let Err(error) = written {
            writeln!(io::stderr(), "nova-files: {trace}: {error}").ok();
        }
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use log::Level;

    use super::*;

    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str) -> Self {
            let path =
                env::temp_dir().join(format!("nova-files-trace-{name}-{}", std::process::id()));
            fs::remove_file(&path).ok();
            Self(path)
        }

        fn open(&self) -> TraceFile {
            TraceFile::open(&self.0).unwrap()
        }

        /// Each line split at its first space.
        fn lines(&self) -> Vec<(u128, String)> {
            fs::read_to_string(&self.0)
                .unwrap_or_default()
                .lines()
                .map(|line| {
                    let (number, rest) = line.split_once(' ').unwrap();
                    (number.parse().unwrap(), rest.to_owned())
                })
                .collect()
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            fs::remove_file(&self.0).ok();
        }
    }

    fn record(traces: &Traces, target: &str, level: Level, message: &str) {
        traces.log(
            &Record::builder()
                .target(target)
                .level(level)
                .args(format_args!("{message}"))
                .build(),
        );
    }

    fn unix_millis() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    }

    #[test]
    fn gpui_phases_are_written_with_the_time() {
        let file = TempFile::new("phases");
        let traces = Traces {
            startup: Some(file.open()),
            frames: None,
        };
        let before = unix_millis();
        record(
            &traces,
            GPUI_STARTUP_TARGET,
            Level::Debug,
            "vulkan-instance",
        );
        record(&traces, GPUI_STARTUP_TARGET, Level::Debug, "first-frame");
        let after = unix_millis();
        let lines = file.lines();
        let labels: Vec<&str> = lines.iter().map(|(_, label)| label.as_str()).collect();
        assert_eq!(labels, ["vulkan-instance", "first-frame"]);
        assert!(lines.iter().all(|(ms, _)| (before..=after).contains(ms)));
    }

    #[test]
    fn a_frame_is_written_as_its_monotonic_start_and_end() {
        let file = TempFile::new("frames");
        let traces = Traces {
            startup: None,
            frames: Some(file.open()),
        };
        let duration: u128 = 2_500_000;
        let before = monotonic_nanos();
        record(
            &traces,
            GPUI_FRAME_TARGET,
            Level::Debug,
            &duration.to_string(),
        );
        let after = monotonic_nanos();
        let lines = file.lines();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let (start, end) = &lines[0];
        let end: u128 = end.parse().unwrap();
        assert!((before..=after).contains(&end), "{before} {end} {after}");
        assert_eq!(end - start, duration);
    }

    #[test]
    fn monotonic_time_counts_whole_seconds_as_nanoseconds() {
        let before = clock_gettime(ClockId::Monotonic);
        let nanos = monotonic_nanos();
        let after = clock_gettime(ClockId::Monotonic);
        let seconds = |time: rustix::time::Timespec| u128::try_from(time.tv_sec).unwrap();
        let whole = nanos / NANOS_PER_SECOND;
        assert!(
            (seconds(before)..=seconds(after)).contains(&whole),
            "{nanos}"
        );
    }

    #[test]
    fn a_frame_record_that_isnt_a_duration_writes_nothing() {
        let file = TempFile::new("bad-frame");
        let trace = file.open();
        assert!(trace.frame("soon").is_err());
        assert_eq!(file.lines(), Vec::new());
    }

    #[test]
    fn each_trace_gets_only_its_own_records() {
        let startup = TempFile::new("own-startup");
        let frames = TempFile::new("own-frames");
        let traces = Traces {
            startup: Some(startup.open()),
            frames: Some(frames.open()),
        };
        record(&traces, GPUI_STARTUP_TARGET, Level::Debug, "first-frame");
        record(&traces, GPUI_FRAME_TARGET, Level::Debug, "1000");
        let startup_lines = startup.lines();
        assert_eq!(startup_lines.len(), 1, "{startup_lines:?}");
        assert_eq!(startup_lines[0].1, "first-frame");
        assert_eq!(frames.lines().len(), 1);
    }

    #[test]
    fn a_trace_is_on_for_its_own_records() {
        let file = TempFile::new("enabled");
        let traces = Traces {
            startup: None,
            frames: Some(file.open()),
        };
        let frame = Metadata::builder().target(GPUI_FRAME_TARGET).build();
        assert!(traces.enabled(&frame));
    }

    #[test]
    fn either_trace_alone_starts_the_traces() {
        let startup = TempFile::new("alone-startup");
        let frames = TempFile::new("alone-frames");

        let (traces, errors) = open_traces(Some(startup.0.clone().into()), None);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(traces.is_some_and(|traces| traces.startup.is_some()));
        assert_eq!(startup.lines()[0].1, PROCESS_START);

        let (traces, errors) = open_traces(None, Some(frames.0.clone().into()));
        assert!(errors.is_empty(), "{errors:?}");
        assert!(traces.is_some_and(|traces| traces.frames.is_some()));
        assert_eq!(frames.lines(), Vec::new());
    }

    #[test]
    fn no_trace_named_starts_nothing() {
        let (traces, errors) = open_traces(None, Some(OsString::new()));
        assert!(traces.is_none());
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_trace_that_cant_open_leaves_the_other_on() {
        let frames = TempFile::new("other-on");
        let (traces, errors) = open_traces(
            Some("/nonexistent/folder/trace".into()),
            Some(frames.0.clone().into()),
        );
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().starts_with(STARTUP_TRACE));
        assert!(traces.is_some_and(|traces| traces.startup.is_none() && traces.frames.is_some()));
    }

    #[test]
    fn records_for_a_trace_that_is_off_are_dropped() {
        let startup = TempFile::new("off-frames");
        let traces = Traces {
            startup: Some(startup.open()),
            frames: None,
        };
        let frame = Metadata::builder().target(GPUI_FRAME_TARGET).build();
        assert!(!traces.enabled(&frame));
        record(&traces, GPUI_FRAME_TARGET, Level::Debug, "1000");
        assert_eq!(startup.lines(), Vec::new());
    }

    #[test]
    fn other_log_records_are_ignored() {
        let file = TempFile::new("ignored");
        let traces = Traces {
            startup: Some(file.open()),
            frames: Some(file.open()),
        };
        assert!(!traces.enabled(&Metadata::builder().target("gpui").build()));
        record(&traces, "wgpu_hal::vulkan", Level::Debug, "instance flags");
        record(&traces, "gpui", Level::Warn, "enabling OpenGL fallback");
        assert_eq!(file.lines(), Vec::new());
    }

    #[test]
    fn an_existing_trace_is_appended_to() {
        let file = TempFile::new("append");
        file.open().startup_phase(PROCESS_START).unwrap();
        file.open().startup_phase(PROCESS_START).unwrap();
        assert_eq!(file.lines().len(), 2);
    }

    #[test]
    fn a_trace_error_names_its_trace() {
        let error = TraceError {
            trace: FRAME_TRACE,
            error: io::Error::other("disk full"),
        };
        assert_eq!(error.to_string(), "frame trace: disk full");
    }
}
