//! Which Vulkan drivers this PC can't use, so the program can tell the
//! Vulkan loader to skip them.
//!
//! The loader opens every installed driver (each `vulkan/icd.d` manifest)
//! when the window sets up the GPU, even drivers for GPUs the PC doesn't
//! have. Each one loads its libraries, which costs a few milliseconds warm
//! and much more on a cold start.
//!
//! [`absent_vendor_drivers`] reads the vendors of the display devices on the
//! PCI bus and the library each manifest loads. A manifest whose library
//! serves only one vendor, or only GPUs that aren't on PCI, and that vendor
//! isn't present, goes in the filter. The software driver (lavapipe) goes
//! too when a driver for a GPU that is present is installed: GPUI only
//! draws with a software Vulkan driver when it has no other, and when the
//! hardware driver fails it moves to OpenGL, which still has a software
//! fallback of its own. Skipping lavapipe saves loading LLVM, the biggest
//! library a start reads. Libraries not in [`LIBRARY_VENDORS`] are always
//! kept. It decides nothing
//! when the user already chose drivers (a `VK_*` driver variable or a loader
//! settings file), when a GPU isn't on PCI, or when the system can't be read:
//! then the loader works as it always does.
//!
//! The answer is the value for [`FILTER_VARIABLE`], which the program sets
//! before it starts any thread. Children inherit it, which is harmless: it
//! only hides drivers for hardware that isn't there.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The loader's own filter: comma-separated globs over manifest file names,
/// read when the GPU is set up.
pub const FILTER_VARIABLE: &str = "VK_LOADER_DRIVERS_DISABLE";

/// Each of these is the user choosing drivers, which is never second-guessed.
const USER_CHOICE_VARIABLES: [&str; 5] = [
    "VK_DRIVER_FILES",
    "VK_ICD_FILENAMES",
    "VK_ADD_DRIVER_FILES",
    "VK_LOADER_DRIVERS_SELECT",
    FILTER_VARIABLE,
];
/// A loader settings file can add or choose drivers, so it is a choice too.
const LOADER_SETTINGS_FILE: &str = "vulkan/loader_settings.d/vk_loader_settings.json";
const MANIFEST_FOLDER: &str = "vulkan/icd.d";
const MANIFEST_EXTENSION: &str = "json";
/// A manifest is a few lines; anything bigger isn't one.
const MANIFEST_MAX_BYTES: u64 = 64 * 1024;
/// The loader reads only this many filter entries and drops the rest.
const LOADER_FILTER_LIMIT: usize = 16;
const FILTER_SEPARATOR: &str = ",";

const PCI_DEVICES: &str = "/sys/bus/pci/devices";
const DRM_DEVICES: &str = "/sys/class/drm";
/// A DRM card is `card0`, `card1`, ...; render nodes and connectors aren't.
const DRM_CARD_PREFIX: &str = "card";
const DRM_CONNECTOR_SEPARATOR: char = '-';
const PCI_SUBSYSTEM: &str = "pci";
/// A PCI class code carries its base class in bits 16 to 23; 0x03 is a
/// display controller (VGA, 3D or other display).
const PCI_CLASS_SHIFT: u32 = 16;
const PCI_CLASS_DISPLAY: u32 = 0x03;
const HEX_PREFIX: &str = "0x";
const HEX_RADIX: u32 = 16;

const CONFIG_HOME_VAR: &str = "XDG_CONFIG_HOME";
const CONFIG_DIRS_VAR: &str = "XDG_CONFIG_DIRS";
const DATA_HOME_VAR: &str = "XDG_DATA_HOME";
const DATA_DIRS_VAR: &str = "XDG_DATA_DIRS";
const HOME_VAR: &str = "HOME";
const CONFIG_IN_HOME: &str = ".config";
const DATA_IN_HOME: &str = ".local/share";
const DEFAULT_CONFIG_DIRS: &str = "/etc/xdg";
const SYSTEM_CONFIG: &str = "/etc";
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// Who a driver library serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Serves {
    /// GPUs of one PCI vendor.
    PciVendor(u32),
    /// Only GPUs built into a chip (not on PCI), such as Arm boards and
    /// Apple Silicon.
    OnChip,
    /// Any machine, drawing on the CPU: needed only when no driver for a
    /// present GPU is installed.
    Software,
}

