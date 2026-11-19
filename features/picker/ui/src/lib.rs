#![forbid(unsafe_code)]

mod accept;
mod bar;
pub mod filter;
mod part;
mod save;
mod size;

pub use part::{Conflict, PickerPart};
pub use size::PickerState;

use std::ffi::OsStr;
use std::path::{Component, Path};

use gpui::{ForeignParent, TitlebarOptions};
use nova_files_picker_api::{ParentWindow, PickKind, PickRequest};

const MNEMONIC: char = '_';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Files,
    Folders,
    Save,
}

impl Mode {
    pub const fn of(kind: &PickKind) -> Self {
        match kind {
            PickKind::Open { folders: false, .. } => Self::Files,
            PickKind::Open { folders: true, .. } => Self::Folders,
            PickKind::Save { .. } => Self::Save,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Files | Self::Folders => "Open",
            Self::Save => "Save",
        }
    }
}

pub fn titlebar(request: &PickRequest) -> TitlebarOptions {
    let title = if request.title.is_empty() {
        Mode::of(&request.kind).label()
    } else {
        &request.title
    };
    TitlebarOptions {
        title: Some(title.to_owned().into()),
        ..TitlebarOptions::default()
    }
}

pub fn foreign_parent(parent: &ParentWindow) -> ForeignParent {
    match parent {
        ParentWindow::Wayland(handle) => ForeignParent::Wayland(handle.clone()),
        ParentWindow::X11(id) => ForeignParent::X11(*id),
    }
}

pub fn accept_label(request: &PickRequest) -> String {
    request.accept_label.as_deref().map_or_else(
        || Mode::of(&request.kind).label().to_owned(),
        without_mnemonic,
    )
}

fn without_mnemonic(label: &str) -> String {
    let mut plain = String::with_capacity(label.len());
    let mut chars = label.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            (MNEMONIC, Some(MNEMONIC)) => {
                plain.push(MNEMONIC);
                chars.next();
            }
            (MNEMONIC, _) => {}
            (c, _) => plain.push(c),
        }
    }
    plain
}

pub fn is_plain_name(name: &OsStr) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(part)), None) if part == name
    )
}

pub fn names_text(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        names => names
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests;
