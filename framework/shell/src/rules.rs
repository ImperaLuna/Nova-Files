use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RulesBook(Rc<RefCell<Vec<Claim>>>);

#[derive(Debug)]
struct Claim {
    owner: &'static str,
    under: PathBuf,
    rules: FolderRules,
}

impl RulesBook {
    pub fn claim(&self, owner: &'static str, under: PathBuf, rules: FolderRules) {
        let mut claims = self.0.borrow_mut();
        claims.retain(|claim| claim.owner != owner);
        claims.push(Claim {
            owner,
            under,
            rules,
        });
    }

    pub fn give_up(&self, owner: &'static str) {
        self.0.borrow_mut().retain(|claim| claim.owner != owner);
    }

    pub fn for_folder(&self, folder: &Path) -> FolderRules {
        self.0
            .borrow()
            .iter()
            .filter(|claim| folder.starts_with(&claim.under))
            .fold(FolderRules::default(), |rules, claim| {
                rules.with(claim.rules)
            })
    }
}
