use std::path::{Path, PathBuf};

use gpui::SharedString;
use nova_files_api::{DiskSpace, DriveId, DriveList, VolumeId};

use crate::columns;

const SYSTEM_LABEL: &str = "System";
const SYSTEM_FOLDER: &str = "/";
pub const NEARLY_FULL: u8 = 90;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownDrive {
    pub name: SharedString,
    pub folder: Option<PathBuf>,
    pub volume: Option<VolumeId>,
    pub eject: Option<DriveId>,
}

pub fn shown(list: &DriveList) -> Vec<ShownDrive> {
    let system = ShownDrive {
        name: SYSTEM_LABEL.into(),
        folder: Some(PathBuf::from(SYSTEM_FOLDER)),
        volume: None,
        eject: None,
    };
    let volumes = list
        .volumes
        .iter()
        .filter(|volume| volume.mount_point.as_deref() != Some(Path::new(SYSTEM_FOLDER)))
        .map(|volume| {
            let drive = volume
                .drive
                .as_ref()
                .and_then(|id| list.drives.iter().find(|drive| drive.id == *id));
            let name = [
                Some(volume.label.as_str()),
                drive.map(|drive| drive.name.as_str()),
            ]
            .into_iter()
            .flatten()
            .find(|name| !name.is_empty())
            .map_or_else(
                || format!("{} drive", columns::size(volume.size)),
                str::to_owned,
            );
            ShownDrive {
                name: name.into(),
                folder: volume.mount_point.clone(),
                volume: Some(volume.id.clone()),
                eject: drive
                    .filter(|drive| drive.removable && drive.ejectable)
                    .map(|drive| drive.id.clone()),
            }
        });
    std::iter::once(system).chain(volumes).collect()
}

pub fn here(drives: &[ShownDrive], here: &Path, home: &Path) -> Option<usize> {
    let inside_home = here.starts_with(home);
    drives
        .iter()
        .enumerate()
        .filter(|(_, drive)| !(inside_home && drive.volume.is_none()))
        .filter_map(|(index, drive)| {
            let folder = drive.folder.as_deref()?;
            here.starts_with(folder)
                .then_some((index, folder.components().count()))
        })
        .max_by_key(|&(_, depth)| depth)
        .map(|(index, _)| index)
}

pub fn free(space: DiskSpace) -> (SharedString, bool) {
    (
        format!("{} free", columns::size(space.free)).into(),
        space.percent_used() >= NEARLY_FULL,
    )
}

#[cfg(test)]
mod tests {
    use nova_files_api::{Drive, Volume};

    use super::*;

    fn volume(id: &str, drive: Option<&str>, label: &str, mounted: Option<&str>) -> Volume {
        Volume {
            id: VolumeId(id.to_owned()),
            drive: drive.map(|drive| DriveId(drive.to_owned())),
            label: label.to_owned(),
            size: 16_000_000_000,
            filesystem: "vfat".to_owned(),
            mount_point: mounted.map(PathBuf::from),
            device: "/dev/sdb1".into(),
            read_only: false,
        }
    }

    fn drive(id: &str, name: &str, removable: bool) -> Drive {
        Drive {
            id: DriveId(id.to_owned()),
            name: name.to_owned(),
            removable,
            ejectable: removable,
        }
    }

    fn list() -> DriveList {
        DriveList {
            drives: vec![drive("stick", "Kingston", true), drive("ssd", "", false)],
            volumes: vec![
                volume(
                    "photos",
                    Some("stick"),
                    "Photos",
                    Some("/run/media/me/Photos"),
                ),
                volume("unlabelled", Some("stick"), "", None),
                volume("games", Some("ssd"), "Games SSD", Some("/mnt/games")),
                volume("loop", None, "", None),
            ],
        }
    }

    #[test]
    fn the_system_drive_comes_first_then_each_volume_named() {
        let rows = shown(&list());
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_ref()).collect();
        assert_eq!(
            names,
            ["System", "Photos", "Kingston", "Games SSD", "16 GB drive"]
        );
        assert_eq!(rows[0].folder.as_deref(), Some(Path::new("/")));
        assert_eq!(rows[0].volume, None);
        assert_eq!(rows[2].folder, None);
    }

    #[test]
    fn only_a_volume_on_a_drive_that_can_be_unplugged_offers_eject() {
        let ejects: Vec<bool> = shown(&list())
            .iter()
            .map(|row| row.eject.is_some())
            .collect();
        assert_eq!(ejects, [false, true, true, false, false]);
    }

    #[test]
    fn a_drive_needs_to_be_both_removable_and_ejectable_to_offer_eject() {
        for (removable, ejectable) in [(true, false), (false, true)] {
            let mut list = list();
            list.drives[0].removable = removable;
            list.drives[0].ejectable = ejectable;
            assert_eq!(shown(&list)[1].eject, None, "{removable} {ejectable}");
        }
    }

    #[test]
    fn you_are_on_the_drive_whose_folder_holds_yours_most_closely() {
        let rows = shown(&list());
        let home = Path::new("/home/me");
        let at = |folder: &str| here(&rows, Path::new(folder), home);
        assert_eq!(at("/mnt/games/steam"), Some(3));
        assert_eq!(at("/run/media/me/Photos"), Some(1));
        assert_eq!(at("/etc"), Some(0));
        assert_eq!(at("/home/me/Documents"), None);
        let mut inside = list();
        inside.volumes[2].mount_point = Some(PathBuf::from("/home/me/Games"));
        let rows = shown(&inside);
        assert_eq!(
            here(&rows, Path::new("/home/me/Games/steam"), home),
            Some(3)
        );
    }

    #[test]
    fn free_space_reads_as_a_number_and_turns_red_when_nearly_full() {
        let space = |total, free| DiskSpace { total, free };
        assert_eq!(
            free(space(1_000_000_000_000, 381_000_000_000)),
            ("381 GB free".into(), false)
        );
        assert!(free(space(100, 10)).1);
        assert!(!free(space(100, 11)).1);
    }
}
