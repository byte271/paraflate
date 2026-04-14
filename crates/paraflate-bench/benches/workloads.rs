use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use paraflate_core::{
    ArchiveProfile, CompressionMethod, CompressionProfile, DeflateStrategy, ExecutionBudget,
    PlanningAggression, PredictiveMode, PredictiveRuntimeConfig, VerificationMode,
};
use paraflate_pipeline::{ArchiveSession, CreateArchiveParams};

fn write_tree(root: &PathBuf, file_count: usize, bytes_each: usize) {
    fs::create_dir_all(root).unwrap();
    for i in 0..file_count {
        let mut p = root.clone();
        p.push(format!("f{i}.bin"));
        let mut f = fs::File::create(&p).unwrap();
        let chunk = vec![((i % 251) + 1) as u8; 4096];
        let mut written = 0usize;
        while written < bytes_each {
            let take = (bytes_each - written).min(chunk.len());
            f.write_all(&chunk[..take]).unwrap();
            written += take;
        }
    }
}

fn profile_serial_stored() -> ArchiveProfile {
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

fn profile_parallel_deflate(threads: usize) -> ArchiveProfile {
    let mut p = ArchiveProfile::default();
    p.budget.worker_threads = threads;
    p.budget.pipeline_depth = threads.saturating_mul(2).max(8);
    p.budget.io_lane_count = threads.min(8).max(2);
    p.budget.max_pending_tasks = threads.saturating_mul(32).max(64);
    p
}

fn profile_parallel_deflate_global(threads: usize, global_huffman: bool) -> ArchiveProfile {
    let mut p = profile_parallel_deflate(threads);
    p.compression.global_huffman = global_huffman;
    p
}

fn profile_with_verification(threads: usize, verify: VerificationMode) -> ArchiveProfile {
    let mut p = profile_parallel_deflate(threads);
    p.predictive.verification = verify;
    p
}

fn profile_predictive(
    threads: usize,
    mode: PredictiveMode,
    planning: PlanningAggression,
    verify: VerificationMode,
) -> ArchiveProfile {
    let mut p = profile_parallel_deflate(threads);
    p.predictive = PredictiveRuntimeConfig {
        mode,
        verification: verify,
        planning,
    };
    p
}

fn bench_small_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("small_files");
    for threads in [1usize, 2, 4] {
        group.bench_with_input(
            BenchmarkId::new("parallel_deflate", threads),
            &threads,
            |b, &threads| {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path().to_path_buf();
                write_tree(&root, 64, 2048);
                let out = dir.path().join("out.zip");
                let profile = profile_parallel_deflate(threads);
                b.iter_custom(|iters| {
                    let mut total = std::time::Duration::ZERO;
                    for i in 0..iters {
                        let target = dir.path().join(format!("o{i}.zip"));
                        let start = Instant::now();
                        let session = ArchiveSession::new();
                        let _ = black_box(
                            session
                                .create_archive(CreateArchiveParams::new(
                                    root.clone(),
                                    target,
                                    profile.clone(),
                                ))
                                .unwrap(),
                        );
                        total += start.elapsed();
                    }
                    total
                });
                let _ = out;
            },
        );
    }
    group.finish();
}

fn bench_mixed_duplicate(c: &mut Criterion) {
    c.bench_function("mixed_duplicate_parallel", |b| {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir_all(&root).unwrap();
        let payload = vec![b'a'; 16 * 1024];
        for i in 0..32 {
            let mut p = root.clone();
            p.push(format!("t{i}.txt"));
            fs::write(&p, &payload).unwrap();
        }
        let profile = profile_parallel_deflate(4);
        b.iter(|| {
            let session = ArchiveSession::new();
            black_box(
                session
                    .create_archive(CreateArchiveParams::new(
                        root.clone(),
                        dir.path().join("m.zip"),
                        profile.clone(),
                    ))
                    .unwrap(),
            );
        });
    });
}

