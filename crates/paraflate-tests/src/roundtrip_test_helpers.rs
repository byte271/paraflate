use std::fs;
use std::path::{Path, PathBuf};

use paraflate_core::ArchiveProfile;
use paraflate_pipeline::{ArchiveSession, CreateArchiveParams};
use paraflate_verify::{read_entry_bytes, verify_zip_bytes};
use tempfile::TempDir;

pub fn temp_input_dir(files: &[(&str, &[u8])]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    for (name, bytes) in files {
        let p = root.join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(&p, bytes).expect("write");
    }
    dir
}

pub fn run_create(profile: ArchiveProfile, input_root: PathBuf, output_zip: PathBuf) {
    let session = ArchiveSession::new();
    session
        .create_archive(CreateArchiveParams::new(input_root, output_zip, profile))
        .expect("create_archive");
}

pub fn read_zip_entry(zip_path: &Path, entry_name: &str) -> Vec<u8> {
    let bytes = fs::read(zip_path).expect("read zip");
    read_entry_bytes(&bytes, entry_name).expect("entry")
}

pub fn zip_entry_count(zip_path: &Path) -> usize {
    let bytes = fs::read(zip_path).expect("read zip");
    verify_zip_bytes(&bytes, false)
        .expect("verify")
        .entries
        .len()
}

pub fn zip_names_sorted(zip_path: &Path) -> Vec<String> {
    let bytes = fs::read(zip_path).expect("read zip");
    let mut names: Vec<String> = verify_zip_bytes(&bytes, false)
        .expect("verify")
        .entries
        .into_iter()
        .map(|e| e.name)
        .collect();
    names.sort();
    names
}

pub fn out_zip(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}
