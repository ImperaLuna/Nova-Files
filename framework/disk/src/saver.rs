use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::private_file;

pub struct Saver {
    path: Option<PathBuf>,
    what: &'static str,
    thread: &'static str,
    writer: RefCell<Option<Sender<Arc<[u8]>>>>,
}

impl Saver {
    pub const fn new(path: Option<PathBuf>, what: &'static str, thread: &'static str) -> Self {
        Self {
            path,
            what,
            thread,
            writer: RefCell::new(None),
        }
    }

    pub fn save(&self, bytes: Arc<[u8]>) {
        let Some(path) = &self.path else {
            return;
        };
        let mut writer = self.writer.borrow_mut();
        if writer.is_none() {
            *writer = start_writer(path.clone(), self.what, self.thread);
        }
        let sent = writer.as_ref().map(|queue| queue.send(bytes));
        if let Some(Err(_)) = sent {
            log::warn!("the {} writer stopped; it isn't saved", self.what);
            *writer = None;
        }
    }
}

impl Saver {
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

fn start_writer(
    path: PathBuf,
    what: &'static str,
    name: &'static str,
) -> Option<Sender<Arc<[u8]>>> {
    let (queue, saves) = mpsc::channel();
    let started = thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || write_each(&path, what, &saves));
    match started {
        Ok(_) => Some(queue),
        Err(error) => {
            log::warn!("can't start the {what} writer, it isn't saved: {error}");
            None
        }
    }
}

fn write_each(path: &Path, what: &str, saves: &Receiver<Arc<[u8]>>) {
    for bytes in saves {
        if let Err(error) = private_file::replace(path, &bytes, private_file::FILE_MODE) {
            log::warn!("can't save the {what} to {}: {error}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::private_file::Read;

    const SAVES_IN_A_ROW: u32 = 20;

    fn temp(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("nova-files-saver-{name}-{}", std::process::id()));
        fs::remove_dir_all(&path).ok();
        path
    }

    fn read(path: &Path) -> Vec<u8> {
        match private_file::read(path, usize::MAX) {
            Read::Bytes(bytes) => bytes,
            Read::Missing | Read::TooBig | Read::Failed(_) => Vec::new(),
        }
    }

    #[test]
    fn the_writer_writes_saves_in_order_and_goes_on_past_a_failure() {
        let home = temp("order");
        let path = home.join("file");
        let (queue, saves) = mpsc::channel();
        for save in 0..=SAVES_IN_A_ROW {
            queue.send(save.to_string().into_bytes().into()).unwrap();
        }
        drop(queue);
        write_each(&path, "test", &saves);
        assert_eq!(read(&path), SAVES_IN_A_ROW.to_string().as_bytes());
        let blocked = temp("order-blocked");
        fs::write(&blocked, b"").unwrap();
        let (queue, saves) = mpsc::channel();
        queue.send(b"x".to_vec().into()).unwrap();
        drop(queue);
        write_each(&blocked.join("file"), "test", &saves);
        fs::remove_file(blocked).ok();
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_saver_without_a_path_keeps_nothing() {
        let saver = Saver::new(None, "test", "test");
        saver.save(b"x".to_vec().into());
        assert!(saver.writer.borrow().is_none());
    }
}
