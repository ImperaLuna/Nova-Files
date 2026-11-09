//! Hands the file picker portal's requests to the UI: a pick opens a
//! picker window, a close closes it (see `nova_files_ui::Request`).
//!
//! A pick whose filters name MIME types gets the system's MIME database,
//! read on the first such pick and kept, so the window can tell which
//! files fit without touching the disk.

use std::sync::{Arc, OnceLock};

use nova_files_api::{FileTypes, FilterRule, PickId, PickRequest};
use nova_files_core::MimeTypes;
use nova_files_dbus::Picker;
use nova_files_ui::{Request, RequestSender};

pub struct UiPicker {
    requests: RequestSender,
    load: fn() -> MimeTypes,
    types: OnceLock<Arc<dyn FileTypes>>,
}

impl UiPicker {
    /// Reads the system's MIME database when a pick first needs it.
    pub const fn new(requests: RequestSender) -> Self {
        Self::with_loader(requests, MimeTypes::from_env)
    }

    /// Gets the MIME database from `load` instead, which tests point at
    /// a temporary folder.
    pub const fn with_loader(requests: RequestSender, load: fn() -> MimeTypes) -> Self {
        Self {
            requests,
            load,
            types: OnceLock::new(),
        }
    }
}

impl Picker for UiPicker {
    fn pick(&self, mut request: PickRequest) {
        let wants_types = request
            .filters
            .iter()
            .flat_map(|filter| &filter.rules)
            .any(|rule| matches!(rule, FilterRule::MimeType(_)));
        if wants_types {
            let types = self.types.get_or_init(|| Arc::new((self.load)()));
            request.types = Some(Arc::clone(types));
        }
        self.requests.send(Request::Pick(Box::new(request)));
    }

    fn close(&self, id: PickId) {
        self.requests.send(Request::ClosePick(id));
    }
}

#[cfg(test)]
mod tests {
    use nova_files_api::{FileFilter, FilterRule, PickId, PickKind, PickReply, PickRequest};
    use nova_files_core::MimeTypes;
    use nova_files_dbus::Picker as _;
    use nova_files_ui::Request;

    use super::UiPicker;

    #[test]
    fn picks_and_closes_reach_the_ui() {
        let (sender, received) = nova_files_ui::channel();
        let picker = UiPicker::new(sender);

        picker.pick(PickRequest {
            id: PickId(4),
            kind: PickKind::Save {
                name: "a.txt".into(),
            },
            title: "Save".to_owned(),
            accept_label: None,
            filters: Vec::new(),
            filter: None,
            folder: None,
            types: None,
            reply: PickReply::new(drop),
        });
        picker.close(PickId(4));
        // With the sender gone, a close that sent nothing ends the wait.
        drop(picker);

        assert!(matches!(received.wait(), Some(Request::Pick(request)) if request.id == PickId(4)));
        assert!(matches!(
            received.wait(),
            Some(Request::ClosePick(PickId(4)))
        ));
    }

    fn open_with(rules: Vec<FilterRule>) -> PickRequest {
        PickRequest {
            id: PickId(5),
            kind: PickKind::Open {
                multiple: false,
                folders: false,
            },
            title: String::new(),
            accept_label: None,
            filters: vec![FileFilter {
                name: "Some".to_owned(),
                rules,
            }],
            filter: None,
            folder: None,
            types: None,
            reply: PickReply::new(drop),
        }
    }

    #[test]
    fn only_a_pick_with_mime_rules_gets_the_database() {
        let (sender, received) = nova_files_ui::channel();
        // No data folders, so nothing outside the test is read.
        let picker = UiPicker::with_loader(sender, || MimeTypes::load(&[]));

        picker.pick(open_with(vec![FilterRule::Glob("*.pdf".to_owned())]));
        picker.pick(open_with(vec![FilterRule::MimeType("image/*".to_owned())]));
        drop(picker);

        assert!(matches!(received.wait(), Some(Request::Pick(request)) if request.types.is_none()));
        assert!(matches!(received.wait(), Some(Request::Pick(request)) if request.types.is_some()));
    }
}