const AMD: u32 = 0x1002;
const INTEL: u32 = 0x8086;
const NVIDIA: u32 = 0x10de;
const VIRTIO: u32 = 0x1af4;
const MICROSOFT: u32 = 0x1414;
const IMAGINATION: u32 = 0x1010;

/// The driver libraries known to serve one kind of GPU only, by file name.
const LIBRARY_VENDORS: [(&str, Serves); 13] = [
    ("libvulkan_radeon.so", Serves::PciVendor(AMD)),
    ("libvulkan_intel.so", Serves::PciVendor(INTEL)),
    ("libvulkan_intel_hasvk.so", Serves::PciVendor(INTEL)),
    ("libGLX_nvidia.so.0", Serves::PciVendor(NVIDIA)),
    ("libvulkan_nouveau.so", Serves::PciVendor(NVIDIA)),
    ("libvulkan_virtio.so", Serves::PciVendor(VIRTIO)),
    ("libvulkan_dzn.so", Serves::PciVendor(MICROSOFT)),
    ("libvulkan_powervr_mesa.so", Serves::PciVendor(IMAGINATION)),
    ("libvulkan_panfrost.so", Serves::OnChip),
    ("libvulkan_asahi.so", Serves::OnChip),
    ("libvulkan_broadcom.so", Serves::OnChip),
    ("libvulkan_freedreno.so", Serves::OnChip),
    ("libvulkan_lvp.so", Serves::Software),
];

/// The part of a Vulkan driver manifest this reads.
#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(rename = "ICD")]
    icd: ManifestDriver,
}

#[derive(Debug, Deserialize)]
struct ManifestDriver {
    library_path: String,
}

/// The filter for [`FILTER_VARIABLE`] on this PC, or `None` to leave the
/// loader alone.
pub fn absent_vendor_drivers() -> Option<String> {
    let roots = search_roots(|name| env::var_os(name));
    decide(
        |name| env::var_os(name),
        Path::new(PCI_DEVICES),
        Path::new(DRM_DEVICES),
        &roots,
    )
}

/// [`absent_vendor_drivers`] with the environment, the sysfs folders and the
/// manifest search roots given, so tests can use recorded copies.
fn decide(
    var: impl Fn(&str) -> Option<OsString>,
    pci: &Path,
    drm: &Path,
    roots: &[PathBuf],
) -> Option<String> {
    if USER_CHOICE_VARIABLES.iter().any(|name| var(name).is_some()) {
        return None;
    }
    if roots
        .iter()
        .any(|root| root.join(LOADER_SETTINGS_FILE).exists())
    {
        return None;
    }
    if !every_card_on_pci(drm)? {
        return None;
    }
    let present = pci_display_vendors(pci)?;
    if present.is_empty() {
        return None;
    }
    let found = manifests(roots);
    let hardware_driver = found
        .iter()
        .any(|(_, library)| serves_a_present_gpu(library, &present));
    // The filter matches file names, so a name is skipped only when every
    // manifest by that name serves nothing present.
    let mut by_name: BTreeMap<String, bool> = BTreeMap::new();
    for (name, library) in found {
        let absent = serves_nothing_present(&library, &present, hardware_driver);
        by_name
            .entry(name)
            .and_modify(|skip| *skip &= absent)
            .or_insert(absent);
    }
    let skipped: Vec<String> = by_name
        .into_iter()
        .filter_map(|(name, skip)| skip.then_some(name))
        .take(LOADER_FILTER_LIMIT)
        .collect();
    (!skipped.is_empty()).then(|| skipped.join(FILTER_SEPARATOR))
}

