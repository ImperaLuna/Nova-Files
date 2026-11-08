//! Start-up phase times, for the benchmark.
//!
//! When [`TRACE_VARIABLE`] names a file, [`start_startup_trace`] appends
//! `<unix ms> process-start` to it and installs a logger that appends one
//! `<unix ms> <label>` line for each start-up phase GPUI logs under its
//! `gpui::startup` target (vulkan-instance, gpu-device, fonts-ready,
//! first-frame). Every other log record is ignored. Without the variable
//! nothing is installed and nothing is written.
//!
//! The file is appended to, never truncated, so a reader removes it before
//! each run.

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

/// Names the file the phase times go to.
pub const TRACE_VARIABLE: &str = "NOVA_FILES_STARTUP_TRACE";

/// `gpui_wgpu::STARTUP_LOG_TARGET` in the ImperaLuna/zed fork. Nova Files
/// doesn't depend on `gpui_wgpu` directly, so the name is repeated here.
const GPUI_STARTUP_TARGET: &str = "gpui::startup";
/// GPUI logs its phases at debug level.
const PHASE_LEVEL: LevelFilter = LevelFilter::Debug;
const PROCESS_START: &str = "process-start";

/// Starts the trace if [`TRACE_VARIABLE`] is set. Call it first in `main`,
/// so `process-start` is as early as it can be.
///
/// # Errors
///
/// When the file can't be opened or written, or another logger is already
/// installed.
pub fn start_startup_trace() -> io::Result<()> {
    let Some(path) = env::var_os(TRACE_VARIABLE).filter(|path| !path.is_empty()) else {
        return Ok(());
    };
    let trace = StartupTrace::open(Path::new(&path))?;
    trace.phase(PROCESS_START)?;
    log::set_boxed_logger(Box::new(trace)).map_err(io::Error::other)?;
    log::set_max_level(PHASE_LEVEL);
    Ok(())
}

struct StartupTrace {
    file: Mutex<File>,
}

impl StartupTrace {
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    /// Appends one line, written in a single call so lines from GPUI's
    /// font thread and the main thread never interleave.
    fn phase(&self, label: &str) -> io::Result<()> {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        let line = format!("{} {label}\n", since_epoch.as_millis());
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("trace lock poisoned"))?;
        file.write_all(line.as_bytes())
    }
}

impl Log for StartupTrace {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.target() == GPUI_STARTUP_TARGET
    }

    fn log(&self, record: &Record) {
        // The log macros don't ask `enabled` first.
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Err(error) = self.phase(&record.args().to_string()) {
            writeln!(io::stderr(), "nova-files: start-up trace: {error}").ok();
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

        fn lines(&self) -> Vec<(u128, String)> {
            fs::read_to_string(&self.0)
                .unwrap()
                .lines()
                .map(|line| {
                    let (ms, label) = line.split_once(' ').unwrap();
                    (ms.parse().unwrap(), label.to_owned())
                })
                .collect()
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            fs::remove_file(&self.0).ok();
        }
    }

    fn record(trace: &StartupTrace, target: &str, level: Level, message: &str) {
        trace.log(
            &Record::builder()
                .target(target)
                .level(level)
                .args(format_args!("{message}"))
                .build(),
        );
    }

    #[test]
    fn gpui_phases_are_written_with_the_time() {
        let file = TempFile::new("phases");
        let trace = StartupTrace::open(&file.0).unwrap();
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        record(&trace, GPUI_STARTUP_TARGET, Level::Debug, "vulkan-instance");
        record(&trace, GPUI_STARTUP_TARGET, Level::Debug, "first-frame");
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let lines = file.lines();
        let labels: Vec<&str> = lines.iter().map(|(_, label)| label.as_str()).collect();
        assert_eq!(labels, ["vulkan-instance", "first-frame"]);
        assert!(lines.iter().all(|(ms, _)| (before..=after).contains(ms)));
    }

    #[test]
    fn other_log_records_are_ignored() {
        let file = TempFile::new("ignored");
        let trace = StartupTrace::open(&file.0).unwrap();
        record(&trace, "wgpu_hal::vulkan", Level::Debug, "instance flags");
        record(&trace, "gpui", Level::Warn, "enabling OpenGL fallback");
        assert_eq!(file.lines(), Vec::new());
    }

    #[test]
    fn an_existing_trace_is_appended_to() {
        let file = TempFile::new("append");
        StartupTrace::open(&file.0)
            .unwrap()
            .phase(PROCESS_START)
            .unwrap();
        StartupTrace::open(&file.0)
            .unwrap()
            .phase(PROCESS_START)
            .unwrap();
        assert_eq!(file.lines().len(), 2);
    }
}
