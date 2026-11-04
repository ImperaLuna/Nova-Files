//! Nova Files: a first GPUI scaffold, a rough build of the iteration 2 mock.
//!
//! A top bar with back, forward and up buttons, a clickable path bar and a
//! search field; a sidebar with places and drives; a visual Home page; and a
//! details list (name, modified, type, size) for every other folder.

mod fs;

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, App, Application, Bounds, Context, Hsla, SharedString, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, relative, rgb, rgba, size, uniform_list,
};

use crate::fs::{Drive, Entry};

/// Colors from the warm dark palette of the HTML mock. A real theme comes later.
mod palette {
    pub const WINDOW: u32 = 0x1a1918;
    pub const BAR: u32 = 0x171615;
    pub const SUNK: u32 = 0x161514;
    pub const RAISED: u32 = 0x2a2724;
    pub const TEXT: u32 = 0xf5e7c6;
    pub const TEXT_SOFT: u32 = 0xd8ccb0;
    pub const TEXT_DIM: u32 = 0xa8a296;
    pub const TEXT_FAINT: u32 = 0x8a857c;
    pub const TEXT_OFF: u32 = 0x4a4743;
    /// Hairlines: the text color at 10% opacity, 25% on hover.
    pub const LINE: u32 = 0xf5e7c61a;
    pub const LINE_HOVER: u32 = 0xf5e7c640;
    /// Series colors, used to tint folders.
    pub const BLUE: u32 = 0x5988db;
    pub const ORANGE: u32 = 0xe8650e;
    pub const TEAL: u32 = 0x07a7a5;
    pub const PURPLE: u32 = 0x9067cc;
    pub const SKY: u32 = 0x049be7;
    pub const PINK: u32 = 0xdd6098;
    pub const OLIVE: u32 = 0x9c7c02;
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn alpha(hex: u32) -> Hsla {
    rgba(hex).into()
}

/// The standard folders, in sidebar order, with their Home card tint.
const PLACES: [(&str, u32); 7] = [
    ("Desktop", palette::SKY),
    ("Documents", palette::BLUE),
    ("Downloads", palette::TEAL),
    ("Pictures", palette::PINK),
    ("Music", palette::PURPLE),
    ("Videos", palette::ORANGE),
    ("Projects", palette::OLIVE),
];

struct Place {
    name: SharedString,
    path: PathBuf,
    tint: u32,
    items: usize,
}

struct NovaFiles {
    home: PathBuf,
    user: SharedString,
    places: Vec<Place>,
    drives: Vec<Drive>,
    recent: Vec<PathBuf>,
    current: PathBuf,
    entries: Vec<Entry>,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
}

impl NovaFiles {
    fn new() -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let user = std::env::var("USER").unwrap_or_else(|_| "there".into());
        let places = PLACES
            .iter()
            .map(|&(name, tint)| (name, tint, home.join(name)))
            .filter(|(_, _, path)| path.is_dir())
            .map(|(name, tint, path)| Place {
                name: name.into(),
                items: fs::count_entries(&path),
                path,
                tint,
            })
            .collect();
        let mut this = Self {
            drives: fs::drives(),
            recent: fs::recent_files(&home, 6),
            user: user.into(),
            places,
            current: PathBuf::new(),
            entries: Vec::new(),
            back: Vec::new(),
            forward: Vec::new(),
            home: home.clone(),
        };
        this.load(home);
        this
    }

    fn load(&mut self, path: PathBuf) {
        self.entries = fs::list_entries(&path);
        self.current = path;
    }