/// Whether the library is known to serve only GPUs this PC doesn't have,
/// or is the software driver while `hardware_driver` (a driver for a
/// present GPU) is installed.
fn serves_nothing_present(library: &str, present: &BTreeSet<u32>, hardware_driver: bool) -> bool {
    known_serves(library).is_some_and(|serves| match serves {
        Serves::PciVendor(vendor) => !present.contains(&vendor),
        // Only reached when every GPU is on PCI.
        Serves::OnChip => true,
        Serves::Software => hardware_driver,
    })
}

/// Whether the library is a known driver for a GPU this PC has.
fn serves_a_present_gpu(library: &str, present: &BTreeSet<u32>) -> bool {
    matches!(known_serves(library), Some(Serves::PciVendor(vendor)) if present.contains(&vendor))
}

/// Who the library serves, when it is in [`LIBRARY_VENDORS`].
fn known_serves(library: &str) -> Option<Serves> {
    let name = Path::new(library)
        .file_name()
        .map_or_else(|| library.into(), |name| name.to_string_lossy());
    LIBRARY_VENDORS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, serves)| *serves)
}

/// Whether every DRM card's device sits on the PCI bus. `None` when the
/// folder can't be read, which decides nothing.
fn every_card_on_pci(drm: &Path) -> Option<bool> {
    for entry in fs::read_dir(drm).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_card = name
            .strip_prefix(DRM_CARD_PREFIX)
            .is_some_and(|rest| !rest.is_empty() && !rest.contains(DRM_CONNECTOR_SEPARATOR));
        if !is_card {
            continue;
        }
        let subsystem = fs::read_link(entry.path().join("device/subsystem")).ok()?;
        if subsystem.file_name().is_none_or(|bus| bus != PCI_SUBSYSTEM) {
            return Some(false);
        }
    }
    Some(true)
}

/// The vendors of the display devices on the PCI bus. `None` when a device
/// can't be read, since an unnamed GPU could need any driver.
fn pci_display_vendors(pci: &Path) -> Option<BTreeSet<u32>> {
    let mut vendors = BTreeSet::new();
    for entry in fs::read_dir(pci).ok()? {
        let device = entry.ok()?.path();
        let class = read_hex(&device.join("class"))?;
        if class >> PCI_CLASS_SHIFT != PCI_CLASS_DISPLAY {
            continue;
        }
        vendors.insert(read_hex(&device.join("vendor"))?);
    }
    Some(vendors)
}

/// A sysfs number such as `0x030000`.
fn read_hex(file: &Path) -> Option<u32> {
    let text = fs::read_to_string(file).ok()?;
    let digits = text.trim();
    let digits = digits.strip_prefix(HEX_PREFIX).unwrap_or(digits);
    u32::from_str_radix(digits, HEX_RADIX).ok()
}

/// Every manifest under the roots as (file name, library it loads), sorted
/// by name. A manifest that can't be read or parsed is left out, so it is
/// never filtered.
fn manifests(roots: &[PathBuf]) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    for folder in roots.iter().map(|root| root.join(MANIFEST_FOLDER)) {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
            if path.extension().is_none_or(|ext| ext != MANIFEST_EXTENSION) {
                continue;
            }
            let (Some(name), Some(library)) = (path.file_name(), library_of(&path)) else {
                continue;
            };
            found.insert((name.to_string_lossy().into_owned(), library));
        }
    }
    found
}