fn bench_serial_vs_parallel(c: &mut Criterion) {
    let mut group = c.benchmark_group("serial_vs_parallel");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_tree(&root, 32, 8192);
    group.bench_function("serial_stored", |b| {
        let profile = profile_serial_stored();
        b.iter(|| {
            let session = ArchiveSession::new();
            black_box(
                session
                    .create_archive(CreateArchiveParams::new(
                        root.clone(),
                        dir.path().join("s.zip"),
                        profile.clone(),
                    ))
                    .unwrap(),
            );
        });
    });
    group.bench_function("parallel_deflate_4", |b| {
        let profile = profile_parallel_deflate(4);
        b.iter(|| {
            let session = ArchiveSession::new();
            black_box(
                session
                    .create_archive(CreateArchiveParams::new(
                        root.clone(),
                        dir.path().join("p.zip"),
                        profile.clone(),
                    ))
                    .unwrap(),
            );
        });
    });
    group.finish();
}

fn bench_predictive_modes(c: &mut Criterion) {
    let mut group = c.benchmark_group("predictive_deflate");
    group.sample_size(10);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_tree(&root, 48, 4096);
    for (label, prof) in [
        (
            "off",
            profile_predictive(
                4,
                PredictiveMode::Off,
                PlanningAggression::Balanced,
                VerificationMode::Off,
            ),
        ),
        (
            "standard",
            profile_predictive(
                4,
                PredictiveMode::Standard,
                PlanningAggression::Balanced,
                VerificationMode::Off,
            ),
        ),
        (
            "aggressive",
            profile_predictive(
                4,
                PredictiveMode::Aggressive,
                PlanningAggression::Aggressive,
                VerificationMode::Off,
            ),
        ),
        (
            "safe_plan",
            profile_predictive(
                4,
                PredictiveMode::Standard,
                PlanningAggression::Safe,
                VerificationMode::Off,
            ),
        ),
    ] {
        group.bench_function(label, |b| {
            b.iter(|| {
                let session = ArchiveSession::new();
                black_box(
                    session
                        .create_archive(CreateArchiveParams::new(
                            root.clone(),
                            dir.path().join(format!("pred_{label}.zip")),
                            prof.clone(),
                        ))
                        .unwrap(),
                );
            });
        });
    }
    group.finish();
}

fn bench_large_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_file");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::create_dir_all(&root).unwrap();
    let mut p = root.clone();
    p.push("big.bin");
    let mut f = fs::File::create(&p).unwrap();
    let chunk = vec![0xC4u8; 65536];
    for _ in 0..128 {
        f.write_all(&chunk).unwrap();
    }
    drop(f);
    group.bench_function("parallel_deflate_4_threads", |b| {
        let profile = profile_parallel_deflate(4);
        b.iter(|| {
            let session = ArchiveSession::new();
            black_box(
                session
                    .create_archive(CreateArchiveParams::new(
                        root.clone(),
                        dir.path().join("large.zip"),
                        profile.clone(),
                    ))
                    .unwrap(),
            );
        });
    });
    group.finish();
}

fn bench_text_vs_binary(c: &mut Criterion) {
    let mut group = c.benchmark_group("content_shape");
    group.bench_function("text_heavy_parallel_4", |b| {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir_all(&root).unwrap();
        let body = "lorem ipsum dolor sit amet.\n".repeat(4096);
        fs::write(root.join("t.txt"), body.as_bytes()).unwrap();
        let profile = profile_parallel_deflate(4);
        b.iter(|| {
            let session = ArchiveSession::new();
            black_box(
                session
                    .create_archive(CreateArchiveParams::new(
                        root.clone(),
                        dir.path().join("text.zip"),
                        profile.clone(),
                    ))
                    .unwrap(),
            );
        });
    });
    group.bench_function("binary_heavy_parallel_4", |b| {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir_all(&root).unwrap();
        let mut raw = vec![0u8; 2 * 1024 * 1024];
        for (i, x) in raw.iter_mut().enumerate() {
            *x = ((i * 1103515245 + 12345) >> 16) as u8;
        }
        fs::write(root.join("b.bin"), &raw).unwrap();
        let profile = profile_parallel_deflate(4);
        b.iter(|| {
            let session = ArchiveSession::new();
            black_box(
                session
                    .create_archive(CreateArchiveParams::new(
                        root.clone(),
                        dir.path().join("bin.zip"),
                        profile.clone(),
                    ))
                    .unwrap(),
            );
        });
    });
    group.finish();
}

