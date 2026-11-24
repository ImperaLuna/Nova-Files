use std::io::{BufReader, Read, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, mpsc};
use std::thread;

use nova_files_api::Picture;
use rustix::process::Pid;

use crate::{
    BYTES_PER_PIXEL, PER_FILE, READER_THREAD, Running, SIZE_SEPARATOR, command, end, read_line,
};

const DOCUMENT_FLAG: &str = "--document";
const LOCKED: &str = "locked";
const PAGES: &str = "pages";
const NONE: &str = "none";
const MAX_PAGES: usize = 100_000;
const MAX_PAGE_SIDE: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub enum Opened {
    Locked,
    Pages(Document),
}

#[derive(Debug)]
pub struct Document {
    child: Option<Child>,
    group: Option<Pid>,
    running: Arc<Running>,
    input: ChildStdin,
    answers: mpsc::Receiver<Answer>,
    sizes: Vec<PageSize>,
}

#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Drawn(Picture),
    Missing,
}

enum Heard {
    Locked,
    Pages(Vec<PageSize>),
}

pub fn document(
    helper: &Path,
    file: &Path,
    running: &Arc<Running>,
    wanted: impl Fn() -> bool,
) -> Option<Opened> {
    let mut command = command(helper);
    command.stdin(Stdio::piped()).arg(DOCUMENT_FLAG).arg(file);
    let mut child = command.spawn().ok()?;
    let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
    running.start(group, wanted);
    let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
        running.forget(group);
        end(&mut child);
        return None;
    };
    let (heard, header) = mpsc::channel();
    let (answered, answers) = mpsc::channel();
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            let mut output = BufReader::new(output);
            let Some(first) = read_header(&mut output) else {
                return;
            };
            let is_open = matches!(first, Heard::Pages(_));
            if heard.send(first).is_err() || !is_open {
                return;
            }
            while let Some(page) = read_page(&mut output) {
                if answered.send(page).is_err() {
                    return;
                }
            }
        })
        .ok()?;
    let sizes = match header.recv_timeout(PER_FILE) {
        Ok(Heard::Pages(sizes)) => sizes,
        Ok(Heard::Locked) => {
            running.forget(group);
            end(&mut child);
            return Some(Opened::Locked);
        }
        Err(_) => {
            running.forget(group);
            end(&mut child);
            return None;
        }
    };
    Some(Opened::Pages(Document {
        child: Some(child),
        group,
        running: Arc::clone(running),
        input,
        answers,
        sizes,
    }))
}

impl Document {
    #[must_use]
    pub fn sizes(&self) -> &[PageSize] {
        &self.sizes
    }

    pub fn page(&mut self, index: usize, width: u32) -> Option<Picture> {
        let width = width.clamp(1, MAX_PAGE_SIDE);
        writeln!(self.input, "{index} {width}").ok()?;
        self.input.flush().ok()?;
        let Ok(answer) = self.answers.recv_timeout(PER_FILE) else {
            self.close();
            return None;
        };
        match answer {
            Answer::Drawn(page) => (page.width <= width.saturating_add(1)).then_some(page),
            Answer::Missing => None,
        }
    }

    fn close(&mut self) {
        self.running.forget(self.group);
        if let Some(mut child) = self.child.take() {
            end(&mut child);
        }
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        self.close();
    }
}

fn read_header(output: &mut impl Read) -> Option<Heard> {
    let line = read_line(output)?;
    if line == LOCKED {
        return Some(Heard::Locked);
    }
    let (word, count) = line.split_once(SIZE_SEPARATOR)?;
    let count: usize = count
        .parse()
        .ok()
        .filter(|count| (1..=MAX_PAGES).contains(count))?;
    if word != PAGES {
        return None;
    }
    let mut sizes = Vec::with_capacity(count);
    for _ in 0..count {
        let (width, height) = read_size(output)?;
        sizes.push(PageSize { width, height });
    }
    Some(Heard::Pages(sizes))
}

fn read_size(output: &mut impl Read) -> Option<(u32, u32)> {
    let line = read_line(output)?;
    let (width, height) = line.split_once(SIZE_SEPARATOR)?;
    let side = |word: &str| word.parse::<u32>().ok().filter(|side| *side >= 1);
    Some((side(width)?, side(height)?))
}

fn read_page(output: &mut impl Read) -> Option<Answer> {
    let line = read_line(output)?;
    if line == NONE {
        return Some(Answer::Missing);
    }
    let (width, height) = line.split_once(SIZE_SEPARATOR)?;
    let side = |word: &str| {
        word.parse::<u32>()
            .ok()
            .filter(|side| (1..=MAX_PAGE_SIDE).contains(side))
    };
    let (width, height) = (side(width)?, side(height)?);
    let length = u64::from(width) * u64::from(height) * BYTES_PER_PIXEL;
    let mut pixels = vec![0; usize::try_from(length).ok()?];
    output.read_exact(&mut pixels).ok()?;
    Some(Answer::Drawn(Picture {
        width,
        height,
        pixels: pixels.into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_names_each_page_size() {
        let Some(Heard::Pages(sizes)) = read_header(&mut b"pages 2\n612 792\n842 595\n".as_slice())
        else {
            panic!("expected pages");
        };
        assert_eq!(
            sizes,
            vec![
                PageSize {
                    width: 612,
                    height: 792
                },
                PageSize {
                    width: 842,
                    height: 595
                }
            ]
        );
        assert!(matches!(
            read_header(&mut b"locked\n".as_slice()),
            Some(Heard::Locked)
        ));
        for wrong in [
            "pages 0\n",
            "pages 2\n612 792\n",
            "pages 1\n0 792\n",
            "pagez 1\n612 792\n",
            "pages 1\n612x792\n",
        ] {
            assert!(read_header(&mut wrong.as_bytes()).is_none(), "{wrong:?}");
        }
    }

    #[test]
    fn a_page_answer_is_its_size_and_pixels_or_none() {
        let mut answer = b"2 1\n".to_vec();
        answer.extend([9; 8]);
        answer.extend(b"none\n");
        let mut output = answer.as_slice();
        let Some(Answer::Drawn(page)) = read_page(&mut output) else {
            panic!("expected a page");
        };
        assert_eq!((page.width, page.height), (2, 1));
        assert_eq!(&*page.pixels, &[9; 8]);
        assert_eq!(read_page(&mut output), Some(Answer::Missing));
        assert_eq!(read_page(&mut output), None);
        assert_eq!(read_page(&mut b"2 1\n1234".as_slice()), None);
        assert_eq!(read_page(&mut b"9000 1\n".as_slice()), None);
    }
}
