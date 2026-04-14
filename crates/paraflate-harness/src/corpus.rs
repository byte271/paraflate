use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::HarnessResult;

#[derive(Clone, Debug, Serialize)]
pub struct ManifestFile {
    pub path: String,
    pub bytes: u64,
    pub sha256_hex: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Manifest {
    pub version: u32,
    pub generator: String,
    pub files: Vec<ManifestFile>,
}

fn xorshift(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

fn high_entropy(len: usize, seed: u64) -> Vec<u8> {
    let mut out = vec![0u8; len];
    let mut s = seed;
    for i in 0..len {
        s = xorshift(s.wrapping_add(i as u64));
        out[i] = (s >> 48) as u8;
    }
    out
}

fn write_bytes(p: &Path, data: &[u8]) -> HarnessResult<()> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(p)?;
    f.write_all(data)?;
    Ok(())
}

fn hash_file(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

pub fn generate(root: &Path, skip_large: bool) -> HarnessResult<Manifest> {
    let corpus = root.join("corpus");
    fs::create_dir_all(&corpus)?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    files.insert("wl_01_very_small_text/tiny_a.txt".into(), b"hi\n".to_vec());
    files.insert("wl_01_very_small_text/tiny_b.txt".into(), b"x".to_vec());

    let mut med = String::new();
    for i in 0..800 {
        med.push_str(&format!("word_{i} some text value={}\n", i % 97));
    }
    files.insert("wl_02_medium_text/medium.txt".into(), med.into_bytes());

    let mut large = String::new();
    for i in 0..12000 {
        large.push_str(&format!(
            "paragraph {i} lorem ipsum dolor sit amet {}\n",
            i % 50
        ));
    }
    files.insert("wl_03_large_text/large.txt".into(), large.into_bytes());

    files.insert(
        "wl_04_highly_repetitive/repetition.txt".into(),
        "A".repeat(200_000).into_bytes(),
    );

    let mut mix = String::new();
    for i in 0..3000 {
        mix.push_str(&format!("row {i} count {} label {}\n", i * 3, i % 13));
    }
    files.insert(
        "wl_05_mixed_text_numbers/mixed.txt".into(),
        mix.into_bytes(),
    );

    let mut jsonish = String::from("[\n");
    for i in 0..400 {
        jsonish.push_str(&format!("{{\"id\":{i},\"tag\":\"v\",\"ok\":true}},\n",));
    }
    jsonish.push(']');
    files.insert("wl_06_json_like/data.jsonish".into(), jsonish.into_bytes());

    let mut csv = String::from("id,name,value,score\n");
    for i in 0..2000 {
        csv.push_str(&format!("{i},cell{i},{},{}\n", i % 7, (i * 13) % 1000));
    }
    files.insert("wl_07_csv_like/table.csv".into(), csv.into_bytes());

    let mut logs = String::new();
    for i in 0..2500 {
        logs.push_str(&format!(
            "2024-01-{:02}T10:{:02}:00Z INFO event={} bytes={}\n",
            (i % 28) + 1,
            i % 60,
            i % 500,
            i * 17
        ));
    }
    files.insert("wl_08_log_like/app.log".into(), logs.into_bytes());

    let source = r#"fn main() {
    let x = 1usize;
    let y = x + 2;
    println!("{}", y);
    mod inner {
        pub fn f(z: u32) -> u32 { z.wrapping_mul(3) }
    }
}
"#;
    let mut src = String::new();
    for rep in 0..400 {
        src.push_str(&format!("// block {rep}\n{source}"));
    }
    files.insert("wl_09_source_like/sample.rs".into(), src.into_bytes());

    let mut bin = Vec::new();
    for round in 0..600 {
        for b in 0u16..256 {
            bin.push(b as u8);
        }
        bin.push((round & 0xff) as u8);
    }
    files.insert("wl_10_binary_like/bytes.dat".into(), bin);

    files.insert(
        "wl_11_random_entropy/rnd.bin".into(),
        high_entropy(180_000, 0xDEAD_BEEF_CAFE),
    );

    let chunk = "duplicate_block_marker;\n".repeat(600).into_bytes();
    let mut dup_big: Vec<u8> = Vec::new();
    for _ in 0..80 {
        dup_big.extend_from_slice(&chunk);
    }
    files.insert("wl_12_duplicate_heavy/dup.txt".into(), dup_big);

    let line = "nearly the same content block 0123456789abcdef\n";
    files.insert(
        "wl_13_near_duplicate/a.txt".into(),
        line.repeat(900).into_bytes(),
    );
    let mut btxt: Vec<u8> = line.repeat(900).into_bytes();
    if !btxt.is_empty() {
        btxt[42] = btxt[42].wrapping_add(1);
    }
    files.insert("wl_13_near_duplicate/b.txt".into(), btxt);

    for d in 0..35 {
        let p = format!("wl_14_nested_many_small/{}/{}/f{}.txt", d / 12, d % 12, d);
        files.insert(
            p,
            format!("small file index {d} data {}\n", d % 3).into_bytes(),
        );
    }

    if !skip_large {
        let large_n = 2 * 1024 * 1024;
        let mut v = Vec::with_capacity(large_n);
        let pat = high_entropy(64 * 1024, 0x1234);
        while v.len() < large_n {
            v.extend_from_slice(&pat);
        }
        v.truncate(large_n);
        files.insert("wl_15_very_large/big.bin".into(), v);
    }

    let mut mix_root: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    mix_root.insert(
        "wl_16_mixed_combined/text/readme.txt".into(),
        "readme mixed workload\n".repeat(400).into_bytes(),
    );
    mix_root.insert(
        "wl_16_mixed_combined/bin/part.dat".into(),
        high_entropy(40_000, 0x55AA),
    );
    mix_root.insert(
        "wl_16_mixed_combined/dup/one.txt".into(),
        "shared payload xyz\n".repeat(2000).into_bytes(),
    );
    mix_root.insert(
        "wl_16_mixed_combined/dup/two.txt".into(),
        "shared payload xyz\n".repeat(2000).into_bytes(),
    );
    let mut csv_mini = String::from("k,v,n\n");
    for i in 0..600 {
        csv_mini.push_str(&format!("{i},{},{}\n", i % 9, i % 41));
    }
    mix_root.insert(
        "wl_16_mixed_combined/csv/summary.csv".into(),
        csv_mini.into_bytes(),
    );
    for (k, v) in mix_root {
        files.insert(k, v);
    }

    let mut manifest_files = Vec::new();
    for (rel, data) in files.iter() {
        let p = corpus.join(rel);
        write_bytes(&p, data)?;
        manifest_files.push(ManifestFile {
            path: rel.clone(),
            bytes: data.len() as u64,
            sha256_hex: hash_file(data),
        });
    }

    let m = Manifest {
        version: 1,
        generator: "paraflate-harness".into(),
        files: manifest_files,
    };
    let manifest_path = root.join("manifest.json");
    let mut mf = fs::File::create(&manifest_path)?;
    mf.write_all(serde_json::to_string_pretty(&m)?.as_bytes())?;

    Ok(m)
}

pub fn workload_dirs(corpus: &Path) -> HarnessResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    for e in fs::read_dir(corpus)? {
        let e = e?;
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("wl_") && p.is_dir() {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

pub fn workload_id(dir: &Path) -> String {
    dir.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}
