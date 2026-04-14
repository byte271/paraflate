use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;

use crossbeam_channel::bounded;
use paraflate_core::{BlockSpan, EntryId, ParaflateError, ParaflateResult};

#[derive(Clone, Debug)]
pub struct WorkerPoolConfig {
    pub worker_threads: usize,
    pub queue_depth: usize,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().max(2))
            .unwrap_or(4);
        Self {
            worker_threads: threads,
            queue_depth: threads.saturating_mul(4).max(16),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompressionWork {
    pub job_key: u64,
    pub entry: EntryId,
    pub data: Arc<Vec<u8>>,
    pub span: BlockSpan,
}

pub struct PoolOutcome<R> {
    pub results: BTreeMap<u64, R>,
}

pub struct WorkerPool {
    cfg: WorkerPoolConfig,
}

impl WorkerPool {
    pub fn new(cfg: WorkerPoolConfig) -> Self {
        Self { cfg }
    }

    pub fn execute<F, R>(
        &self,
        items: Vec<CompressionWork>,
        f: F,
    ) -> ParaflateResult<PoolOutcome<R>>
    where
        F: Fn(CompressionWork) -> ParaflateResult<R> + Send + Clone + 'static,
        R: Send + 'static,
    {
        let threads = self.cfg.worker_threads.max(1);
        let depth = self
            .cfg
            .queue_depth
            .max(threads)
            .max(items.len().saturating_add(threads).max(1));
        let (job_tx, job_rx) = bounded::<Option<CompressionWork>>(depth);
        let (res_tx, res_rx) = bounded::<ParaflateResult<(u64, R)>>(depth);
        let mut handles = Vec::new();
        for _ in 0..threads {
            let job_rx = job_rx.clone();
            let res_tx = res_tx.clone();
            let ff = f.clone();
            handles.push(thread::spawn(move || {
                while let Ok(maybe) = job_rx.recv() {
                    let Some(w) = maybe else {
                        break;
                    };
                    let key = w.job_key;
                    let r = ff(w).map(|v| (key, v));
                    if res_tx.send(r).is_err() {
                        break;
                    }
                }
            }));
        }
        for item in items {
            job_tx
                .send(Some(item))
                .map_err(|_| ParaflateError::SchedulerShutdown)?;
        }
        for _ in 0..threads {
            job_tx
                .send(None)
                .map_err(|_| ParaflateError::SchedulerShutdown)?;
        }
        drop(job_tx);
        drop(res_tx);
        let mut map = BTreeMap::new();
        while let Ok(r) = res_rx.recv() {
            let (id, v) = r?;
            map.insert(id, v);
        }
        for h in handles {
            h.join().map_err(|_| ParaflateError::WorkerJoin)?;
        }
        Ok(PoolOutcome { results: map })
    }
}
