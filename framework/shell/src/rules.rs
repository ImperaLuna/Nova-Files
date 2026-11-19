use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::SharedString;

use crate::assets::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Views {
    #[default]
    Any,
    DetailsOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FolderRules {
    pub read_only: bool,
    pub views: Views,
    pub replaces_menu: bool,
    pub trashed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderName {
    pub label: SharedString,
    pub icon: Icon,
    pub empty_when_missing: bool,
}

impl FolderRules {
    #[must_use]
    pub fn with(self, other: Self) -> Self {
        Self {
            read_only: self.read_only || other.read_only,
            views: if self.views == Views::DetailsOnly || other.views == Views::DetailsOnly {
                Views::DetailsOnly
            } else {
                Views::Any
            },
            replaces_menu: self.replaces_menu || other.replaces_menu,
            trashed: self.trashed || other.trashed,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RulesBook(Rc<RefCell<Book>>);

#[derive(Debug, Default)]
struct Book {
    claims: Vec<Claim>,
    names: Vec<(&'static str, PathBuf, FolderName)>,
}

#[derive(Debug)]
struct Claim {
    owner: &'static str,
    under: PathBuf,
    rules: FolderRules,
}

impl RulesBook {
    pub fn claim(&self, owner: &'static str, under: PathBuf, rules: FolderRules) {
        let claims = &mut self.0.borrow_mut().claims;
        claims.retain(|claim| claim.owner != owner);
        claims.push(Claim {
            owner,
            under,
            rules,
        });
    }

    pub fn give_up(&self, owner: &'static str) {
        let mut book = self.0.borrow_mut();
        book.claims.retain(|claim| claim.owner != owner);
        book.names.retain(|(named_by, _, _)| *named_by != owner);
    }

    pub fn name(&self, owner: &'static str, at: PathBuf, name: FolderName) {
        let names = &mut self.0.borrow_mut().names;
        names.retain(|(named_by, _, _)| *named_by != owner);
        names.push((owner, at, name));
    }

    pub fn named(&self, folder: &Path) -> Option<FolderName> {
        self.0
            .borrow()
            .names
            .iter()
            .find(|(_, at, _)| at == folder)
            .map(|(_, _, name)| name.clone())
    }

    pub fn for_folder(&self, folder: &Path) -> FolderRules {
        self.0
            .borrow()
            .claims
            .iter()
            .filter(|claim| folder.starts_with(&claim.under))
            .fold(FolderRules::default(), |rules, claim| {
                rules.with(claim.rules)
            })
    }
}
