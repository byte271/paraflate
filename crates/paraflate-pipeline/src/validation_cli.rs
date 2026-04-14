use std::path::PathBuf;

use paraflate_core::{ParaflateResult, VerificationMode};

use crate::verification::{verify_zip_path, VerificationReport};

pub fn validate_archive_path(path: PathBuf, strict: bool) -> ParaflateResult<VerificationReport> {
    let mode = if strict {
        VerificationMode::Strict
    } else {
        VerificationMode::AfterWrite
    };
    verify_zip_path(&path, mode)
}
