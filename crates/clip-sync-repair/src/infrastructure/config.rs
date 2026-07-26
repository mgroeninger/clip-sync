use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use clip_sync::{unknown_toml_keys, AlignConfig, AppError, ConfigError, LoggingConfig};

use crate::application::mux_bitrate::parse_mux_audio_bitrate_policy;
use crate::application::patch_audio::PatchRequestSettings;
use crate::domain::{
    FitBoundarySearch, RepairProfile, RepairProfileBundle, RepairProfileFieldMask,
};

/// Default clip count for repair alignment (start + end windows on long media).
pub const REPAIR_DEFAULT_NUM_CLIPS: u32 = 2;

fn default_repair_align_config() -> AlignConfig {
    let mut align = AlignConfig::default();
    align.clip.num_clips = REPAIR_DEFAULT_NUM_CLIPS;
    // Auto (default): query-reference when inputs differ greatly in length; symmetric otherwise.
    align.alignment.mode = clip_sync::AlignmentMode::Auto;
    // Keep start-clip offset for fill when end disagrees; drift is surfaced in the report.
    align.alignment.require_consistent_offsets = false;
    // Sub-sample offset refinement is worth the extra cost for repair accuracy.
    align.alignment.refine_offset_high_rate = true;
    align
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepairConfig {
    /// Minimum silent window duration (ms) to include in the gap report.
    #[serde(default = "default_min_gap_ms")]
    pub min_gap_ms: u64,
    /// Fraction of peak amplitude below which a block is considered silent.
    #[serde(default = "default_silence_peak_fraction")]
    pub silence_peak_fraction: f32,
    /// Analysis block size (ms) when scanning for silent runs in decoded PCM.
    #[serde(default = "default_scan_block_ms")]
    pub scan_block_ms: u64,
    /// Decode chunk size (seconds) for sequential PCM scan — not gap detection granularity.
    #[serde(default = "default_decode_chunk_secs", alias = "scan_window_secs")]
    pub decode_chunk_secs: u64,
    /// Number of consecutive non-silent blocks to absorb before closing a silence run.
    /// Derived from `silence_hold_ms / scan_block_ms`; use `silence_hold_blocks()`.
    #[serde(default = "default_silence_hold_ms")]
    pub silence_hold_ms: u64,
    /// Absolute RMS floor (0–32767 scale) below which a block is always silent regardless of peak.
    /// Catches low-level codec noise in compressed-audio gaps. Set to 0 to disable.
    #[serde(default = "default_absolute_silence_rms")]
    pub absolute_silence_rms: f32,
    /// Also scan B's native timeline for silence to produce `gap_offset_agreement`.
    #[serde(default = "default_true")]
    pub scan_both: bool,
    /// Drop already-equivalent gaps (mutual/ambient silence — nothing to repair) from the fill plan at
    /// plan time, before decode/patch (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate). **On by default** (2026-07-20)
    /// after media validation (8 pairs, 121 gaps, 0 divergent vs the fine fingerprint reference). Disable
    /// with `--no-skip-equivalent-gaps` to patch every scanned gap regardless of silence character.
    #[serde(default = "default_true")]
    pub skip_equivalent_gaps: bool,
    /// Maximum |silence_offset − alignment_offset| (seconds) to count as agreement.
    #[serde(default = "default_gap_offset_tolerance_secs")]
    pub gap_offset_tolerance_secs: f64,
    /// Minimum normalized Pearson correlation at each gap seam (pre and post). Regions below
    /// this threshold on either seam are skipped during patch.
    #[serde(default = "default_min_fill_correlation")]
    pub min_fill_correlation: f32,
    /// Extra B audio (seconds) extracted on each side of the mapped gap for boundary alignment.
    #[serde(default = "default_fill_align_margin_secs")]
    pub fill_align_margin_secs: f64,
    /// Maximum slide (seconds) when searching for the best-matching B fill position.
    #[serde(default = "default_max_fill_align_adjustment_secs")]
    pub max_fill_align_adjustment_secs: f64,
    /// How far (seconds) to search in B for A's pre-gap border before local alignment.
    #[serde(default = "default_fill_border_search_secs")]
    pub fill_border_search_secs: f64,
    /// Minimum border template length (seconds) for discovery/correlation on short gaps.
    #[serde(default = "default_min_border_discovery_secs")]
    pub min_border_discovery_secs: f64,
    /// A-side only: exclude audio this close (seconds) to the dropout when building border templates.
    #[serde(default = "default_border_standoff_secs")]
    pub border_standoff_secs: f64,
    /// Gaps at or below this length use mean(pre, post) correlation for the fill gate.
    #[serde(default = "default_short_gap_mean_correlation_secs")]
    pub short_gap_mean_correlation_secs: f64,
    /// How far B fill length may differ from A's scanned gap when locating the post-border
    /// (end-search / `max_fill` only).
    #[serde(default = "default_fill_length_slack_secs")]
    pub fill_length_slack_secs: f64,
    /// Extra B haystack tail beyond the refined mapped end (seconds), before `max` with
    /// `fill_align_margin_secs`. Sizes `b_extract_end` / fingerprint `pad_tail` only — not the
    /// end-search range. Split from `fill_length_slack_secs` so the two can move independently.
    #[serde(default = "default_fill_extract_tail_slack_secs")]
    pub fill_extract_tail_slack_secs: f64,
    /// Seam correlation window (seconds) for fine align slide search and the fill gate.
    #[serde(default = "default_fill_seam_search_secs")]
    pub fill_seam_search_secs: f64,
    /// Seconds of A audio on each side of the gap used to build the structure signature.
    #[serde(default = "default_gap_signature_context_secs")]
    pub gap_signature_context_secs: f64,
    /// Bin width (milliseconds) for active/silent structure signatures.
    #[serde(default = "default_gap_signature_bin_ms")]
    pub gap_signature_bin_ms: u64,
    /// Minimum active/silent pattern match score (0–1) at each seam before waveform gate.
    #[serde(default = "default_min_structure_match_score")]
    pub min_structure_match_score: f32,
    /// Both structure seam scores must meet this to skip the waveform Pearson gate.
    #[serde(default = "default_strong_structure_trust")]
    pub strong_structure_trust: f64,
    /// When true: always run the waveform Pearson seam gate (never skip via
    /// `strong_structure_trust`), disable `partial_structure_waveform_soften`, and require
    /// both waveform seams to pass (no short-gap mean / one-strong-seam shortcuts). Structure
    /// matching on B still runs. CLI: `--no-structure-trust`.
    #[serde(default)]
    pub disable_structure_trust: bool,
    /// In the waveform gate path, soften Pearson threshold when structure scores meet this.
    #[serde(default = "default_partial_structure_waveform_soften")]
    pub partial_structure_waveform_soften: f64,
    /// Crossfade duration at gap boundaries (ms).
    #[serde(default = "default_crossfade_ms")]
    pub crossfade_ms: u64,
    /// Normalize fill segment loudness to match A's border.
    #[serde(default = "default_true")]
    pub normalize_fill: bool,
    /// **A3 dual-fit repair** — fallback fill for bracket-exhausted skips (independent per-shoulder fit +
    /// interior trim, validated by the unchanged gate). On by default; `--no-dual-fit` to disable.
    #[serde(default = "default_true")]
    pub dual_fit: bool,
    /// Tier-3 gap-fingerprint fields (`seam_probe`, `wide_envelope`, diagnostic `lag`, `b_levels`).
    /// Off by default — `--gap-fingerprints` emits decision/repair (D/R) axes only unless enabled.
    #[serde(default)]
    pub fingerprint_diagnostics: bool,
    /// Window size (seconds) for computing A's border RMS.
    #[serde(default = "default_normalize_window_secs")]
    pub normalize_window_secs: f64,
    /// Maximum gain (dB) applied during normalization.
    #[serde(default = "default_max_fill_gain_db")]
    pub max_fill_gain_db: f64,
    /// Output configuration (paths for writing repaired audio/video).
    #[serde(default)]
    pub output: RepairOutputConfig,
    /// Dry-run: scan and report only; do not write any output files.
    /// Cleared when `--wav` / `--mux` (or output paths) select write mode; kept when
    /// [`Self::repair_preview`] is set (characterize without write).
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// After scan, run pass-1 characterize and report would-be patch/skip decisions without
    /// splice/write (`--repair-preview`). Mutually exclusive with output paths; implies
    /// scan+characterize rather than scan-only. See [`crate::application::run_repair::PendingAfterScan`].
    #[serde(default)]
    pub repair_preview: bool,
    /// When query-reference alignment is used, only treat gaps inside the mapped B coverage
    /// region as fillable (gaps outside are still reported).
    #[serde(default = "default_true")]
    pub limit_fill_to_mapped_region: bool,
    /// How to map each gap on A to B during patch (`recommended` or drift-interpolated).
    #[serde(default)]
    pub fill_offset_mode: crate::domain::FillOffsetMode,
    /// Gap-fill placement after structure match: `gate` (legacy thresholds) or `fit` (waveform slide search).
    #[serde(default)]
    pub fill_mode: crate::domain::FillMode,
    /// Unified fit: weight on structure combined score (Phase B; fit mode only).
    #[serde(default = "default_fill_fit_structure_weight")]
    pub fill_fit_structure_weight: f64,
    /// Unified fit: weight on `min(pre, post)` waveform Pearson (Phase B; fit mode only).
    #[serde(default = "default_fill_fit_waveform_weight")]
    pub fill_fit_waveform_weight: f64,
    /// Unified fit: scale distance-from-nominal penalty in structure tier (1.0 = production default).
    #[serde(default = "default_fill_fit_nominal_bias_scale")]
    pub fill_fit_nominal_bias_scale: f64,
    /// Unified fit: distance-from-nominal penalty scale used when the resolved gap signature is
    /// **energy** (mode-coupled bias). An energy match signals the nominal map may be wrong, so the
    /// default is lower than [`Self::fill_fit_nominal_bias_scale`] to let a confident energy
    /// contour override a drifted nominal. Bool-resolved gaps keep the base scale.
    #[serde(default = "default_fill_fit_energy_nominal_bias_scale")]
    pub fill_fit_energy_nominal_bias_scale: f64,
    /// Unified fit: scale late-start penalty when candidate start exceeds nominal (1.0 = default).
    #[serde(default = "default_fill_fit_late_start_penalty_scale")]
    pub fill_fit_late_start_penalty_scale: f64,
    /// Fit mode: patch band below `min_fill_correlation` with a marginal warning (Phase C).
    #[serde(default = "default_fill_marginal_margin")]
    pub fill_marginal_margin: f32,
    /// Fit mode: hard skip when `min(pre, post)` is below this (Phase C).
    #[serde(default = "default_fill_absolute_floor")]
    pub fill_absolute_floor: f32,
    /// Fit mode: penalize high A-border vs B-fill repeat correlation when seams are weak (Phase D).
    #[serde(default = "default_fill_repeat_penalty_weight")]
    pub fill_repeat_penalty_weight: f64,
    /// Lever 1 (TEMP-production-repair-perf-plan.md §2.5): FFT-accelerated seam band in the production unified
    /// start-search refine (perf only; output-neutral up to a sub-ms near-tie, guarded by the exact naive
    /// re-score + placement-diff test). On by default; `--no-fft-seam-search` opts out to the exact naive search.
    #[serde(default = "default_true")]
    pub fft_seam_search: bool,
    /// Minimum `min(pre, post)` for a pass-1 patch to become an offset anchor (`anchored_retry`).
    #[serde(default = "default_fill_anchor_min_correlation")]
    pub fill_anchor_min_correlation: f32,
    /// Exclude structure-trusted gate patches (no waveform) from the anchor table.
    #[serde(default = "default_true")]
    pub fill_anchor_exclude_structure_trusted: bool,
    /// Reject anchors whose `|align_adjustment|` exceeds this fraction of `fill_border_search_secs`.
    #[serde(default = "default_fill_anchor_max_adjustment_frac")]
    pub fill_anchor_max_adjustment_frac: f64,
    /// Fit mode: penalize unified-search candidates far from patch-anchor prediction (0 = off).
    #[serde(default)]
    pub fill_anchor_search_prior_weight: f64,
    /// `anchored_retry` pass 2: re-run fit-mode marginal pass-1 patches with anchored offset.
    #[serde(default)]
    pub fill_anchor_retry_marginal: bool,
    /// Structure signature for gap fill search (`bool`, `energy`, or `auto`).
    #[serde(default)]
    pub gap_signature_mode: crate::domain::GapSignatureMode,
    /// When waveform post-seam correlation fails, try extending the gap end on A.
    #[serde(default = "default_true")]
    pub gap_end_extend_on_post_seam_fail: bool,
    /// When waveform pre-seam correlation fails, try extending the gap start on A.
    #[serde(default = "default_true")]
    pub gap_start_extend_on_pre_seam_fail: bool,
    /// Maximum gap-end extension when retrying a failed post seam (ms).
    #[serde(default = "default_gap_end_extend_max_ms")]
    pub gap_end_extend_max_ms: u64,
    /// Step size for gap-end extension retries (ms).
    #[serde(default = "default_gap_end_extend_step_ms")]
    pub gap_end_extend_step_ms: u64,
    /// For short gaps, allow patch when either seam meets the threshold (after mean fails).
    #[serde(default = "default_true")]
    pub short_gap_one_strong_seam_fallback: bool,
    /// Repair speed/quality preset (`default`, `quick`, `full`).
    #[serde(default)]
    pub profile: RepairProfile,
    /// Fit mode: skip boundary grid on marginal baseline (`baseline_only`) or run full grid.
    #[serde(default)]
    pub fit_boundary_search: FitBoundarySearch,
    /// Editorial seam anchor search (`off`, `auto`, `force`); fit mode only.
    #[serde(default)]
    pub anchor_seam_mode: crate::domain::AnchorSeamMode,
    /// Maximum A bracket span when searching editorial seam anchors (seconds).
    #[serde(default = "default_max_anchor_bracket_secs")]
    pub max_anchor_bracket_secs: f64,
    /// Max anchor candidates per side of the scan hole.
    #[serde(default = "default_max_anchors_per_side")]
    pub max_anchors_per_side: usize,
    /// Minimum energy-bin prominence for peak anchor candidates (0 = any local maximum).
    #[serde(default)]
    pub anchor_seam_min_prominence: f32,
    /// Minimum anchor-window Pearson on B for editorial seam brackets.
    #[serde(default = "default_anchor_seam_min_match_pearson")]
    pub anchor_seam_min_match_pearson: f32,
    /// Minimum GCC-PHAT peak at an anchor when Pearson is ambiguous.
    #[serde(default = "default_anchor_seam_min_xcorr_peak")]
    pub anchor_seam_min_xcorr_peak: f32,
    /// Pearson band below `anchor_seam_min_match_pearson` that may trigger xcorr.
    #[serde(default = "default_anchor_seam_xcorr_ambiguous_band")]
    pub anchor_seam_xcorr_ambiguous_band: f32,
    /// Residual headroom gate (`off`, `veto`, `veto_rescue`); fit mode only.
    #[serde(default)]
    pub residual_gate: crate::domain::ResidualGateMode,
    /// Nominal floor must be at or below this (dB) for the residual gate to apply.
    #[serde(default = "default_residual_floor_ok_db")]
    pub residual_floor_ok_db: f64,
    /// Max headroom (dB) before informative residual veto / rescue.
    #[serde(default = "default_residual_headroom_margin_db")]
    pub residual_headroom_margin_db: f64,
    /// Unified seam/floor integer-lag search radius (seconds).
    #[serde(default = "default_residual_lag_secs")]
    pub residual_lag_secs: f64,
    /// Fields set explicitly in TOML (or by later CLI overrides of profile-bundle
    /// knobs). Survives CLI `--quick`/`--full` profile re-application.
    ///
    /// Runtime-only: skipped by serde and ignored by `PartialEq` so TOML
    /// round-trips compare equal to a freshly loaded config's values.
    #[serde(skip)]
    #[doc(hidden)]
    pub profile_field_mask: RepairProfileFieldMask,
}

fn default_min_gap_ms() -> u64 {
    // Sensitive scan default (2026-07-20): catches sub-second dropouts ffmpeg `silencedetect` sees. The
    // scan-time equivalence gate (`skip_equivalent_gaps`) drops the mutual/ambient-silence extras this
    // surfaces, so "find everything, patch only real dropouts" is the default pairing.
    500
}
fn default_silence_peak_fraction() -> f32 {
    0.01
}
fn default_scan_block_ms() -> u64 {
    // 100 ms analysis blocks (sensitive default) — finer silence-run resolution and the equivalence gate's
    // measurement granularity. Media-validated identical to the fine fingerprint reference (8 pairs, 121 gaps).
    100
}
fn default_decode_chunk_secs() -> u64 {
    10
}
fn default_silence_hold_ms() -> u64 {
    500
}
fn default_absolute_silence_rms() -> f32 {
    33.0 / 32767.0
}
fn default_true() -> bool {
    true
}
fn default_gap_offset_tolerance_secs() -> f64 {
    0.5
}
fn default_min_fill_correlation() -> f32 {
    0.35
}
fn default_fill_align_margin_secs() -> f64 {
    1.0
}
fn default_max_fill_align_adjustment_secs() -> f64 {
    0.5
}
fn default_fill_border_search_secs() -> f64 {
    10.0
}
fn default_min_border_discovery_secs() -> f64 {
    2.0
}
fn default_border_standoff_secs() -> f64 {
    0.35
}
fn default_short_gap_mean_correlation_secs() -> f64 {
    2.0
}
fn default_fill_length_slack_secs() -> f64 {
    1.0
}
fn default_fill_extract_tail_slack_secs() -> f64 {
    5.0
}
fn default_fill_seam_search_secs() -> f64 {
    0.25
}
fn default_gap_signature_context_secs() -> f64 {
    3.0
}
fn default_gap_signature_bin_ms() -> u64 {
    50
}
fn default_min_structure_match_score() -> f32 {
    0.55
}
fn default_strong_structure_trust() -> f64 {
    0.90
}
fn default_partial_structure_waveform_soften() -> f64 {
    0.85
}
fn default_crossfade_ms() -> u64 {
    10
}
fn default_normalize_window_secs() -> f64 {
    5.0
}
fn default_max_fill_gain_db() -> f64 {
    12.0
}
fn default_gap_end_extend_max_ms() -> u64 {
    500
}
fn default_gap_end_extend_step_ms() -> u64 {
    20
}
fn default_fill_fit_structure_weight() -> f64 {
    0.35
}
fn default_fill_fit_waveform_weight() -> f64 {
    0.65
}
fn default_fill_fit_nominal_bias_scale() -> f64 {
    1.0
}
fn default_fill_fit_energy_nominal_bias_scale() -> f64 {
    // Lower than the base 1.0: the penalty grows linearly with distance from the nominal map
    // (`0.02 × scale × bins`), so this only loosens far-off (drift) candidates while small offsets
    // win under either scale. 0.25 recovers a 7 s-off nominal in the F4 EC-6 sweep.
    0.25
}
fn default_fill_fit_late_start_penalty_scale() -> f64 {
    1.0
}
fn default_fill_marginal_margin() -> f32 {
    crate::domain::gap_fill_fit::DEFAULT_FILL_MARGINAL_MARGIN
}
fn default_fill_absolute_floor() -> f32 {
    crate::domain::gap_fill_fit::DEFAULT_FILL_ABSOLUTE_FLOOR
}
fn default_residual_floor_ok_db() -> f64 {
    crate::domain::policies::DEFAULT_RESIDUAL_FLOOR_OK_DB
}
fn default_residual_headroom_margin_db() -> f64 {
    crate::domain::DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB
}
fn default_residual_lag_secs() -> f64 {
    crate::domain::DEFAULT_RESIDUAL_LAG_SECS
}
fn default_fill_repeat_penalty_weight() -> f64 {
    0.4
}
fn default_fill_anchor_min_correlation() -> f32 {
    default_min_fill_correlation()
}
fn default_fill_anchor_max_adjustment_frac() -> f64 {
    0.9
}
fn default_max_anchor_bracket_secs() -> f64 {
    5.0
}
fn default_max_anchors_per_side() -> usize {
    5
}
fn default_anchor_seam_min_match_pearson() -> f32 {
    crate::domain::DEFAULT_ANCHOR_MATCH_MIN_PEARSON
}
fn default_anchor_seam_min_xcorr_peak() -> f32 {
    crate::domain::DEFAULT_ANCHOR_MATCH_MIN_XCORR_PEAK
}
fn default_anchor_seam_xcorr_ambiguous_band() -> f32 {
    crate::domain::DEFAULT_ANCHOR_MATCH_XCORR_AMBIGUOUS_BAND
}

fn apply_profile_bundle_fields(
    repair: &mut RepairConfig,
    bundle: RepairProfileBundle,
    mask: RepairProfileFieldMask,
) {
    if !mask.fill_border_search_secs {
        repair.fill_border_search_secs = bundle.fill_border_search_secs;
    }
    if !mask.gap_end_extend_on_post_seam_fail {
        repair.gap_end_extend_on_post_seam_fail = bundle.gap_end_extend_on_post_seam_fail;
    }
    if !mask.gap_start_extend_on_pre_seam_fail {
        repair.gap_start_extend_on_pre_seam_fail = bundle.gap_start_extend_on_pre_seam_fail;
    }
    if !mask.fit_boundary_search {
        repair.fit_boundary_search = bundle.fit_boundary_search;
    }
}

pub(crate) fn repair_profile_field_mask_from_toml(repair_table: &toml::Table) -> RepairProfileFieldMask {
    RepairProfileFieldMask {
        fill_border_search_secs: repair_table.contains_key("fill_border_search_secs"),
        gap_end_extend_on_post_seam_fail: repair_table
            .contains_key("gap_end_extend_on_post_seam_fail"),
        gap_start_extend_on_pre_seam_fail: repair_table
            .contains_key("gap_start_extend_on_pre_seam_fail"),
        fit_boundary_search: repair_table.contains_key("fit_boundary_search"),
    }
}

impl Default for RepairConfig {
    fn default() -> Self {
        Self {
            min_gap_ms: default_min_gap_ms(),
            silence_peak_fraction: default_silence_peak_fraction(),
            scan_block_ms: default_scan_block_ms(),
            decode_chunk_secs: default_decode_chunk_secs(),
            silence_hold_ms: default_silence_hold_ms(),
            absolute_silence_rms: default_absolute_silence_rms(),
            scan_both: default_true(),
            skip_equivalent_gaps: true,
            gap_offset_tolerance_secs: default_gap_offset_tolerance_secs(),
            min_fill_correlation: default_min_fill_correlation(),
            fill_align_margin_secs: default_fill_align_margin_secs(),
            max_fill_align_adjustment_secs: default_max_fill_align_adjustment_secs(),
            fill_border_search_secs: default_fill_border_search_secs(),
            min_border_discovery_secs: default_min_border_discovery_secs(),
            border_standoff_secs: default_border_standoff_secs(),
            short_gap_mean_correlation_secs: default_short_gap_mean_correlation_secs(),
            fill_length_slack_secs: default_fill_length_slack_secs(),
            fill_extract_tail_slack_secs: default_fill_extract_tail_slack_secs(),
            fill_seam_search_secs: default_fill_seam_search_secs(),
            gap_signature_context_secs: default_gap_signature_context_secs(),
            gap_signature_bin_ms: default_gap_signature_bin_ms(),
            min_structure_match_score: default_min_structure_match_score(),
            strong_structure_trust: default_strong_structure_trust(),
            disable_structure_trust: false,
            partial_structure_waveform_soften: default_partial_structure_waveform_soften(),
            crossfade_ms: default_crossfade_ms(),
            normalize_fill: default_true(),
            dual_fit: true,
            fingerprint_diagnostics: false,
            normalize_window_secs: default_normalize_window_secs(),
            max_fill_gain_db: default_max_fill_gain_db(),
            output: RepairOutputConfig::default(),
            dry_run: default_true(),
            repair_preview: false,
            limit_fill_to_mapped_region: default_true(),
            fill_offset_mode: crate::domain::FillOffsetMode::default(),
            fill_mode: crate::domain::FillMode::default(),
            fill_fit_structure_weight: default_fill_fit_structure_weight(),
            fill_fit_waveform_weight: default_fill_fit_waveform_weight(),
            fill_fit_nominal_bias_scale: default_fill_fit_nominal_bias_scale(),
            fill_fit_energy_nominal_bias_scale: default_fill_fit_energy_nominal_bias_scale(),
            fill_fit_late_start_penalty_scale: default_fill_fit_late_start_penalty_scale(),
            fill_marginal_margin: default_fill_marginal_margin(),
            fill_absolute_floor: default_fill_absolute_floor(),
            fill_repeat_penalty_weight: default_fill_repeat_penalty_weight(),
            fft_seam_search: true,
            fill_anchor_min_correlation: default_fill_anchor_min_correlation(),
            fill_anchor_exclude_structure_trusted: true,
            fill_anchor_max_adjustment_frac: default_fill_anchor_max_adjustment_frac(),
            fill_anchor_search_prior_weight: 0.0,
            fill_anchor_retry_marginal: false,
            gap_signature_mode: crate::domain::GapSignatureMode::default(),
            gap_end_extend_on_post_seam_fail: true,
            gap_start_extend_on_pre_seam_fail: true,
            gap_end_extend_max_ms: default_gap_end_extend_max_ms(),
            gap_end_extend_step_ms: default_gap_end_extend_step_ms(),
            short_gap_one_strong_seam_fallback: true,
            profile: RepairProfile::default(),
            fit_boundary_search: FitBoundarySearch::default(),
            anchor_seam_mode: crate::domain::AnchorSeamMode::default(),
            max_anchor_bracket_secs: default_max_anchor_bracket_secs(),
            max_anchors_per_side: default_max_anchors_per_side(),
            anchor_seam_min_prominence: 0.0,
            anchor_seam_min_match_pearson: default_anchor_seam_min_match_pearson(),
            anchor_seam_min_xcorr_peak: default_anchor_seam_min_xcorr_peak(),
            anchor_seam_xcorr_ambiguous_band: default_anchor_seam_xcorr_ambiguous_band(),
            residual_gate: crate::domain::ResidualGateMode::default(),
            residual_floor_ok_db: default_residual_floor_ok_db(),
            residual_headroom_margin_db: default_residual_headroom_margin_db(),
            residual_lag_secs: default_residual_lag_secs(),
            profile_field_mask: RepairProfileFieldMask::default(),
        }
    }
}

fn default_video_codec() -> String {
    "copy".into()
}

fn default_audio_codec() -> String {
    "aac".into()
}

fn default_mux_audio_bitrate() -> String {
    "match_min".into()
}

/// Output configuration for the repair tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairOutputConfig {
    /// Write patched audio to this WAV file.
    pub wav_path: Option<PathBuf>,
    /// Mux patched audio into video A via ffmpeg (R5, requires `ffmpeg-mux` feature).
    pub video_path: Option<PathBuf>,
    /// Video stream codec for mux (`copy` preserves the source stream).
    #[serde(default = "default_video_codec")]
    pub video_codec: String,
    /// Audio stream codec for mux (patched WAV is encoded with this codec).
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
    /// Mux AAC bitrate: `match_min`, `match_a`, `default`, or explicit e.g. `256k`.
    #[serde(default = "default_mux_audio_bitrate")]
    pub mux_audio_bitrate: String,
}

