use std::ops::Range;

use gpui::{App, Global, Rgba, SharedString};
use nova_files_preview_api::{Mark, Run};
use nova_files_shell::theme::{Code, text};

const MONO_FAMILIES: [&str; 12] = [
    "JetBrains Mono",
    "JetBrainsMono Nerd Font",
    "Fira Code",
    "Cascadia Code",
    "Source Code Pro",
    "Hack",
    "Iosevka",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Ubuntu Mono",
    "Liberation Mono",
    "Adwaita Mono",
];

pub struct Coded {
    pub family: SharedString,
    pub lines: Vec<Line>,
}

pub struct Line {
    pub text: SharedString,
    pub marks: Vec<(Range<usize>, Mark)>,
}

impl Coded {
    pub fn new(code: &str, runs: &[Run], family: SharedString) -> Self {
        let mut pieces = pieces(runs);
        let mut piece = pieces.next();
        let mut lines = Vec::new();
        let mut start = 0;
        for line in code.split_inclusive('\n') {
            let end = start + line.len();
            let shown = line.trim_end_matches(['\n', '\r']);
            let shown_end = start + shown.len();
            let mut marks = Vec::new();
            while let Some((range, mark)) = piece.clone() {
                let from = range.start.max(start);
                let to = range.end.min(shown_end);
                if from < to && mark != Mark::Plain {
                    marks.push((from - start..to - start, mark));
                }
                if range.end > end {
                    break;
                }
                piece = pieces.next();
            }
            lines.push(Line {
                text: SharedString::from(shown.to_owned()),
                marks,
            });
            start = end;
        }
        Self { family, lines }
    }
}

fn pieces(runs: &[Run]) -> impl Iterator<Item = (Range<usize>, Mark)> + '_ {
    runs.iter().scan(0, |at: &mut usize, run| {
        let start = *at;
        *at += usize::try_from(run.bytes).unwrap_or(0);
        Some((start..*at, run.mark))
    })
}

pub const fn color(mark: Mark, code: &Code) -> Rgba {
    match mark {
        Mark::Plain => code.plain,
        Mark::Comment => code.comment,
        Mark::Keyword => code.keyword,
        Mark::Quoted => code.quoted,
        Mark::Number => code.number,
        Mark::Function => code.function,
        Mark::Type => code.kind,
        Mark::Tag => code.tag,
        Mark::Attribute => code.attribute,
    }
}

struct MonoFamily(SharedString);

impl Global for MonoFamily {}

pub fn mono_family(cx: &mut App) -> SharedString {
    if let Some(MonoFamily(family)) = cx.try_global::<MonoFamily>() {
        return family.clone();
    }
    let installed = cx.text_system().all_font_names();
    let family = pick_mono(&installed);
    cx.set_global(MonoFamily(family.clone()));
    family
}

fn pick_mono(installed: &[String]) -> SharedString {
    MONO_FAMILIES
        .into_iter()
        .find(|wanted| installed.iter().any(|name| name == wanted))
        .unwrap_or(text::FAMILY)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bytes: u32, mark: Mark) -> Run {
        Run { bytes, mark }
    }

    #[test]
    fn marks_split_at_line_ends_and_skip_plain() {
        let code = "let a\n// b\n";
        let runs = [
            run(3, Mark::Keyword),
            run(3, Mark::Plain),
            run(5, Mark::Comment),
        ];
        let coded = Coded::new(code, &runs, "Mono".into());
        assert_eq!(coded.lines.len(), 2);
        assert_eq!(coded.lines[0].text.as_ref(), "let a");
        assert_eq!(coded.lines[0].marks, vec![(0..3, Mark::Keyword)]);
        assert_eq!(coded.lines[1].text.as_ref(), "// b");
        assert_eq!(coded.lines[1].marks, vec![(0..4, Mark::Comment)]);
    }

    #[test]
    fn a_run_across_lines_marks_each_line() {
        let code = "/* a\nb */ x";
        let runs = [run(9, Mark::Comment), run(2, Mark::Plain)];
        let coded = Coded::new(code, &runs, "Mono".into());
        assert_eq!(coded.lines[0].marks, vec![(0..4, Mark::Comment)]);
        assert_eq!(coded.lines[1].marks, vec![(0..4, Mark::Comment)]);
    }

    #[test]
    fn the_first_installed_mono_family_wins() {
        let installed = [
            "Inter".to_owned(),
            "DejaVu Sans Mono".to_owned(),
            "Hack".to_owned(),
        ];
        assert_eq!(pick_mono(&installed).as_ref(), "Hack");
        assert_eq!(pick_mono(&["Inter".to_owned()]).as_ref(), text::FAMILY);
    }
}
