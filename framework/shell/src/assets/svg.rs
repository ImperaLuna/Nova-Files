// GPUI draws an SVG as a mask, so only its alpha matters.
// https://github.com/ImperaLuna/zed/tree/47424cdc3f1a49829960e1ecbcdcdd967d034265/crates/gpui
pub const FOLDER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M2.5 7A2.5 2.5 0 0 1 5 4.5h3.6c.75 0 1.45.33 1.93.9l.92 1.1H19A2.5 2.5 0 0 1 21.5 9v3h-19z"/><rect x="2.5" y="8.6" width="19" height="11.4" rx="2.8"/></svg>"##;
macro_rules! file_with {
    ($mark:literal) => {
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".38" d="M6.5 2.5h6.8L19.5 8.7V19a2.5 2.5 0 0 1-2.5 2.5H6.5A2.5 2.5 0 0 1 4 19V5a2.5 2.5 0 0 1 2.5-2.5z"/><path opacity=".7" d="M13.3 2.5l6.2 6.2h-4a2.2 2.2 0 0 1-2.2-2.2z"/>"##,
            $mark,
            "</svg>"
        )
    };
}

pub const FILE: &str = file_with!("");
pub const FILE_TEXT: &str = file_with!(
    r##"<rect x="7.2" y="11.2" width="9.2" height="1.7" rx=".85"/><rect x="7.2" y="14.4" width="9.2" height="1.7" rx=".85"/><rect x="7.2" y="17.6" width="5.6" height="1.7" rx=".85"/>"##
);
pub const FILE_PICTURE: &str = file_with!(
    r##"<circle cx="9.2" cy="12.6" r="1.7"/><path d="M6.6 19.1 10 15.2l2.1 2.2 2-2.1 3 3.8z"/>"##
);
pub const FILE_VIDEO: &str = file_with!(
    r##"<path d="M9.4 11.6v6.8c0 .5.5.8.9.5l5.2-3.4a.6.6 0 0 0 0-1l-5.2-3.4a.6.6 0 0 0-.9.5z"/>"##
);
pub const FILE_AUDIO: &str = file_with!(
    r##"<path d="M11.4 11.4 16.4 10v6.8h-1.6v-4.6l-1.8.5v5.2h-1.6z"/><circle cx="10.4" cy="18" r="1.9"/><circle cx="14.8" cy="16.8" r="1.9"/>"##
);
pub const FILE_ARCHIVE: &str = file_with!(
    r##"<rect x="9.9" y="3" width="1.9" height="1.6" rx=".5"/><rect x="11.6" y="4.8" width="1.9" height="1.6" rx=".5"/><rect x="9.9" y="6.6" width="1.9" height="1.6" rx=".5"/><rect x="11.6" y="8.4" width="1.9" height="1.6" rx=".5"/><rect x="9.9" y="10.2" width="1.9" height="1.6" rx=".5"/><rect x="9.7" y="12.4" width="4" height="5.4" rx="1.2"/>"##
);
pub const FILE_PROGRAM: &str = file_with!(
    r##"<path fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" d="m7.6 12.3 2.8 2.6-2.8 2.6m4.6.4h4.2"/>"##
);
pub const HOME: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M3.5 10.6 12 3.4l8.5 7.2V18a2.6 2.6 0 0 1-2.6 2.6H6.1A2.6 2.6 0 0 1 3.5 18z"/><path d="M9.6 20.6v-5a1.6 1.6 0 0 1 1.6-1.6h1.6a1.6 1.6 0 0 1 1.6 1.6v5z"/></svg>"##;
pub const DESKTOP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect x="2.5" y="4" width="19" height="12.6" rx="2.8"/><path opacity=".5" d="M9.4 16.6h5.2l.9 3.6h-7z"/></svg>"##;
pub const DOCUMENTS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="4.5" y="2.5" width="15" height="19" rx="2.8"/><rect x="7.6" y="7" width="8.8" height="1.9" rx=".95"/><rect x="7.6" y="11" width="8.8" height="1.9" rx=".95"/><rect x="7.6" y="15" width="5" height="1.9" rx=".95"/></svg>"##;
pub const DOWNLOADS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M3 14.5h4.4l1.2 2.2h6.8l1.2-2.2H21V18a2.6 2.6 0 0 1-2.6 2.6H5.6A2.6 2.6 0 0 1 3 18z"/><path fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" d="M12 3.6v8.6m-3.4-3.3 3.4 3.4 3.4-3.4"/></svg>"##;
pub const PICTURES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="2.5" y="4" width="19" height="16" rx="3"/><circle cx="8.3" cy="9.4" r="1.9"/><path d="M4.6 18.4 9.4 13l2.9 3 2.7-2.8 4.4 5.2z"/></svg>"##;
pub const MUSIC: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M8.9 6.1 19.6 3.6v11.6h-1.9V7.6l-6.9 1.6v8.4H8.9z"/><circle cx="7.3" cy="17.6" r="3.1"/><circle cx="16.9" cy="15.4" r="3.1"/></svg>"##;
pub const VIDEOS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="2.5" y="4.5" width="19" height="15" rx="3"/><path d="M10 8.9v6.2c0 .5.5.8.9.5l5.2-3.1a.6.6 0 0 0 0-1l-5.2-3.1a.6.6 0 0 0-.9.5z"/></svg>"##;
pub const TRASH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M5.4 9h13.2l-.9 10.1a2.6 2.6 0 0 1-2.6 2.4H8.9a2.6 2.6 0 0 1-2.6-2.4z"/><rect x="3.5" y="5.2" width="17" height="2.5" rx="1.25"/><path d="M9 5.4V4.1a1.6 1.6 0 0 1 1.6-1.6h2.8A1.6 1.6 0 0 1 15 4.1v1.3z"/></svg>"##;
pub const PROJECTS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="2.5" y="4" width="19" height="16" rx="3"/><path fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M9.6 9.4 7 12l2.6 2.6m4.8-5.2L17 12l-2.6 2.6"/></svg>"##;
pub const GAMES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path opacity=".5" d="M7.6 6.5h8.8a5.4 5.4 0 0 1 5.3 4.4l.9 4.9a2.6 2.6 0 0 1-4.6 2.1L16.4 16H7.6L6 17.9a2.6 2.6 0 0 1-4.6-2.1l.9-4.9a5.4 5.4 0 0 1 5.3-4.4z"/><path d="M7 9h1.8v1.8h1.8v1.8H8.8v1.8H7v-1.8H5.2v-1.8H7z"/><circle cx="14.9" cy="12.9" r="1.2"/><circle cx="17.8" cy="10.5" r="1.2"/></svg>"##;
pub const START: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path d="M12 2c.8 5.6 4.4 9.2 10 10-5.6.8-9.2 4.4-10 10-.8-5.6-4.4-9.2-10-10 5.6-.8 9.2-4.4 10-10z"/></svg>"##;
pub const DRIVE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><rect opacity=".5" x="2.5" y="4.5" width="19" height="15" rx="3"/><path d="M2.5 13h19v3.5a3 3 0 0 1-3 3h-13a3 3 0 0 1-3-3z"/></svg>"##;

