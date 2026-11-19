pub use nova_files_shell::format::{modified, size};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnText {
    pub modified: String,
    pub size: String,
}