    /// Opens a folder and records it in the back history.
    fn open(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path == self.current || !path.is_dir() {
            return;
        }
        self.back.push(std::mem::take(&mut self.current));
        self.forward.clear();
        self.load(path);
        cx.notify();
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.back.pop() {
            self.forward.push(std::mem::take(&mut self.current));
            self.load(path);
            cx.notify();
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.forward.pop() {
            self.back.push(std::mem::take(&mut self.current));
            self.load(path);
            cx.notify();
        }
    }

    fn go_up(&mut self, cx: &mut Context<Self>) {
        if let Some(parent) = self.current.parent() {
            self.open(parent.to_path_buf(), cx);
        }
    }

    fn at_home(&self) -> bool {
        self.current == self.home
    }

    /// The path as clickable segments. Inside the home folder it starts at
    /// "Home", elsewhere at "/".
    fn crumbs(&self) -> Vec<(SharedString, PathBuf)> {
        let (mut path, first, rest) = match self.current.strip_prefix(&self.home) {
            Ok(rest) => (self.home.clone(), "Home", rest),
            Err(_) => (
                PathBuf::from("/"),
                "/",
                self.current.strip_prefix("/").unwrap_or(&self.current),
            ),
        };
        let mut crumbs = vec![(SharedString::from(first), path.clone())];
        for part in rest.components() {
            path.push(part);
            crumbs.push((
                part.as_os_str().to_string_lossy().into_owned().into(),
                path.clone(),
            ));
        }
        crumbs
    }
}

// Shared pieces.

/// A rounded folder shape: a tab on top of a rounded body.
fn folder_glyph(tint: Hsla, scale: f32) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .w(px(7. * scale))
                .h(px(3. * scale))
                .rounded_t(px(2. * scale))
                .bg(tint),
        )
        .child(
            div()
                .w(px(16. * scale))
                .h(px(11. * scale))
                .rounded(px(3. * scale))
                .bg(tint),
        )
}

/// A page outline for files.
fn file_glyph(tint: Hsla) -> impl IntoElement {
    div()
        .w(px(11.))
        .h(px(14.))
        .rounded(px(3.))
        .border_2()
        .border_color(tint)
}

fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(color(palette::TEXT_FAINT))
        .child(text)
}

/// A rounded free-space meter.
fn meter(fraction: f32) -> impl IntoElement {
    div()
        .h(px(6.))
        .w_full()
        .rounded_full()
        .bg(color(palette::RAISED))
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0., 1.)))
                .rounded_full()
                .bg(color(palette::BLUE)),
        )
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// Top bar.

impl NovaFiles {
    fn nav_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        enabled: bool,
        on_click: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(34.))
            .rounded_full()
            .text_color(color(if enabled {
                palette::TEXT_DIM
            } else {
                palette::TEXT_OFF
            }))
            .when(enabled, |d| {
                d.cursor_pointer()
                    .hover(|d| {
                        d.bg(color(palette::RAISED))
                            .text_color(color(palette::TEXT))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
            })
            .child(glyph)
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let crumbs = self.crumbs();
        let depth = crumbs.len() - 1;
        let last = crumbs.len() - 1;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_3p5()
            .py_3()
            .bg(color(palette::BAR))
            .border_b_1()
            .border_color(alpha(palette::LINE))
            .child(self.nav_button("back", "←", !self.back.is_empty(), Self::go_back, cx))
            .child(self.nav_button(
                "forward",
                "→",
                !self.forward.is_empty(),
                Self::go_forward,
                cx,
            ))
            .child(self.nav_button("up", "↑", self.current.parent().is_some(), Self::go_up, cx))
            // The path bar: every level is a button.
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .h(px(38.))
                    .px_1p5()
                    .rounded_full()
                    .bg(color(palette::SUNK))
                    .border_1()
                    .border_color(alpha(palette::LINE))
                    .hover(|d| d.border_color(alpha(palette::LINE_HOVER)))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .overflow_hidden()
                            .children(crumbs.into_iter().enumerate().flat_map(
                                |(ix, (name, path))| {
                                    let separator = (ix > 0).then(|| {
                                        div()
                                            .flex_none()
                                            .px_0p5()
                                            .text_xs()
                                            .text_color(color(palette::TEXT_FAINT))
                                            .child("›")
                                            .into_any_element()
                                    });
                                    let crumb = div()
                                        .id(("crumb", ix))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .h(px(28.))
                                        .px_2p5()
                                        .rounded_full()
                                        .text_sm()
                                        .text_color(color(if ix == last {
                                            palette::TEXT
                                        } else {
                                            palette::TEXT_SOFT
                                        }))
                                        .when(ix == last, |d| {
                                            d.font_weight(gpui::FontWeight::SEMIBOLD)
                                        })
                                        .cursor_pointer()
                                        .hover(|d| {
                                            d.bg(color(palette::RAISED))
                                                .text_color(color(palette::TEXT))
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.open(path.clone(), cx)
                                        }))
                                        .child(name)
                                        .into_any_element();
                                    separator.into_iter().chain([crumb])
                                },
                            )),
                    )
                    .when(depth > 0, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .px_2()
                                .text_xs()
                                .text_color(color(palette::TEXT_FAINT))
                                .child(format!("{} deep", plural(depth, "level", "levels"))),
                        )
                    }),
            )
            // Search is a placeholder for now.
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .w(px(200.))
                    .h(px(38.))
                    .px_3p5()
                    .rounded_full()
                    .bg(color(palette::SUNK))
                    .border_1()
                    .border_color(alpha(palette::LINE))
                    .text_sm()
                    .text_color(color(palette::TEXT_FAINT))
                    .child("Search"),
            )
    }
}

