use std::fs;
use std::io::{Seek as _, SeekFrom as StdSeekFrom, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

use nova_files_api::Failure;
use rustix::fs::{XattrFlags, getxattr, setxattr};

use super::*;
use crate::ops::test_folder::TestFolder;

const MODE: u32 = 0o640;
const SPARSE_SIZE: u64 = 64 * 1024 * 1024;
const TAG: &str = "user.xdg.tags";
/// Another filesystem than the scratch folders', where the system has one.
const OTHER_DRIVE: &str = "/dev/shm";

/// Long enough for any copy here; a copy that takes longer is stuck.
const STUCK: Duration = Duration::from_secs(10);

/// Runs `step` on a thread of its own, failing the test if it takes
/// longer than [`STUCK`] instead of hanging it.
fn within<T: Send + 'static>(step: impl FnOnce() -> T + Send + 'static) -> T {
    let (done, result) = mpsc::channel();
    thread::spawn(move || done.send(step()));
    result.recv_timeout(STUCK).expect("the step got stuck")
}

/// Copies `from` to `to`, cancelled first when `cancelled`, and returns
/// how it went and how many bytes it said it copied.
fn copy(from: &Path, to: &Path, cancelled: bool) -> (Result<(), Stop>, u64) {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    let part = to.with_file_name(".part");
    let checked = part.clone();
    let (result, copied) = within(move || {
        let control = Control::default();
        if cancelled {
            control.cancel();
        }
        let mut copied = 0;
        let result = copy_file(&from, &to, &part, &control, &mut |bytes| {
            copied += bytes;
        });
        (result, copied)
    });
    assert!(!checked.exists(), "the part file is gone");
    (result, copied)
}

#[test]
fn copies_bytes_mode_times_and_tags() {
    let folder = TestFolder::new("copy-file");
    let from = folder.file("a", "hello");
    fs::set_permissions(&from, fs::Permissions::from_mode(MODE)).unwrap();
    let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    File::options()
        .write(true)
        .open(&from)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let tagged = setxattr(&from, TAG, b"work", XattrFlags::empty()).is_ok();
    let to = folder.path().join("b");

    let (result, copied) = copy(&from, &to, false);

    result.unwrap();
    assert_eq!(copied, 5);
    assert_eq!(fs::read_to_string(&to).unwrap(), "hello");
    let metadata = fs::metadata(&to).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o7777, MODE);
    assert_eq!(metadata.modified().unwrap(), modified);
    if tagged {
        let mut value = Vec::with_capacity(4);
        getxattr(&to, TAG, rustix::buffer::spare_capacity(&mut value)).unwrap();
        assert_eq!(value, b"work");
    }
}

#[test]
fn a_sparse_file_stays_sparse() {
    let folder = TestFolder::new("copy-sparse");
    let from = folder.path().join("sparse");
    let mut file = File::create(&from).unwrap();
    file.set_len(SPARSE_SIZE).unwrap();
    file.seek(StdSeekFrom::Start(SPARSE_SIZE / 2)).unwrap();
    file.write_all(b"middle").unwrap();
    drop(file);
    let to = folder.path().join("copy");

    let (result, copied) = copy(&from, &to, false);

    result.unwrap();
    assert!(copied < SPARSE_SIZE, "only data ranges were copied");
    let copy = fs::metadata(&to).unwrap();
    assert_eq!(copy.len(), SPARSE_SIZE);
    assert!(copy.blocks() * 512 < SPARSE_SIZE);
    assert_eq!(fs::read(&to).unwrap(), fs::read(&from).unwrap());
}

#[test]
fn finds_only_the_ranges_that_hold_data() {
    let folder = TestFolder::new("copy-ranges");
    let path = folder.path().join("sparse");
    let mut file = File::create(&path).unwrap();
    file.set_len(SPARSE_SIZE).unwrap();
    file.seek(StdSeekFrom::Start(SPARSE_SIZE / 2)).unwrap();
    file.write_all(b"x").unwrap();
    let file = File::open(&path).unwrap();

    let ranges = data_ranges(&file, SPARSE_SIZE);

    assert_eq!(ranges.len(), 1);
    let (start, end) = ranges[0];
    assert!(start <= SPARSE_SIZE / 2 && SPARSE_SIZE / 2 < end && end < SPARSE_SIZE);
    let file = file.try_clone().unwrap();
    assert_eq!(within(move || data_ranges(&file, 0)), Vec::new());
}

