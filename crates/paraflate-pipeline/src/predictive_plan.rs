use std::sync::Arc;

use paraflate_core::{
    ArchiveEntryDescriptor, ArchiveProfile, CompressionMethod, DeflateStrategy,
    EntryCompressionPlan, ExecutionPolicy, PredictedDeflatePath, PredictiveArchivePlan,
    PredictiveMode, PredictiveRuntimeConfig,
};
use paraflate_deflate::EntryCompressHints;
use paraflate_dictionary::GlobalModel;

use crate::block_cost_model::{
    bigram_repeat_proxy, byte_entropy_bits, global_huffman_hint, lz77_chain_multiplier,
    match_strength_proxy, recommend_deflate_path, recommend_stored, repeat_density,
    target_block_bytes,
};

pub fn build_predictive_archive_plan(
    entries: &[ArchiveEntryDescriptor],
    blobs: &[Option<Arc<Vec<u8>>>],
    model: &GlobalModel,
    policy: &ExecutionPolicy,
    cfg: &PredictiveRuntimeConfig,
    profile_method: CompressionMethod,
) -> PredictiveArchivePlan {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let data = blobs
            .get(e.id.0 as usize)
            .and_then(|b| b.as_ref())
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        out.push(build_entry_plan(
            e,
            data,
            model,
            policy,
            cfg,
            profile_method,
        ));
    }
    PredictiveArchivePlan { entries: out }
}

fn build_entry_plan(
    e: &ArchiveEntryDescriptor,
    data: &[u8],
    model: &GlobalModel,
    policy: &ExecutionPolicy,
    cfg: &PredictiveRuntimeConfig,
    profile_method: CompressionMethod,
) -> EntryCompressionPlan {
    let h = byte_entropy_bits(data);
    let r = repeat_density(data);
    let b = bigram_repeat_proxy(data);
    let m = match_strength_proxy(h, r, b);
    let dup = model.summary.duplicate_mass.max(1) as f64 / model.summary.text_mass.max(1) as f64;
    let dup_proxy = dup.min(8.0);
    let stored = recommend_stored(
        e.uncompressed_size,
        h,
        m,
        profile_method,
        cfg.planning,
        cfg.mode,
    );
    let deflate_path = recommend_deflate_path(h, m, cfg.mode, cfg.planning);
    let tb = target_block_bytes(
        e.uncompressed_size,
        h,
        r,
        model.suggested_block_bytes,
        policy.min_block_bytes as u64,
        policy.max_block_bytes as u64,
        cfg.mode,
        cfg.planning,
    );
    let lz = lz77_chain_multiplier(m, cfg.planning, cfg.mode);
    let gh = global_huffman_hint(e.uncompressed_size, tb, r, cfg.mode);
    EntryCompressionPlan {
        entry_id: e.id,
        entropy_bits: h,
        repeat_density: r,
        duplicate_proxy: dup_proxy,
        match_strength_proxy: m,
        recommended_stored: stored,
        deflate_path,
        target_block_bytes: tb,
        lz77_chain_mult: lz,
        use_global_huffman: gh,
    }
}

pub fn build_entry_compress_hints(
    archive: &ArchiveProfile,
    entry_plan: Option<&EntryCompressionPlan>,
    model: &GlobalModel,
) -> EntryCompressHints {
    let base = EntryCompressHints {
        profile: archive.compression.clone(),
        lz77_max_chain: None,
        lz77_nice_match: None,
        predicted_size_ratio: 0.72,
    };
    if archive.predictive.mode == PredictiveMode::Off {
        return base;
    }
    let Some(ep) = entry_plan else {
        return base;
    };
    let mut p = archive.compression.clone();
    if ep.recommended_stored && archive.compression.method == CompressionMethod::Deflate {
        p.method = CompressionMethod::Stored;
    }
    if p.method == CompressionMethod::Deflate {
        if ep.use_global_huffman {
            p.global_huffman = true;
        }
        match ep.deflate_path {
            PredictedDeflatePath::Fixed => {
                p.strategy = DeflateStrategy::Fixed;
            }
            PredictedDeflatePath::Dynamic => {
                p.strategy = DeflateStrategy::Default;
            }
            PredictedDeflatePath::Auto => {}
        }
    }
    let mc = (model.lz77_max_chain as f64 * ep.lz77_chain_mult)
        .round()
        .clamp(32.0, 2048.0) as u32;
    let nm = (model.lz77_nice_match as f64 * (0.82 + 0.18 * ep.match_strength_proxy.min(3.0) / 3.0))
        .round()
        .clamp(16.0, 258.0) as u32;
    let predicted_size_ratio = (ep.entropy_bits / 8.0).clamp(0.06, 1.0);
    EntryCompressHints {
        profile: p,
        lz77_max_chain: Some(mc),
        lz77_nice_match: Some(nm),
        predicted_size_ratio,
    }
}