// Sidebar.

impl NovaFiles {
    fn sidebar_item(
        &self,
        ix: usize,
        name: SharedString,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = path == self.current;
        div()
            .id(("place", ix))
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(36.))
            .px_3()
            .rounded(px(10.))
            .text_sm()
            .text_color(color(if active {
                palette::TEXT
            } else {
                palette::TEXT_SOFT
            }))
            .when(active, |d| d.bg(color(palette::RAISED)))
            .hover(|d| {
                d.bg(color(palette::RAISED))
                    .text_color(color(palette::TEXT))
            })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.open(path.clone(), cx)))
            .child(folder_glyph(color(palette::TEXT_DIM), 1.))
            .child(name)
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let home = self.sidebar_item(0, "Home".into(), self.home.clone(), cx);
        let mut places = Vec::with_capacity(self.places.len());
        for (ix, p) in self.places.iter().enumerate() {
            places.push(self.sidebar_item(ix + 1, p.name.clone(), p.path.clone(), cx));
        }
        div()
            .id("sidebar")
            .flex()
            .flex_col()
            .gap_0p5()
            .flex_none()
            .w(px(220.))
            .h_full()
            .overflow_y_scroll()
            .px_2p5()
            .py_3p5()
            .border_r_1()
            .border_color(alpha(palette::LINE))
            .child(
                div()
                    .px_3()
                    .pt_2p5()
                    .pb_1p5()
                    .child(section_label("PLACES")),
            )
            .child(home)
            .children(places)
            .when(!self.drives.is_empty(), |d| {
                d.child(div().px_3().pt_4().pb_1p5().child(section_label("DRIVES")))
            })
            .children(self.drives.iter().enumerate().map(|(ix, drive)| {
                let mount = drive.mount.clone();
                div()
                    .id(("drive", ix))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .mx_0p5()
                    .my_1()
                    .p_3()
                    .rounded(px(14.))
                    .bg(color(palette::SUNK))
                    .border_1()
                    .border_color(alpha(palette::LINE))
                    .hover(|d| d.border_color(alpha(palette::LINE_HOVER)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.open(mount.clone(), cx)))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(drive.name.clone()),
                    )
                    .child(meter(drive.used_fraction()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(color(palette::TEXT_DIM))
                            .child(format!(
                                "{} free of {}",
                                fs::format_size(drive.free),
                                fs::format_size(drive.total)
                            )),
                    )
            }))
    }
}

// Home: a visual start page.

