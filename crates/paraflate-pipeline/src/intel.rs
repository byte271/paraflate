use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use paraflate_core::{
    ArchiveEntryDescriptor, ArchiveProfile, ParaflateError, ParaflateResult, PredictedDeflatePath,
};
use paraflate_dictionary::{GlobalAnalyzer, SamplePlan};
use paraflate_io::{DirectoryScanner, FileReadPlan, FileReader};

use crate::predictive_plan::build_predictive_archive_plan;
use crate::session::ArchiveSession;

#[derive(Clone, Debug)]
pub struct FileIntelRow {
    pub path: String,
    pub bytes: u64,
    pub entropy_bits: f64,
    pub repeat_density: f64,
    pub duplicate_proxy: f64,
    pub match_strength_proxy: f64,
    pub predicted_stored: bool,
    pub predicted_deflate_path: PredictedDeflatePath,
    pub target_block_bytes: u64,
    pub est_compressed_ratio: f64,
    pub est_decode_ns_per_byte: f64,
}

#[derive(Clone, Debug)]
pub struct ArchiveIntelReport {
    pub entry_count: u64,
    pub total_raw_bytes: u64,
    pub global_entropy_bits: f64,
    pub duplicate_cluster_score: f64,
    pub files: Vec<FileIntelRow>,
}

pub fn analyze_directory(
    input_root: PathBuf,
    profile: &ArchiveProfile,
) -> ParaflateResult<ArchiveIntelReport> {
    let scanner = DirectoryScanner::new(&input_root);
    let scan = scanner.scan()?;
    if scan.entries.is_empty() {
        return Err(ParaflateError::EmptyArchive);
    }
    let mut entries = scan.entries;
    ArchiveSession::apply_layout(&mut entries, profile);
    let plan = SamplePlan::from_policy(&profile.execution);
    let samples = collect_samples(&entries, profile, &plan)?;
    let model = GlobalAnalyzer::analyze(&entries, &samples, &profile.execution, &plan);
    let mut blobs: Vec<Option<Arc<Vec<u8>>>> = vec![None; entries.len()];
    for e in &entries {
        let v = read_file_bytes(&e.path)?;
        blobs[e.id.0 as usize] = Some(Arc::new(v));
    }
    let pred_plan = build_predictive_archive_plan(
        &entries,
        &blobs,
        &model,
        &profile.execution,
        &profile.predictive,
        profile.compression.method,
    );
    let mut total = 0u64;
    let mut w_ent = 0f64;
    let mut files = Vec::new();
    for e in &entries {
        total = total.saturating_add(e.uncompressed_size);
        let ep = pred_plan
            .for_entry(e.id)
            .cloned()
            .unwrap_or_else(|| default_row_plan(e.id));
        w_ent += ep.entropy_bits * e.uncompressed_size as f64;
        let est_ratio = ep.entropy_bits / 8.0;
        let est_decode = 2.5 + ep.repeat_density * 6.0;
        files.push(FileIntelRow {
            path: e.logical_name.clone(),
            bytes: e.uncompressed_size,
            entropy_bits: ep.entropy_bits,
            repeat_density: ep.repeat_density,
            duplicate_proxy: ep.duplicate_proxy,
            match_strength_proxy: ep.match_strength_proxy,
            predicted_stored: ep.recommended_stored,
            predicted_deflate_path: ep.deflate_path,
            target_block_bytes: ep.target_block_bytes,
            est_compressed_ratio: est_ratio.clamp(0.05, 1.2),
            est_decode_ns_per_byte: est_decode,
        });
    }
    let global_entropy = if total > 0 { w_ent / total as f64 } else { 0.0 };
    let dup_score = model.summary.duplicate_mass as f64 / model.summary.text_mass.max(1) as f64;
    Ok(ArchiveIntelReport {
        entry_count: entries.len() as u64,
        total_raw_bytes: total,
        global_entropy_bits: global_entropy,
        duplicate_cluster_score: dup_score,
        files,
    })
}

fn default_row_plan(id: paraflate_core::EntryId) -> paraflate_core::EntryCompressionPlan {
    paraflate_core::EntryCompressionPlan {
        entry_id: id,
        entropy_bits: 4.0,
        repeat_density: 0.0,
        duplicate_proxy: 0.0,
        match_strength_proxy: 0.0,
        recommended_stored: false,
        deflate_path: PredictedDeflatePath::Auto,
        target_block_bytes: 256 * 1024,
        lz77_chain_mult: 1.0,
        use_global_huffman: false,
    }
}

fn collect_samples(
    entries: &[ArchiveEntryDescriptor],
    profile: &ArchiveProfile,
    plan: &SamplePlan,
) -> ParaflateResult<Vec<(usize, Vec<u8>)>> {
    let reader = FileReader::new(FileReadPlan {
        prefer_mmap_bytes: u64::MAX,
        chunk_bytes: profile.budget.memory.read_chunk_bytes,
    });
    let mut out = Vec::new();
    for (idx, e) in entries.iter().enumerate() {
        let window = plan.window_for_entry(e.uncompressed_size);
        if window == 0 {
            out.push((idx, Vec::new()));
            continue;
        }
        let mut file = File::open(&e.path)?;
        let mut buf = vec![0u8; window];
        let n = file.read(&mut buf)?;
        buf.truncate(n);
        out.push((idx, buf));
    }
    let _ = reader;
    Ok(out)
}

fn read_file_bytes(path: &PathBuf) -> ParaflateResult<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut v = Vec::new();
    f.read_to_end(&mut v)?;
    Ok(v)
}
