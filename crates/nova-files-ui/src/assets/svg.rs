//! The icons' SVG sources, as drawn on the design page. Things you can
//! open (folders, files, places) are solid two-tone shapes: the front at
//! full strength, the back at half, so one tint draws both. Things you can
//! do are thin line icons. GPUI draws each as a mask, so the color written
//! here doesn't matter; only how much of it shows does.

// Things: solid, two-tone.
pub const FOLDER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M2.5 7A2.5 2.5 0 0 1 5 4.5h3.6c.75 0 1.45.33 1.93.9l.92 1.1H19A2.5 2.5 0 0 1 21.5 9v3h-19z"/><rect x="2.5" y="8.6" width="19" height="11.4" rx="2.8"/></svg>"##;
pub const FILE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".38" d="M6.5 2.5h6.8L19.5 8.7V19a2.5 2.5 0 0 1-2.5 2.5H6.5A2.5 2.5 0 0 1 4 19V5a2.5 2.5 0 0 1 2.5-2.5z"/><path opacity=".7" d="M13.3 2.5l6.2 6.2h-4a2.2 2.2 0 0 1-2.2-2.2z"/></svg>"##;
pub const HOME: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M3.5 10.6 12 3.4l8.5 7.2V18a2.6 2.6 0 0 1-2.6 2.6H6.1A2.6 2.6 0 0 1 3.5 18z"/><path d="M9.6 20.6v-5a1.6 1.6 0 0 1 1.6-1.6h1.6a1.6 1.6 0 0 1 1.6 1.6v5z"/></svg>"##;
pub const DESKTOP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect x="2.5" y="4" width="19" height="12.6" rx="2.8"/><path opacity=".5" d="M9.4 16.6h5.2l.9 3.6h-7z"/></svg>"##;
pub const DOCUMENTS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="4.5" y="2.5" width="15" height="19" rx="2.8"/><rect x="7.6" y="7" width="8.8" height="1.9" rx=".95"/><rect x="7.6" y="11" width="8.8" height="1.9" rx=".95"/><rect x="7.6" y="15" width="5" height="1.9" rx=".95"/></svg>"##;
pub const DOWNLOADS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M3 14.5h4.4l1.2 2.2h6.8l1.2-2.2H21V18a2.6 2.6 0 0 1-2.6 2.6H5.6A2.6 2.6 0 0 1 3 18z"/><path fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" d="M12 3.6v8.6m-3.4-3.3 3.4 3.4 3.4-3.4"/></svg>"##;
pub const PICTURES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="2.5" y="4" width="19" height="16" rx="3"/><circle cx="8.3" cy="9.4" r="1.9"/><path d="M4.6 18.4 9.4 13l2.9 3 2.7-2.8 4.4 5.2z"/></svg>"##;
pub const MUSIC: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M8.9 6.1 19.6 3.6v11.6h-1.9V7.6l-6.9 1.6v8.4H8.9z"/><circle cx="7.3" cy="17.6" r="3.1"/><circle cx="16.9" cy="15.4" r="3.1"/></svg>"##;
pub const VIDEOS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="2.5" y="4.5" width="19" height="15" rx="3"/><path d="M10 8.9v6.2c0 .5.5.8.9.5l5.2-3.1a.6.6 0 0 0 0-1l-5.2-3.1a.6.6 0 0 0-.9.5z"/></svg>"##;
pub const DRIVE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="2.5" y="4.5" width="19" height="15" rx="3"/><path d="M2.5 13h19v3.5a3 3 0 0 1-3 3h-13a3 3 0 0 1-3-3z"/></svg>"##;

// Actions: lines.
pub const BACK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14.5 6 8.5 12l6 6"/></svg>"##;
pub const FORWARD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m9.5 6 6 6-6 6"/></svg>"##;
pub const UP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5.5m-5.5 5.5L12 5.5l5.5 5.5"/></svg>"##;
/// A column head's arrow while its column orders the list, A to Z or
/// oldest or smallest first; [`SORT_DOWN`] the other way.
pub const SORT_UP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m5 16 7-7 7 7"/></svg>"##;
pub const SORT_DOWN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 7 7 7-7"/></svg>"##;
pub const SEARCH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/></svg>"##;
pub const LIST: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 7h15m-15 5h15m-15 5h15"/></svg>"##;
pub const TILES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4.2" y="4.2" width="6.4" height="6.4" rx="1.6"/><rect x="13.4" y="4.2" width="6.4" height="6.4" rx="1.6"/><rect x="4.2" y="13.4" width="6.4" height="6.4" rx="1.6"/><rect x="13.4" y="13.4" width="6.4" height="6.4" rx="1.6"/></svg>"##;
/// The arrow after a crumb in the path bar.
pub const CHEVRON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m9 5 7 7-7 7"/></svg>"##;
pub const CLOSE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m6 6 12 12M18 6 6 18"/></svg>"##;
