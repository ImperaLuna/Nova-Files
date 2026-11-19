use nova_files_gathered_api::Gathering;

use crate::assets::Icon;

pub const START_LABEL: &str = "Start";
pub const PROJECTS_LABEL: &str = "Projects";
pub const GAMES_LABEL: &str = "Games";

pub const fn gathering_look(place: Gathering) -> (&'static str, Icon) {
    match place {
        Gathering::Projects => (PROJECTS_LABEL, Icon::Projects),
        Gathering::Games => (GAMES_LABEL, Icon::Games),
    }
}
