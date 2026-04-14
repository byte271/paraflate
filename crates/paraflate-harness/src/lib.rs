mod corpus;
mod error;
mod modes;
mod profile;
mod reference;
mod report;
mod validate;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

pub use error::{HarnessError, HarnessResult};
use modes::all_modes;
use paraflate_io::DirectoryScanner;
use paraflate_pipeline::{ArchiveSession, CreateArchiveParams};
use paraflate_verify::{read_entry_bytes, verify_zip_bytes};

use crate::corpus::{generate, workload_dirs, workload_id, Manifest};
use crate::profile::build_profile;
use crate::reference::{
    flatten_corpus, run_flate2_zlib, run_naive_flate, store_flate2_blob, write_reference_zip,
};
use crate::report::{
    mode_def_json, write_comparison_table, write_json, write_json_typed, write_summary_txt,
    ComparisonRow, FailedCase, HarnessJsonReport, ParaflateRow, ReferenceBlock,
};
use crate::validate::full_validate;

#[derive(Clone, Debug)]
pub struct HarnessConfig {
    pub root: PathBuf,
    pub level: u32,
    pub threads: usize,
    pub skip_large: bool,
}

pub fn run_harness(cfg: HarnessConfig) -> HarnessResult<()> {
    let root = cfg.root.clone();
    fs::create_dir_all(&root)?;
    let corpus_root = root.join("corpus");
    let reference_root = root.join("reference");
    let archives_root = root.join("paraflate_archives");
    let extracted_root = root.join("extracted");
    let reports_root = root.join("reports");
    for d in [
        &reference_root,
        &archives_root,
        &extracted_root,
        &reports_root,
    ] {
        fs::create_dir_all(d)?;
    }

    let log_path = reports_root.join("harness.log");
    let mut log = File::create(&log_path)?;
    let mut log_line = |s: &str| -> HarnessResult<()> {
        writeln!(log, "{}", s)?;
        Ok(())
    };

    let manifest: Manifest = generate(&root, cfg.skip_large)?;
    let total_input_files = manifest.files.len() as u64;
    let total_corpus_bytes: u64 = manifest.files.iter().map(|f| f.bytes).sum();
    log_line(&format!(
        "corpus_files {} bytes {}",
        total_input_files, total_corpus_bytes
    ))?;

    let workloads = workload_dirs(&corpus_root)?;
    let workload_count = workloads.len();
    let modes = all_modes();
    let modes_tested = modes.len();

    let mut reference_map: BTreeMap<String, ReferenceBlock> = BTreeMap::new();
    let mut comp_lookup: BTreeMap<String, (f64, f64, f64, u64)> = BTreeMap::new();

    for wl in &workloads {
        let wid = workload_id(wl);
        let (raw_flat, raw_total) = flatten_corpus(wl)?;
        let flate_dir = reference_root.join(&wid);
        fs::create_dir_all(&flate_dir)?;

        let (f2m, f2blob) = run_flate2_zlib(&raw_flat, cfg.level)?;
        store_flate2_blob(&flate_dir.join("flate2_zlib.bin"), &f2blob)?;

        let (naive_m, naive_blob) = run_naive_flate(&raw_flat)?;
        store_flate2_blob(&flate_dir.join("naive_zlib_fast.bin"), &naive_blob)?;

        let ref_zip_path = flate_dir.join("reference_deflated.zip");
        let refz = write_reference_zip(wl, &ref_zip_path)?;

        reference_map.insert(
            wid.clone(),
            ReferenceBlock {
                flate2: f2m.clone(),
                naive: naive_m.clone(),
                reference_zip: refz.clone(),
            },
        );
        comp_lookup.insert(
            wid.clone(),
            (f2m.ratio, naive_m.ratio, refz.ratio, raw_total),
        );
        log_line(&format!("reference_ok {}", wid))?;
    }

    let mut paraflate_rows: Vec<ParaflateRow> = Vec::new();
    let mut failed: Vec<FailedCase> = Vec::new();
    let mut archives_built: u64 = 0;
    let mut ver_pass = 0u64;
    let mut ver_total = 0u64;

    for wl in &workloads {
        let wid = workload_id(wl);
        for mode in &modes {
            let profile = build_profile(
                cfg.level,
                cfg.threads,
                mode.prediction,
                mode.verification,
                mode.adaptive,
                mode.global_huffman,
                mode.scheduler,
                mode.planning,
            );
            let zip_name = format!("{}__{}.zip", wid, mode.id);
            let zip_path = archives_root.join(&zip_name);
            let t0 = Instant::now();
            let session = ArchiveSession::new();
            let res = session.create_archive(
                CreateArchiveParams::new(wl.clone(), zip_path.clone(), profile.clone())
                    .with_debug(true)
                    .with_entry_stats(true),
            );
            let elapsed_ms = t0.elapsed().as_millis();
            match res {
                Ok(rep) => {
                    archives_built += 1;
                    let ratio = if rep.uncompressed_bytes == 0 {
                        0.0
                    } else {
                        rep.compressed_bytes as f64 / rep.uncompressed_bytes as f64
                    };
                    let mb = rep.uncompressed_bytes as f64 / (1024.0 * 1024.0);
                    let ms = elapsed_ms.max(1) as f64;
                    let tp = mb / (ms / 1000.0);
                    let fb = rep
                        .debug_blocks
                        .as_ref()
                        .map(|v| v.iter().filter(|b| b.fallback_code != 0).count() as u64)
                        .unwrap_or(0);

                    ver_total += 1;
                    let zip_data = fs::read(&zip_path)?;
                    let strict_ok = verify_zip_bytes(&zip_data, true).is_ok();
                    let aw_ok = verify_zip_bytes(&zip_data, false).is_ok();
                    if strict_ok && aw_ok {
                        ver_pass += 1;
                    }

                    let rt = full_validate(&zip_path, wl);
                    let ex_dir = extracted_root.join(format!("{}__{}", wid, mode.id));
                    let (verification_strict_ok, verification_after_write_ok, roundtrip_entries) =
                        match &rt {
                            Ok(o) => (o.strict_ok, o.after_write_ok, o.entries_checked),
                            Err(_) => (false, false, 0),
                        };

                    if rt.is_ok() {
                        fs::create_dir_all(&ex_dir)?;
                        for ent in DirectoryScanner::new(wl)
                            .scan()
                            .map_err(HarnessError::Paraflate)?
                            .entries
                        {
                            let b = read_entry_bytes(&zip_data, &ent.logical_name)
                                .map_err(HarnessError::Paraflate)?;
                            let p = ex_dir.join(&ent.logical_name);
                            if let Some(pa) = p.parent() {
                                fs::create_dir_all(pa)?;
                            }
                            fs::write(p, b)?;
                        }
                    }

                    if let Err(e) = rt {
                        failed.push(FailedCase {
                            workload_id: wid.clone(),
                            mode_id: mode.id.to_string(),
                            reason: e.to_string(),
                        });
                    }

                    paraflate_rows.push(ParaflateRow {
                        workload_id: wid.clone(),
                        mode_id: mode.id.to_string(),
                        mode_label: mode.label.to_string(),
                        compressed_bytes: rep.compressed_bytes,
                        uncompressed_bytes: rep.uncompressed_bytes,
                        ratio,
                        elapsed_ms,
                        throughput_mb_s: tp,
                        verification_strict_ok,
                        verification_after_write_ok,
                        roundtrip_entries,
                        fallback_events: fb,
                    });
                }
                Err(e) => {
                    failed.push(FailedCase {
                        workload_id: wid.clone(),
                        mode_id: mode.id.to_string(),
                        reason: e.to_string(),
                    });
                }
            }
            log_line(&format!("run {} {}", wid, mode.id))?;
        }
    }

    let mut comparisons: Vec<ComparisonRow> = Vec::new();
    for wl in &workloads {
        let wid = workload_id(wl);
        let Some(&(f2r, nav, zr, _raw_b)) = comp_lookup.get(&wid) else {
            continue;
        };
        let pf_best = paraflate_rows
            .iter()
            .filter(|r| r.workload_id == wid)
            .map(|r| r.ratio)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(1.0);
        let candidates = [
            ("paraflate", pf_best),
            ("flate2", f2r),
            ("naive", nav),
            ("reference_zip", zr),
        ];
        let ratio_winner = candidates
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|c| c.0.to_string())
            .unwrap_or_else(|| "none".into());

        let pf_speed = paraflate_rows
            .iter()
            .filter(|r| r.workload_id == wid)
            .map(|r| r.throughput_mb_s)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);
        let ref_blk = reference_map.get(&wid).unwrap();
        let speed_candidates = [
            ("paraflate", pf_speed),
            ("flate2", ref_blk.flate2.throughput_mb_s),
            ("naive", ref_blk.naive.throughput_mb_s),
            ("reference_zip", ref_blk.reference_zip.throughput_mb_s),
        ];
        let speed_winner = speed_candidates
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|c| c.0.to_string())
            .unwrap_or_else(|| "none".into());

        comparisons.push(ComparisonRow {
            workload_id: wid.clone(),
            paraflate_ratio: pf_best,
            flate2_ratio: f2r,
            naive_ratio: nav,
            reference_zip_ratio: zr,
            ratio_winner,
            paraflate_throughput_mb_s: pf_speed,
            flate2_throughput_mb_s: ref_blk.flate2.throughput_mb_s,
            speed_winner,
        });
    }

    let ok_run = paraflate_rows.len() as f64;
    let verification_pass_rate = if ver_total == 0 {
        0.0
    } else {
        ver_pass as f64 / ver_total as f64
    };
    let average_compression_ratio_paraflate = if paraflate_rows.is_empty() {
        0.0
    } else {
        paraflate_rows.iter().map(|r| r.ratio).sum::<f64>() / ok_run
    };
    let (best_ratio, best_ratio_workload) = paraflate_rows
        .iter()
        .map(|r| (r.ratio, r.workload_id.clone()))
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .unwrap_or((0.0, String::new()));
    let (best_tp, best_tp_wl) = paraflate_rows
        .iter()
        .map(|r| (r.throughput_mb_s, r.workload_id.clone()))
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .unwrap_or((0.0, String::new()));

    let report = HarnessJsonReport {
        manifest: manifest.clone(),
        total_input_files,
        total_corpus_bytes,
        workload_count,
        mode_definitions: mode_def_json(&modes),
        archives_built,
        modes_tested,
        reference: reference_map,
        paraflate: paraflate_rows.clone(),
        comparisons,
        failed: failed.clone(),
        verification_pass_rate,
        average_compression_ratio_paraflate,
        best_ratio,
        best_ratio_workload: best_ratio_workload.clone(),
        best_throughput_mb_s: best_tp,
        best_throughput_workload: best_tp_wl.clone(),
    };

    write_json(&reports_root.join("harness_run.json"), &report)?;
    write_summary_txt(&reports_root.join("summary.txt"), &report)?;
    write_json_typed(&reports_root.join("comparisons.json"), &report.comparisons)?;
    write_comparison_table(&reports_root.join("comparison_table.tsv"), &report)?;

    writeln!(
        log,
        "done archives_built={} failed={}",
        archives_built,
        failed.len()
    )?;

    if !failed.is_empty() {
        return Err(HarnessError::Other(format!(
            "harness failures {}",
            failed.len()
        )));
    }

    Ok(())
}
