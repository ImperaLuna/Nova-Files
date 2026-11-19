use std::sync::{Arc, OnceLock};

use nova_files::MimeTypes;
use nova_files_api::FileTypes;
use nova_files_dbus::Picker;
use nova_files_picker_api::{FilterRule, PickId, PickRequest};
use nova_files_picker_ui::{PickWindow, closes};
use nova_files_window::{Request, RequestSender};

pub struct UiPicker {
    requests: RequestSender,
    load: fn() -> MimeTypes,
    types: OnceLock<Arc<dyn FileTypes>>,
}

impl UiPicker {
    pub const fn new(requests: RequestSender) -> Self {
        Self::with_loader(requests, MimeTypes::from_env)
    }

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
        let ask = Box::new(PickWindow(Box::new(request)));
        self.requests.send(Request::Open(ask));
    }

    fn close(&self, id: PickId) {
        self.requests.send(Request::Close(closes(id)));
    }
}

#[cfg(test)]
mod tests {
    use nova_files::MimeTypes;
    use nova_files_dbus::Picker as _;
    use nova_files_picker_api::{FileFilter, FilterRule, PickId, PickKind, PickReply, PickRequest};
    use nova_files_picker_ui::PickOpening;
    use nova_files_shell::Opening;
    use nova_files_window::Request;

    use super::UiPicker;

    fn picked(request: Option<Request>) -> Option<Opening> {
        match request {
            Some(Request::Open(ask)) => Some(ask.opening()),
            _ => None,
        }
    }

    fn pick_id(opening: &Opening) -> Option<PickId> {
        opening.get::<PickOpening>().map(|pick| pick.id)
    }

    fn has_types(opening: &Opening) -> bool {
        opening
            .get::<PickOpening>()
            .and_then(|pick| {
                pick.request
                    .borrow()
                    .as_ref()
                    .map(|request| request.types.is_some())
            })
            .unwrap_or_default()
    }

    #[test]
    fn picks_and_closes_reach_the_ui() {
        let (sender, received) = nova_files_window::channel();
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
            parent: None,
            types: None,
            reply: PickReply::new(drop),
        });
        picker.close(PickId(4));
        drop(picker);

        let opening = picked(received.wait()).unwrap();
        assert_eq!(pick_id(&opening), Some(PickId(4)));
        let Some(Request::Close(when)) = received.wait() else {
            panic!("expected a close");
        };
        assert!(when(&opening));
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
            parent: None,
            types: None,
            reply: PickReply::new(drop),
        }
    }

    #[test]
    fn only_a_pick_with_mime_rules_gets_the_database() {
        let (sender, received) = nova_files_window::channel();
        let picker = UiPicker::with_loader(sender, || MimeTypes::load(&[]));

        picker.pick(open_with(vec![FilterRule::Glob("*.pdf".to_owned())]));
        picker.pick(open_with(vec![FilterRule::MimeType("image/*".to_owned())]));
        drop(picker);

        assert!(!has_types(&picked(received.wait()).unwrap()));
        assert!(has_types(&picked(received.wait()).unwrap()));
    }
}
