use gpui::SharedString;
use nova_files_api::{Envelope, ListingId, Order};
use nova_files_gathered_api::{Command, Gathering};
use nova_files_shell::PageKind;
use nova_files_shell::assets::Icon;

pub const PROJECTS_LABEL: &str = "Projects";
pub const GAMES_LABEL: &str = "Games";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatheredPage {
    pub place: Gathering,
}

impl GatheredPage {
    pub const fn new(place: Gathering) -> Self {
        Self { place }
    }

    pub fn list(&self, id: ListingId, order: Order) -> Envelope {
        Envelope::new(Command::ListGathered {
            id,
            place: self.place,
            order,
        })
    }
}

pub const fn look(place: Gathering) -> (&'static str, Icon) {
    match place {
        Gathering::Projects => (PROJECTS_LABEL, Icon::Projects),
        Gathering::Games => (GAMES_LABEL, Icon::Games),
    }
}

impl PageKind for GatheredPage {
    fn title(&self) -> SharedString {
        SharedString::new_static(look(self.place).0)
    }

    fn icon(&self) -> Icon {
        look(self.place).1
    }

    fn folder_column(&self) -> bool {
        self.place == Gathering::Projects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_place_has_its_own_name_and_icon() {
        assert_eq!(look(Gathering::Projects), (PROJECTS_LABEL, Icon::Projects));
        assert_eq!(look(Gathering::Games), (GAMES_LABEL, Icon::Games));
    }
}
