//! `check` against hand-made files: a good tree, then one fault at a time.

use super::*;
use crate::FileId;
use crate::format::NO_STAMP;

const NAMES: &[u8] = b"/homedocsa.txtb.txt";

/// `/home` holding `docs/a.txt` and `b.txt`: parent, end, where the name
/// starts and how long it is, and kind.
const TREE: [(u32, u32, u32, u16, Kind); 4] = [
    (NO_PARENT, 4, 0, 5, Kind::Folder),
    (0, 3, 5, 4, Kind::Folder),
    (1, 3, 9, 5, Kind::File),
    (0, 4, 14, 5, Kind::File),
];

fn tree() -> Vec<Row> {
    TREE.iter()
        .map(|&(parent, end, name_at, name_len, kind)| Row {
            parent,
            end,
            name_at,
            name_len,
            kind: Some(kind),
            flags: 0,
            id: FileId {
                device: 1,
                inode: u64::from(end),
            },
            size: 0,
            modified: NO_STAMP,
            created: NO_STAMP,
        })
        .collect()
}

fn file_with(header: Header, rows: &[Row], names: &[u8]) -> Vec<u8> {
    let mut bytes = header.write().to_vec();
    for row in rows {
        bytes.extend_from_slice(&row.write());
    }
    bytes.extend_from_slice(names);
    bytes
}

fn file(rows: &[Row]) -> Vec<u8> {
    let header = Header::current(rows.len() as u64, NAMES.len() as u64, NO_STAMP);
    file_with(header, rows, NAMES)
}

fn damaged(rows: &[Row]) -> &'static str {
    match check(&file(rows)) {
        Err(OpenError::Damaged(reason)) => reason,
        other => panic!("expected a damaged file, got {other:?}"),
    }
}

#[test]
fn a_good_tree_passes() {
    let header = check(&file(&tree())).unwrap();
    assert_eq!(header.rows, 4);
}

#[test]
fn another_file_is_not_an_index() {
    let mut bytes = file(&tree());
    bytes[0] = b'X';
    assert_eq!(check(&bytes), Err(OpenError::NotAnIndex));
}

#[test]
fn another_layout_is_out_of_date() {
    let rows = tree();
    let mut header = Header::current(4, NAMES.len() as u64, NO_STAMP);
    header.version = VERSION + 1;
    assert_eq!(
        check(&file_with(header, &rows, NAMES)),
        Err(OpenError::OutOfDate)
    );
    let mut header = Header::current(4, NAMES.len() as u64, NO_STAMP);
    header.row_len += 1;
    assert_eq!(
        check(&file_with(header, &rows, NAMES)),
        Err(OpenError::OutOfDate)
    );
}

#[test]
fn a_cut_or_padded_file_is_damaged() {
    let bytes = file(&tree());
    let wrong_length = Err(OpenError::Damaged("length doesn't match the header"));
    assert_eq!(check(&bytes[..bytes.len() - 1]), wrong_length);
    let mut padded = bytes.clone();
    padded.push(0);
    assert_eq!(check(&padded), wrong_length);
    assert_eq!(
        check(&bytes[..HEADER_LEN - 1]),
        Err(OpenError::Damaged("shorter than a header"))
    );
}

#[test]
fn the_row_count_must_fit_a_row_number() {
    let out_of_range = Err(OpenError::Damaged("row count out of range"));
    let header = |rows| file_with(Header::current(rows, 0, NO_STAMP), &[], &[]);
    assert_eq!(check(&header(0)), out_of_range);
    assert_eq!(check(&header(u64::from(NO_PARENT) + 1)), out_of_range);
    // As many rows as a row number counts is allowed; this file just
    // doesn't hold them.
    assert_eq!(
        check(&header(u64::from(NO_PARENT))),
        Err(OpenError::Damaged("length doesn't match the header"))
    );
}

#[test]
fn a_row_of_no_kind_is_damaged() {
    let mut rows = tree();
    rows[3].kind = None;
    assert_eq!(damaged(&rows), "a row of no known kind");
}

#[test]
fn the_first_row_is_a_folder() {
    let mut rows = tree();
    rows.truncate(1);
    rows[0].end = 1;
    rows[0].kind = Some(Kind::File);
    let header = Header::current(1, NAMES.len() as u64, NO_STAMP);
    assert_eq!(
        check(&file_with(header, &rows, NAMES)),
        Err(OpenError::Damaged("the first row isn't a folder"))
    );
}

#[test]
fn names_stay_inside_the_names_block() {
    let mut rows = tree();
    rows[3].name_len += 1;
    assert_eq!(damaged(&rows), "a name runs past the names block");
    // Up to the very last byte is fine.
    let mut rows = tree();
    rows[3].name_at -= 1;
    assert!(check(&file(&rows)).is_ok());
}

#[test]
fn a_row_names_the_folder_around_it_as_its_parent() {
    let wrong = "a row's parent isn't the folder around it";
    let mut rows = tree();
    rows[2].parent = 0;
    assert_eq!(damaged(&rows), wrong);
    let mut rows = tree();
    rows[3].parent = 1;
    assert_eq!(damaged(&rows), wrong);
    let mut rows = tree();
    rows[0].parent = 0;
    assert_eq!(damaged(&rows), wrong);
}

#[test]
fn runs_nest() {
    let wrong = "a row's run doesn't fit its parent's";
    // A folder's run past its parent's.
    let mut rows = tree();
    rows[0].end = 3;
    rows[1].end = 4;
    assert_eq!(damaged(&rows), wrong);
    // A file with a run of more than itself.
    let mut rows = tree();
    rows[1].end = 4;
    rows[2].end = 4;
    assert_eq!(damaged(&rows), wrong);
    // A run that ends before its row.
    let mut rows = tree();
    rows[1].end = 1;
    assert_eq!(damaged(&rows), wrong);
    // The first row's run is the whole index.
    let mut rows = tree();
    rows[0].end = 3;
    assert_eq!(damaged(&rows), wrong);
}

#[test]
fn an_empty_folder_is_a_run_of_one() {
    let mut rows = tree();
    // docs holds nothing; a.txt moves to the top level.
    rows[1].end = 2;
    rows[2].parent = 0;
    assert!(check(&file(&rows)).is_ok());
}
