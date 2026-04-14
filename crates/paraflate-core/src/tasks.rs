use crate::{BlockId, EntryId, TaskId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionTaskKind {
    ReadEntry,
    AnalyzeEntry,
    CompressBlock,
    EncodeBlock,
    WriteEntry,
}

#[derive(Clone, Debug)]
pub struct TaskNode {
    pub id: TaskId,
    pub kind: CompressionTaskKind,
    pub entry: Option<EntryId>,
    pub block: Option<BlockId>,
    pub depends_on: Vec<TaskId>,
}

#[derive(Clone, Debug, Default)]
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,
}
