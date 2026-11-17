//! File icons against a scripted core: a theme icon the core sends with
//! a row's details is made once and shared by every row of its type, one
//! that doesn't decode leaves the row's built-in icon, and a new scale
//! makes them again.

use nova_files_api::{FileIcon, IconFormat, IconKind, ThemeIcon};

use super::*;

const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#f00"/></svg>"##;

fn theme_icon(path: &str, bytes: &[u8]) -> FileIcon {
    FileIcon::Theme {
        icon: Arc::new(ThemeIcon {
            path: PathBuf::from(path),
            format: IconFormat::Svg,
            bytes: bytes.into(),
        }),
        kind: IconKind::Picture,
    }
}

/// A window on [`SHOWN`] whose rows came with `icons`.
fn shown_with_icons(
    cx: &mut TestAppContext,
    icons: Vec<FileIcon>,
) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let (view, cx, mut core) = window(cx);
    let id = answer_listing(&mut core, ENTRIES);
    core.send(Event::DetailsRead {
        id,
        first: EntryId(0),
        details: icons.iter().map(|_| sized(1)).collect(),
        icons: icons.into(),
    });
    cx.run_until_parked();
    (view, cx, core)
}

fn made(view: &Entity<MainWindow>, cx: &VisualTestContext, row: usize) -> Option<gpui::ImageId> {
    view.read_with(cx, |view, _| view.theme_icon(row).map(|image| image.id))
}

#[gpui::test]
fn rows_of_one_type_share_one_made_icon(cx: &mut TestAppContext) {
    let picture = theme_icon("/icons/picture.svg", SQUARE.as_bytes());
    let icons = vec![picture.clone(), picture, FileIcon::BuiltIn(IconKind::Text)];
    let (view, cx, _core) = shown_with_icons(cx, icons);

    let first = made(&view, cx, 0);
    assert!(first.is_some());
    assert_eq!(made(&view, cx, 1), first);
    assert_eq!(made(&view, cx, 2), None);
}

#[gpui::test]
fn an_icon_that_does_not_decode_is_not_drawn(cx: &mut TestAppContext) {
    let icons = vec![theme_icon("/icons/broken.svg", b"not svg")];
    let (view, cx, _core) = shown_with_icons(cx, icons);

    assert_eq!(made(&view, cx, 0), None);
}

#[gpui::test]
fn another_scale_makes_the_icons_again(cx: &mut TestAppContext) {
    let icons = vec![theme_icon("/icons/picture.svg", SQUARE.as_bytes())];
    let (view, cx, _core) = shown_with_icons(cx, icons);
    let before = made(&view, cx, 0).unwrap();

    let scale = cx.update(|window, _| window.scale_factor());
    view.update_in(cx, |view, window, cx| {
        view.set_scale_factor(scale, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(made(&view, cx, 0), Some(before));

    view.update_in(cx, |view, window, cx| {
        view.set_scale_factor(scale * 2.0, window, cx);
    });
    cx.run_until_parked();
    let after = made(&view, cx, 0);
    assert!(after.is_some());
    assert_ne!(after, Some(before));
}
