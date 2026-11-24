use super::*;
use crate::FileId;
use crate::format::{HEADER_LEN, NO_STAMP};
use nova_files_test_folder::TestFolder;

const NAMES: &[u8] = b"/homedocsa.txtb.txt";
const ROWS: usize = 4;

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

fn file(rows: &[Row]) -> Vec<u8> {
    let mut bytes = Vec::new();
    format::write(&mut bytes, NO_STAMP, rows, NAMES).unwrap();
    bytes
}

fn with_header(mut bytes: Vec<u8>, edit: impl FnOnce(&mut Header)) -> Vec<u8> {
    let mut header = Header::read(&bytes).unwrap();
    edit(&mut header);
    bytes[..HEADER_LEN].copy_from_slice(&header.write());
    bytes
}

fn layout(bytes: &[u8]) -> Layout {
    Layout::of(&Header::read(bytes).unwrap()).unwrap()
}

fn damaged(rows: &[Row]) -> &'static str {
    match check(&file(rows)) {
        Err(OpenError::Damaged(reason)) => reason,
        other => panic!("expected a damaged file, got {other:?}"),
    }
}

#[test]
fn a_good_tree_passes() {
    let (header, _) = check(&file(&tree())).unwrap();
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
    let newer = with_header(file(&tree()), |header| header.version = VERSION + 1);
    assert_eq!(check(&newer), Err(OpenError::OutOfDate));
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
    let header = |rows| Header::current(rows, 0, 0, NO_STAMP).write().to_vec();
    assert_eq!(check(&header(0)), out_of_range);
    assert_eq!(check(&header(u64::from(NO_PARENT) + 1)), out_of_range);
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
    assert_eq!(damaged(&rows), "the first row isn't a folder");
}

#[test]
fn folded_names_follow_each_other_inside_their_block() {
    let wrong = Err(OpenError::Damaged("a name runs past the names block"));
    let good = file(&tree());
    let starts = layout(&good).folded_at;
    let width = size_of::<u32>();
    let mut past = good.clone();
    past[starts + ROWS * width..][..width].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(check(&past), wrong);
    let mut backwards = good;
    backwards[starts + width..][..width].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(check(&backwards), wrong);
}

#[test]
fn a_name_past_the_names_block_reads_as_missing() {
    let mut rows = tree();
    rows[3].name_len += 1;
    let folder = TestFolder::new("reader-name-past");
    let path = folder.join("index");
    std::fs::write(&path, file(&rows)).unwrap();
    let index = Index::open(&path).unwrap();
    assert_eq!(index.name(RowId(3)), None);
    assert_eq!(index.name(RowId(2)), Some(OsStr::new("a.txt")));
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
    let mut rows = tree();
    rows[0].end = 3;
    rows[1].end = 4;
    assert_eq!(damaged(&rows), wrong);
    let mut rows = tree();
    rows[1].end = 4;
    rows[2].end = 4;
    assert_eq!(damaged(&rows), wrong);
    let mut rows = tree();
    rows[1].end = 1;
    assert_eq!(damaged(&rows), wrong);
    let mut rows = tree();
    rows[0].end = 3;
    assert_eq!(damaged(&rows), wrong);
}

#[test]
fn an_empty_folder_is_a_run_of_one() {
    let mut rows = tree();
    rows[1].end = 2;
    rows[2].parent = 0;
    assert!(check(&file(&rows)).is_ok());
}

fn opened(name: &str, rows: &[Row], names: &[u8]) -> (TestFolder, Index) {
    let folder = TestFolder::new(name);
    let path = folder.join("index");
    let mut bytes = Vec::new();
    format::write(&mut bytes, NO_STAMP, rows, names).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let index = Index::open(&path).unwrap();
    (folder, index)
}

#[test]
fn the_search_part_holds_folded_names_word_starts_and_what_is_hidden() {
    let names = b"/home.CfgMyFile.TXTb.txt";
    let mut rows = tree();
    for (row, (at, len)) in rows.iter_mut().zip([(0, 5), (5, 4), (9, 10), (19, 5)]) {
        (row.name_at, row.name_len) = (at, len);
    }
    let (_folder, index) = opened("reader-search-part", &rows, names);
    assert_eq!(index.folded(RowId(1)), Some(&b".cfg"[..]));
    assert_eq!(index.folded(RowId(2)), Some(&b"myfile.txt"[..]));
    assert_eq!(index.name(RowId(2)), Some(OsStr::new("MyFile.TXT")));
    let starts: Vec<usize> = (0..10)
        .filter(|&at| index.starts_word(RowId(2), at))
        .collect();
    assert_eq!(starts, [0, 2, 7]);
    let hidden: Vec<bool> = (0..4).map(|row| index.is_hidden(RowId(row))).collect();
    assert_eq!(hidden, [false, true, true, false]);
    assert_eq!(index.kind(RowId(1)), Some(Kind::Folder));
    assert_eq!(index.end(RowId(1)), Some(RowId(3)));
    assert_eq!(index.folded(RowId(4)), None);
    assert_eq!(index.end(RowId(4)), None);
}

#[test]
fn released_details_read_back_from_the_file() {
    let (_folder, index) = opened("reader-released", &tree(), NAMES);
    let before = index.entry(RowId(3));
    index.release_details().unwrap();
    assert_eq!(index.entry(RowId(3)), before);
    assert_eq!(index.name(RowId(3)), Some(OsStr::new("b.txt")));
}
