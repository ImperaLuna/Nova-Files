//! The search interface: what launchers call to find files by name. Empty
//! until search lands (FIL-29); the layout is fixed here so search and the
//! file picker grow apart.
//!
//! It is its own interface, versioned on its own, beside the Nova Files
//! interface on the same object. A search that is still sending stages, or
//! a live subscription to results, takes a hold on the core (see `holds`)
//! until it ends, is cancelled or its caller leaves the bus.

/// The interface name. The `#[zbus::interface]` attribute needs the literal
/// too; a test checks the two match.
pub const INTERFACE: &str = "io.github.ImperaLuna.NovaFiles.Search1";

pub struct Search;

#[zbus::interface(name = "io.github.ImperaLuna.NovaFiles.Search1")]
impl Search {}

#[cfg(test)]
mod tests {
    use zbus::object_server::Interface as _;

    use super::{INTERFACE, Search};

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(Search::name().as_str(), INTERFACE);
    }
}
