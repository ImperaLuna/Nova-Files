use nova_files_api::{MessageType, missing_routes};

#[test]
fn every_command_a_front_sends_has_a_route_in_the_core() {
    let routed = nova_files_core::routed_types().unwrap();
    let sent: Vec<MessageType> = [
        nova_files_ui::sent_types(),
        nova_files::parts().sends(),
        nova_files_dbus::sent_types(),
    ]
    .concat();
    let missing: Vec<&str> = missing_routes(&sent, &routed)
        .iter()
        .map(MessageType::name)
        .collect();
    assert!(missing.is_empty(), "sent but routed nowhere: {missing:?}");
}

#[test]
fn every_event_the_core_sends_has_a_front_that_takes_it() {
    let taken: Vec<MessageType> = [
        nova_files_ui::taken_types(),
        nova_files::parts().takes(),
        nova_files_dbus::taken_types(),
    ]
    .concat();
    let emitted = nova_files_core::emitted_types();
    let missing: Vec<&str> = missing_routes(&emitted, &taken)
        .iter()
        .map(MessageType::name)
        .collect();
    assert!(
        missing.is_empty(),
        "sent by the core but taken by nothing: {missing:?}"
    );
}
