//! Hands the file picker portal's requests to the UI: a pick opens a
//! picker window, a close closes it (see `nova_files_ui::Request`).

use nova_files_api::{PickId, PickRequest};
use nova_files_dbus::Picker;
use nova_files_ui::{Request, RequestSender};

pub struct UiPicker(pub RequestSender);

impl Picker for UiPicker {
    fn pick(&self, request: PickRequest) {
        self.0.send(Request::Pick(Box::new(request)));
    }

    fn close(&self, id: PickId) {
        self.0.send(Request::ClosePick(id));
    }
}

#[cfg(test)]
mod tests {
    use nova_files_api::{PickId, PickKind, PickReply, PickRequest};
    use nova_files_dbus::Picker as _;
    use nova_files_ui::Request;

    use super::UiPicker;

    #[test]
    fn picks_and_closes_reach_the_ui() {
        let (sender, received) = nova_files_ui::channel();
        let picker = UiPicker(sender);

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
}
