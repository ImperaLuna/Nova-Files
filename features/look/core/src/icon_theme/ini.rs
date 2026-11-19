const SECTION_OPEN: char = '[';
const SECTION_CLOSE: char = ']';
const COMMENT: char = '#';
const ASSIGN: char = '=';

#[derive(Debug, PartialEq, Eq)]
pub enum Line<'a> {
    Section(&'a str),
    Entry { key: &'a str, value: &'a str },
}

pub fn lines(text: &str) -> impl Iterator<Item = Line<'_>> {
    text.lines().map(str::trim).filter_map(|line| {
        if line.is_empty() || line.starts_with(COMMENT) {
            return None;
        }
        if let Some(name) = line
            .strip_prefix(SECTION_OPEN)
            .and_then(|rest| rest.strip_suffix(SECTION_CLOSE))
        {
            return Some(Line::Section(name));
        }
        let (key, value) = line.split_once(ASSIGN)?;
        Some(Line::Entry {
            key: key.trim(),
            value: value.trim(),
        })
    })
}

pub fn value<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    lines(text).find_map(|line| match line {
        Line::Section(name) => {
            inside = name == section;
            None
        }
        Line::Entry { key: found, value } => (inside && found == key).then_some(value),
    })
}
