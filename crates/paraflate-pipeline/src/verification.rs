use std::path::Path;

use paraflate_core::{ParaflateError, ParaflateResult, VerificationMode};
use paraflate_verify::verify_zip_bytes as verify_native;

pub use paraflate_verify::VerificationReport;

pub fn verify_zip_path(path: &Path, mode: VerificationMode) -> ParaflateResult<VerificationReport> {
    match mode {
        VerificationMode::Off => Err(ParaflateError::UnsupportedInput(
            "verification mode is off".into(),
        )),
        VerificationMode::AfterWrite | VerificationMode::Strict => {
            let data = std::fs::read(path).map_err(ParaflateError::Io)?;
            verify_zip_bytes(&data, mode)
        }
    }
}

pub fn verify_zip_bytes(
    data: &[u8],
    mode: VerificationMode,
) -> ParaflateResult<VerificationReport> {
    match mode {
        VerificationMode::Off => Err(ParaflateError::UnsupportedInput(
            "verification mode is off".into(),
        )),
        VerificationMode::AfterWrite => verify_native(data, false),
        VerificationMode::Strict => verify_native(data, true),
    }
}
