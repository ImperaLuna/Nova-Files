//! How the windows look: the theme's colors, which follow the theme file,
//! the appearance setting and the desktop, and the icon theme file icons
//! come from.
//!
//! A client asks with [`Command::Watch`] and gets [`Event::Theme`] then and
//! each time it changes. The D-Bus side tells the core what the desktop
//! asks for with [`Command::DesktopLook`] and [`Command::DesktopIconThemes`].

#![forbid(unsafe_code)]

use std::sync::Arc;

use nova_files_api::{DesktopLook, Theme};

/// What a client asks of the look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask for the theme; the answer is [`Event::Theme`], and from then on
    /// the client gets [`Event::Theme`] again each time it changes. Clients
    /// that never ask never get it.
    Watch,
    /// Tell the core what the desktop asks apps to look like; sent by the
    /// D-Bus side each time the desktop changes it. Clients that asked for
    /// the theme get [`Event::Theme`] if the theme changed; nothing
    /// answers the sender.
    DesktopLook(DesktopLook),
    /// Tell the core which icon themes the desktop names, in its order
    /// (none when it names none); sent by the D-Bus side each time they
    /// change. The core takes the first one installed when the toolkits'
    /// settings files name none; nothing answers.
    DesktopIconThemes(Vec<String>),
}

/// What the look tells a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The theme, answering [`Command::Watch`], and sent again to each
    /// client that asked whenever it changes: from the theme file, or the
    /// appearance setting.
    Theme(Arc<Theme>),
}
