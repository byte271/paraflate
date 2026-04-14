use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use flate2::write::DeflateEncoder;
use flate2::Compression;
use paraflate_core::{
    ArchiveLayout, ArchiveProfile, CompressionMethod, CompressionProfile, DeflateStrategy,
    ExecutionBudget, ExecutionPolicy, ExplainReport, ParaflateError, PlanningAggression,
    PredictiveMode, PredictiveRuntimeConfig, SchedulerPolicy, VerificationMode,
};
use paraflate_io::DirectoryScanner;
use paraflate_pipeline::{
    analyze_directory, build_explain_report, validate_archive_path, ArchiveIntelReport,
    ArchiveSession, CreateArchiveParams,
};

#[derive(Parser)]
#[command(
    name = "paraflate",
    version,
    about = "Paraflate: predictive parallel self-verifying DEFLATE for ZIP archives"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Create {
        #[arg(short = 'i', long)]
        input: PathBuf,
        #[arg(short = 'o', long)]
        output: PathBuf,
        #[arg(long, default_value_t = 6)]
        level: u32,
        #[arg(long, default_value_t = false)]
        stored: bool,
        #[arg(long, default_value_t = false)]
        verbose: bool,
        #[arg(long, default_value_t = false)]
        stats: bool,
        #[arg(long, default_value_t = false)]
        timing: bool,
        #[arg(long, default_value_t = false)]
        print_blocks: bool,
        #[arg(long, default_value_t = false)]
        validate_after: bool,
        #[arg(long, default_value_t = 0)]
        threads: usize,
        #[arg(long, value_enum, default_value_t = LayoutArg::Lexical)]
        layout: LayoutArg,
        #[arg(long, value_enum, default_value_t = SchedulerArg::Balanced)]
        scheduler: SchedulerArg,
        #[arg(long, value_enum, default_value_t = PredictionArg::Standard)]
        prediction: PredictionArg,
        #[arg(long, value_enum, default_value_t = VerifyArg::AfterWrite)]
        verification: VerifyArg,
        #[arg(long, value_enum, default_value_t = PlanningArg::Balanced)]
        planning: PlanningArg,
        #[arg(long, default_value_t = false)]
        global_huffman: bool,
        #[arg(long, default_value_t = false)]
        adaptive: bool,
        #[arg(long, default_value_t = false)]
        debug: bool,
    },
    Explain {
        #[arg(short = 'i', long)]
        input: PathBuf,
        #[arg(long, default_value_t = true, help = "Emit structured JSON (default)")]
        json: bool,
        #[arg(long, default_value_t = false)]
        pretty: bool,
        #[arg(long, default_value_t = false)]
        no_run: bool,
        #[arg(long, default_value_t = 0)]
        threads: usize,
        #[arg(long, value_enum, default_value_t = PredictionArg::Standard)]
        prediction: PredictionArg,
        #[arg(long, value_enum, default_value_t = PlanningArg::Balanced)]
        planning: PlanningArg,
        #[arg(long, default_value_t = false)]
        global_huffman: bool,
        #[arg(long, default_value_t = false)]
        adaptive: bool,
    },
    Analyze {
        #[arg(short = 'i', long)]
        input: PathBuf,
        #[arg(long, default_value_t = 6)]
        level: u32,
        #[arg(long, default_value_t = 0)]
        threads: usize,
        #[arg(long, value_enum, default_value_t = PredictionArg::Standard)]
        prediction: PredictionArg,
        #[arg(long, value_enum, default_value_t = PlanningArg::Balanced)]
        planning: PlanningArg,
    },
    Debug {
        #[arg(short = 'i', long)]
        input: PathBuf,
        #[arg(short = 'o', long)]
        output: PathBuf,
        #[arg(long, default_value_t = 6)]
        level: u32,
        #[arg(long, default_value_t = 0)]
        threads: usize,
        #[arg(long, value_enum, default_value_t = VerifyArg::Off)]
        verification: VerifyArg,
    },
    Compare {
        #[arg(short = 'i', long)]
        input: PathBuf,
        #[arg(long, default_value_t = 6)]
        level: u32,
        #[arg(long, default_value_t = 4)]
        threads: usize,
    },
    Validate {
        #[arg(short = 'p', long)]
        zip: PathBuf,
        #[arg(long, default_value_t = false)]
        strict: bool,
    },
    Bench {
        #[arg(short = 'i', long)]
        input: PathBuf,
        #[arg(short = 'o', long)]
        output: PathBuf,
        #[arg(long, default_value_t = 6)]
        level: u32,
        #[arg(long, default_value_t = 0)]
        threads: usize,
        #[arg(long, value_enum, default_value_t = PredictionArg::Standard)]
        prediction: PredictionArg,
        #[arg(long, value_enum, default_value_t = VerifyArg::Off)]
        verification: VerifyArg,
        #[arg(long, value_enum, default_value_t = PlanningArg::Aggressive)]
        planning: PlanningArg,
    },
    Harness {
        #[arg(long, default_value = "test_file")]
        output: PathBuf,
        #[arg(long, default_value_t = 6)]
        level: u32,
        #[arg(long, default_value_t = 4)]
        threads: usize,
        #[arg(long, default_value_t = false)]
        skip_large: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum LayoutArg {
    Lexical,
    Size,
    Score,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum SchedulerArg {
    Balanced,
    Throughput,
    Ratio,
    Memory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum PredictionArg {
    Off,
    Standard,
    Aggressive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum VerifyArg {
    Off,
    AfterWrite,
    Strict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum PlanningArg {
    Safe,
    Balanced,
    Aggressive,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), ParaflateError> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Create {
            input,
            output,
            level,
            stored,
            verbose,
            stats,
            timing,
            print_blocks,
            validate_after,
            threads,
            layout,
            scheduler,
            prediction,
            verification,
            planning,
            global_huffman,
            adaptive,
            debug,
        } => {
            let t0 = Instant::now();
            let mut profile = build_profile(
                level,
                stored,
                threads,
                map_layout(layout),
                map_scheduler(scheduler),
                map_prediction(prediction),
                map_verify(verification),
                map_planning(planning),
                global_huffman,
            );
            profile.execution.adaptive_block_feedback = adaptive;
            if validate_after {
                profile.predictive.verification = VerificationMode::AfterWrite;
            }
            let session = ArchiveSession::new();
            let report = session.create_archive(
                CreateArchiveParams::new(input, output.clone(), profile.clone()).with_debug(debug),
            )?;
            if verbose || stats {
                println!(
                    "entries={} raw={} compressed={} threads={}",
                    report.entries,
                    report.uncompressed_bytes,
                    report.compressed_bytes,
                    report.worker_threads
                );
                if let Some(z) = &report.zip {
                    println!(
                        "archive_size={} cd_entries={} cd_size={} cd_off={}",
                        z.archive_size,
                        z.total_entries,
                        z.central_directory_size,
                        z.central_directory_offset
                    );
                }
                if let Some(v) = &report.verification {
                    println!(
                        "verified_entries={} inflated={} central_off={}",
                        v.entries_verified, v.inflated_bytes, v.structure.central_offset
                    );
                }
            }
            if print_blocks {
                println!("predictive_mode={:?}", report.predictive_mode);
                println!("verification_mode={:?}", report.verification_mode);
            }
            if let Some(rows) = &report.debug_blocks {
                for r in rows {
                    println!(
                        "debug entry={} block={} raw={} comp={} tok={} pred={:.5} act={:.5} lz_win={} lz_idx={} fb={}",
                        r.entry_id,
                        r.block_id,
                        r.raw_span_bytes,
                        r.compressed_bytes,
                        r.token_count,
                        r.predicted_ratio,
                        r.actual_ratio,
                        r.lz_matches_window,
                        r.lz_matches_index,
                        r.fallback_code
                    );
                }
            }
            if timing || verbose {
                println!(
                    "elapsed_ms={} pipeline_elapsed_ms={}",
                    t0.elapsed().as_millis(),
                    report.elapsed_ms
                );
            }
        }
        Commands::Explain {
            input,
            json: _,
            pretty,
            no_run,
            threads,
            prediction,
            planning,
            global_huffman,
            adaptive,
        } => {
            let mut profile = build_profile(
                6,
                false,
                threads,
                ArchiveLayout::DeterministicLexical,
                SchedulerPolicy::Balanced,
                map_prediction(prediction),
                VerificationMode::Off,
                map_planning(planning),
                global_huffman,
            );
            profile.execution.adaptive_block_feedback = adaptive;
            let intel = analyze_directory(input.clone(), &profile)?;
            let report = if no_run {
                build_explain_report(&intel, None, &profile, true)
            } else {
                let dir = tempfile::tempdir().map_err(ParaflateError::Io)?;
                let out_zip = dir.path().join("explain.zip");
                let session = ArchiveSession::new();
                let run = session.create_archive(
                    CreateArchiveParams::new(input, out_zip, profile.clone())
                        .with_debug(true)
                        .with_entry_stats(true),
                )?;
                build_explain_report(&intel, Some(&run), &profile, false)
            };
            write_explain_json(&report, pretty)?;
        }
        Commands::Analyze {
            input,
            level,
            threads,
            prediction,
            planning,
        } => {
            let profile = build_profile(
                level,
                false,
                threads,
                ArchiveLayout::DeterministicLexical,
                SchedulerPolicy::Balanced,
                map_prediction(prediction),
                VerificationMode::Off,
                map_planning(planning),
                false,
            );
            let r = analyze_directory(input, &profile)?;
            print_intel_report(&r);
        }
        Commands::Debug {
            input,
            output,
            level,
            threads,
            verification,
        } => {
            let mut profile = build_profile(
                level,
                false,
                threads,
                ArchiveLayout::DeterministicLexical,
                SchedulerPolicy::ThroughputBiased,
                PredictiveMode::Standard,
                map_verify(verification),
                PlanningAggression::Balanced,
                false,
            );
            profile.execution.adaptive_block_feedback = true;
            let session = ArchiveSession::new();
            let report = session.create_archive(
                CreateArchiveParams::new(input, output, profile).with_debug(true),
            )?;
            if let Some(rows) = report.debug_blocks {
                for r in rows {
                    println!(
                        "debug entry={} block={} raw={} comp={} tok={} pred={:.5} act={:.5} lz_win={} lz_idx={} fb={}",
                        r.entry_id,
                        r.block_id,
                        r.raw_span_bytes,
                        r.compressed_bytes,
                        r.token_count,
                        r.predicted_ratio,
                        r.actual_ratio,
                        r.lz_matches_window,
                        r.lz_matches_index,
                        r.fallback_code
                    );
                }
            }
        }
        Commands::Compare {
            input,
            level,
            threads,
        } => {
            run_compare(input, level, threads)?;
        }
        Commands::Validate { zip, strict } => {
            let r = validate_archive_path(zip, strict)?;
            println!(
                "ok entries={} inflated={} central_off={}",
                r.entries_verified, r.inflated_bytes, r.structure.central_offset
            );
        }
        Commands::Bench {
            input,
            output,
            level,
            threads,
            prediction,
            verification,
            planning,
        } => {
            let t0 = Instant::now();
            let profile = build_profile(
                level,
                false,
                threads,
                ArchiveLayout::DeterministicLexical,
                SchedulerPolicy::ThroughputBiased,
                map_prediction(prediction),
                map_verify(verification),
                map_planning(planning),
                false,
            );
            let session = ArchiveSession::new();
            let report =
                session.create_archive(CreateArchiveParams::new(input, output, profile.clone()))?;
            println!(
                "raw={} compressed={} threads={} ms={}",
                report.uncompressed_bytes,
                report.compressed_bytes,
                report.worker_threads,
                t0.elapsed().as_millis()
            );
        }
        Commands::Harness {
            output,
            level,
            threads,
            skip_large,
        } => {
            paraflate_harness::run_harness(paraflate_harness::HarnessConfig {
                root: output,
                level,
                threads,
                skip_large,
            })
            .map_err(|e| ParaflateError::CompressionFailed(format!("{e}")))?;
        }
    }
    Ok(())
}

fn print_intel_report(r: &ArchiveIntelReport) {
    println!(
        "summary entries={} raw_bytes={} global_entropy_bits={:.4} duplicate_cluster={:.4}",
        r.entry_count, r.total_raw_bytes, r.global_entropy_bits, r.duplicate_cluster_score
    );
    for f in &r.files {
        println!(
            "file path={} bytes={} entropy_bits={:.4} repeat={:.4} dup_proxy={:.4} match_proxy={:.4} stored_pred={} path_pred={:?} block_target={} est_ratio={:.4} est_decode_ns_b={:.2}",
            f.path,
            f.bytes,
            f.entropy_bits,
            f.repeat_density,
            f.duplicate_proxy,
            f.match_strength_proxy,
            f.predicted_stored,
            f.predicted_deflate_path,
            f.target_block_bytes,
            f.est_compressed_ratio,
            f.est_decode_ns_per_byte
        );
    }
}

fn write_explain_json(report: &ExplainReport, pretty: bool) -> Result<(), ParaflateError> {
    let s = if pretty {
        serde_json::to_string_pretty(report)
            .map_err(|e| ParaflateError::CompressionFailed(e.to_string()))?
    } else {
        serde_json::to_string(report)
            .map_err(|e| ParaflateError::CompressionFailed(e.to_string()))?
    };
    println!("{}", s);
    Ok(())
}

fn corpus_from_input(input: &PathBuf) -> Result<(Vec<u8>, u64), ParaflateError> {
    let scan = DirectoryScanner::new(input).scan()?;
    let mut v = scan.entries;
    v.sort_by(|a, b| a.logical_name.cmp(&b.logical_name));
    let mut out = Vec::new();
    let mut raw = 0u64;
    for e in &v {
        let mut f = File::open(&e.path).map_err(ParaflateError::Io)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(ParaflateError::Io)?;
        raw = raw.saturating_add(buf.len() as u64);
        out.extend_from_slice(&buf);
    }
    Ok((out, raw))
}

fn run_compare(input: PathBuf, level: u32, threads: usize) -> Result<(), ParaflateError> {
    let (corpus, raw) = corpus_from_input(&input)?;
    if raw == 0 {
        return Err(ParaflateError::EmptyArchive);
    }
    let t1 = Instant::now();
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(level));
    enc.write_all(&corpus).map_err(ParaflateError::Io)?;
    let zlib_out = enc.finish().map_err(ParaflateError::Io)?;
    let ms_f2 = t1.elapsed().as_millis().max(1) as f64;
    let mb = raw as f64 / (1024.0 * 1024.0);
    let mb_s_f2 = mb / (ms_f2 / 1000.0);
    let ratio_f2 = zlib_out.len() as f64 / raw as f64;
    let dir = tempfile::tempdir().map_err(ParaflateError::Io)?;
    for e in DirectoryScanner::new(&input).scan()?.entries {
        let dst = dir.path().join(&e.logical_name);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(ParaflateError::Io)?;
        }
        std::fs::copy(&e.path, &dst).map_err(ParaflateError::Io)?;
    }
    let zip_path = dir.path().join("compare.zip");
    let profile = build_profile(
        level,
        false,
        threads,
        ArchiveLayout::DeterministicLexical,
        SchedulerPolicy::ThroughputBiased,
        PredictiveMode::Standard,
        VerificationMode::Off,
        PlanningAggression::Balanced,
        false,
    );
    let t2 = Instant::now();
    let session = ArchiveSession::new();
    let report = session.create_archive(CreateArchiveParams::new(
        dir.path().to_path_buf(),
        zip_path.clone(),
        profile,
    ))?;
    let ms_pf = t2.elapsed().as_millis().max(1) as f64;
    let mb_s_pf = mb / (ms_pf / 1000.0);
    let ratio_pf = report.compressed_bytes as f64 / raw as f64;
    let winner = if ratio_pf < ratio_f2 {
        "paraflate_ratio"
    } else if ratio_pf > ratio_f2 {
        "flate2_ratio"
    } else {
        "tie_ratio"
    };
    let speed_winner = if mb_s_pf > mb_s_f2 {
        "paraflate_speed"
    } else if mb_s_pf < mb_s_f2 {
        "flate2_speed"
    } else {
        "tie_speed"
    };
    println!(
        "{{\"kind\":\"compare\",\"raw_bytes\":{},\"paraflate_ms\":{},\"paraflate_mb_s\":{:.3},\"paraflate_ratio\":{:.5},\"flate2_zlib_ms\":{},\"flate2_zlib_mb_s\":{:.3},\"flate2_zlib_ratio\":{:.5},\"ratio_winner\":\"{}\",\"speed_winner\":\"{}\",\"threads\":{}}}",
        raw,
        ms_pf as u64,
        mb_s_pf,
        ratio_pf,
        ms_f2 as u64,
        mb_s_f2,
        ratio_f2,
        winner,
        speed_winner,
        threads
    );
    Ok(())
}

fn map_layout(layout: LayoutArg) -> ArchiveLayout {
    match layout {
        LayoutArg::Lexical => ArchiveLayout::DeterministicLexical,
        LayoutArg::Size => ArchiveLayout::SizeDescending,
        LayoutArg::Score => ArchiveLayout::GlobalScoreDescending,
    }
}

fn map_scheduler(scheduler: SchedulerArg) -> SchedulerPolicy {
    match scheduler {
        SchedulerArg::Balanced => SchedulerPolicy::Balanced,
        SchedulerArg::Throughput => SchedulerPolicy::ThroughputBiased,
        SchedulerArg::Ratio => SchedulerPolicy::RatioBiased,
        SchedulerArg::Memory => SchedulerPolicy::MemoryConstrained,
    }
}

fn map_prediction(p: PredictionArg) -> PredictiveMode {
    match p {
        PredictionArg::Off => PredictiveMode::Off,
        PredictionArg::Standard => PredictiveMode::Standard,
        PredictionArg::Aggressive => PredictiveMode::Aggressive,
    }
}

fn map_verify(v: VerifyArg) -> VerificationMode {
    match v {
        VerifyArg::Off => VerificationMode::Off,
        VerifyArg::AfterWrite => VerificationMode::AfterWrite,
        VerifyArg::Strict => VerificationMode::Strict,
    }
}

fn map_planning(p: PlanningArg) -> PlanningAggression {
    match p {
        PlanningArg::Safe => PlanningAggression::Safe,
        PlanningArg::Balanced => PlanningAggression::Balanced,
        PlanningArg::Aggressive => PlanningAggression::Aggressive,
    }
}

fn build_profile(
    level: u32,
    stored: bool,
    threads: usize,
    layout: ArchiveLayout,
    scheduler: SchedulerPolicy,
    prediction: PredictiveMode,
    verification: VerificationMode,
    planning: PlanningAggression,
    global_huffman: bool,
) -> ArchiveProfile {
    let mut budget = ExecutionBudget::default();
    if threads > 0 {
        budget.worker_threads = threads;
        budget.pipeline_depth = threads.saturating_mul(2).max(8);
        budget.io_lane_count = threads.min(8).max(2);
        budget.max_pending_tasks = threads.saturating_mul(32).max(64);
    }
    let mut execution = ExecutionPolicy::default();
    execution.scheduler = scheduler;
    let method = if stored {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflate
    };
    let compression = CompressionProfile {
        method,
        level,
        strategy: DeflateStrategy::Default,
        window_bits: 15,
        global_huffman,
    };
    ArchiveProfile {
        layout,
        compression,
        execution,
        budget,
        reproducible: true,
        predictive: PredictiveRuntimeConfig {
            mode: prediction,
            verification,
            planning,
        },
    }
}
