use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::MetadataExt as _;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use super::*;

const LIMIT: u32 = 50;
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
const QUIET: Duration = Duration::from_millis(200);
const DAMAGED: &[u8] = &[0; 8];

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("nova-files-searcher-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(root.join("home/docs")).unwrap();
        fs::write(root.join("home/docs/report.pdf"), b"").unwrap();
        Self { root }
    }

    fn place(&self) -> IndexPlace {
        IndexPlace {
            folder: self.root.join("cache"),
        }
    }

    fn searcher(&self) -> Searcher {
        Searcher::new(Some(self.place()), Some(self.root.join("home")))
    }

    fn index_inode(&self) -> u64 {
        fs::metadata(self.place().index()).unwrap().ino()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

type Asked = (Arc<dyn EventSink>, Arc<AtomicU64>);

fn client() -> (Asked, Receiver<Event>) {
    let (sink, events) = mpsc::channel::<Event>();
    ((Arc::new(sink), Arc::new(AtomicU64::new(0))), events)
}

impl Searcher {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.shared.state.lock().unwrap()
    }

    fn search_and_settle(&self) {
        self.start_build(false);
        assert!(self.run(
            &Query::everywhere("report", LIMIT as usize),
            || true,
            |_, _| true
        ));
        self.settle();
    }

    fn settle(&self) {
        let state = self.state();
        drop(
            self.shared
                .built
                .wait_while(state, |state| state.building)
                .unwrap(),
        );
    }

    fn live(&self, edit: impl FnOnce(&mut Live)) {
        edit(self.state().live.as_mut().unwrap());
    }
}

#[test]
fn a_newer_search_cancels_one_waiting_for_the_index() {
    let scratch = Scratch::new("cancel");
    let searcher = scratch.searcher();
    let (client, events) = client();
    searcher.state().building = true;
    let (sink, current) = &client;
    searcher.search((sink, current), SearchId(1), "report".to_owned(), LIMIT);
    searcher.search((sink, current), SearchId(2), "report".to_owned(), LIMIT);
    let live = Live::start(&scratch.place(), &scratch.root.join("home"), false);
    let mut state = searcher.state();
    state.live = live;
    state.building = false;
    searcher.shared.built.notify_all();
    drop(state);
    let mut done = false;
    while !done {
        let Event::Found { id, done: last, .. } = events.recv_timeout(ANSWER_TIMEOUT).unwrap()
        else {
            panic!("expected hits");
        };
        assert_eq!(id, SearchId(2));
        done = last;
    }
    assert!(events.recv_timeout(QUIET).is_err());
}

#[test]
fn a_replaced_search_stops_before_it_scans() {
    let scratch = Scratch::new("replaced");
    let searcher = scratch.searcher();
    searcher.search_and_settle();
    let answered = searcher.run(
        &Query::everywhere("report", LIMIT as usize),
        || false,
        |_, _| {
            panic!("a replaced search answered");
        },
    );
    assert!(answered);
}

#[test]
fn one_build_runs_at_a_time() {
    let scratch = Scratch::new("one-build");
    let searcher = scratch.searcher();
    searcher.state().building = true;
    searcher.start_build(true);
    let (state, waited) = searcher
        .shared
        .built
        .wait_timeout_while(searcher.state(), QUIET, |state| state.building)
        .unwrap();
    drop(state);
    assert!(waited.timed_out());
    assert!(!scratch.place().index().exists());
}

#[test]
fn an_open_index_is_kept() {
    let scratch = Scratch::new("kept");
    let searcher = scratch.searcher();
    searcher.search_and_settle();
    let built = scratch.index_inode();
    searcher.live(|live| live.changes = REBUILD_AFTER);
    searcher.search_and_settle();
    assert_eq!(scratch.index_inode(), built);
}

#[test]
fn many_changes_crawl_anew() {
    let scratch = Scratch::new("many");
    let searcher = scratch.searcher();
    searcher.search_and_settle();
    let built = scratch.index_inode();
    searcher.live(|live| live.changes = REBUILD_AFTER + 1);
    searcher.search_and_settle();
    assert_ne!(scratch.index_inode(), built);
    searcher.live(|live| assert_eq!(live.changes, 0));
}

#[test]
fn a_damaged_log_crawls_anew() {
    let scratch = Scratch::new("damaged");
    let searcher = scratch.searcher();
    searcher.search_and_settle();
    let built = scratch.index_inode();
    let mut log = OpenOptions::new()
        .append(true)
        .open(scratch.place().changes())
        .unwrap();
    log.write_all(DAMAGED).unwrap();
    searcher.search_and_settle();
    assert_ne!(scratch.index_inode(), built);
}

#[test]
fn lost_changes_crawl_anew() {
    let scratch = Scratch::new("lost");
    let searcher = scratch.searcher();
    searcher.search_and_settle();
    let built = scratch.index_inode();
    let home = scratch.root.join("home");
    searcher.live(|live| {
        live.take(&[Change::Removed {
            path: home.join("docs"),
        }]);
        assert!(!live.lost);
        live.take(&[Change::Rescan { folder: home }]);
        assert!(live.lost);
        assert_eq!(live.changes, 2);
    });
    searcher.search_and_settle();
    assert_ne!(scratch.index_inode(), built);
}

#[test]
fn a_failed_crawl_after_lost_changes_fails_searches_until_one_works() {
    let scratch = Scratch::new("failed");
    let searcher = scratch.searcher();
    searcher.search_and_settle();
    let home = scratch.root.join("home");
    searcher.live(|live| live.take(&[Change::Rescan { folder: home }]));
    let cache = scratch.place().folder;
    fs::remove_dir_all(&cache).unwrap();
    fs::write(&cache, b"").unwrap();
    searcher.search_and_settle();
    let search = || {
        searcher.run(
            &Query::everywhere("report", LIMIT as usize),
            || true,
            |_, _| true,
        )
    };
    assert!(!search());
    searcher.settle();
    fs::remove_file(&cache).unwrap();
    assert!(!search());
    searcher.settle();
    assert!(search());
    searcher.live(|live| assert!(!live.lost));
}

#[test]
fn every_kind_has_its_match() {
    let pairs = [
        (MatchKind::Exact, Matched::Exact),
        (MatchKind::WordStart, Matched::WordStart),
        (MatchKind::Contains, Matched::Contains),
        (MatchKind::InOrder, Matched::InOrder),
        (MatchKind::OneTypo, Matched::OneTypo),
    ];
    for (kind, expected) in pairs {
        assert_eq!(matched(kind), expected);
    }
}
