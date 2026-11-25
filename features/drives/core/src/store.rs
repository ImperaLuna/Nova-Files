use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread;

use nova_files_drives_api::{DriveAction, DriveFailure, DriveList, DriveService, Event};
use nova_files_messages::{ClientGone, ClientId, EventSink};

const ACTION_THREAD: &str = "nova-files-drive-action";

#[derive(Default)]
pub struct Store {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    service: Option<Arc<dyn DriveService>>,
    following: bool,
    list: Option<Arc<DriveList>>,
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
}

impl Store {
    pub fn offer(&self, service: Arc<dyn DriveService>) {
        let start = {
            let mut shared = lock(&self.shared);
            if shared.service.is_some() {
                return;
            }
            shared.service = Some(service);
            shared.following = !shared.listeners.is_empty();
            shared.following.then(|| shared.service.clone()).flatten()
        };
        if let Some(service) = start {
            self.start(&service);
        }
    }

    fn start(&self, service: &Arc<dyn DriveService>) {
        let shared = Arc::downgrade(&self.shared);
        service.follow(Box::new(move |list| seen(&shared, list)));
    }

    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) -> Result<(), ClientGone> {
        let start = {
            let mut shared = lock(&self.shared);
            match (&shared.list, &shared.service) {
                (Some(list), _) => {
                    events.deliver(Event::Drives(Arc::clone(list)))?;
                }
                (None, None) => {
                    events.deliver(Event::Drives(Arc::new(DriveList::default())))?;
                }
                (None, Some(_)) => {}
            }
            shared.listeners.insert(client, Arc::clone(events));
            if shared.following {
                None
            } else {
                shared.following = shared.service.is_some();
                shared.service.clone()
            }
        };
        if let Some(service) = start {
            self.start(&service);
        }
        Ok(())
    }

    pub fn act(&self, events: &Arc<dyn EventSink>, action: DriveAction) -> Result<(), ClientGone> {
        let Some(service) = lock(&self.shared).service.clone() else {
            return events.deliver(Event::DriveDone {
                action,
                result: Err(DriveFailure::NoService),
            });
        };
        let answer = Arc::clone(events);
        let asked = action.clone();
        let spawned = thread::Builder::new()
            .name(ACTION_THREAD.to_owned())
            .spawn(move || {
                let result = service.act(&asked);
                answer
                    .deliver(Event::DriveDone {
                        action: asked,
                        result,
                    })
                    .ok();
            });
        match spawned {
            Ok(_) => Ok(()),
            Err(error) => events.deliver(Event::DriveDone {
                action,
                result: Err(DriveFailure::Failed(error.to_string())),
            }),
        }
    }

    pub fn forget(&self, client: ClientId) {
        lock(&self.shared).listeners.remove(&client);
    }
}

