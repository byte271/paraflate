use std::fs::File;
use std::io::Read;
use std::path::Path;

use memmap2::MmapOptions;
use paraflate_buffers::BufferHandle;
use paraflate_core::{ParaflateError, ParaflateResult};

pub struct FileReadPlan {
    pub prefer_mmap_bytes: u64,
    pub chunk_bytes: usize,
}

impl Default for FileReadPlan {
    fn default() -> Self {
        Self {
            prefer_mmap_bytes: 4 * 1024 * 1024,
            chunk_bytes: 1024 * 1024,
        }
    }
}

pub enum ReadOutcome {
    Mmap(memmap2::Mmap),
    Buffer(BufferHandle),
    Inline(Vec<u8>),
}

pub struct FileReader {
    plan: FileReadPlan,
}

impl FileReader {
    pub fn new(plan: FileReadPlan) -> Self {
        Self { plan }
    }

    pub fn read_path_mmap(&self, path: &Path) -> ParaflateResult<ReadOutcome> {
        let file = File::open(path)?;
        let meta = file.metadata()?;
        if meta.len() >= self.plan.prefer_mmap_bytes {
            let mmap = unsafe { MmapOptions::new().map(&file)? };
            return Ok(ReadOutcome::Mmap(mmap));
        }
        let mut buf = Vec::with_capacity(meta.len() as usize);
        let mut f = file;
        f.read_to_end(&mut buf)?;
        Ok(ReadOutcome::Inline(buf))
    }

    pub fn read_path_chunks(
        &self,
        path: &Path,
        mut sink: impl FnMut(&[u8]) -> ParaflateResult<()>,
    ) -> ParaflateResult<u64> {
        let mut file = File::open(path)?;
        let meta = file.metadata()?;
        let len = meta.len();
        let mut scratch = vec![0u8; self.plan.chunk_bytes.max(4096)];
        let mut read_total = 0u64;
        loop {
            let n = file.read(&mut scratch)?;
            if n == 0 {
                break;
            }
            read_total = read_total.saturating_add(n as u64);
            sink(&scratch[..n])?;
        }
        if read_total != len {
            return Err(ParaflateError::InvariantViolated(
                "read length mismatch".to_string(),
            ));
        }
        Ok(read_total)
    }
}