impl Default for RepairOutputConfig {
    fn default() -> Self {
        Self {
            wav_path: None,
            video_path: None,
            video_codec: default_video_codec(),
            audio_codec: default_audio_codec(),
            mux_audio_bitrate: default_mux_audio_bitrate(),
        }
    }
}

impl RepairConfig {
    pub fn scan_block_secs(&self) -> f64 {
        self.scan_block_ms as f64 / 1000.0
    }

    pub fn min_gap_secs(&self) -> f64 {
        self.min_gap_ms as f64 / 1000.0
    }

    pub fn silence_hold_blocks(&self) -> u32 {
        if self.scan_block_ms == 0 {
            return 0;
        }
        (self.silence_hold_ms as f64 / self.scan_block_ms as f64).ceil() as u32
    }

    /// Apply profile bundle fields unless masked as explicitly set (TOML keys or CLI overrides).
    pub fn apply_profile_bundle(&mut self, mask: RepairProfileFieldMask) {
        let bundle = self.profile.bundle();
        apply_profile_bundle_fields(self, bundle, mask);
    }

    pub fn patch_settings(&self) -> PatchRequestSettings {
        PatchRequestSettings {
            skip_equivalent_gaps: self.skip_equivalent_gaps,
            normalize_fill: self.normalize_fill,
            dual_fit: self.dual_fit,
            normalize_window_secs: self.normalize_window_secs,
            max_fill_gain_db: self.max_fill_gain_db,
            min_fill_correlation: self.min_fill_correlation,
            fill_align_margin_secs: self.fill_align_margin_secs,
            max_fill_align_adjustment_secs: self.max_fill_align_adjustment_secs,
            fill_border_search_secs: self.fill_border_search_secs,
            min_border_discovery_secs: self.min_border_discovery_secs,
            border_standoff_secs: self.border_standoff_secs,
            short_gap_mean_correlation_secs: self.short_gap_mean_correlation_secs,
            fill_length_slack_secs: self.fill_length_slack_secs,
            fill_extract_tail_slack_secs: self.fill_extract_tail_slack_secs,
            fill_seam_search_secs: self.fill_seam_search_secs,
            gap_signature_context_secs: self.gap_signature_context_secs,
            gap_signature_bin_ms: self.gap_signature_bin_ms,
            min_structure_match_score: self.min_structure_match_score,
            strong_structure_trust: self.strong_structure_trust,
            disable_structure_trust: self.disable_structure_trust,
            partial_structure_waveform_soften: self.partial_structure_waveform_soften,
            absolute_silence_rms: self.absolute_silence_rms,
            fill_offset_mode: self.fill_offset_mode,
            gap_end_extend_on_post_seam_fail: self.gap_end_extend_on_post_seam_fail,
            gap_start_extend_on_pre_seam_fail: self.gap_start_extend_on_pre_seam_fail,
            gap_end_extend_max_ms: self.gap_end_extend_max_ms,
            gap_end_extend_step_ms: self.gap_end_extend_step_ms,
            short_gap_one_strong_seam_fallback: self.short_gap_one_strong_seam_fallback,
            fill_mode: self.fill_mode,
            fill_fit_structure_weight: self.fill_fit_structure_weight,
            fill_fit_waveform_weight: self.fill_fit_waveform_weight,
            fill_fit_nominal_bias_scale: self.fill_fit_nominal_bias_scale,
            fill_fit_energy_nominal_bias_scale: self.fill_fit_energy_nominal_bias_scale,
            fill_fit_late_start_penalty_scale: self.fill_fit_late_start_penalty_scale,
            fill_marginal_margin: self.fill_marginal_margin,
            fill_absolute_floor: self.fill_absolute_floor,
            fill_repeat_penalty_weight: self.fill_repeat_penalty_weight,
            fft_seam_search: self.fft_seam_search,
            fill_anchor_min_correlation: self.fill_anchor_min_correlation,
            fill_anchor_exclude_structure_trusted: self.fill_anchor_exclude_structure_trusted,
            fill_anchor_max_adjustment_frac: self.fill_anchor_max_adjustment_frac,
            fill_anchor_search_prior_weight: self.fill_anchor_search_prior_weight,
            fill_anchor_retry_marginal: self.fill_anchor_retry_marginal,
            gap_signature_mode: self.gap_signature_mode,
            profile: self.profile,
            fit_boundary_search: self.fit_boundary_search,
            anchor_seam_mode: self.anchor_seam_mode,
            max_anchor_bracket_secs: self.max_anchor_bracket_secs,
            max_anchors_per_side: self.max_anchors_per_side,
            anchor_seam_min_prominence: self.anchor_seam_min_prominence,
            anchor_seam_min_match_pearson: self.anchor_seam_min_match_pearson,
            anchor_seam_min_xcorr_peak: self.anchor_seam_min_xcorr_peak,
            anchor_seam_xcorr_ambiguous_band: self.anchor_seam_xcorr_ambiguous_band,
            residual_gate: self.residual_gate,
            residual_floor_ok_db: self.residual_floor_ok_db,
            residual_headroom_margin_db: self.residual_headroom_margin_db,
            residual_lag_secs: self.residual_lag_secs,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        // NaN/Inf pass ordinary range comparisons (`NaN < 0.0` is false), so reject
        // non-finite floats before any threshold check.
        for (field, value) in [
            ("silence_peak_fraction", f64::from(self.silence_peak_fraction)),
            ("absolute_silence_rms", f64::from(self.absolute_silence_rms)),
            ("gap_offset_tolerance_secs", self.gap_offset_tolerance_secs),
            ("min_fill_correlation", f64::from(self.min_fill_correlation)),
            ("fill_align_margin_secs", self.fill_align_margin_secs),
            (
                "max_fill_align_adjustment_secs",
                self.max_fill_align_adjustment_secs,
            ),
            ("fill_border_search_secs", self.fill_border_search_secs),
            ("min_border_discovery_secs", self.min_border_discovery_secs),
            ("border_standoff_secs", self.border_standoff_secs),
            (
                "short_gap_mean_correlation_secs",
                self.short_gap_mean_correlation_secs,
            ),
            ("fill_length_slack_secs", self.fill_length_slack_secs),
            (
                "fill_extract_tail_slack_secs",
                self.fill_extract_tail_slack_secs,
            ),
            ("fill_seam_search_secs", self.fill_seam_search_secs),
            ("gap_signature_context_secs", self.gap_signature_context_secs),
            (
                "min_structure_match_score",
                f64::from(self.min_structure_match_score),
            ),
            ("strong_structure_trust", self.strong_structure_trust),
            (
                "partial_structure_waveform_soften",
                self.partial_structure_waveform_soften,
            ),
            ("normalize_window_secs", self.normalize_window_secs),
            ("max_fill_gain_db", self.max_fill_gain_db),
            ("fill_fit_structure_weight", self.fill_fit_structure_weight),
            ("fill_fit_waveform_weight", self.fill_fit_waveform_weight),
            (
                "fill_fit_nominal_bias_scale",
                self.fill_fit_nominal_bias_scale,
            ),
            (
                "fill_fit_energy_nominal_bias_scale",
                self.fill_fit_energy_nominal_bias_scale,
            ),
            (
                "fill_fit_late_start_penalty_scale",
                self.fill_fit_late_start_penalty_scale,
            ),
            ("fill_marginal_margin", f64::from(self.fill_marginal_margin)),
            ("fill_absolute_floor", f64::from(self.fill_absolute_floor)),
            ("fill_repeat_penalty_weight", self.fill_repeat_penalty_weight),
            (
                "fill_anchor_min_correlation",
                f64::from(self.fill_anchor_min_correlation),
            ),
            (
                "fill_anchor_max_adjustment_frac",
                self.fill_anchor_max_adjustment_frac,
            ),
            (
                "fill_anchor_search_prior_weight",
                self.fill_anchor_search_prior_weight,
            ),
            ("max_anchor_bracket_secs", self.max_anchor_bracket_secs),
            (
                "anchor_seam_min_prominence",
                f64::from(self.anchor_seam_min_prominence),
            ),
            (
                "anchor_seam_min_match_pearson",
                f64::from(self.anchor_seam_min_match_pearson),
            ),
            (
                "anchor_seam_min_xcorr_peak",
                f64::from(self.anchor_seam_min_xcorr_peak),
            ),
            (
                "anchor_seam_xcorr_ambiguous_band",
                f64::from(self.anchor_seam_xcorr_ambiguous_band),
            ),
            ("residual_floor_ok_db", self.residual_floor_ok_db),
            (
                "residual_headroom_margin_db",
                self.residual_headroom_margin_db,
            ),
            ("residual_lag_secs", self.residual_lag_secs),
        ] {
            if !value.is_finite() {
                return Err(ConfigError::InvalidValue {
                    field: field.into(),
                    reason: "must be a finite number".into(),
                });
            }
        }

        if self.min_gap_ms == 0 {
            return Err(ConfigError::InvalidValue {
                field: "min_gap_ms".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.silence_peak_fraction <= 0.0 || self.silence_peak_fraction >= 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "silence_peak_fraction".into(),
                reason: "must be between 0 and 1 exclusive".into(),
            });
        }
        if self.decode_chunk_secs == 0 {
            return Err(ConfigError::InvalidValue {
                field: "decode_chunk_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.scan_block_ms == 0 {
            return Err(ConfigError::InvalidValue {
                field: "scan_block_ms".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.min_fill_correlation < -1.0 || self.min_fill_correlation > 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "min_fill_correlation".into(),
                reason: "must be between -1.0 and 1.0 inclusive".into(),
            });
        }
        if self.max_fill_gain_db <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "max_fill_gain_db".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.normalize_window_secs <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "normalize_window_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.gap_offset_tolerance_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "gap_offset_tolerance_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_align_margin_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_align_margin_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.max_fill_align_adjustment_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "max_fill_align_adjustment_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_border_search_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_border_search_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.min_border_discovery_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "min_border_discovery_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.border_standoff_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "border_standoff_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.short_gap_mean_correlation_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "short_gap_mean_correlation_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_length_slack_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_length_slack_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_extract_tail_slack_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_extract_tail_slack_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_seam_search_secs <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_seam_search_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.gap_signature_context_secs <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "gap_signature_context_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.gap_signature_bin_ms == 0 {
            return Err(ConfigError::InvalidValue {
                field: "gap_signature_bin_ms".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.min_structure_match_score < 0.0 || self.min_structure_match_score > 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "min_structure_match_score".into(),
                reason: "must be between 0.0 and 1.0 inclusive".into(),
            });
        }
        if self.strong_structure_trust < 0.0 || self.strong_structure_trust > 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "strong_structure_trust".into(),
                reason: "must be between 0.0 and 1.0 inclusive".into(),
            });
        }
        if self.partial_structure_waveform_soften < 0.0
            || self.partial_structure_waveform_soften > 1.0
        {
            return Err(ConfigError::InvalidValue {
                field: "partial_structure_waveform_soften".into(),
                reason: "must be between 0.0 and 1.0 inclusive".into(),
            });
        }
        if self.partial_structure_waveform_soften > self.strong_structure_trust {
            return Err(ConfigError::InvalidValue {
                field: "partial_structure_waveform_soften".into(),
                reason: "must be less than or equal to strong_structure_trust".into(),
            });
        }
        if self.absolute_silence_rms < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "absolute_silence_rms".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.gap_end_extend_max_ms == 0
            && (self.gap_end_extend_on_post_seam_fail || self.gap_start_extend_on_pre_seam_fail)
        {
            return Err(ConfigError::InvalidValue {
                field: "gap_end_extend_max_ms".into(),
                reason: "must be greater than zero when gap seam extension is enabled".into(),
            });
        }
        if self.gap_end_extend_step_ms == 0
            && (self.gap_end_extend_on_post_seam_fail || self.gap_start_extend_on_pre_seam_fail)
        {
            return Err(ConfigError::InvalidValue {
                field: "gap_end_extend_step_ms".into(),
                reason: "must be greater than zero when gap seam extension is enabled".into(),
            });
        }
        #[cfg(not(feature = "ffmpeg-mux"))]
        if self.output.video_path.is_some() {
            return Err(ConfigError::InvalidValue {
                field: "repair.output.video_path".into(),
                reason: "requires building clip-sync-repair with --features ffmpeg-mux".into(),
            });
        }
        if parse_mux_audio_bitrate_policy(&self.output.mux_audio_bitrate).is_err() {
            return Err(ConfigError::InvalidValue {
                field: "repair.output.mux_audio_bitrate".into(),
                reason: "must be match_min, match_a, default, or a rate like 256k".into(),
            });
        }
        if self.fill_anchor_search_prior_weight < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_anchor_search_prior_weight".into(),
                reason: "must be >= 0".into(),
            });
        }
        if self.fill_anchor_max_adjustment_frac <= 0.0
            || self.fill_anchor_max_adjustment_frac > 1.0
        {
            return Err(ConfigError::InvalidValue {
                field: "fill_anchor_max_adjustment_frac".into(),
                reason: "must be in (0, 1]".into(),
            });
        }
        if self.fill_repeat_penalty_weight < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_repeat_penalty_weight".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.repair_preview
            && (self.output.wav_path.is_some() || self.output.video_path.is_some())
        {
            return Err(ConfigError::InvalidValue {
                field: "repair_preview".into(),
                reason: "cannot combine with output.wav_path / output.video_path (--wav / --mux)"
                    .into(),
            });
        }
        if self.max_anchor_bracket_secs <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "max_anchor_bracket_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.max_anchors_per_side == 0 {
            return Err(ConfigError::InvalidValue {
                field: "max_anchors_per_side".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if !self.anchor_seam_min_match_pearson.is_finite()
            || self.anchor_seam_min_match_pearson < -1.0
            || self.anchor_seam_min_match_pearson > 1.0
        {
            return Err(ConfigError::InvalidValue {
                field: "anchor_seam_min_match_pearson".into(),
                reason: "must be finite and in [-1, 1]".into(),
            });
        }
        if !self.anchor_seam_min_xcorr_peak.is_finite()
            || self.anchor_seam_min_xcorr_peak < 0.0
            || self.anchor_seam_min_xcorr_peak > 1.0
        {
            return Err(ConfigError::InvalidValue {
                field: "anchor_seam_min_xcorr_peak".into(),
                reason: "must be finite and in [0, 1]".into(),
            });
        }
        if !self.anchor_seam_xcorr_ambiguous_band.is_finite()
            || self.anchor_seam_xcorr_ambiguous_band < 0.0
            || self.anchor_seam_xcorr_ambiguous_band > 1.0
        {
            return Err(ConfigError::InvalidValue {
                field: "anchor_seam_xcorr_ambiguous_band".into(),
                reason: "must be finite and in [0, 1]".into(),
            });
        }
        if self.residual_lag_secs <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "residual_lag_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.residual_headroom_margin_db < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "residual_headroom_margin_db".into(),
                reason: "must be non-negative".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepairAppConfig {
    #[serde(flatten)]
    pub align: AlignConfig,
    #[serde(default)]
    pub repair: RepairConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl Default for RepairAppConfig {
    fn default() -> Self {
        Self {
            align: default_repair_align_config(),
            repair: RepairConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

pub fn load_repair_app_config(path: Option<&Path>) -> Result<RepairAppConfig, AppError> {
    let Some(path) = path else {
        return Ok(RepairAppConfig::default());
    };

    let text = std::fs::read_to_string(path).map_err(|error| {
        AppError::Config(ConfigError::FileRead {
            path: path.to_path_buf(),
            source: Some(std::sync::Arc::new(error)),
        })
    })?;

    let num_clips_explicit = match toml::from_str::<toml::Table>(&text) {
        Ok(table) => table
            .get("clip")
            .and_then(toml::Value::as_table)
            .is_some_and(|clip| clip.contains_key("num_clips")),
        Err(_) => false,
    };

    let alignment_table = toml::from_str::<toml::Table>(&text)
        .ok()
        .and_then(|t| t.get("alignment").and_then(toml::Value::as_table).cloned())
        .unwrap_or_default();

    let require_consistent_explicit = alignment_table.contains_key("require_consistent_offsets");
    let high_rate_explicit = alignment_table.contains_key("refine_offset_high_rate");

    let mut config: RepairAppConfig = toml::from_str(&text).map_err(|error| {
        AppError::Config(ConfigError::Parse {
            detail: error.to_string(),
            source: Some(std::sync::Arc::new(error)),
        })
    })?;

    if !num_clips_explicit {
        config.align.clip.num_clips = REPAIR_DEFAULT_NUM_CLIPS;
    }
    if !require_consistent_explicit {
        config.align.alignment.require_consistent_offsets = false;
    }
    if !high_rate_explicit {
        config.align.alignment.refine_offset_high_rate = true;
    }

    if let Some(repair_table) = toml::from_str::<toml::Table>(&text)
        .ok()
        .and_then(|t| t.get("repair").and_then(toml::Value::as_table).cloned())
    {
        let profile_explicit = repair_table.contains_key("profile");
        let mask = repair_profile_field_mask_from_toml(&repair_table);
        config.repair.profile_field_mask = mask;
        if profile_explicit || config.repair.profile != RepairProfile::Default {
            config.repair.apply_profile_bundle(mask);
        }
    }

    // Surface keys serde silently ignored (flatten rules out deny_unknown_fields),
    // so a typo does not read as "setting had no effect". `eprintln!` because
    // tracing is not yet initialized at config-load time. `profile_field_mask` is
    // `#[serde(skip)]`, so the diff is unaffected by the mutations above.
    for key in unknown_toml_keys(&text, &config) {
        eprintln!("warning: unknown config key `{key}` was ignored");
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_app_config_defaults_to_two_clips() {
        let config = RepairAppConfig::default();
        assert_eq!(config.align.clip.num_clips, REPAIR_DEFAULT_NUM_CLIPS);
    }

    #[test]
    fn repair_app_config_allows_inconsistent_clip_offsets() {
        let config = RepairAppConfig::default();
        assert!(!config.align.alignment.require_consistent_offsets);
        assert!(config.align.alignment.prefer_start_clip);
    }

    #[test]
    fn load_config_applies_repair_num_clips_when_omitted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[repair]
dry_run = true
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert_eq!(config.align.clip.num_clips, REPAIR_DEFAULT_NUM_CLIPS);
    }

    #[test]
    fn load_config_respects_explicit_num_clips_one() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[clip]
num_clips = 1
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert_eq!(config.align.clip.num_clips, 1);
    }

    #[test]
    fn load_config_applies_repair_require_consistent_when_omitted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[repair]
dry_run = true
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!(!config.align.alignment.require_consistent_offsets);
    }

    #[test]
    fn repair_app_config_defaults_anchor_seam_mode_to_auto() {
        let config = RepairAppConfig::default();
        assert_eq!(
            config.repair.anchor_seam_mode,
            crate::domain::AnchorSeamMode::Auto,
        );
    }

    #[test]
    fn repair_app_config_defaults_fill_mode_to_fit() {
        let config = RepairAppConfig::default();
        assert_eq!(config.repair.fill_mode, crate::domain::FillMode::Fit);
    }

    #[test]
    fn repair_app_config_defaults_gap_signature_mode_to_auto() {
        let config = RepairAppConfig::default();
        assert_eq!(
            config.repair.gap_signature_mode,
            crate::domain::GapSignatureMode::Auto
        );
    }

    #[test]
    fn repair_app_config_defaults_repeat_penalty_weight() {
        let config = RepairAppConfig::default();
        assert!((config.repair.fill_repeat_penalty_weight - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn repair_app_config_enables_high_rate_refinement_by_default() {
        let config = RepairAppConfig::default();
        assert!(config.align.alignment.refine_offset_high_rate);
    }

    #[test]
    fn load_config_applies_high_rate_refinement_when_omitted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(&path, "[repair]\ndry_run = true\n").expect("write config");
        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!(config.align.alignment.refine_offset_high_rate);
    }

    #[test]
    fn load_config_respects_explicit_high_rate_refinement_false() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(&path, "[alignment]\nrefine_offset_high_rate = false\n")
            .expect("write config");
        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!(!config.align.alignment.refine_offset_high_rate);
    }

    #[test]
    fn rejects_repair_preview_with_output_paths() {
        let config = RepairConfig {
            repair_preview: true,
            output: RepairOutputConfig {
                wav_path: Some(PathBuf::from("out.wav")),
                ..RepairOutputConfig::default()
            },
            ..RepairConfig::default()
        };
        let err = config.validate().expect_err("preview + wav");
        assert!(
            format!("{err:?}").contains("repair_preview"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_min_gap_ms_zero() {
        let config = RepairConfig {
            min_gap_ms: 0,
            ..RepairConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_min_fill_correlation_out_of_range() {
        let config = RepairConfig {
            min_fill_correlation: 1.5,
            ..RepairConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn allows_min_fill_correlation_disable_gate() {
        let config = RepairConfig {
            min_fill_correlation: -1.0,
            ..RepairConfig::default()
        };
        config.validate().expect("disable gate should be valid");
    }

    #[test]
    fn rejects_negative_fill_repeat_penalty_weight() {
        let config = RepairConfig {
            fill_repeat_penalty_weight: -0.01,
            ..RepairConfig::default()
        };
        let err = config.validate().expect_err("negative weight");
        assert!(
            format!("{err:?}").contains("fill_repeat_penalty_weight"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn rejects_nan_float_thresholds() {
        let config = RepairConfig {
            min_fill_correlation: f32::NAN,
            ..RepairConfig::default()
        };
        let err = config.validate().expect_err("NaN");
        assert!(
            format!("{err:?}").contains("min_fill_correlation"),
            "{err:?}"
        );
        assert!(format!("{err:?}").contains("finite"), "{err:?}");
    }

    #[test]
    fn rejects_infinite_residual_lag() {
        let config = RepairConfig {
            residual_lag_secs: f64::INFINITY,
            ..RepairConfig::default()
        };
        let err = config.validate().expect_err("Inf");
        assert!(format!("{err:?}").contains("residual_lag_secs"), "{err:?}");
    }

    #[test]
    fn load_config_applies_quick_profile_bundle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[repair]
dry_run = true
profile = "quick"
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert_eq!(config.repair.profile, RepairProfile::Quick);
        assert!((config.repair.fill_border_search_secs - 5.0).abs() < f64::EPSILON);
        assert!(!config.repair.gap_end_extend_on_post_seam_fail);
    }

    #[test]
    fn load_config_profile_respects_explicit_border_override() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[repair]
dry_run = true
profile = "quick"
fill_border_search_secs = 8.0
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!((config.repair.fill_border_search_secs - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn load_config_respects_explicit_require_consistent_true() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[alignment]
require_consistent_offsets = true
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!(config.align.alignment.require_consistent_offsets);
    }
}
