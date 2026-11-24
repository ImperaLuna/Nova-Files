use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Div, InteractiveElement as _, IntoElement as _, ParentElement as _,
    Styled as _, div,
};
use nova_files_api::Listing;
use nova_files_git_api::{Command, Event, Head, Repo};
use nova_files_shell::theme::{Colors, size};
use nova_files_shell::widgets::detail;

pub const BRANCH_LABEL: &str = "Branch";
pub const BRANCH_HEAD: &str = "head-branch";
const CHANGES_PREFIX: &str = "changes-";
const AHEAD: char = '↑';
const BEHIND: char = '↓';

#[derive(Default)]
pub struct Repos {
    listed: HashSet<PathBuf>,
    known: HashMap<PathBuf, Repo>,
}

impl Repos {
    pub fn ask(&mut self, listing: &Listing) -> Command {
        let folders: Vec<PathBuf> = listing
            .entries
            .iter()
            .map(|entry| listing.path_of(entry))
            .collect();
        self.listed = folders.iter().cloned().collect();
        self.known.retain(|folder, _| self.listed.contains(folder));
        Command::Repos(folders)
    }

    pub fn leave(&mut self) -> Option<Command> {
        if self.listed.is_empty() {
            return None;
        }
        self.listed.clear();
        self.known.clear();
        Some(Command::Repos(Vec::new()))
    }

    pub fn heard(&mut self, event: Event) {
        match event {
            Event::Repo {
                folder,
                repo: Some(repo),
            } if self.listed.contains(&folder) => {
                self.known.insert(folder, repo);
            }
            Event::Repo { folder, .. } => {
                self.known.remove(&folder);
            }
        }
    }

    pub fn of(&self, folder: &PathBuf) -> Option<&Repo> {
        self.known.get(folder)
    }
}

pub fn cell(repo: &Repo, name: &str, cx: &App) -> AnyElement {
    let colors = Colors::of(cx);
    let selector = format!("{CHANGES_PREFIX}{name}");
    let head = match &repo.head {
        Head::Branch(name) | Head::Detached(name) => name.clone(),
    };
    div()
        .flex()
        .items_center()
        .gap(size::MARK_GAP)
        .min_w_0()
        .when(repo.changed, |cell| {
            cell.child(dot(&colors).debug_selector(move || selector))
        })
        .child(detail(head, &colors).text_color(colors.text_soft))
        .children(counts(repo).map(|counts| detail(counts, &colors).flex_none()))
        .into_any_element()
}

fn dot(colors: &Colors) -> Div {
    div()
        .flex_none()
        .size(size::MARK_DOT)
        .rounded_full()
        .bg(colors.changed)
}

pub fn counts(repo: &Repo) -> Option<String> {
    let ahead = (repo.ahead > 0).then(|| format!("{AHEAD}{}", repo.ahead));
    let behind = (repo.behind > 0).then(|| format!("{BEHIND}{}", repo.behind));
    let both: Vec<String> = ahead.into_iter().chain(behind).collect();
    (!both.is_empty()).then(|| both.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_main(ahead: u32, behind: u32) -> Repo {
        Repo {
            head: Head::Branch("main".to_owned()),
            changed: false,
            ahead,
            behind,
        }
    }

    #[test]
    fn counts_show_only_what_is_not_zero() {
        assert_eq!(counts(&on_main(0, 0)), None);
        assert_eq!(counts(&on_main(2, 0)).as_deref(), Some("↑2"));
        assert_eq!(counts(&on_main(0, 3)).as_deref(), Some("↓3"));
        assert_eq!(counts(&on_main(2, 1)).as_deref(), Some("↑2 ↓1"));
    }
}
