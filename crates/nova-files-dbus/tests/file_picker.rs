//! The file picker against a private D-Bus daemon and a real core, called
//! as xdg-desktop-portal calls it: `OpenFile` and `SaveFile` hand the
//! request to the picker and answer what it picked; a `Close` on the
//! request object closes the picker and the call answers cancelled, as it
//! does when the picker drops the request unanswered; and a pick holds the
//! core until it is answered.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nova_files_api::{
    Connector, FileFilter, FilterRule, PickAnswer, PickId, PickKind, PickRequest,
};
use nova_files_dbus::{
    FILE_CHOOSER_INTERFACE, PORTAL_BUS_NAME, PORTAL_OBJECT_PATH, Picker, Service, Started,
};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

mod support;

use support::{PrivateBus, WAIT};

/// Where the portal would put the request object for this call.
const HANDLE: &str = "/org/freedesktop/portal/desktop/request/1_7/t1";
const REQUEST_INTERFACE: &str = "org.freedesktop.impl.portal.Request";

/// A short idle time for the core, and a wait past it.
const IDLE: Duration = Duration::from_millis(300);
const PAST_IDLE: Duration = Duration::from_millis(900);

type Answer = (u32, HashMap<String, OwnedValue>);

/// Hands each request and each close to the test.
struct TestPicker {
    requests: Sender<PickRequest>,
    closes: Sender<PickId>,
}

impl Picker for TestPicker {
    fn pick(&self, request: PickRequest) {
        self.requests.send(request).ok();
    }
    fn close(&self, id: PickId) {
        self.closes.send(id).ok();
    }
}

struct Setup {
    bus: PrivateBus,
    _service: Service,
    requests: Receiver<PickRequest>,
    closes: Receiver<PickId>,
}

fn setup(connector: &Connector) -> Setup {
    let bus = PrivateBus::start();
    let Started::Service(mut service) =
        nova_files_dbus::start(bus.bus(), connector.clone(), |_| {}).unwrap()
    else {
        panic!("the application name is free");
    };
    let (requests_sender, requests) = mpsc::channel();
    let (closes_sender, closes) = mpsc::channel();
    service.serve_file_picker(
        &bus.bus(),
        connector.clone(),
        TestPicker {
            requests: requests_sender,
            closes: closes_sender,
        },
    );
    assert!(service.serves_portal());
    Setup {
        bus,
        _service: service,
        requests,
        closes,
    }
}

fn client(bus: &PrivateBus) -> Connection {
    zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .method_timeout(WAIT)
        .build()
        .unwrap()
}

