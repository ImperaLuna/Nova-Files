use std::path::PathBuf;

use nova_files_places_api::BookmarkChange;

pub fn pin_at(pinned: &[PathBuf], at: usize, dropped: Vec<PathBuf>) -> Vec<BookmarkChange> {
    let mut order = pinned.to_vec();
    let mut at = at.min(order.len());
    let mut changes = Vec::new();
    for path in dropped {
        if let Some(from) = order.iter().position(|pin| *pin == path) {
            order.remove(from);
            if from < at {
                at -= 1;
            }
            order.insert(at, path.clone());
            if from != at {
                changes.push(BookmarkChange::Move {
                    path,
                    to: index(at),
                });
            }
        } else {
            order.insert(at, path.clone());
            changes.push(BookmarkChange::Add {
                path,
                label: None,
                at: Some(index(at)),
            });
        }
        at += 1;
    }
    changes
}

fn index(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn add(path: &str, at: u32) -> BookmarkChange {
        BookmarkChange::Add {
            path: path.into(),
            label: None,
            at: Some(at),
        }
    }

    fn moved(path: &str, to: u32) -> BookmarkChange {
        BookmarkChange::Move {
            path: path.into(),
            to,
        }
    }

    #[test]
    fn new_folders_are_added_in_order_at_the_line() {
        let pinned = paths(&["/a", "/b"]);
        assert_eq!(
            pin_at(&pinned, 1, paths(&["/x", "/y"])),
            [add("/x", 1), add("/y", 2)]
        );
    }

    #[test]
    fn a_pin_moved_down_lands_under_the_row_above_the_line() {
        let pinned = paths(&["/a", "/b", "/c"]);
        assert_eq!(pin_at(&pinned, 3, paths(&["/a"])), [moved("/a", 2)]);
        assert_eq!(pin_at(&pinned, 2, paths(&["/a"])), [moved("/a", 1)]);
    }

    #[test]
    fn a_pin_moved_up_lands_above_the_row_under_the_line() {
        let pinned = paths(&["/a", "/b", "/c"]);
        assert_eq!(pin_at(&pinned, 0, paths(&["/c"])), [moved("/c", 0)]);
    }

    #[test]
    fn a_pin_dropped_next_to_itself_stays() {
        let pinned = paths(&["/a", "/b", "/c"]);
        let none: Vec<BookmarkChange> = Vec::new();
        assert_eq!(pin_at(&pinned, 1, paths(&["/b"])), none);
        assert_eq!(pin_at(&pinned, 2, paths(&["/b"])), none);
    }
}
