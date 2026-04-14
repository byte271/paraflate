use crate::BlockId;
use crate::EntryId;

#[derive(Clone, Copy, Debug)]
pub struct BlockSpan {
    pub entry: EntryId,
    pub offset: u64,
    pub len: u64,
    pub block: BlockId,
}

#[derive(Clone, Debug)]
pub struct BlockDescriptor {
    pub id: BlockId,
    pub entry: EntryId,
    pub span: BlockSpan,
    pub planned_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct ChunkPlan {
    pub entry: EntryId,
    pub spans: Vec<BlockSpan>,
}