impl NovaFiles {
    fn home_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let folder_cards = self.places.iter().enumerate().map(|(ix, place)| {
            let tint = color(place.tint);
            let path = place.path.clone();
            div()
                .id(("card", ix))
                .flex()
                .flex_col()
                .justify_between()
                .w(px(170.))
                .h(px(112.))
                .p_4()
                .rounded(px(20.))
                .border_1()
                .border_color(alpha(palette::LINE))
                .bg(tint.opacity(0.09))
                .hover(|d| {
                    d.bg(tint.opacity(0.16))
                        .border_color(alpha(palette::LINE_HOVER))
                })
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.open(path.clone(), cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(38.))
                        .rounded(px(10.))
                        .bg(tint.opacity(0.22))
                        .child(folder_glyph(tint, 1.2)),
                )
                .child(
                    div()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(place.name.clone()),
                        )
                        .child(div().text_xs().text_color(color(palette::TEXT_DIM)).child(
                            if place.items == 0 {
                                "Empty".into()
                            } else {
                                plural(place.items, "item", "items")
                            },
                        )),
                )
        });

        let drive_cards = self.drives.iter().map(|drive| {
            div()
                .flex()
                .flex_col()
                .gap_2p5()
                .w(px(260.))
                .p_4()
                .rounded(px(20.))
                .bg(color(palette::SUNK))
                .border_1()
                .border_color(alpha(palette::LINE))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_baseline()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(drive.name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(color(palette::TEXT_DIM))
                                .child(format!("{:.0}% used", drive.used_fraction() * 100.)),
                        ),
                )
                .child(meter(drive.used_fraction()))
                .child(
                    div()
                        .text_xs()
                        .text_color(color(palette::TEXT_DIM))
                        .child(format!(
                            "{} free of {}",
                            fs::format_size(drive.free),
                            fs::format_size(drive.total)
                        )),
                )
        });

        let recent_rows = self.recent.iter().enumerate().map(|(ix, file)| {
            let name: SharedString = file
                .file_name()
                .map_or("".into(), |n| n.to_string_lossy().into_owned())
                .into();
            let parent = file.parent().map(Path::to_path_buf).unwrap_or_default();
            let shown_parent = match parent.strip_prefix(&self.home) {
                Ok(rest) => format!("~/{}", rest.display()),
                Err(_) => parent.display().to_string(),
            };
            let size = std::fs::metadata(file)
                .map(|m| fs::format_size(m.len()))
                .unwrap_or_default();
            let kind = fs::kind(&name, false);
            details_row(("recent", ix), name, false, shown_parent, kind, size)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.open(parent.clone(), cx)))
        });

        div()
            .id("home")
            .flex()
            .flex_col()
            .gap_3p5()
            .flex_1()
            .overflow_y_scroll()
            .px_5()
            .py(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .pb_1()
                    .child(
                        div()
                            .text_3xl()
                            .child(format!("Welcome back, {}", self.user)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(color(palette::TEXT_DIM))
                            .child("Your folders, drives and what you opened last."),
                    ),
            )
            .child(div().pt_2().child(section_label("FOLDERS")))
            .child(div().flex().flex_wrap().gap_3().children(folder_cards))
            .when(!self.drives.is_empty(), |d| {
                d.child(div().pt_2().child(section_label("DRIVES")))
                    .child(div().flex().flex_wrap().gap_3().children(drive_cards))
            })
            .when(!self.recent.is_empty(), |d| {
                d.child(div().pt_2().child(section_label("RECENT"))).child(
                    div()
                        .flex()
                        .flex_col()
                        .p_1()
                        .rounded(px(20.))
                        .border_1()
                        .border_color(alpha(palette::LINE))
                        .children(recent_rows),
                )
            })
            .into_any_element()
    }
}

// Every other folder: a details list, top to bottom.

const ROW_HEIGHT: f32 = 36.;
const COL_MODIFIED: f32 = 160.;
const COL_TYPE: f32 = 120.;
const COL_SIZE: f32 = 80.;

