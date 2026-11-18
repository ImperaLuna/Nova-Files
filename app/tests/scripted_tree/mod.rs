use super::*;

impl Core {
    fn tree_listing(&mut self) -> (ListingId, PathBuf) {
        while self.try_recv().is_some() {}
        assert!(!self.tree_listings.is_empty(), "the tree asked for nothing");
        self.tree_listings.remove(0)
    }

    fn send_to_tree(&self, event: Event) {
        let (_, events) = self.tree.as_ref().unwrap();
        events.deliver(event).unwrap();
    }
}

fn folders(names: &[&str]) -> Arc<[Entry]> {
    names
        .iter()
        .zip(0..)
        .map(|(name, index)| Entry {
            id: EntryId(index),
            name: (*name).into(),
            kind: EntryKind::Folder,
        })
        .collect()
}

fn tree_opened(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let (view, cx, mut core) = window(cx);
    let (id, path) = core.next_listing().unwrap();
    core.send(Event::FolderListed(Listing {
        id,
        path,
        entries: folders(&["a", "b"]),
        folders: None,
    }));
    cx.run_until_parked();
    view.update(cx, |view, cx| view.toggle_tree(Path::new(SHOWN), cx));
    cx.run_until_parked();
    (view, cx, core)
}

fn shows(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.run_until_parked();
    cx.debug_bounds(selector).is_some()
}

#[gpui::test]
fn the_tree_takes_only_the_listing_it_asked_for(cx: &mut TestAppContext) {
    let (view, cx, mut core) = tree_opened(cx);
    let (id, path) = core.tree_listing();
    assert_eq!(path, Path::new(SHOWN));
    assert!(shows(cx, "tree-2"), "a and b under the root");
    let settled = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.tree_settled());
    assert!(!settled(cx), "the tree waits on its listing");

    core.send_to_tree(Event::FolderListed(Listing {
        id: ListingId(id.0 + 1),
        path: path.clone(),
        entries: folders(&["c"]),
        folders: None,
    }));
    assert!(shows(cx, "tree-2"), "an answer to another ask is dropped");
    assert!(!settled(cx));

    core.send_to_tree(Event::FolderListed(Listing {
        id,
        path,
        entries: folders(&["c"]),
        folders: None,
    }));
    assert!(shows(cx, "tree-1"));
    assert!(!shows(cx, "tree-2"), "c alone under the root");
    assert!(settled(cx));
}

#[gpui::test]
fn a_folder_the_tree_couldnt_read_loses_its_arrow(cx: &mut TestAppContext) {
    let (view, cx, mut core) = tree_opened(cx);
    let (root_id, path) = core.tree_listing();
    core.send_to_tree(Event::FolderListed(Listing {
        id: root_id,
        path,
        entries: folders(&["a", "b"]),
        folders: None,
    }));
    cx.run_until_parked();
    let a = Path::new(SHOWN).join("a");
    view.update(cx, |view, cx| view.toggle_tree(&a, cx));
    let (id, path) = core.tree_listing();
    assert_eq!(path, a);
    assert_ne!(id, root_id, "each ask has an id of its own");

    core.send_to_tree(Event::FolderUnreadable {
        id: ListingId(id.0 + 1),
        path: path.clone(),
        reason: Unreadable::Missing,
    });
    assert!(
        shows(cx, "tree-toggle-1"),
        "a refusal of another ask is dropped"
    );

    core.send_to_tree(Event::FolderUnreadable {
        id,
        path,
        reason: Unreadable::Missing,
    });
    assert!(!shows(cx, "tree-toggle-1"));
}
