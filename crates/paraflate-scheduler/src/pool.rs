use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{bounded, Sender};
use parking_lot::Mutex;
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
            queue_depth: threads.saturating_mul(8).max(64),
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

// A type-erased work item sent to persistent workers.
type BoxedJob = Box<dyn FnOnce() + Send + 'static>;

struct Inner {
    job_tx: Sender<Option<BoxedJob>>,
    handles: Vec<thread::JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Send one poison pill per worker.
        for _ in 0..self.handles.len() {
            let _ = self.job_tx.send(None);
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

/// A persistent thread pool: workers are spawned once and reused across
/// every `execute` call, eliminating per-call thread-spawn overhead.
pub struct WorkerPool {
    cfg: WorkerPoolConfig,
    inner: Mutex<Option<Arc<Inner>>>,
}

impl WorkerPool {
    pub fn new(cfg: WorkerPoolConfig) -> Self {
        Self {
            cfg,
            inner: Mutex::new(None),
        }
    }

    /// Lazily initialise the persistent worker threads on first use.
    fn ensure_started(&self) -> Arc<Inner> {
        let mut guard = self.inner.lock();
        if let Some(ref arc) = *guard {
            return Arc::clone(arc);
        }
        let threads = self.cfg.worker_threads.max(1);
        let depth = self.cfg.queue_depth.max(threads * 8).max(256);
        let (job_tx, job_rx) = bounded::<Option<BoxedJob>>(depth);
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let rx = job_rx.clone();
            handles.push(thread::spawn(move || {
                while let Ok(maybe) = rx.recv() {
                    match maybe {
                        Some(f) => f(),
                        None => break,
                    }
                }
            }));
        }
        let arc = Arc::new(Inner { job_tx, handles });
        *guard = Some(Arc::clone(&arc));
        arc
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
        if items.is_empty() {
            return Ok(PoolOutcome {
                results: BTreeMap::new(),
            });
        }

        // Fast path: single item — run inline, no channel overhead.
        if items.len() == 1 {
            let w = items.into_iter().next().unwrap();
            let key = w.job_key;
            let v = f(w)?;
            let mut map = BTreeMap::new();
            map.insert(key, v);
            return Ok(PoolOutcome { results: map });
        }

        let pool = self.ensure_started();
        let n = items.len();
        // Use a bounded result channel sized to the number of jobs.
        let (res_tx, res_rx) = bounded::<ParaflateResult<(u64, R)>>(n);

        for w in items {
            let key = w.job_key;
            let ff = f.clone();
            let tx = res_tx.clone();
            pool.job_tx
                .send(Some(Box::new(move || {
                    let r = ff(w).map(|v| (key, v));
                    let _ = tx.send(r);
                })))
                .map_err(|_| ParaflateError::SchedulerShutdown)?;
        }
        drop(res_tx); // so the channel closes when all workers are done

        let mut map = BTreeMap::new();
        for _ in 0..n {
            match res_rx.recv() {
                Ok(Ok((id, v))) => {
                    map.insert(id, v);
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(ParaflateError::WorkerJoin),
            }
        }
        Ok(PoolOutcome { results: map })
    }

    /// Run a collection of independent closures in parallel on the persistent
    /// worker threads. Results are returned in the same order as `jobs`.
    pub fn run_parallel<F, R>(&self, jobs: Vec<F>) -> ParaflateResult<Vec<R>>
    where
        F: FnOnce() -> ParaflateResult<R> + Send + 'static,
        R: Send + 'static,
    {
        let n = jobs.len();
        if n == 0 {
            return Ok(Vec::new());
        }
        // Single job: run inline.
        if n == 1 {
            let f = jobs.into_iter().next().unwrap();
            return Ok(vec![f()?]);
        }

        let pool = self.ensure_started();
        let (res_tx, res_rx) = bounded::<ParaflateResult<(usize, R)>>(n);

        for (idx, f) in jobs.into_iter().enumerate() {
            let tx = res_tx.clone();
            pool.job_tx
                .send(Some(Box::new(move || {
                    let r = f().map(|v| (idx, v));
                    let _ = tx.send(r);
                })))
                .map_err(|_| ParaflateError::SchedulerShutdown)?;
        }
        drop(res_tx);

        let mut indexed: Vec<Option<R>> = (0..n).map(|_| None).collect();
        for _ in 0..n {
            match res_rx.recv() {
                Ok(Ok((idx, v))) => indexed[idx] = Some(v),
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(ParaflateError::WorkerJoin),
            }
        }
        Ok(indexed.into_iter().map(|v| v.unwrap()).collect())
    }
}
