//! The drives and volumes, as the program's [`DriveService`] reports them,
//! sent to the clients that asked each time they change.
//!
//! The service arrives once, at start-up, and reads nothing until the
//! first client asks ([`Store::listen`]); the core then starts it
//! following the system's drives. Until its first list arrives, clients
//! that ask wait for it, so none sees an empty list flash by. Without a
//! service, a client gets an empty list at once, and the real one if a
//! service arrives later.
//!
//! The service's thread hands each list to [`seen`], which keeps it and
//! tells every listening client under one lock, so a client that asks at
//! that moment gets either the old list and then the new one, or the new
//! one alone. A list equal to the last sends nothing.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use nova_files_api::{ClientGone, ClientId, DriveList, DriveService, Event, EventSink};

/// The drives and who hears about them.
#[derive(Default)]
pub struct Store {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    /// What watches the drives; `None` until the program hands it over.
    service: Option<Arc<dyn DriveService>>,
    /// Whether the service was told to follow the drives.
    following: bool,
    /// The newest list; `None` until the service's first one arrives.
    list: Option<Arc<DriveList>>,
    /// The clients that asked for the drives.
    listeners: BTreeMap<ClientId, Arc<dyn EventSink>>,
}

impl Store {
    /// Keeps `service` to watch the drives with, and starts it following
    /// them if a client already asked. Only the first one counts; a later
    /// one is dropped.
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

    /// Has `service` send its lists here, outside the lock, in case it
    /// reports its first list before it returns.
    fn start(&self, service: &Arc<dyn DriveService>) {
        let shared = Arc::downgrade(&self.shared);
        service.follow(Box::new(move |list| seen(&shared, list)));
    }

    /// Sends `client` the drives through `events`, and again each time
    /// they change, until [`Self::forget`]. The first client to ask starts
    /// the service following the drives.
    ///
    /// # Errors
    /// [`ClientGone`] when the client stopped listening; it then isn't
    /// kept.
    pub fn listen(&self, client: ClientId, events: &Arc<dyn EventSink>) -> Result<(), ClientGone> {
        let start = {
            let mut shared = lock(&self.shared);
            match (&shared.list, &shared.service) {
                (Some(list), _) => {
                    events.deliver(Event::Drives(Arc::clone(list)))?;
                }
                (None, None) => {
                    // No service to ask yet, so no drives known.
                    events.deliver(Event::Drives(Arc::new(DriveList::default())))?;
                }
                // The first list is on its way; it goes to every listener.
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

    /// Stops sending `client` the drives.
    pub fn forget(&self, client: ClientId) {
        lock(&self.shared).listeners.remove(&client);
    }
}

/// Keeps `list` as the newest and tells every listener, unless it is the
/// list they already have. Does nothing once the store is gone.
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

/// The shared state, even after a thread panicked holding it: each change
/// to it is whole before the lock is let go.
fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver, Sender};

    use nova_files_api::{Drive, DriveId, Volume, VolumeId};

    use super::*;

    /// A service the test drives by hand: it keeps the `seen` it was
    /// given, and counts how often it was told to follow.
    #[derive(Default)]
    struct Hand {
        seen: Mutex<Vec<Seen>>,
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

    /// What the store holds of a [`Hand`]; the test keeps the other end.
    struct Handle(Arc<Hand>);

    impl DriveService for Handle {
        fn follow(&self, seen: Seen) {
            self.0.seen.lock().unwrap().push(seen);
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
            }],
        }
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