#[test]
fn never_replaces_a_taken_name() {
    let folder = TestFolder::new("copy-taken");
    let from = folder.file("a", "new");
    let to = folder.file("b", "old");

    let (result, _) = copy(&from, &to, false);

    assert_eq!(result, Err(Stop::because(&to, Failure::NameTaken)));
    assert_eq!(fs::read_to_string(&to).unwrap(), "old");
}

#[test]
fn a_cancelled_copy_leaves_nothing() {
    let folder = TestFolder::new("copy-cancel");
    let from = folder.file("a", "text");
    let to = folder.path().join("b");
    let (result, _) = copy(&from, &to, true);

    assert_eq!(result, Err(Stop::Cancelled));
    assert!(!to.exists());
}

#[test]
fn a_missing_source_or_folder_fails_at_its_end() {
    let folder = TestFolder::new("copy-missing");
    let gone = folder.path().join("gone");
    let from = folder.file("a", "text");
    let nowhere = folder.path().join("no/b");

    assert_eq!(
        copy(&gone, &folder.path().join("b"), false).0,
        Err(Stop::because(&gone, Failure::NotFound))
    );
    let part = folder.path().join("no/.part");
    let mut copied = 0;
    assert_eq!(
        copy_file(&from, &nowhere, &part, &Control::default(), &mut |bytes| {
            copied += bytes;
        }),
        Err(Stop::because(&nowhere, Failure::NotFound))
    );
}

#[test]
fn plain_reads_and_writes_copy_a_range() {
    let folder = TestFolder::new("copy-plain");
    let bytes: Vec<u8> = (0..=255).cycle().take(BUFFER * 2 + 10).collect();
    let from = folder.path().join("a");
    fs::write(&from, &bytes).unwrap();
    let to = folder.path().join("b");
    let (source, target) = (File::open(&from).unwrap(), File::create(&to).unwrap());
    let ends = Ends {
        source: &source,
        target: &target,
        from: &from,
        to: &to,
    };
    let mut copied = 0;
    let end = bytes.len() as u64;

    read_and_write(&ends, (1, end), &Control::default(), &mut |n| {
        copied += n;
    })
    .unwrap();

    assert_eq!(copied, end - 1);
    let written = fs::read(&to).unwrap();
    assert_eq!(written[0], 0);
    assert_eq!(&written[1..], &bytes[1..]);
}

#[test]
fn plain_reads_and_writes_stop_when_cancelled_or_the_file_shrank() {
    let folder = TestFolder::new("copy-plain-stop");
    let from = folder.file("a", "abc");
    let to = folder.path().join("b");
    let (source, target) = (File::open(&from).unwrap(), File::create(&to).unwrap());
    let ends = Ends {
        source: &source,
        target: &target,
        from: &from,
        to: &to,
    };
    let mut copied = 0;

    // Past the end of a file that shrank since it was planned.
    let shrank = read_and_write(&ends, (0, 10), &Control::default(), &mut |n| {
        copied += n;
    });
    assert_eq!(shrank, Err(Stop::because(&from, Failure::SourceChanged)));
    assert_eq!(copied, 3);

    let control = Control::default();
    control.cancel();
    assert_eq!(
        read_and_write(&ends, (0, 3), &control, &mut |_| ()),
        Err(Stop::Cancelled)
    );
}

#[test]
fn only_unsupported_copies_fall_back() {
    assert!(kernel_cant_copy(Errno::XDEV));
    assert!(kernel_cant_copy(Errno::INVAL));
    assert!(kernel_cant_copy(Errno::NOSYS));
    assert!(kernel_cant_copy(Errno::OPNOTSUPP));
    assert!(!kernel_cant_copy(Errno::NOSPC));
    assert!(!kernel_cant_copy(Errno::IO));
}

#[test]
fn a_file_written_while_copied_isnt_kept() {
    let folder = TestFolder::new("copy-changed");
    let from = folder.file("a", "before");
    let to = folder.path().join("b");
    let part = folder.path().join(".part");
    let mut written = false;

    // The first bytes copied are the moment another program writes.
    let result = copy_file(&from, &to, &part, &Control::default(), &mut |_| {
        if !written {
            fs::write(&from, "after, and longer").unwrap();
            written = true;
        }
    });

    assert_eq!(result, Err(Stop::because(&from, Failure::SourceChanged)));
    assert!(!to.exists());
    assert!(!part.exists());
}

