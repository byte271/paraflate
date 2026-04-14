use std::fs;

use paraflate_core::{
    ArchiveProfile, CompressionMethod, CompressionProfile, DeflateStrategy, ExecutionBudget,
    PlanningAggression, PredictiveMode, PredictiveRuntimeConfig, VerificationMode,
};
use paraflate_pipeline::{
    validate_archive_path, verify_zip_bytes, ArchiveSession, CreateArchiveParams,
};
use paraflate_tests::roundtrip_test_helpers::{
    out_zip, read_zip_entry, run_create, temp_input_dir, zip_entry_count,
};

fn profile_predictive(
    mode: PredictiveMode,
    verification: VerificationMode,
    planning: PlanningAggression,
) -> ArchiveProfile {
    let mut p = ArchiveProfile::default();
    p.compression = CompressionProfile {
        method: CompressionMethod::Deflate,
        level: 6,
        strategy: DeflateStrategy::Default,
        window_bits: 15,
        global_huffman: true,
    };
    p.budget = ExecutionBudget {
        worker_threads: 2,
        pipeline_depth: 4,
        io_lane_count: 2,
        max_pending_tasks: 32,
        memory: p.budget.memory,
    };
    p.predictive = PredictiveRuntimeConfig {
        mode,
        verification,
        planning,
    };
    p
}

#[test]
fn predictive_aggressive_verify_after_write() {
    let dir = temp_input_dir(&[("a.txt", b"predictive deflate verification")]);
    let zip = out_zip(&dir, "p.zip");
    let profile = profile_predictive(
        PredictiveMode::Aggressive,
        VerificationMode::AfterWrite,
        PlanningAggression::Aggressive,
    );
    run_create(profile, dir.path().to_path_buf(), zip.clone());
    let v = validate_archive_path(zip.clone(), false).expect("validate");
    assert!(v.entries_verified >= 1);
    let got = read_zip_entry(&zip, "a.txt");
    assert_eq!(got, b"predictive deflate verification");
}

#[test]
fn predictive_off_matches_baseline_crc() {
    let dir = temp_input_dir(&[("b.bin", &[7u8; 4096])]);
    let zip = out_zip(&dir, "o.zip");
    let mut profile = profile_predictive(
        PredictiveMode::Off,
        VerificationMode::Strict,
        PlanningAggression::Safe,
    );
    profile.predictive.verification = VerificationMode::Strict;
    run_create(profile, dir.path().to_path_buf(), zip.clone());
    let bytes = fs::read(&zip).expect("read");
    let r = verify_zip_bytes(&bytes, VerificationMode::Strict).expect("verify bytes");
    assert_eq!(r.entries_verified as usize, zip_entry_count(&zip));
}

#[test]
fn stored_mode_roundtrip_with_verification() {
    let dir = temp_input_dir(&[("c.dat", &[0u8; 512])]);
    let zip = out_zip(&dir, "s.zip");
    let mut p = ArchiveProfile::default();
    p.compression.method = CompressionMethod::Stored;
    p.predictive.verification = VerificationMode::AfterWrite;
    run_create(p, dir.path().to_path_buf(), zip.clone());
    validate_archive_path(zip, false).expect("validate stored");
}

#[test]
fn fixed_strategy_roundtrip() {
    let dir = temp_input_dir(&[("d.txt", b"aaaaaaaa")]);
    let zip = out_zip(&dir, "f.zip");
    let mut p = ArchiveProfile::default();
    p.compression.strategy = DeflateStrategy::Fixed;
    p.predictive.verification = VerificationMode::Strict;
    run_create(p, dir.path().to_path_buf(), zip.clone());
    validate_archive_path(zip, true).expect("strict");
}

#[test]
fn many_small_duplicate_heavy() {
    let mut files = Vec::new();
    for i in 0u8..24 {
        files.push((format!("x{i}.txt"), vec![b'z'; 64]));
    }
    let dir = tempfile::tempdir().unwrap();
    for (n, b) in &files {
        fs::write(dir.path().join(n), b).unwrap();
    }
    let zip = dir.path().join("dup.zip");
    let profile = profile_predictive(
        PredictiveMode::Standard,
        VerificationMode::AfterWrite,
        PlanningAggression::Balanced,
    );
    let session = ArchiveSession::new();
    session
        .create_archive(CreateArchiveParams::new(
            dir.path().to_path_buf(),
            zip.clone(),
            profile,
        ))
        .unwrap();
    assert_eq!(zip_entry_count(&zip), 24);
    validate_archive_path(zip, false).unwrap();
}

#[test]
fn mixed_binary_and_text() {
    let dir = temp_input_dir(&[
        ("u.bin", &[0xFF, 0, 0xFE, 1, 2, 3]),
        ("v.txt", b"hello\nworld\n"),
    ]);
    let zip = out_zip(&dir, "mix.zip");
    let mut p = ArchiveProfile::default();
    p.predictive.mode = PredictiveMode::Standard;
    p.predictive.verification = VerificationMode::Strict;
    run_create(p, dir.path().to_path_buf(), zip.clone());
    validate_archive_path(zip, true).unwrap();
}

#[test]
fn verification_rejects_truncated_zip() {
    let dir = temp_input_dir(&[("e.txt", b"x")]);
    let zip = out_zip(&dir, "bad.zip");
    run_create(
        ArchiveProfile::default(),
        dir.path().to_path_buf(),
        zip.clone(),
    );
    let mut b = fs::read(&zip).unwrap();
    b.truncate(b.len().saturating_sub(5));
    fs::write(&zip, &b).unwrap();
    let err = validate_archive_path(zip, true).unwrap_err();
    let s = err.to_string();
    assert!(
        s.contains("verification")
            || s.contains("Zip")
            || s.contains("zip")
            || s.contains("structure")
            || s.contains("invalid")
            || s.contains("archive"),
        "{s}"
    );
}
