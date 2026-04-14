use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::corpus::Manifest;
use crate::error::HarnessResult;
use crate::modes::ModeSpec;
use crate::reference::{Flate2Metrics, NaiveMetrics, ReferenceZipMetrics};

#[derive(Clone, Debug, Serialize)]
pub struct ParaflateRow {
    pub workload_id: String,
    pub mode_id: String,
    pub mode_label: String,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub ratio: f64,
    pub elapsed_ms: u128,
    pub throughput_mb_s: f64,
    pub verification_strict_ok: bool,
    pub verification_after_write_ok: bool,
    pub roundtrip_entries: u64,
    pub fallback_events: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ComparisonRow {
    pub workload_id: String,
    pub paraflate_ratio: f64,
    pub flate2_ratio: f64,
    pub naive_ratio: f64,
    pub reference_zip_ratio: f64,
    pub ratio_winner: String,
    pub paraflate_throughput_mb_s: f64,
    pub flate2_throughput_mb_s: f64,
    pub speed_winner: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FailedCase {
    pub workload_id: String,
    pub mode_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct HarnessJsonReport {
    pub manifest: Manifest,
    pub total_input_files: u64,
    pub total_corpus_bytes: u64,
    pub workload_count: usize,
    pub mode_definitions: Vec<ModeDefJson>,
    pub archives_built: u64,
    pub modes_tested: usize,
    pub reference: BTreeMap<String, ReferenceBlock>,
    pub paraflate: Vec<ParaflateRow>,
    pub comparisons: Vec<ComparisonRow>,
    pub failed: Vec<FailedCase>,
    pub verification_pass_rate: f64,
    pub average_compression_ratio_paraflate: f64,
    pub best_ratio: f64,
    pub best_ratio_workload: String,
    pub best_throughput_mb_s: f64,
    pub best_throughput_workload: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModeDefJson {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReferenceBlock {
    pub flate2: Flate2Metrics,
    pub naive: NaiveMetrics,
    pub reference_zip: ReferenceZipMetrics,
}

pub fn write_json(path: &Path, report: &HarnessJsonReport) -> HarnessResult<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = File::create(path)?;
    f.write_all(serde_json::to_string_pretty(report)?.as_bytes())?;
    Ok(())
}

pub fn write_json_typed<T: serde::Serialize>(path: &Path, value: &T) -> HarnessResult<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = File::create(path)?;
    f.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    Ok(())
}

pub fn write_summary_txt(path: &Path, report: &HarnessJsonReport) -> HarnessResult<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = File::create(path)?;
    writeln!(f, "Paraflate evaluation harness")?;
    writeln!(f, "total_input_files: {}", report.total_input_files)?;
    writeln!(f, "total_corpus_bytes: {}", report.total_corpus_bytes)?;
    writeln!(f, "workload_count: {}", report.workload_count)?;
    writeln!(f, "archives_built: {}", report.archives_built)?;
    writeln!(f, "modes_tested: {}", report.modes_tested)?;
    writeln!(
        f,
        "verification_pass_rate: {:.4}",
        report.verification_pass_rate
    )?;
    writeln!(
        f,
        "average_compression_ratio_paraflate: {:.6}",
        report.average_compression_ratio_paraflate
    )?;
    writeln!(
        f,
        "best_ratio: {:.6} ({})",
        report.best_ratio, report.best_ratio_workload
    )?;
    writeln!(
        f,
        "best_throughput_mb_s: {:.3} ({})",
        report.best_throughput_mb_s, report.best_throughput_workload
    )?;
    writeln!(f, "failed_cases: {}", report.failed.len())?;
    for fc in &report.failed {
        writeln!(f, "  FAIL {} {} {}", fc.workload_id, fc.mode_id, fc.reason)?;
    }
    writeln!(f, "comparisons (winner by ratio = smallest)")?;
    for c in &report.comparisons {
        writeln!(
            f,
            "  {} ratio_winner={} paraflate={:.5} flate2={:.5} zip_ref={:.5}",
            c.workload_id, c.ratio_winner, c.paraflate_ratio, c.flate2_ratio, c.reference_zip_ratio
        )?;
    }
    Ok(())
}

pub fn write_comparison_table(path: &Path, report: &HarnessJsonReport) -> HarnessResult<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = File::create(path)?;
    writeln!(
        f,
        "workload\tparaflate_ratio\tflate2_ratio\tnaive_ratio\tref_zip_ratio\tratio_winner\tparaflate_mb_s\tflate2_mb_s\tspeed_winner"
    )?;
    for c in &report.comparisons {
        writeln!(
            f,
            "{}\t{:.6}\t{:.6}\t{:.6}\t{:.6}\t{}\t{:.3}\t{:.3}\t{}",
            c.workload_id,
            c.paraflate_ratio,
            c.flate2_ratio,
            c.naive_ratio,
            c.reference_zip_ratio,
            c.ratio_winner,
            c.paraflate_throughput_mb_s,
            c.flate2_throughput_mb_s,
            c.speed_winner
        )?;
    }
    Ok(())
}

pub fn mode_def_json(modes: &[ModeSpec]) -> Vec<ModeDefJson> {
    modes
        .iter()
        .map(|m| ModeDefJson {
            id: m.id.to_string(),
            label: m.label.to_string(),
        })
        .collect()
}
