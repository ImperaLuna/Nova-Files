use gpui::SharedString;
use nova_files_api::{Entry, EntryKind};
use nova_files_picker_api::{FilterRule, PickRequest};

use crate::Mode;

const ALL_FILES_LABEL: &str = "All files";
const EVERY_NAME: &str = "*";

pub fn first(request: &PickRequest) -> Option<usize> {
    let count = request.filters.len();
    request
        .filter
        .filter(|&filter| filter < count)
        .or_else(|| (count > 0).then_some(0))
}

pub fn offers_all(request: &PickRequest) -> bool {
    request.filters.iter().any(|filter| {
        filter
            .rules
            .iter()
            .any(|rule| matches!(rule, FilterRule::Glob(glob) if glob == EVERY_NAME))
    })
}

pub fn label(request: &PickRequest, filter: Option<usize>) -> SharedString {
    filter
        .and_then(|filter| request.filters.get(filter))
        .map_or_else(
            || ALL_FILES_LABEL.into(),
            |filter| filter.name.clone().into(),
        )
}

pub fn is_chosen(chosen: Option<usize>, filter: Option<usize>) -> bool {
    chosen == filter
}

pub fn dims(request: &PickRequest, mode: Mode, filter: Option<usize>, entry: &Entry) -> bool {
    if entry.kind != EntryKind::File {
        return false;
    }
    if mode == Mode::Folders {
        return true;
    }
    filter
        .and_then(|filter| request.filters.get(filter))
        .is_some_and(|filter| !filter.admits(&entry.name, request.types.as_deref()))
}
