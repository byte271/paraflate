use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use flate2::write::DeflateEncoder;
use flate2::Compression;
use paraflate_io::DirectoryScanner;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::error::{HarnessError, HarnessResult};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Flate2Metrics {
    pub zlib_bytes: u64,
    pub ratio: f64,
    pub elapsed_ms: u128,
    pub throughput_mb_s: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NaiveMetrics {
    pub zlib_bytes: u64,
    pub ratio: f64,
    pub elapsed_ms: u128,
    pub throughput_mb_s: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ReferenceZipMetrics {
    pub zip_bytes: u64,
    pub ratio: f64,
    pub elapsed_ms: u128,
    pub throughput_mb_s: f64,
}

pub fn flatten_corpus(workload_root: &Path) -> HarnessResult<(Vec<u8>, u64)> {
    let scan = DirectoryScanner::new(workload_root).scan()?;
    let mut v = scan.entries;
    v.sort_by(|a, b| a.logical_name.cmp(&b.logical_name));
    let mut out = Vec::new();
    let mut raw = 0u64;
    for e in &v {
        let mut f = fs::File::open(&e.path)?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut buf)?;
        raw = raw.saturating_add(buf.len() as u64);
        out.extend_from_slice(&buf);
    }
    Ok((out, raw))
}

pub fn run_flate2_zlib(raw: &[u8], level: u32) -> HarnessResult<(Flate2Metrics, Vec<u8>)> {
    if raw.is_empty() {
        return Err(HarnessError::Other("empty corpus".into()));
    }
    let t0 = Instant::now();
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(level));
    enc.write_all(raw)?;
    let zlib = enc.finish()?;
    let ms = t0.elapsed().as_millis().max(1) as f64;
    let raw_u = raw.len() as u64;
    let mb = raw_u as f64 / (1024.0 * 1024.0);
    Ok((
        Flate2Metrics {
            zlib_bytes: zlib.len() as u64,
            ratio: zlib.len() as f64 / raw_u as f64,
            elapsed_ms: t0.elapsed().as_millis(),
            throughput_mb_s: mb / (ms / 1000.0),
        },
        zlib,
    ))
}

pub fn run_naive_flate(raw: &[u8]) -> HarnessResult<(NaiveMetrics, Vec<u8>)> {
    if raw.is_empty() {
        return Err(HarnessError::Other("empty corpus".into()));
    }
    let t0 = Instant::now();
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(raw)?;
    let zlib = enc.finish()?;
    let ms = t0.elapsed().as_millis().max(1) as f64;
    let raw_u = raw.len() as u64;
    let mb = raw_u as f64 / (1024.0 * 1024.0);
    Ok((
        NaiveMetrics {
            zlib_bytes: zlib.len() as u64,
            ratio: zlib.len() as f64 / raw_u as f64,
            elapsed_ms: t0.elapsed().as_millis(),
            throughput_mb_s: mb / (ms / 1000.0),
        },
        zlib,
    ))
}

pub fn write_reference_zip(
    workload_root: &Path,
    dest: &Path,
) -> HarnessResult<ReferenceZipMetrics> {
    let scan = DirectoryScanner::new(workload_root).scan()?;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut raw_total = 0u64;
    for e in scan.entries.iter() {
        let mut f = fs::File::open(&e.path)?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut buf)?;
        raw_total += buf.len() as u64;
        entries.push((e.logical_name.clone(), buf));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if raw_total == 0 {
        return Err(HarnessError::Other("empty workload".into()));
    }
    let t0 = Instant::now();
    let file = File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (name, data) in &entries {
        zip.start_file(name.clone(), opts.clone())
            .map_err(|e| HarnessError::Zip(e.to_string()))?;
        zip.write_all(data)?;
    }
    zip.finish().map_err(|e| HarnessError::Zip(e.to_string()))?;
    let zip_len = fs::metadata(dest)?.len();
    let ms = t0.elapsed().as_millis().max(1) as f64;
    let mb = raw_total as f64 / (1024.0 * 1024.0);
    Ok(ReferenceZipMetrics {
        zip_bytes: zip_len,
        ratio: zip_len as f64 / raw_total as f64,
        elapsed_ms: t0.elapsed().as_millis(),
        throughput_mb_s: mb / (ms / 1000.0),
    })
}

pub fn store_flate2_blob(path: &Path, data: &[u8]) -> HarnessResult<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = File::create(path)?;
    f.write_all(data)?;
    Ok(())
}
