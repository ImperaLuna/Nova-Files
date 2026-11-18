use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::defaults::{App, Associations, Places, installed};
use super::{opens_files, types_of};
use crate::mime::MimeTypes;

pub const FOLDER_TYPE: &str = "inode/directory";

#[derive(Debug, PartialEq, Eq)]
pub struct Listed {
    pub app: App,
    pub default: bool,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub struct Shared {
    pub ancestry: Vec<String>,
    pub same: bool,
}

pub fn shared_types(paths: &[&Path], types: &MimeTypes) -> Shared {
    let mut each = paths.iter().map(|path| ancestry(path, types));
    let Some(first) = each.next() else {
        return Shared::default();
    };
    let others: Vec<Vec<String>> = each.collect();
    let sets: Vec<HashSet<&String>> = others.iter().map(|other| other.iter().collect()).collect();
    let Some(at) = first
        .iter()
        .position(|mime| sets.iter().all(|set| set.contains(mime)))
    else {
        return Shared::default();
    };
    Shared {
        same: at == 0 && others.iter().all(|other| other.first() == first.first()),
        ancestry: first[at..].to_vec(),
    }
}

fn ancestry(path: &Path, types: &MimeTypes) -> Vec<String> {
    if fs::metadata(path).is_ok_and(|found| found.is_dir()) {
        types.ancestry(FOLDER_TYPE)
    } else {
        types_of(path, types)
    }
}

pub fn apps(types: &[&str], with_default: bool, places: &Places) -> Vec<Listed> {
    let associations = Associations::read(places);
    let default = with_default
        .then(|| associations.default_app(types, places))
        .flatten();
    let mut seen: HashSet<String> = default.iter().map(|app| app.id.clone()).collect();
    let others = types
        .iter()
        .flat_map(|mime| associations.ids_for(mime))
        .filter(|id| seen.insert((*id).to_owned()))
        .filter_map(|id| installed(id, places))
        .map(|app| Listed {
            app,
            default: false,
        });
    default
        .map(|app| Listed { app, default: true })
        .into_iter()
        .chain(others)
        .filter(|listed| opens_files(&listed.app))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    use crate::tree::Tree;

    trait Apps {
        fn app(&self, id: &str, extra: &str) -> &Self;
        fn ids(&self, types: &[&str]) -> Vec<(String, bool)>;
    }

    impl Apps for Tree {
        fn app(&self, id: &str, extra: &str) -> &Self {
            self.write(
                &format!("share/applications/{id}"),
                &format!("[Desktop Entry]\nType=Application\nName={id}\nExec=app %f\n{extra}"),
            )
        }

        fn ids(&self, types: &[&str]) -> Vec<(String, bool)> {
            apps(types, true, &self.places())
                .into_iter()
                .map(|listed| (listed.app.id, listed.default))
                .collect()
        }
    }

    fn found(ids: &[(&str, bool)]) -> Vec<(String, bool)> {
        ids.iter()
            .map(|&(id, default)| (id.to_owned(), default))
            .collect()
    }

    #[test]
    fn the_default_comes_first_then_each_types_apps_best_first() {
        let tree = Tree::new("apps-order");
        for id in [
            "cached.desktop",
            "added.desktop",
            "default.desktop",
            "text.desktop",
            "removed.desktop",
            "vim.desktop",
        ] {
            tree.app(id, "");
        }
        tree.app("vim.desktop", "Terminal=true\n")
            .write(
                "config/mimeapps.list",
                "[Default Applications]\ntext/plain=default.desktop\n[Added Associations]\ntext/x-rust=added.desktop;missing.desktop\n[Removed Associations]\ntext/x-rust=removed.desktop\n",
            )
            .write(
                "share/applications/mimeinfo.cache",
                "[MIME Cache]\ntext/x-rust=cached.desktop;removed.desktop;added.desktop;vim.desktop\ntext/plain=text.desktop;default.desktop\n",
            );
        assert_eq!(
            tree.ids(&["text/x-rust", "text/plain"]),
            found(&[
                ("added.desktop", true),
                ("cached.desktop", false),
                ("default.desktop", false),
                ("text.desktop", false),
            ])
        );
        assert_eq!(tree.ids(&[]), found(&[]));
        let without_default: Vec<bool> = apps(&["text/x-rust"], false, &tree.places())
            .iter()
            .map(|listed| listed.default)
            .collect();
        assert_eq!(without_default, [false, false]);
    }

    #[test]
    fn a_default_that_runs_in_a_terminal_or_takes_no_file_is_left_out() {
        let tree = Tree::new("apps-terminal");
        tree.app("vim.desktop", "Terminal=true\n")
            .app("gui.desktop", "")
            .write(
                "share/applications/fileless.desktop",
                "[Desktop Entry]\nType=Application\nExec=app --new\n",
            )
            .write(
                "config/mimeapps.list",
                "[Default Applications]\ntext/plain=vim.desktop\n[Added Associations]\ntext/plain=fileless.desktop;gui.desktop\n",
            );
        assert_eq!(tree.ids(&["text/plain"]), found(&[("gui.desktop", false)]));
    }

    #[test]
    fn files_share_the_nearest_type_all_of_them_are() {
        let tree = Tree::new("apps-shared");
        tree.write(
            "mime/globs2",
            "50:image/png:*.png\n50:image/jpeg:*.jpg\n50:image/x-nova-moving:*.apng\n50:text/plain:*.txt\n",
        )
        .write(
            "mime/subclasses",
            "image/png image/x-nova-picture\nimage/jpeg image/x-nova-picture\nimage/x-nova-moving image/png\n",
        );
        fs::create_dir_all(tree.0.join("folder")).unwrap();
        let types = MimeTypes::load(std::slice::from_ref(&tree.0));
        let shared = |names: &[&str]| {
            let paths: Vec<PathBuf> = names.iter().map(|name| tree.0.join(name)).collect();
            let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
            shared_types(&paths, &types)
        };
        let same = shared(&["a.png", "b.png"]);
        assert_eq!(same.ancestry[..2], ["image/png", "image/x-nova-picture"]);
        assert!(same.same);
        let mixed = shared(&["a.png", "b.jpg"]);
        assert_eq!(mixed.ancestry[0], "image/x-nova-picture");
        assert!(!mixed.same);
        let kind = shared(&["a.png", "b.apng"]);
        assert_eq!(kind.ancestry[0], "image/png");
        assert!(!kind.same);
        assert_eq!(shared(&["folder"]).ancestry[0], FOLDER_TYPE);
        assert!(shared(&["folder"]).same);
        assert_eq!(shared(&["a.png", "untyped"]), Shared::default());
        assert_eq!(shared(&[]), Shared::default());
    }
}
