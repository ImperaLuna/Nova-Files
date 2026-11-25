#![forbid(unsafe_code)]

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    Styled as _, Window, div,
};
use nova_files_api::{Entry, Listing};
use nova_files_git_api::{Command, Event, Mark, Marks};
use nova_files_shell::theme::{Colors, size, text};
use nova_files_shell::widgets::detail;
use nova_files_shell::{Host, Part, Scene, Slots};

const CHANGED: &str = "M";
const NEW: &str = "U";
const MARK_PREFIX: &str = "git-mark-";
pub const BRANCH_CHIP: &str = "git-branch";
pub const CHIP_CHANGES: &str = "git-branch-changes";

pub struct GitPart {
    host: Host,
    asked: Option<PathBuf>,
    marks: Option<Marks>,
}

impl GitPart {
    pub fn mark_of(&self, name: &str) -> Option<Mark> {
        self.marks
            .as_ref()?
            .entries
            .get(std::ffi::OsStr::new(name))
            .copied()
    }

    fn mark(&self, entry: &Entry) -> Option<Mark> {
        self.marks.as_ref()?.entries.get(&entry.name).copied()
    }

    fn listed(&mut self, listing: &Listing, _: bool, cx: &mut Context<Self>) {
        let folder = (listing.folders.is_none() && !listing.path.as_os_str().is_empty())
            .then(|| listing.path.clone());
        if folder == self.asked {
            return;
        }
        self.asked.clone_from(&folder);
        self.marks = None;
        self.host.link.send(Command::Marks(folder)).ok();
        cx.notify();
    }

    fn heard(&mut self, event: Event, cx: &mut Context<Self>) {
        if let Event::Marks { folder, marks } = event
            && self.asked.as_ref() == Some(&folder)
        {
            self.marks = marks;
            cx.notify();
        }
    }
}

fn row_mark(part: &GitPart, entry: &Entry, cx: &App) -> Option<AnyElement> {
    let colors = Colors::of(cx);
    let (letter, tint) = match part.mark(entry)? {
        Mark::Changed => (CHANGED, colors.changed),
        Mark::New => (NEW, colors.added),
        Mark::Ignored => return None,
    };
    let selector = format!("{MARK_PREFIX}{}", entry.name.to_string_lossy());
    Some(
        div()
            .debug_selector(move || selector)
            .flex_none()
            .w(size::MARK_WIDTH)
            .text_size(text::LABEL)
            .font_weight(text::LABEL_WEIGHT)
            .text_color(tint)
            .text_center()
            .child(letter)
            .into_any_element(),
    )
}

fn marks_shown(part: &GitPart, scene: &Scene, _: &App) -> bool {
    part.marks.is_some()
        && part
            .asked
            .as_ref()
            .is_some_and(|asked| scene.in_folder(asked))
}

fn branch_chip(
    part: &mut GitPart,
    scene: &Scene,
    _: &mut Window,
    cx: &mut Context<GitPart>,
) -> Option<AnyElement> {
    if !marks_shown(part, scene, cx) {
        return None;
    }
    let repo = &part.marks.as_ref()?.repo;
    let colors = Colors::of(cx);
    let chip = div()
        .debug_selector(|| BRANCH_CHIP.to_owned())
        .flex()
        .flex_none()
        .items_center()
        .gap(size::MARK_GAP)
        .h(size::CHIP_HEIGHT)
        .px(size::CHIP_PAD)
        .when(repo.changed, |chip| {
            chip.child(
                div()
                    .debug_selector(|| CHIP_CHANGES.to_owned())
                    .flex_none()
                    .size(size::MARK_DOT)
                    .rounded_full()
                    .bg(colors.changed),
            )
        })
        .child(detail(repo.head.name().to_owned(), &colors).text_color(colors.text_soft))
        .children(
            repo.counts()
                .map(|counts| detail(counts, &colors).flex_none()),
        );
    Some(chip.into_any_element())
}

fn ignored(part: &GitPart, entry: &Entry, _: &App) -> bool {
    part.mark(entry) == Some(Mark::Ignored)
}

impl Part for GitPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self {
            host: host.clone(),
            asked: None,
            marks: None,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::heard);
        slots.sends::<Command>();
        slots.listed(Self::listed);
        slots.row_mark(marks_shown, row_mark);
        slots.dims(ignored);
        slots.path_chip(branch_chip);
    }
}