fn bench_global_huffman(c: &mut Criterion) {
    let mut group = c.benchmark_group("global_huffman");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::create_dir_all(&root).unwrap();
    let block = vec![b'z'; 64 * 1024];
    for i in 0..12 {
        fs::write(root.join(format!("g{i}.dat")), &block).unwrap();
    }
    for (label, gh) in [("disabled", false), ("enabled", true)] {
        group.bench_with_input(BenchmarkId::new("parallel_4", label), &gh, |b, &gh| {
            let profile = profile_parallel_deflate_global(4, gh);
            b.iter(|| {
                let session = ArchiveSession::new();
                black_box(
                    session
                        .create_archive(CreateArchiveParams::new(
                            root.clone(),
                            dir.path().join(format!("gh_{label}.zip")),
                            profile.clone(),
                        ))
                        .unwrap(),
                );
            });
        });
    }
    group.finish();
}

fn bench_parallel_thread_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_scaling");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_tree(&root, 24, 64 * 1024);
    for threads in [1usize, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("deflate", threads),
            &threads,
            |b, &threads| {
                let profile = profile_parallel_deflate(threads);
                b.iter(|| {
                    let session = ArchiveSession::new();
                    black_box(
                        session
                            .create_archive(CreateArchiveParams::new(
                                root.clone(),
                                dir.path().join(format!("scale_{threads}.zip")),
                                profile.clone(),
                            ))
                            .unwrap(),
                    );
                });
            },
        );
    }
    group.finish();
}

fn bench_create_with_verification_toggle(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline_verification");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_tree(&root, 20, 8192);
    for (label, mode) in [
        ("off", VerificationMode::Off),
        ("after_write", VerificationMode::AfterWrite),
        ("strict", VerificationMode::Strict),
    ] {
        group.bench_function(label, |b| {
            let profile = profile_with_verification(2, mode);
            b.iter(|| {
                let session = ArchiveSession::new();
                black_box(
                    session
                        .create_archive(CreateArchiveParams::new(
                            root.clone(),
                            dir.path().join(format!("pv_{label}.zip")),
                            profile.clone(),
                        ))
                        .unwrap(),
                );
            });
        });
    }
    group.finish();
}

fn bench_block_sensitivity_level(c: &mut Criterion) {
    let mut group = c.benchmark_group("deflate_level_sensitivity");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_tree(&root, 16, 16384);
    for level in [1u32, 6, 9] {
        group.bench_with_input(BenchmarkId::new("level", level), &level, |b, &level| {
            let mut p = profile_parallel_deflate(4);
            p.compression.level = level;
            p.compression.strategy = DeflateStrategy::Default;
            b.iter(|| {
                let session = ArchiveSession::new();
                black_box(
                    session
                        .create_archive(CreateArchiveParams::new(
                            root.clone(),
                            dir.path().join(format!("lvl_{level}.zip")),
                            p.clone(),
                        ))
                        .unwrap(),
                );
            });
        });
    }
    group.finish();
}

fn bench_validation_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation_overhead");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_tree(&root, 16, 2048);
    let zip_path = dir.path().join("vbase.zip");
    let session = ArchiveSession::new();
    session
        .create_archive(CreateArchiveParams::new(
            root.clone(),
            zip_path.clone(),
            profile_parallel_deflate(2),
        ))
        .unwrap();
    let bytes = fs::read(&zip_path).unwrap();
    group.bench_function("verify_zip_bytes_strict", |b| {
        b.iter(|| {
            black_box(
                paraflate_pipeline::verify_zip_bytes(black_box(&bytes), VerificationMode::Strict)
                    .unwrap(),
            );
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_small_files,
    bench_mixed_duplicate,
    bench_serial_vs_parallel,
    bench_predictive_modes,
    bench_large_file,
    bench_text_vs_binary,
    bench_global_huffman,
    bench_parallel_thread_scaling,
    bench_create_with_verification_toggle,
    bench_block_sensitivity_level,
    bench_validation_overhead
);
criterion_main!(benches);