/// Calls `method` as the portal does, on a thread of its own, since it
/// returns only once the picker answers.
fn call(
    bus: &PrivateBus,
    method: &'static str,
    title: &'static str,
    options: Vec<(&'static str, OwnedValue)>,
) -> JoinHandle<Answer> {
    let client = client(bus);
    thread::spawn(move || {
        let chooser = Proxy::new(
            &client,
            PORTAL_BUS_NAME,
            PORTAL_OBJECT_PATH,
            FILE_CHOOSER_INTERFACE,
        )
        .unwrap();
        let handle = zbus::zvariant::ObjectPath::try_from(HANDLE).unwrap();
        let options: HashMap<&str, OwnedValue> = options.into_iter().collect();
        chooser
            .call(method, &(handle, "org.example.App", "", title, options))
            .unwrap()
    })
}

fn value(value: Value<'_>) -> OwnedValue {
    OwnedValue::try_from(value).unwrap()
}

fn uris(answer: &Answer) -> Vec<String> {
    answer.1["uris"].try_clone().unwrap().try_into().unwrap()
}

#[test]
fn open_file_answers_the_picked_files() {
    let core = nova_files_core::spawn().unwrap();
    let setup = setup(&core.connector);
    let pdf = ("PDF".to_owned(), vec![(0_u32, "*.pdf".to_owned())]);
    let pending = call(
        &setup.bus,
        "OpenFile",
        "Open a report",
        vec![
            ("multiple", value(Value::from(true))),
            ("filters", value(Value::from(vec![pdf]))),
        ],
    );

    let request = setup.requests.recv_timeout(WAIT).unwrap();
    assert_eq!(request.title, "Open a report");
    assert_eq!(
        request.kind,
        PickKind::Open {
            multiple: true,
            folders: false
        }
    );
    assert_eq!(
        request.filters,
        [FileFilter {
            name: "PDF".to_owned(),
            rules: vec![FilterRule::Glob("*.pdf".to_owned())]
        }]
    );
    request.reply.answer(PickAnswer::Picked {
        paths: vec![PathBuf::from("/srv/a.pdf"), PathBuf::from("/srv/b c.pdf")],
        filter: None,
    });

    let answer = pending.join().unwrap();
    assert_eq!(answer.0, 0);
    assert_eq!(
        uris(&answer),
        ["file:///srv/a.pdf", "file:///srv/b%20c.pdf"]
    );
}

#[test]
fn save_file_answers_the_path_to_save_at() {
    let core = nova_files_core::spawn().unwrap();
    let setup = setup(&core.connector);
    let pending = call(
        &setup.bus,
        "SaveFile",
        "Save the invoice",
        vec![("current_name", value(Value::from("invoice.pdf")))],
    );

    let request = setup.requests.recv_timeout(WAIT).unwrap();
    assert_eq!(
        request.kind,
        PickKind::Save {
            name: "invoice.pdf".to_owned()
        }
    );
    request.reply.answer(PickAnswer::Picked {
        paths: vec![PathBuf::from("/srv/invoice.pdf")],
        filter: None,
    });

    let answer = pending.join().unwrap();
    assert_eq!(answer.0, 0);
    assert_eq!(uris(&answer), ["file:///srv/invoice.pdf"]);
}

#[test]
fn a_picker_closed_unanswered_answers_cancelled() {
    let core = nova_files_core::spawn().unwrap();
    let setup = setup(&core.connector);
    let pending = call(&setup.bus, "OpenFile", "Open", Vec::new());

    drop(setup.requests.recv_timeout(WAIT).unwrap());

    let answer = pending.join().unwrap();
    assert_eq!(answer.0, 1);
    assert!(answer.1.is_empty());
}

#[test]
fn close_on_the_request_closes_the_picker() {
    let core = nova_files_core::spawn().unwrap();
    let setup = setup(&core.connector);
    let pending = call(&setup.bus, "OpenFile", "Open", Vec::new());
    let request = setup.requests.recv_timeout(WAIT).unwrap();

    let portal = client(&setup.bus);
    Proxy::new(&portal, PORTAL_BUS_NAME, HANDLE, REQUEST_INTERFACE)
        .unwrap()
        .call::<_, _, ()>("Close", &())
        .unwrap();

    assert_eq!(setup.closes.recv_timeout(WAIT), Ok(request.id));
    // The UI closes the window, which drops the request.
    drop(request);
    assert_eq!(pending.join().unwrap().0, 1);
    // The request object is gone with the pick.
    let gone = Proxy::new(&portal, PORTAL_BUS_NAME, HANDLE, REQUEST_INTERFACE)
        .unwrap()
        .call::<_, _, ()>("Close", &());
    assert!(gone.is_err());
}

#[test]
fn a_pick_holds_the_core_until_it_is_answered() {
    let core = nova_files_core::spawn_with_idle_exit(IDLE).unwrap();
    // Keeps the core up while the test sets up, as a window would.
    let (events, _received) = mpsc::channel();
    let window = core.connector.connect(events).unwrap();
    let setup = setup(&core.connector);
    let (ended, core_ended) = mpsc::channel();
    let thread = core.thread;
    thread::spawn(move || {
        thread.join().ok();
        ended.send(()).ok();
    });
    let pending = call(&setup.bus, "OpenFile", "Open", Vec::new());
    let request = setup.requests.recv_timeout(WAIT).unwrap();
    drop(window);

    assert!(core_ended.recv_timeout(PAST_IDLE).is_err());

    drop(request);
    assert_eq!(pending.join().unwrap().0, 1);
    core_ended.recv_timeout(WAIT).unwrap();
}
