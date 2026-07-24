//! Licensing-safe numeric characterization of a repair gap ("gap fingerprint").
//!
//! Every field here is a number or enum — no audio samples, no transcripts — so a fingerprint can be
//! committed as a regression/calibration corpus from real (licensed) media. See
//! `docs/dev/archive/TEMP-gap-fingerprint-plan.md` and `docs/dev/gap-fingerprint.md`.
//!
//! Module map (M-MOD `gap_fingerprint` split): [`schema`] owns the serde corpus / per-gap types and
//! the [`source_id`] identity digest; [`project`] owns the Spec↔Fingerprint projection; [`measure`]
//! owns the PCM measurement / fingerprint-builder / corpus-writer path. This facade only re-exports
//! them so the public path `crate::application::gap_fingerprint::*` is unchanged. See
//! `docs/dev/TEMP-gap-fingerprint-module-split-plan.md`.

mod schema;
mod project;
mod measure;

pub use measure::*;
pub use project::*;
pub use schema::*;
