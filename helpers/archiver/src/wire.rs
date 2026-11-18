use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Folder,
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    Utc,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<'a> {
    pub kind: Kind,
    pub locked: bool,
    pub size: u64,
    pub modified: i64,
    pub clock: Clock,
    pub mode: u32,
    pub path: &'a [u8],
    pub link: &'a [u8],
}

const END: u8 = 0;
const MODE_BITS: u32 = 0o7777;

pub fn write(out: &mut impl Write, entry: &Entry<'_>) -> io::Result<()> {
    let path = clean(entry.path);
    if path.is_empty() {
        return Ok(());
    }
    let kind: &[u8] = match entry.kind {
        Kind::File => b"f",
        Kind::Folder => b"d",
        Kind::Link => b"l",
    };
    let link: &[u8] = if entry.kind == Kind::Link {
        entry.link
    } else {
        b""
    };
    for field in [
        kind,
        if entry.locked { b"1" } else { b"0" },
        entry.size.to_string().as_bytes(),
        entry.modified.to_string().as_bytes(),
        match entry.clock {
            Clock::Utc => b"u",
            Clock::Local => b"l",
        },
        (entry.mode & MODE_BITS).to_string().as_bytes(),
        path,
        link,
    ] {
        let field = field.split(|&byte| byte == END).next().unwrap_or_default();
        out.write_all(field)?;
        out.write_all(&[END])?;
    }
    Ok(())
}

fn clean(path: &[u8]) -> &[u8] {
    let mut path = path;
    loop {
        if let Some(rest) = path.strip_prefix(b"./") {
            path = rest;
        } else if let Some(rest) = path.strip_prefix(b"/") {
            path = rest;
        } else {
            break;
        }
    }
    while let Some(rest) = path.strip_suffix(b"/") {
        path = rest;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(entry: &Entry<'_>) -> Vec<u8> {
        let mut out = Vec::new();
        write(&mut out, entry).unwrap();
        out
    }

    fn file(path: &[u8]) -> Entry<'_> {
        Entry {
            kind: Kind::File,
            locked: false,
            size: 12,
            modified: -5,
            clock: Clock::Utc,
            mode: 0o100_644,
            path,
            link: b"ignored",
        }
    }

    #[test]
    fn writes_eight_fields_each_ended() {
        assert_eq!(
            written(&file(b"a/b.txt")),
            b"f\x000\x0012\x00-5\x00u\x00420\x00a/b.txt\x00\x00"
        );
        let link = Entry {
            kind: Kind::Link,
            locked: true,
            clock: Clock::Local,
            link: b"../x",
            ..file(b"l")
        };
        assert_eq!(
            written(&link),
            b"l\x001\x0012\x00-5\x00l\x00420\x00l\x00../x\x00"
        );
        let folder = Entry {
            kind: Kind::Folder,
            ..file(b"d/")
        };
        assert_eq!(
            written(&folder),
            b"d\x000\x0012\x00-5\x00u\x00420\x00d\x00\x00"
        );
    }

    #[test]
    fn cleans_leading_and_trailing_slashes() {
        assert_eq!(clean(b"./a/"), b"a");
        assert_eq!(clean(b"//./b//"), b"b");
        assert_eq!(clean(b"a/./b"), b"a/./b");
        assert_eq!(clean(b"../a"), b"../a");
        assert_eq!(written(&file(b"./")), b"");
        assert_eq!(written(&file(b"")), b"");
    }

    #[test]
    fn a_nul_cuts_the_field() {
        assert_eq!(
            written(&file(b"a\x00b")),
            b"f\x000\x0012\x00-5\x00u\x00420\x00a\x00\x00"
        );
    }
}
