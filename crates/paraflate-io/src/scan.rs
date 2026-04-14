use std::fs;
use std::path::{Path, PathBuf};

use paraflate_core::{ArchiveEntryDescriptor, EntryId, ParaflateError, ParaflateResult};

pub struct DirectoryScanner {
    root: PathBuf,
}

impl DirectoryScanner {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn scan(&self) -> ParaflateResult<ScanOutcome> {
        let mut entries = Vec::new();
        let mut next_id: u32 = 0;
        self.walk(&self.root, "", &mut entries, &mut next_id)?;
        entries.sort_by(|a, b| a.logical_name.cmp(&b.logical_name));
        for (idx, e) in entries.iter_mut().enumerate() {
            e.id = EntryId(idx as u32);
        }
        Ok(ScanOutcome { entries })
    }

    fn walk(
        &self,
        dir: &Path,
        prefix: &str,
        out: &mut Vec<ArchiveEntryDescriptor>,
        next_id: &mut u32,
    ) -> ParaflateResult<()> {
        let read_dir = fs::read_dir(dir).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ParaflateError::InvalidPath(dir.to_path_buf())
            } else {
                ParaflateError::Io(e)
            }
        })?;
        let mut names: Vec<_> = read_dir.filter_map(|r| r.ok()).collect();
        names.sort_by_key(|e| e.file_name());
        for entry in names {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let logical = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let meta = entry.metadata()?;
            if meta.is_dir() {
                self.walk(&path, &logical, out, next_id)?;
            } else if meta.is_file() {
                let id = EntryId(*next_id);
                *next_id = next_id.saturating_add(1);
                out.push(ArchiveEntryDescriptor {
                    id,
                    path,
                    logical_name: logical,
                    uncompressed_size: meta.len(),
                    is_directory: false,
                });
            }
        }
        Ok(())
    }
}

pub struct ScanOutcome {
    pub entries: Vec<ArchiveEntryDescriptor>,
}