#[test]
fn a_step_that_reads_and_writes_blames_the_right_end() {
    let (from, to) = (Path::new("/from"), Path::new("/to"));
    let file = File::open("/").unwrap();
    let ends = Ends {
        source: &file,
        target: &file,
        from,
        to,
    };
    for errno in [Errno::NOSPC, Errno::DQUOT, Errno::ROFS, Errno::FBIG] {
        let Stop::Failed { path, .. } = ends.failed(&errno.into()) else {
            panic!("a failure");
        };
        assert_eq!(path, to, "{errno}");
    }
    let Stop::Failed { path, .. } = ends.failed(&Errno::IO.into()) else {
        panic!("a failure");
    };
    assert_eq!(path, from);
}

/// The open ends of a copy from `from` to `to`, `to` opened as `open`
/// says.
fn ends_of(from: &Path, to: &Path, open: &OpenOptions) -> (File, File) {
    (File::open(from).unwrap(), open.open(to).unwrap())
}

#[test]
fn plain_reads_and_writes_stop_at_the_end_of_their_range() {
    let folder = TestFolder::new("copy-plain-end");
    let from = folder.file("a", "0123456789");
    let to = folder.path().join("b");
    let (source, target) = ends_of(&from, &to, File::options().write(true).create(true));
    let ends = Ends {
        source: &source,
        target: &target,
        from: &from,
        to: &to,
    };
    let mut copied = 0;

    read_and_write(&ends, (2, 5), &Control::default(), &mut |n| copied += n).unwrap();

    assert_eq!(copied, 3);
    assert_eq!(fs::read(&to).unwrap(), [0, 0, b'2', b'3', b'4']);
}

#[test]
fn a_kernel_copy_moves_more_than_a_buffer_at_a_time() {
    let folder = TestFolder::new("copy-range-chunks");
    let bytes = BUFFER * 3;
    let from = folder.path().join("a");
    fs::write(&from, vec![b'x'; bytes]).unwrap();
    let to = folder.path().join("b");
    let (source, target) = ends_of(&from, &to, File::options().write(true).create(true));
    let ends = Ends {
        source: &source,
        target: &target,
        from: &from,
        to: &to,
    };
    let mut chunks = Vec::new();
    let end = u64::try_from(bytes).unwrap();

    copy_range(&ends, (0, end), &Control::default(), &mut |n| {
        chunks.push(n);
    })
    .unwrap();

    assert_eq!(chunks.iter().sum::<u64>(), end);
    assert!(
        chunks.iter().any(|&chunk| chunk > BUFFER as u64),
        "plain reads would move a buffer at a time: {chunks:?}"
    );
    assert_eq!(fs::read(&to).unwrap().len(), bytes);
}

#[test]
fn a_range_past_the_end_of_a_file_that_shrank_fails_as_changed() {
    let folder = TestFolder::new("copy-range-shrank");
    let from = folder.file("a", "abc");
    let to = folder.path().join("b");
    let (source, target) = ends_of(&from, &to, File::options().write(true).create(true));

    let stopped = within(move || {
        let ends = Ends {
            source: &source,
            target: &target,
            from: &from,
            to: &to,
        };
        copy_range(&ends, (0, 10), &Control::default(), &mut |_| ())
    });

    assert_eq!(
        stopped,
        Err(Stop::because(
            &folder.path().join("a"),
            Failure::SourceChanged
        ))
    );
}

#[test]
fn a_kernel_copy_that_fails_for_another_reason_fails_the_step() {
    let folder = TestFolder::new("copy-range-refused");
    let from = folder.file("a", "abc");
    let to = folder.path().join("b");
    // The kernel copies into no file opened to append.
    let (source, target) = ends_of(&from, &to, File::options().append(true).create(true));
    let ends = Ends {
        source: &source,
        target: &target,
        from: &from,
        to: &to,
    };

    let result = copy_range(&ends, (0, 3), &Control::default(), &mut |_| ());

    assert!(result.is_err(), "not read and written instead");
}

