use std::fs;
use std::path::Path;

use paraflate_verify::{read_entry_bytes, verify_zip_bytes};

use crate::error::{HarnessError, HarnessResult};
use paraflate_io::DirectoryScanner;

pub struct RoundTripOutcome {
    pub strict_ok: bool,
    pub after_write_ok: bool,
    pub entries_checked: u64,
}

pub fn full_validate(zip_path: &Path, source_root: &Path) -> HarnessResult<RoundTripOutcome> {
    let data = fs::read(zip_path)?;
    let rep_aw = verify_zip_bytes(&data, false);
    let rep_st = verify_zip_bytes(&data, true);
    let after_write_ok = rep_aw.is_ok();
    let strict_ok = rep_st.is_ok();
    rep_aw.map_err(HarnessError::Paraflate)?;

    let scan = DirectoryScanner::new(source_root).scan()?;
    let mut checked = 0u64;
    let mut matched = 0u64;
    let mut err_detail = None;
    for e in &scan.entries {
        checked += 1;
        let got = read_entry_bytes(&data, &e.logical_name)?;
        let expected = fs::read(&e.path)?;
        if got != expected {
            err_detail = Some(format!("bytes mismatch {}", e.logical_name));
        } else {
            matched += 1;
        }
    }
    if matched != checked {
        return Err(HarnessError::Validation(
            err_detail.unwrap_or_else(|| "roundtrip mismatch".into()),
        ));
    }
    Ok(RoundTripOutcome {
        strict_ok,
        after_write_ok,
        entries_checked: checked,
    })
}
