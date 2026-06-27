//! Shared integration/validation runners for clip-sync-repair tier binaries.
//! Not linked into the product library; dev-dep of clip-sync-repair tests only.

pub mod energy_matrix;
pub mod floor_oracle;
pub mod patch_audio;
pub mod residual_gate;
pub mod seam_residual;
pub mod w5_anchor_rescue_sweep;

/// `clip-sync-repair` crate root for corpus paths — must expand in a repair `[[test]]` binary.
///
/// ```ignore
/// use clip_sync_repair_harness::{floor_oracle::load_manifest, repair_tests_dir};
/// let manifest = load_manifest(repair_tests_dir!());
/// ```
#[macro_export]
macro_rules! repair_tests_dir {
    () => {
        ::std::path::Path::new(::core::env!("CARGO_MANIFEST_DIR"))
    };
}