fn library_of(manifest: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(manifest)
        .ok()?
        .take(MANIFEST_MAX_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    let parsed: Manifest = serde_json::from_str(&text).ok()?;
    Some(parsed.icd.library_path)
}

/// Where the loader looks for manifests, in its own order: the user's and
/// the system's config folders, then the user's and the system's data
/// folders.
fn search_roots(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let home = var(HOME_VAR).map(PathBuf::from);
    let in_home = |path: &str| home.as_ref().map(|home| home.join(path));
    let one = |name: &str, fallback: Option<PathBuf>| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or(fallback)
    };
    let list = |name: &str, fallback: &str| {
        let value = var(name)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| fallback.into());
        env::split_paths(&value).collect::<Vec<_>>()
    };
    one(CONFIG_HOME_VAR, in_home(CONFIG_IN_HOME))
        .into_iter()
        .chain(list(CONFIG_DIRS_VAR, DEFAULT_CONFIG_DIRS))
        .chain([PathBuf::from(SYSTEM_CONFIG)])
        .chain(one(DATA_HOME_VAR, in_home(DATA_IN_HOME)))
        .chain(list(DATA_DIRS_VAR, DEFAULT_DATA_DIRS))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    /// The Vulkan drivers NixOS installs with Mesa and the NVIDIA driver, as
    /// (manifest, library), recorded from the maintainer's `NixOS` PC.
    const NIXOS_DRIVERS: [(&str, &str); 14] = [
        (
            "asahi_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_asahi.so",
        ),
        (
            "broadcom_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_broadcom.so",
        ),
        (
            "dzn_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_dzn.so",
        ),
        (
            "freedreno_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_freedreno.so",
        ),
        (
            "gfxstream_vk_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_gfxstream.so",
        ),
        (
            "intel_hasvk_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_intel_hasvk.so",
        ),
        (
            "intel_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_intel.so",
        ),
        (
            "lvp_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_lvp.so",
        ),
        (
            "nouveau_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_nouveau.so",
        ),
        ("nvidia_icd.json", "libGLX_nvidia.so.0"),
        (
            "panfrost_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_panfrost.so",
        ),
        (
            "powervr_mesa_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_powervr_mesa.so",
        ),
        (
            "radeon_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_radeon.so",
        ),
        (
            "virtio_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_virtio.so",
        ),
    ];
    const VGA_CLASS: &str = "0x030000";
    const USB_CLASS: &str = "0x0c0330";

    /// A temp folder standing in for sysfs and the manifest folders.
    struct FakeSystem(PathBuf);

    impl FakeSystem {
        fn new(name: &str) -> Self {
            let root =
                env::temp_dir().join(format!("nova-files-vulkan-{name}-{}", std::process::id()));
            fs::remove_dir_all(&root).ok();
            for folder in [
                "pci",
                "drm",
                "share/vulkan/icd.d",
                "bus/pci",
                "bus/platform",
            ] {
                fs::create_dir_all(root.join(folder)).unwrap();
            }
            Self(root)
        }

        fn pci_device(&self, slot: &str, class: &str, vendor: &str) {
            let device = self.0.join("pci").join(slot);
            fs::create_dir_all(&device).unwrap();
            fs::write(device.join("class"), format!("{class}\n")).unwrap();
            fs::write(device.join("vendor"), format!("{vendor}\n")).unwrap();
        }

        fn card(&self, name: &str, bus: &str) {
            let device = self.0.join("drm").join(name).join("device");
            fs::create_dir_all(&device).unwrap();
            symlink(self.0.join("bus").join(bus), device.join("subsystem")).unwrap();
        }

        fn manifest(&self, name: &str, library: &str) {
            let text = format!(
                r#"{{"file_format_version": "1.0.1", "ICD": {{"library_path": "{library}", "api_version": "1.4.0"}}}}"#
            );
            fs::write(self.0.join("share/vulkan/icd.d").join(name), text).unwrap();
        }

        fn nixos_drivers(&self) {
            for (name, library) in NIXOS_DRIVERS {
                self.manifest(name, library);
            }
        }

        fn decide_with(&self, var: impl Fn(&str) -> Option<OsString>) -> Option<String> {
            decide(
                var,
                &self.0.join("pci"),
                &self.0.join("drm"),
                &[self.0.join("share")],
            )
        }

        fn decide(&self) -> Option<String> {
            self.decide_with(|_| None)
        }
    }

    impl Drop for FakeSystem {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn nvidia_desktop(name: &str) -> FakeSystem {
        let system = FakeSystem::new(name);
        system.pci_device("0000:01:00.0", VGA_CLASS, "0x10de");
        system.pci_device("0000:00:14.0", USB_CLASS, "0x8086");
        system.card("card1", "pci");
        system.nixos_drivers();
        system
    }

    #[test]
    fn an_nvidia_desktop_skips_every_driver_for_other_hardware() {
        let system = nvidia_desktop("nvidia");
        let skipped = system.decide().unwrap();
        assert_eq!(
            skipped.split(',').collect::<Vec<_>>(),
            [
                "asahi_icd.x86_64.json",
                "broadcom_icd.x86_64.json",
                "dzn_icd.x86_64.json",
                "freedreno_icd.x86_64.json",
                "intel_hasvk_icd.x86_64.json",
                "intel_icd.x86_64.json",
                "lvp_icd.x86_64.json",
                "panfrost_icd.x86_64.json",
                "powervr_mesa_icd.x86_64.json",
                "radeon_icd.x86_64.json",
                "virtio_icd.x86_64.json",
            ]
        );
    }

    #[test]
    fn unknown_and_present_vendor_drivers_stay() {
        let system = nvidia_desktop("kept");
        let skipped = system.decide().unwrap();
        for kept in [
            "gfxstream_vk_icd.x86_64.json",
            "nvidia_icd.json",
            "nouveau_icd.x86_64.json",
        ] {
            assert!(!skipped.contains(kept), "{kept} was skipped");
        }
    }

    #[test]
    fn a_name_is_skipped_only_when_every_copy_serves_absent_hardware() {
        let system = nvidia_desktop("same-name");
        let user_drivers = system.0.join("config/vulkan/icd.d");
        fs::create_dir_all(&user_drivers).unwrap();
        fs::write(
            user_drivers.join("intel_icd.x86_64.json"),
            r#"{"ICD": {"library_path": "/opt/own/libGLX_nvidia.so.0"}}"#,
        )
        .unwrap();
        fs::write(
            user_drivers.join("radeon_icd.x86_64.json"),
            r#"{"ICD": {"library_path": "/opt/own/libvulkan_radeon.so"}}"#,
        )
        .unwrap();
        let roots = [system.0.join("config"), system.0.join("share")];
        let skipped = decide(
            |_| None,
            &system.0.join("pci"),
            &system.0.join("drm"),
            &roots,
        )
        .unwrap();
        let names: Vec<&str> = skipped.split(',').collect();
        assert!(!names.contains(&"intel_icd.x86_64.json"));
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "radeon_icd.x86_64.json")
                .count(),
            1
        );
    }

    #[test]
    fn the_software_driver_stays_without_a_driver_for_the_gpu() {
        // A VM's display device, which no installed driver serves.
        let system = FakeSystem::new("vm");
        system.pci_device("0000:00:02.0", VGA_CLASS, "0x1234");
        system.card("card0", "pci");
        system.nixos_drivers();
        let skipped = system.decide().unwrap();
        assert!(!skipped.contains("lvp_icd"));
        assert!(skipped.contains("radeon_icd"));
    }

    #[test]
    fn the_software_driver_stays_when_the_only_driver_for_the_gpu_is_unknown() {
        let system = FakeSystem::new("unknown-driver");
        system.pci_device("0000:01:00.0", VGA_CLASS, "0x10de");
        system.card("card1", "pci");
        system.manifest("own_icd.json", "/opt/own/libvulkan_own.so");
        system.manifest(
            "lvp_icd.x86_64.json",
            "/nix/store/x-mesa/lib/libvulkan_lvp.so",
        );
        assert_eq!(system.decide(), None);
    }

    #[test]
    fn a_laptop_with_two_gpus_keeps_both_vendors() {
        let system = nvidia_desktop("hybrid");
        system.pci_device("0000:06:00.0", VGA_CLASS, "0x1002");
        system.card("card2", "pci");
        let skipped = system.decide().unwrap();
        assert!(!skipped.contains("radeon_icd"));
        assert!(!skipped.contains("nvidia_icd"));
        assert!(skipped.contains("intel_icd"));
    }

    #[test]
    fn a_driver_choice_by_the_user_wins() {
        let system = nvidia_desktop("user-choice");
        for name in USER_CHOICE_VARIABLES {
            let chosen = system.decide_with(|asked| (asked == name).then(OsString::new));
            assert_eq!(chosen, None, "{name} was overridden");
        }
    }

    #[test]
    fn a_loader_settings_file_wins() {
        let system = nvidia_desktop("settings");
        let settings = system.0.join("share").join(LOADER_SETTINGS_FILE);
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(settings, "{}").unwrap();
        assert_eq!(system.decide(), None);
    }

    #[test]
    fn a_gpu_built_into_the_chip_leaves_the_loader_alone() {
        let system = nvidia_desktop("on-chip");
        system.card("card0", "platform");
        assert_eq!(system.decide(), None);
    }

    #[test]
    fn a_gpu_that_cant_be_read_leaves_the_loader_alone() {
        let system = nvidia_desktop("unreadable");
        let device = system.0.join("pci/0000:07:00.0");
        fs::create_dir_all(&device).unwrap();
        fs::write(device.join("class"), VGA_CLASS).unwrap();
        assert_eq!(system.decide(), None);
    }

    #[test]
    fn no_display_device_on_pci_leaves_the_loader_alone() {
        let system = FakeSystem::new("no-gpu");
        system.pci_device("0000:00:14.0", USB_CLASS, "0x8086");
        system.nixos_drivers();
        assert_eq!(system.decide(), None);
    }

    #[test]
    fn nothing_to_skip_sets_nothing() {
        let system = FakeSystem::new("only-nvidia");
        system.pci_device("0000:01:00.0", VGA_CLASS, "0x10de");
        system.card("card1", "pci");
        system.manifest("nvidia_icd.json", "libGLX_nvidia.so.0");
        fs::write(system.0.join("share/vulkan/icd.d/broken_icd.json"), "{").unwrap();
        assert_eq!(system.decide(), None);
    }

    #[test]
    fn the_filter_stops_at_the_loaders_limit() {
        let system = nvidia_desktop("limit");
        for arch in ["i686", "aarch64"] {
            for (name, library) in NIXOS_DRIVERS {
                system.manifest(&name.replace("x86_64", arch), library);
            }
        }
        let skipped = system.decide().unwrap();
        assert_eq!(skipped.split(',').count(), LOADER_FILTER_LIMIT);
    }

    #[test]
    fn display_connectors_are_not_cards() {
        let system = nvidia_desktop("connectors");
        fs::create_dir_all(system.0.join("drm/card1-DP-1")).unwrap();
        fs::create_dir_all(system.0.join("drm/renderD128")).unwrap();
        assert!(system.decide().is_some());
    }

    #[test]
    fn a_manifest_padded_past_a_kilobyte_is_still_read() {
        let system = nvidia_desktop("padded");
        let padding = " ".repeat(4096);
        fs::write(
            system.0.join("share/vulkan/icd.d/radeon_icd.x86_64.json"),
            format!(r#"{{{padding}"ICD": {{"library_path": "libvulkan_radeon.so"}}}}"#),
        )
        .unwrap();
        assert!(system.decide().unwrap().contains("radeon_icd"));
    }

    #[test]
    fn empty_folder_variables_mean_the_defaults() {
        let roots = search_roots(|name| match name {
            "HOME" => Some("/home/someone".into()),
            "XDG_CONFIG_HOME" | "XDG_DATA_DIRS" => Some(OsString::new()),
            _ => None,
        });
        assert_eq!(roots.first(), Some(&PathBuf::from("/home/someone/.config")));
        assert_eq!(roots.last(), Some(&PathBuf::from("/usr/share")));
    }

    #[test]
    fn manifests_are_looked_for_where_the_loader_looks() {
        let roots = search_roots(|name| match name {
            "HOME" => Some("/home/someone".into()),
            "XDG_DATA_DIRS" => Some("/run/opengl-driver/share:/usr/share".into()),
            _ => None,
        });
        let expected: Vec<PathBuf> = [
            "/home/someone/.config",
            "/etc/xdg",
            "/etc",
            "/home/someone/.local/share",
            "/run/opengl-driver/share",
            "/usr/share",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(roots, expected);
    }
}
