use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Div, InteractiveElement as _, IntoElement as _, ParentElement as _,
    Styled as _, div,
};
use nova_files_api::Listing;
use nova_files_git_api::{Command, Event, Repo};
use nova_files_shell::theme::{Colors, size};
use nova_files_shell::widgets::detail;

pub const BRANCH_LABEL: &str = "Branch";
pub const BRANCH_HEAD: &str = "head-branch";
const CHANGES_PREFIX: &str = "changes-";

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
            Event::Marks { .. } => {}
        }
    }

    pub fn of(&self, folder: &PathBuf) -> Option<&Repo> {
        self.known.get(folder)
    }
}

pub fn cell(repo: &Repo, name: &str, cx: &App) -> AnyElement {
    let colors = Colors::of(cx);
    let selector = format!("{CHANGES_PREFIX}{name}");
    let head = repo.head.name().to_owned();
    div()
        .flex()
        .items_center()
        .gap(size::MARK_GAP)
        .min_w_0()
        .when(repo.changed, |cell| {
            cell.child(dot(&colors).debug_selector(move || selector))
        })
        .child(detail(head, &colors).text_color(colors.text_soft))
        .children(
            repo.counts()
                .map(|counts| detail(counts, &colors).flex_none()),
        )
        .into_any_element()
}

fn dot(colors: &Colors) -> Div {
    div()
        .flex_none()
        .size(size::MARK_DOT)
        .rounded_full()
        .bg(colors.changed)
}
