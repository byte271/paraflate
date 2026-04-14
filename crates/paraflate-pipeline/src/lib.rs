mod archive_checker;
mod block_cost_model;
mod explain;
mod fallback_policy;
mod intel;
mod predictive_plan;
mod report;
mod session;
mod validation_cli;
mod verification;

pub use archive_checker::{
    local_header_payload_bounds, scan_end_of_central_directory, ArchiveStructuralSummary,
};
pub use explain::build_explain_report;
pub use intel::{analyze_directory, ArchiveIntelReport, FileIntelRow};
pub use predictive_plan::{build_entry_compress_hints, build_predictive_archive_plan};
pub use report::RunReport;
pub use session::{ArchiveSession, CreateArchiveParams};
pub use validation_cli::validate_archive_path;
pub use verification::{verify_zip_bytes, verify_zip_path, VerificationReport};
