use std::ffi::OsStr;
use std::path::Path;
use std::sync::LazyLock;

use nova_files_preview_api::{Mark, Run};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

const LONGEST_LINE: usize = 2000;

const PROSE: [&str; 4] = [
    "Plain Text",
    "Markdown",
    "MultiMarkdown",
    "reStructuredText",
];

const MARKS: [(&str, Mark); 21] = [
    ("comment", Mark::Comment),
    ("string", Mark::Quoted),
    ("constant.character.escape", Mark::Quoted),
    ("constant.numeric", Mark::Number),
    ("constant.language", Mark::Number),
    ("constant.other", Mark::Number),
    ("keyword.operator", Mark::Plain),
    ("keyword", Mark::Keyword),
    ("storage.modifier", Mark::Keyword),
    ("storage.type", Mark::Keyword),
    ("variable.language", Mark::Keyword),
    ("entity.name.function", Mark::Function),
    ("support.function", Mark::Function),
    ("variable.function", Mark::Function),
    ("entity.name.type", Mark::Type),
    ("entity.name.class", Mark::Type),
    ("entity.name.struct", Mark::Type),
    ("support.type.property-name", Mark::Attribute),
    ("support.type", Mark::Type),
    ("entity.name.tag", Mark::Tag),
    ("entity.other.attribute-name", Mark::Attribute),
];

static SCOPES: LazyLock<Vec<(Scope, Mark)>> = LazyLock::new(|| {
    MARKS
        .iter()
        .filter_map(|(name, mark)| Scope::new(name).ok().map(|scope| (scope, *mark)))
        .collect()
});

pub fn runs(name: &OsStr, text: &str, cancelled: impl Fn() -> bool) -> Option<Vec<Run>> {
    if text.split('\n').any(|line| line.len() > LONGEST_LINE) {
        return None;
    }
    let syntax = syntax(name, text)?;
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut runs = Runs::default();
    for line in text.split_inclusive('\n') {
        if cancelled() {
            return None;
        }
        let ops = state.parse_line(line, &SYNTAXES).ok()?;
        let mut at = 0;
        for (index, op) in ops {
            runs.push(mark(&stack), index.saturating_sub(at));
            at = at.max(index);
            stack.apply(&op).ok()?;
        }
        runs.push(mark(&stack), line.len().saturating_sub(at));
    }
    Some(runs.0)
}

fn syntax<'a>(name: &OsStr, text: &str) -> Option<&'a SyntaxReference> {
    let path = Path::new(name);
    let by_name = path
        .extension()
        .and_then(OsStr::to_str)
        .and_then(|extension| SYNTAXES.find_syntax_by_extension(extension))
        .or_else(|| {
            name.to_str()
                .and_then(|whole| SYNTAXES.find_syntax_by_extension(whole))
        });
    let found = by_name.or_else(|| SYNTAXES.find_syntax_by_first_line(text))?;
    (!PROSE.contains(&found.name.as_str())).then_some(found)
}

fn mark(stack: &ScopeStack) -> Mark {
    stack
        .as_slice()
        .iter()
        .rev()
        .find_map(|scope| {
            SCOPES
                .iter()
                .find(|(prefix, _)| prefix.is_prefix_of(*scope))
                .map(|(_, mark)| *mark)
        })
        .unwrap_or(Mark::Plain)
}

#[derive(Default)]
struct Runs(Vec<Run>);

impl Runs {
    fn push(&mut self, mark: Mark, bytes: usize) {
        let Ok(bytes) = u32::try_from(bytes) else {
            return;
        };
        if bytes == 0 {
            return;
        }
        match self.0.last_mut() {
            Some(last) if last.mark == mark => last.bytes += bytes,
            _ => self.0.push(Run { bytes, mark }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(name: &str, text: &str) -> Vec<(String, Mark)> {
        let runs = runs(OsStr::new(name), text, || false).unwrap();
        let mut at = 0;
        runs.iter()
            .map(|run| {
                let end = at + run.bytes as usize;
                let piece = text[at..end].to_owned();
                at = end;
                (piece, run.mark)
            })
            .collect()
    }

    fn mark_of(name: &str, text: &str, piece: &str) -> Mark {
        marked(name, text)
            .into_iter()
            .find(|(found, _)| found.contains(piece))
            .map_or(Mark::Plain, |(_, mark)| mark)
    }

    #[test]
    fn the_runs_cover_the_whole_text() {
        let text = "fn main() {\n    let x = \"hi\"; // greet\n}\n";
        let runs = runs(OsStr::new("main.rs"), text, || false).unwrap();
        let covered: u32 = runs.iter().map(|run| run.bytes).sum();
        assert_eq!(covered as usize, text.len());
    }

    #[test]
    fn rust_has_keywords_strings_and_comments() {
        let text = "fn main() {\n    let x = \"hi\"; // greet\n}\n";
        assert_eq!(mark_of("main.rs", text, "let"), Mark::Keyword);
        assert_eq!(mark_of("main.rs", text, "hi"), Mark::Quoted);
        assert_eq!(mark_of("main.rs", text, "greet"), Mark::Comment);
    }

    #[test]
    fn config_languages_are_known() {
        assert_eq!(
            mark_of("Cargo.toml", "name = \"nova\"\n", "nova"),
            Mark::Quoted
        );
        assert_eq!(mark_of("flake.nix", "{ x = \"a\"; }\n", "a"), Mark::Quoted);
        assert_eq!(mark_of("data.json", "{\"n\": 12}\n", "12"), Mark::Number);
    }

    #[test]
    fn a_script_is_known_by_its_first_line() {
        assert_eq!(
            mark_of("run", "#!/bin/sh\n# say\necho hi\n", "say"),
            Mark::Comment
        );
    }

    #[test]
    fn plain_text_has_no_runs() {
        assert!(runs(OsStr::new("notes.txt"), "just words\n", || false).is_none());
        assert!(runs(OsStr::new("README"), "just words\n", || false).is_none());
        assert!(runs(OsStr::new("notes.md"), "# Notes\n", || false).is_none());
    }

    #[test]
    fn call_arguments_keep_their_own_colour() {
        assert_eq!(
            mark_of("run.py", "print(count, name)\n", "count"),
            Mark::Plain
        );
    }

    #[test]
    fn minified_code_is_left_plain() {
        let line = format!("var a={};\n", "1+".repeat(LONGEST_LINE));
        assert!(runs(OsStr::new("app.min.js"), &line, || false).is_none());
    }

    #[test]
    fn a_cancelled_look_gives_up() {
        assert!(runs(OsStr::new("main.rs"), "fn main() {}\n", || true).is_none());
    }
}
