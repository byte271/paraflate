use std::fs;
use std::sync::Arc;

use paraflate_core::{
    ArchiveProfile, BlockId, BlockSpan, CompressionMethod, CompressionProfile, ExecutionBudget,
};
use paraflate_pipeline::{ArchiveSession, CreateArchiveParams};
use paraflate_scheduler::{TaskGraphBuilder, WorkerPool, WorkerPoolConfig};
use paraflate_verify::read_entry_bytes;
use tempfile::tempdir;

fn session_stored_single_thread() -> ArchiveProfile {
    let mut p = ArchiveProfile::default();
    p.compression = CompressionProfile {
        method: CompressionMethod::Stored,
        level: 1,
        strategy: paraflate_core::DeflateStrategy::Default,
        window_bits: 15,
        global_huffman: false,
    };
    p.budget = ExecutionBudget {
        worker_threads: 1,
        pipeline_depth: 1,
        io_lane_count: 1,
        max_pending_tasks: 4,
        memory: p.budget.memory,
    };
    p
}

fn session_deflate_parallel() -> ArchiveProfile {
    let mut p = ArchiveProfile::default();
    p.budget.worker_threads = 4;
    p.budget.pipeline_depth = 16;
    p.budget.max_pending_tasks = 128;
    p
}

#[test]
fn empty_dir_errors() {
    let dir = tempdir().unwrap();
    let session = ArchiveSession::new();
    let err = session
        .create_archive(CreateArchiveParams::new(
            dir.path().to_path_buf(),
            dir.path().join("a.zip"),
            ArchiveProfile::default(),
        ))
        .unwrap_err();
    match err {
        paraflate_core::ParaflateError::EmptyArchive => {}
        _ => panic!("unexpected"),
    }
}

#[test]
fn single_file_roundtrip_deflate() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("in");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("hello.txt"), b"hello world").unwrap();
    let zip_path = dir.path().join("out.zip");
    let session = ArchiveSession::new();
    session
        .create_archive(CreateArchiveParams::new(
            root.clone(),
            zip_path.clone(),
            session_deflate_parallel(),
        ))
        .unwrap();
    let bytes = fs::read(&zip_path).unwrap();
    let got = read_entry_bytes(&bytes, "hello.txt").unwrap();
    assert_eq!(got, b"hello world");
}

#[test]
fn many_small_files_order_stable() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("in");
    fs::create_dir_all(&root).unwrap();
    for i in 0..20 {
        fs::write(root.join(format!("{i:02}.txt")), format!("x{i}").as_bytes()).unwrap();
    }
    let zip_path = dir.path().join("many.zip");
    let session = ArchiveSession::new();
    session
        .create_archive(CreateArchiveParams::new(
            root,
            zip_path.clone(),
            session_deflate_parallel(),
        ))
        .unwrap();
    let bytes = fs::read(&zip_path).unwrap();
    use paraflate_verify::verify_zip_bytes;
    let r = verify_zip_bytes(&bytes, false).unwrap();
    assert_eq!(r.entries.len(), 20);
    let names: Vec<String> = r.entries.iter().map(|e| e.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn large_file_compresses() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("in");
    fs::create_dir_all(&root).unwrap();
    let mut big = vec![0u8; 2 * 1024 * 1024];
    for (i, b) in big.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    fs::write(root.join("big.bin"), &big).unwrap();
    let zip_path = dir.path().join("big.zip");
    let session = ArchiveSession::new();
    session
        .create_archive(CreateArchiveParams::new(
            root,
            zip_path.clone(),
            session_deflate_parallel(),
        ))
        .unwrap();
    let bytes = fs::read(&zip_path).unwrap();
    let out = read_entry_bytes(&bytes, "big.bin").unwrap();
    assert_eq!(out, big);
}

#[test]
fn offsets_and_central_directory_present() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("in");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.txt"), b"a").unwrap();
    fs::write(root.join("b.txt"), b"bb").unwrap();
    let zip_path = dir.path().join("cd.zip");
    let session = ArchiveSession::new();
    let report = session
        .create_archive(CreateArchiveParams::new(
            root,
            zip_path.clone(),
            session_stored_single_thread(),
        ))
        .unwrap();
    let z = report.zip.unwrap();
    assert!(z.central_directory_offset > 0);
    assert!(z.central_directory_size > 0);
    assert_eq!(z.total_entries, 2);
    let bytes = fs::read(&zip_path).unwrap();
    assert!(bytes.len() as u64 >= z.archive_size);
}

#[test]
fn scheduler_worker_pool_executes_all() {
    let cfg = WorkerPoolConfig {
        worker_threads: 3,
        queue_depth: 8,
    };
    let pool = WorkerPool::new(cfg);
    let items: Vec<_> = (0u32..10)
        .map(|i| paraflate_scheduler::CompressionWork {
            job_key: i as u64,
            entry: paraflate_core::EntryId(i),
            data: Arc::new(vec![i as u8; 4]),
            span: BlockSpan {
                entry: paraflate_core::EntryId(i),
                offset: 0,
                len: 4,
                block: BlockId(i as u64),
            },
        })
        .collect();
    let out = pool
        .execute(items, |w| Ok(w.data.iter().sum::<u8>() as u32))
        .unwrap();
    assert_eq!(out.results.len(), 10);
}

#[test]
fn task_graph_builder_monotonic_ids() {
    let mut b = TaskGraphBuilder::new();
    let g = b.linear_pipeline(&[paraflate_core::EntryId(0), paraflate_core::EntryId(1)]);
    assert_eq!(g.nodes.len(), 8);
}