fn seen(shared: &Weak<Mutex<Shared>>, list: DriveList) {
    let Some(shared) = shared.upgrade() else {
        return;
    };
    let mut shared = lock(&shared);
    if shared.list.as_deref() == Some(&list) {
        return;
    }
    let list = Arc::new(list);
    shared.list = Some(Arc::clone(&list));
    shared
        .listeners
        .retain(|_, events| events.deliver(Event::Drives(Arc::clone(&list))).is_ok());
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver, Sender};

    use nova_files_drives_api::{Drive, DriveId, Volume, VolumeId};

    use super::*;

    #[derive(Default)]
    struct Hand {
        seen: Mutex<Vec<Seen>>,
        acted: Mutex<Vec<DriveAction>>,
    }

    type Seen = Box<dyn Fn(DriveList) + Send + Sync>;

    impl Hand {
        fn report(&self, list: &DriveList) {
            for seen in self.seen.lock().unwrap().iter() {
                seen(list.clone());
            }
        }

        fn follows(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
    }

    struct Handle(Arc<Hand>);

    impl DriveService for Handle {
        fn follow(&self, seen: Seen) {
            self.0.seen.lock().unwrap().push(seen);
        }

        fn act(&self, action: &DriveAction) -> Result<Option<PathBuf>, DriveFailure> {
            self.0.acted.lock().unwrap().push(action.clone());
            match action {
                DriveAction::Mount(_) | DriveAction::Unlock(..) => {
                    Ok(Some(PathBuf::from("/run/media/me/Photos")))
                }
                DriveAction::Unmount(_) => Err(DriveFailure::Busy),
                DriveAction::Eject(_) => Ok(None),
            }
        }
    }

    fn client() -> (Arc<dyn EventSink>, Receiver<Event>) {
        let (sender, receiver): (Sender<Event>, _) = mpsc::channel();
        (Arc::new(sender), receiver)
    }

    fn stick() -> DriveList {
        DriveList {
            drives: vec![Drive {
                id: DriveId("stick".to_owned()),
                name: "Stick".to_owned(),
                removable: true,
                ejectable: true,
            }],
            volumes: vec![Volume {
                id: VolumeId("stick1".to_owned()),
                drive: Some(DriveId("stick".to_owned())),
                label: "Photos".to_owned(),
                size: 1,
                filesystem: "vfat".to_owned(),
                mount_point: Some(PathBuf::from("/run/media/me/Photos")),
                device: "/dev/sdb1".into(),
                read_only: false,
                locked: false,
            }],
        }
    }

    fn mount() -> DriveAction {
        DriveAction::Mount(VolumeId("stick1".to_owned()))
    }

    fn done(receiver: &Receiver<Event>) -> Event {
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
    }

    fn drives(receiver: &Receiver<Event>) -> Vec<DriveList> {
        receiver
            .try_iter()
            .map(|event| match event {
                Event::Drives(list) => (*list).clone(),
                other => panic!("not drives: {other:?}"),
            })
            .collect()
    }

    fn with_hand() -> (Store, Arc<Hand>) {
        let store = Store::default();
        let hand = Arc::new(Hand::default());
        store.offer(Arc::new(Handle(Arc::clone(&hand))));
        (store, hand)
    }

    #[test]
    fn without_a_service_a_client_hears_of_no_drives_at_once() {
        let store = Store::default();
        let (events, receiver) = client();
        store.listen(ClientId(1), &events).unwrap();
        assert_eq!(drives(&receiver), vec![DriveList::default()]);
    }

    #[test]
    fn the_first_client_waits_for_the_first_list_then_hears_each_change() {
        let (store, hand) = with_hand();
        let (events, receiver) = client();
        store.listen(ClientId(1), &events).unwrap();
        assert_eq!(hand.follows(), 1);
        assert_eq!(drives(&receiver), vec![]);

        hand.report(&DriveList::default());
        hand.report(&stick());
        assert_eq!(drives(&receiver), vec![DriveList::default(), stick()]);
    }

    #[test]
    fn the_service_follows_once_and_a_later_client_gets_the_newest_list() {
        let (store, hand) = with_hand();
        let (first, _first_events) = client();
        store.listen(ClientId(1), &first).unwrap();
        hand.report(&stick());

        let (second, receiver) = client();
        store.listen(ClientId(2), &second).unwrap();
        assert_eq!(hand.follows(), 1);
        assert_eq!(drives(&receiver), vec![stick()]);
    }

    #[test]
    fn the_same_list_again_sends_nothing() {
        let (store, hand) = with_hand();
        let (events, receiver) = client();
        store.listen(ClientId(1), &events).unwrap();
        hand.report(&stick());
        hand.report(&stick());
        assert_eq!(drives(&receiver), vec![stick()]);
    }

    #[test]
    fn a_forgotten_client_hears_nothing_more() {
        let (store, hand) = with_hand();
        let (events, receiver) = client();
        store.listen(ClientId(1), &events).unwrap();
        store.forget(ClientId(1));
        hand.report(&stick());
        assert_eq!(drives(&receiver), vec![]);
    }

    #[test]
    fn a_client_that_stopped_listening_is_dropped() {
        let (store, hand) = with_hand();
        let (events, receiver) = client();
        store.listen(ClientId(1), &events).unwrap();
        drop(receiver);
        hand.report(&stick());
        assert!(lock(&store.shared).listeners.is_empty());
    }

    #[test]
    fn a_client_gone_before_its_first_list_is_not_kept() {
        let store = Store::default();
        let (events, receiver) = client();
        drop(receiver);
        assert_eq!(store.listen(ClientId(1), &events), Err(ClientGone));
        assert!(lock(&store.shared).listeners.is_empty());
    }

    #[test]
    fn a_service_offered_late_reaches_clients_that_already_asked() {
        let store = Store::default();
        let (events, receiver) = client();
        store.listen(ClientId(1), &events).unwrap();
        let hand = Arc::new(Hand::default());
        store.offer(Arc::new(Handle(Arc::clone(&hand))));
        assert_eq!(hand.follows(), 1);
        hand.report(&stick());
        assert_eq!(drives(&receiver), vec![DriveList::default(), stick()]);
    }

    #[test]
    fn a_service_offered_before_anyone_asks_waits() {
        let (_store, hand) = with_hand();
        assert_eq!(hand.follows(), 0);
    }

    #[test]
    fn an_action_answers_the_client_that_asked_with_the_service_s_result() {
        let (store, hand) = with_hand();
        let (events, receiver) = client();
        store.act(&events, mount()).unwrap();
        assert_eq!(
            done(&receiver),
            Event::DriveDone {
                action: mount(),
                result: Ok(Some(PathBuf::from("/run/media/me/Photos"))),
            }
        );
        let unmount = DriveAction::Unmount(VolumeId("stick1".to_owned()));
        store.act(&events, unmount.clone()).unwrap();
        assert_eq!(
            done(&receiver),
            Event::DriveDone {
                action: unmount.clone(),
                result: Err(DriveFailure::Busy),
            }
        );
        assert_eq!(*hand.acted.lock().unwrap(), vec![mount(), unmount]);
    }

    #[test]
    fn without_a_service_an_action_fails_at_once() {
        let store = Store::default();
        let (events, receiver) = client();
        store.act(&events, mount()).unwrap();
        assert_eq!(
            receiver.try_recv().unwrap(),
            Event::DriveDone {
                action: mount(),
                result: Err(DriveFailure::NoService),
            }
        );
        drop(receiver);
        assert_eq!(store.act(&events, mount()), Err(ClientGone));
    }

    #[test]
    fn only_the_first_service_counts() {
        let (store, hand) = with_hand();
        let later = Arc::new(Hand::default());
        store.offer(Arc::new(Handle(Arc::clone(&later))));
        let (events, _receiver) = client();
        store.listen(ClientId(1), &events).unwrap();
        assert_eq!((hand.follows(), later.follows()), (1, 0));
    }

    #[test]
    fn lists_after_the_store_is_gone_go_nowhere() {
        let (store, hand) = with_hand();
        let (events, receiver) = client();
        store.listen(ClientId(1), &events).unwrap();
        drop(store);
        hand.report(&stick());
        assert_eq!(drives(&receiver), vec![]);
    }
}
