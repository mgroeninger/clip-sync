//! PCM measurement + fingerprint/corpus builders (M-MOD `measure` slice).
//!
//! Owns the lag summarize/classify path, the placement / seam-probe / dual-fit / wide-envelope
//! builders, [`build_gap_fingerprint`], [`compute_region_measurements`], `characterize_gaps*`, and
//! [`write_corpus_dir`]. Depends on [`super::schema`] (types) and [`super::project`]
//! (`tags_from_fields`) — never the reverse.

mod pcm;
mod config;
mod lag;
mod placement;
#[cfg(not(test))]
mod probes;
#[cfg(test)]
pub(super) mod probes;
mod build;
mod overlay;
mod characterize;

pub use config::{DEFAULT_LAG_ENVELOPE_MAX_LAG_MS, FingerprintConfig, GapInputs};
pub use lag::{
    lag_correlation_curve, lag_correlation_curve_auto, seam_local_peak, summarize_lag_curve,
};
// rustdoc: `schema.rs` intra-doc-links `super::measure::PROBE_BORDER_STANDOFF_FRAMES`.
#[allow(unused_imports)]
pub(crate) use pcm::PROBE_BORDER_STANDOFF_FRAMES;
pub use probes::splice_summary_from_lag;
pub use build::build_gap_fingerprint;
pub use characterize::{characterize_gaps, characterize_gaps_from_decode, CharacterizeAbPcm};
#[cfg(feature = "calibration")]
pub(crate) use characterize::{entry_stem, write_corpus_dir};
