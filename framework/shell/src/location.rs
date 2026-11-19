use std::any::{Any, TypeId};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::SharedString;

use crate::assets::Icon;
use crate::path_bar::PageCrumbs;

pub trait PageKind: fmt::Debug + Clone + Eq + Send + Sync + 'static {
    fn title(&self) -> SharedString;

    fn icon(&self) -> Icon {
        Icon::Folder
    }

    fn crumbs(&self) -> Option<PageCrumbs> {
        None
    }
}

trait AnyPage: fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn same(&self, other: &dyn AnyPage) -> bool;
    fn title(&self) -> SharedString;
    fn icon(&self) -> Icon;
    fn crumbs(&self) -> Option<PageCrumbs>;
}

impl<K: PageKind> AnyPage for K {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn AnyPage) -> bool {
        other.as_any().downcast_ref::<K>() == Some(self)
    }

    fn title(&self) -> SharedString {
        PageKind::title(self)
    }

    fn icon(&self) -> Icon {
        PageKind::icon(self)
    }

    fn crumbs(&self) -> Option<PageCrumbs> {
        PageKind::crumbs(self)
    }
}

#[derive(Debug, Clone)]
pub struct PageRef(Arc<dyn AnyPage>);

impl PageRef {
    pub fn new<K: PageKind>(page: K) -> Self {
        Self(Arc::new(page))
    }

    pub fn get<K: PageKind>(&self) -> Option<&K> {
        self.0.as_any().downcast_ref()
    }

    pub fn title(&self) -> SharedString {
        self.0.title()
    }

    pub fn icon(&self) -> Icon {
        self.0.icon()
    }

    pub fn crumbs(&self) -> Option<PageCrumbs> {
        self.0.crumbs()
    }

    pub fn kind(&self) -> TypeId {
        self.0.as_any().type_id()
    }
}

impl PartialEq for PageRef {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_ref())
    }
}

impl Eq for PageRef {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Folder(PathBuf),
    Page(PageRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartPage;

impl PageKind for StartPage {
    fn title(&self) -> SharedString {
        SharedString::new_static("Start")
    }

    fn icon(&self) -> Icon {
        Icon::Start
    }
}

impl Location {
    pub fn start() -> Self {
        Self::Page(PageRef::new(StartPage))
    }

    pub fn folder(&self) -> Option<&Path> {
        match self {
            Self::Folder(folder) => Some(folder),
            Self::Page(_) => None,
        }
    }

    pub fn page<K: PageKind>(&self) -> Option<&K> {
        match self {
            Self::Folder(_) => None,
            Self::Page(page) => page.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Games;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Shown(u8);

    impl PageKind for Games {
        fn title(&self) -> SharedString {
            "Games".into()
        }
    }

    impl PageKind for Shown {
        fn title(&self) -> SharedString {
            self.0.to_string().into()
        }
    }

    #[test]
    fn pages_are_equal_only_with_the_same_kind_and_value() {
        assert_eq!(PageRef::new(Shown(1)), PageRef::new(Shown(1)));
        assert_ne!(PageRef::new(Shown(1)), PageRef::new(Shown(2)));
        assert_ne!(PageRef::new(Shown(1)), PageRef::new(Games));
    }

    #[test]
    fn a_page_location_gives_back_its_own_kind() {
        let here = Location::Page(PageRef::new(Shown(3)));

        assert_eq!(here.page::<Shown>(), Some(&Shown(3)));
        assert_eq!(here.page::<Games>(), None);
        assert_eq!(here.folder(), None);
        assert_eq!(PageRef::new(Games).title(), "Games");
        assert_eq!(PageRef::new(Games).kind(), TypeId::of::<Games>());
    }
}
