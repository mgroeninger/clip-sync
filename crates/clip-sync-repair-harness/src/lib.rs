//! Shared integration/validation runners for clip-sync-repair tier binaries.
//! Not linked into the product library; dev-dep of clip-sync-repair tests only.

pub mod energy_matrix;
pub mod floor_oracle;
pub mod residual_gate;
pub mod seam_residual;
