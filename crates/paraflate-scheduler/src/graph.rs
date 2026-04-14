use paraflate_core::{CompressionTaskKind, EntryId, TaskGraph, TaskId, TaskNode};

pub struct TaskGraphBuilder {
    next_task: u64,
}

impl TaskGraphBuilder {
    pub fn new() -> Self {
        Self { next_task: 0 }
    }

    fn alloc(&mut self) -> TaskId {
        let id = TaskId(self.next_task);
        self.next_task = self.next_task.saturating_add(1);
        id
    }

    pub fn linear_pipeline(&mut self, entries: &[EntryId]) -> TaskGraph {
        let mut nodes = Vec::new();
        let mut prev: Option<TaskId> = None;
        for entry in entries {
            let read = self.alloc();
            let mut depends = Vec::new();
            if let Some(p) = prev {
                depends.push(p);
            }
            nodes.push(TaskNode {
                id: read,
                kind: CompressionTaskKind::ReadEntry,
                entry: Some(*entry),
                block: None,
                depends_on: depends.clone(),
            });
            let analyze = self.alloc();
            nodes.push(TaskNode {
                id: analyze,
                kind: CompressionTaskKind::AnalyzeEntry,
                entry: Some(*entry),
                block: None,
                depends_on: vec![read],
            });
            let compress = self.alloc();
            nodes.push(TaskNode {
                id: compress,
                kind: CompressionTaskKind::CompressBlock,
                entry: Some(*entry),
                block: None,
                depends_on: vec![analyze],
            });
            let write = self.alloc();
            nodes.push(TaskNode {
                id: write,
                kind: CompressionTaskKind::WriteEntry,
                entry: Some(*entry),
                block: None,
                depends_on: vec![compress],
            });
            prev = Some(write);
        }
        TaskGraph { nodes }
    }
}

impl Default for TaskGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}