#[test]
fn a_copy_between_two_kinds_of_filesystem_reads_and_writes() {
    let folder = TestFolder::new("copy-across");
    let other = Path::new(OTHER_DRIVE).join(format!("nova-files-copy-{}", std::process::id()));
    if fs::create_dir_all(&other).is_err() {
        return;
    }
    let from = other.join("a");
    fs::write(&from, "across").unwrap();
    let to = folder.path().join("b");

    let (result, copied) = copy(&from, &to, false);

    fs::remove_dir_all(&other).ok();
    result.unwrap();
    assert_eq!(copied, 6);
    assert_eq!(fs::read_to_string(&to).unwrap(), "across");
}

#[test]
fn a_file_that_grew_but_kept_its_time_isnt_kept() {
    let folder = TestFolder::new("copy-grew");
    let from = folder.file("a", "before");
    let modified = fs::metadata(&from).unwrap().modified().unwrap();
    let to = folder.path().join("b");
    let part = folder.path().join(".part");
    let mut written = false;

    let result = copy_file(&from, &to, &part, &Control::default(), &mut |_| {
        if !written {
            let mut file = File::options().append(true).open(&from).unwrap();
            file.write_all(b" and after").unwrap();
            file.set_modified(modified).unwrap();
            written = true;
        }
    });

    assert_eq!(result, Err(Stop::because(&from, Failure::SourceChanged)));
    assert!(!to.exists());
}

#[test]
fn a_kernel_copy_stops_at_the_end_of_its_range() {
    let folder = TestFolder::new("copy-range-end");
    let from = folder.file("a", "0123456789");
    let to = folder.path().join("b");
    let (source, target) = ends_of(&from, &to, File::options().write(true).create(true));
    let ends = Ends {
        source: &source,
        target: &target,
        from: &from,
        to: &to,
    };
    let mut copied = 0;

    copy_range(&ends, (2, 5), &Control::default(), &mut |n| copied += n).unwrap();

    assert_eq!(copied, 3);
    assert_eq!(fs::read(&to).unwrap(), [0, 0, b'2', b'3', b'4']);
}

#[test]
fn a_file_rewritten_in_place_with_its_time_set_back_isnt_kept() {
    let folder = TestFolder::new("copy-rewritten");
    let from = folder.file("a", "before");
    let modified = fs::metadata(&from).unwrap().modified().unwrap();
    let to = folder.path().join("b");
    let part = folder.path().join(".part");
    let mut written = false;

    let result = copy_file(&from, &to, &part, &Control::default(), &mut |_| {
        if !written {
            // Same length, same modified time: only the change time tells.
            let file = File::options().write(true).open(&from).unwrap();
            file.write_all_at(b"AFTER!", 0).unwrap();
            file.set_modified(modified).unwrap();
            written = true;
        }
    });

    assert_eq!(result, Err(Stop::because(&from, Failure::SourceChanged)));
    assert!(!to.exists());
}

#[test]
fn a_file_that_says_its_empty_is_read_to_its_end() {
    let folder = TestFolder::new("copy-proc");
    let from = Path::new("/proc/version");
    if fs::metadata(from).map_or(true, |metadata| metadata.len() != 0) {
        return;
    }
    let to = folder.path().join("version");
    let part = folder.path().join(".part");
    let mut copied = 0;

    copy_file(from, &to, &part, &Control::default(), &mut |n| copied += n).unwrap();

    let text = fs::read_to_string(&to).unwrap();
    assert_ne!(text, "");
    assert_eq!(copied, text.len() as u64);
    assert_eq!(text, fs::read_to_string(from).unwrap());
}

#[test]
fn a_pipe_where_a_file_was_planned_is_refused_without_waiting() {
    let folder = TestFolder::new("copy-fifo");
    let from = folder.path().join("pipe");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &from,
        rustix::fs::FileType::Fifo,
        Mode::from_raw_mode(MODE),
        0,
    )
    .unwrap();
    let to = folder.path().join("b");
    let (paths, checked) = ((from.clone(), to.clone()), to);

    let result = within(move || {
        let part = paths.1.with_file_name(".part");
        copy_file(&paths.0, &paths.1, &part, &Control::default(), &mut |_| {})
    });

    assert_eq!(result, Err(Stop::because(&from, Failure::Unsupported)));
    assert!(!checked.exists());
}
