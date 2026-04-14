use std::path::PathBuf;

use crate::EntryId;

#[derive(Clone, Debug)]
pub struct ArchiveEntryDescriptor {
    pub id: EntryId,
    pub path: PathBuf,
    pub logical_name: String,
    pub uncompressed_size: u64,
    pub is_directory: bool,
}