pub const BACK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14.5 6 8.5 12l6 6"/></svg>"##;
pub const FORWARD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m9.5 6 6 6-6 6"/></svg>"##;
pub const UP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5.5m-5.5 5.5L12 5.5l5.5 5.5"/></svg>"##;
pub const SORT_UP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m5 16 7-7 7 7"/></svg>"##;
pub const SORT_DOWN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 7 7 7-7"/></svg>"##;
pub const SEARCH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/></svg>"##;
pub const LIST: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 7h15m-15 5h15m-15 5h15"/></svg>"##;
pub const TILES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4.2" y="4.2" width="6.4" height="6.4" rx="1.6"/><rect x="13.4" y="4.2" width="6.4" height="6.4" rx="1.6"/><rect x="4.2" y="13.4" width="6.4" height="6.4" rx="1.6"/><rect x="13.4" y="13.4" width="6.4" height="6.4" rx="1.6"/></svg>"##;
pub const COPY: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="8.5" y="8.5" width="11.5" height="11.5" rx="2.6"/><path d="M5.6 15.5A1.9 1.9 0 0 1 4 13.6V6a2 2 0 0 1 2-2h7.6a1.9 1.9 0 0 1 1.9 1.6"/></svg>"##;
pub const CUT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="6.5" cy="6.5" r="2.6"/><circle cx="6.5" cy="17.5" r="2.6"/><path d="M8.7 8.2 20 18.5M8.7 15.8 20 5.5"/></svg>"##;
pub const RENAME: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20l1.1-4.6L16.4 4.1a2.1 2.1 0 0 1 3 3L8.1 18.4z"/></svg>"##;
pub const PLUS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14m-7-7h14"/></svg>"##;
pub const CLOSE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 6 12 12M18 6 6 18"/></svg>"##;
pub const CHEVRON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m9 5 7 7-7 7"/></svg>"##;
pub const EJECT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5 5.5 13.5h13zM5.5 18.5h13"/></svg>"##;
pub const OPEN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h13.5m-5-5 5 5-5 5"/></svg>"##;
pub const CHECK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path stroke-width="2" d="m5 12.5 4.8 4.8L19 7.5"/></svg>"##;
pub const EYE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M2.6 12s3.4-6.4 9.4-6.4 9.4 6.4 9.4 6.4-3.4 6.4-9.4 6.4S2.6 12 2.6 12z"/><circle cx="12" cy="12" r="2.8"/></svg>"##;
pub const PASTE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="4.8" width="14" height="16.2" rx="2.6"/><rect x="9" y="2.8" width="6" height="4" rx="1.3"/></svg>"##;
pub const SELECT_ALL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="4" width="16" height="16" rx="3.2"/><path d="m8.5 12.3 2.5 2.5 4.5-5"/></svg>"##;
pub const NEW_FOLDER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 7.2a2.2 2.2 0 0 1 2.2-2.2h3.4l2 2.5h7.2a2.2 2.2 0 0 1 2.2 2.2V17a2.2 2.2 0 0 1-2.2 2.2H5.7A2.2 2.2 0 0 1 3.5 17zM12 10.8v4.8m-2.4-2.4h4.8"/></svg>"##;
pub const UNDO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M8 5 4 9l4 4M4 9h9.5a5.5 5.5 0 0 1 0 11H10"/></svg>"##;
pub const INFO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.5"/><path d="M12 7.7v.1" stroke-width="2.4"/></svg>"##;
pub const PERSON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="8.2" r="3.6"/><path d="M5 19.5a7 7 0 0 1 14 0"/></svg>"##;
pub const COPY_PATH: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="5" width="17" height="14" rx="3.2"/><path d="m10.6 15.4 2.8-6.8"/></svg>"##;
pub const TERMINAL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="15" rx="3.2"/><path d="m7.5 9.5 2.8 2.5-2.8 2.5M12.5 15h4"/></svg>"##;
pub const NEW_FILE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M13 3.5H7.5a2.2 2.2 0 0 0-2.2 2.2v12.6a2.2 2.2 0 0 0 2.2 2.2h9a2.2 2.2 0 0 0 2.2-2.2V9.2zM13 3.5v5.7h5.7M12 12.3v5m-2.5-2.5h5"/></svg>"##;