fn details_row(
    id: impl Into<gpui::ElementId>,
    name: SharedString,
    is_dir: bool,
    modified: String,
    kind: String,
    size: String,
) -> gpui::Stateful<gpui::Div> {
    let glyph = if is_dir {
        folder_glyph(color(palette::BLUE), 1.).into_any_element()
    } else {
        file_glyph(color(palette::TEXT_DIM)).into_any_element()
    };
    let cell = |w: f32| {
        div()
            .flex_none()
            .w(px(w))
            .text_xs()
            .text_color(color(palette::TEXT_DIM))
            .truncate()
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_3()
        .h(px(ROW_HEIGHT))
        .px_3()
        .rounded(px(10.))
        .text_sm()
        .text_color(color(palette::TEXT_SOFT))
        .hover(|d| {
            d.bg(color(palette::RAISED))
                .text_color(color(palette::TEXT))
        })
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_2p5()
                .child(
                    div()
                        .flex_none()
                        .w(px(18.))
                        .flex()
                        .justify_center()
                        .child(glyph),
                )
                .child(div().min_w_0().truncate().child(name)),
        )
        .child(cell(COL_MODIFIED).child(modified))
        .child(cell(COL_TYPE).child(kind))
        .child(cell(COL_SIZE).flex().justify_end().child(size))
}

impl NovaFiles {
    fn folder_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let title = self
            .current
            .file_name()
            .map_or("/".to_owned(), |n| n.to_string_lossy().into_owned());
        let folders = self.entries.iter().filter(|e| e.is_dir).count();
        let files = self.entries.len() - folders;
        let header_cell = |text: &'static str, w: f32| div().flex_none().w(px(w)).child(text);

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_3p5()
            .px_5()
            .py(px(18.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(div().text_3xl().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(color(palette::TEXT_FAINT))
                            .child(format!(
                                "{} · {}",
                                plural(folders, "folder", "folders"),
                                plural(files, "file", "files")
                            )),
                    ),
            )
            .child(if self.entries.is_empty() {
                div()
                    .p_7()
                    .rounded(px(14.))
                    .border_1()
                    .border_color(alpha(palette::LINE_HOVER))
                    .text_sm()
                    .text_color(color(palette::TEXT_DIM))
                    .flex()
                    .justify_center()
                    .child("This folder is empty.")
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap_3()
                            .h(px(30.))
                            .px_3()
                            .mb_1()
                            .border_b_1()
                            .border_color(alpha(palette::LINE))
                            .text_xs()
                            .text_color(color(palette::TEXT_FAINT))
                            .child(div().flex_1().pl(px(28.)).child("NAME"))
                            .child(header_cell("MODIFIED", COL_MODIFIED))
                            .child(header_cell("TYPE", COL_TYPE))
                            .child(header_cell("SIZE", COL_SIZE).flex().justify_end()),
                    )
                    // Only the visible rows are built, so big folders stay fast.
                    .child(
                        uniform_list(
                            "entries",
                            self.entries.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                                range
                                    .map(|ix| {
                                        let entry = &this.entries[ix];
                                        let row = details_row(
                                            ("entry", ix),
                                            entry.name.clone(),
                                            entry.is_dir,
                                            entry.modified.map(fs::format_time).unwrap_or_default(),
                                            fs::kind(&entry.name, entry.is_dir),
                                            entry.size.map(fs::format_size).unwrap_or_default(),
                                        );
                                        if entry.is_dir {
                                            let path = entry.path.clone();
                                            row.cursor_pointer().on_click(cx.listener(
                                                move |this, _, _, cx| this.open(path.clone(), cx),
                                            ))
                                        } else {
                                            row
                                        }
                                    })
                                    .collect()
                            }),
                        )
                        .flex_1(),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }
}

impl Render for NovaFiles {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = if self.at_home() {
            self.home_page(cx)
        } else {
            self.folder_page(cx)
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(color(palette::WINDOW))
            .text_color(color(palette::TEXT))
            .child(self.top_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(page)),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some("nova-files".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|_| NovaFiles::new()),
        )
        .expect("failed to open the window");
        cx.activate(true);
    });
}
