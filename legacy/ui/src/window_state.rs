use std::sync::mpsc;
use std::time::Duration;

use nova_files_api::{Command, Connector, Event};
use nova_files_picker_ui::PickerState;

const FETCH_WAIT: Duration = Duration::from_millis(100);

pub fn fetch(connector: &Connector) -> PickerState {
    let (sink, events) = mpsc::channel();
    let Ok(connection) = connector.connect(sink) else {
        return PickerState::default();
    };
    if connection.send(Command::WindowState).is_err() {
        return PickerState::default();
    }
    match events.recv_timeout(FETCH_WAIT) {
        Ok(Event::WindowState(bytes)) => PickerState::parse(&bytes),
        _ => PickerState::default(),
    }
}
