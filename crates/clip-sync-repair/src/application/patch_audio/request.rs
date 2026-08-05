use clip_sync::MultiChannelPcm;

use crate::domain::gap_fill::{GapSelection, GapSelectionMode};
use crate::domain::patch_result::PatchSummary;
use crate::domain::GapReport;

pub struct PatchAudioResult {
    /// Present when A was decoded for patching; `None` when the fill plan was empty or this was a
    /// [`preview`](crate::application::PatchAudio::preview) run (no splice).
    pub pcm: Option<MultiChannelPcm>,
    pub summary: PatchSummary,
    /// True when this result came from [`PatchAudio::preview`](crate::application::PatchAudio::preview)
    /// (characterize without execute/splice). Drives human "would repair" wording.
    pub preview: bool,
    /// Measured encoded bitrate of video A's selected audio track (bits/s).
    pub source_audio_bitrate_a_bps: Option<u32>,
    /// Measured encoded bitrate of video B's selected audio track (bits/s).
    pub source_audio_bitrate_b_bps: Option<u32>,
    /// Present when patched PCM length differs materially from the container duration.
    pub pcm_container_skew: Option<crate::domain::diagnostics::PcmContainerDurationSkew>,
}

/// A patch run's inputs: the scan [`GapReport`] plus the policy that governs the patch.
///
/// Policy lives in the embedded [`PatchRequestSettings`] and is read directly at use sites
/// (`request.fill_mode`) through a read-only [`Deref`](std::ops::Deref). There is deliberately
/// **no `DerefMut`**: policy is set once where the settings are built
/// ([`RepairConfig::patch_settings`](crate::infrastructure::config::RepairConfig::patch_settings)),
/// so a stray `request.fill_mode = …` is a compile error rather than a second source of truth.
/// Only per-run opt-ins that no config key feeds stay as mutable fields here.
pub struct PatchAudioRequest {
    pub report: GapReport,
    /// Patch policy. Read through `Deref` (`request.fill_mode`); assign only via `patch_settings`.
    pub settings: PatchRequestSettings,
    /// P1 report-only: compute the residual/floor verdict per gap and attach it to the outcome/JSON.
    /// Off by default (no cost, no field); enabled for calibration runs. Set directly on the request.
    pub measure_residual: bool,
    /// Resolved gap subset for this run (`--only-gaps` / `--skip-gaps`). Defaults to all gaps via
    /// [`into_request`](PatchRequestSettings::into_request); `run_repair` overwrites after resolve.
    pub gap_selection: GapSelection,
}

impl std::ops::Deref for PatchAudioRequest {
    type Target = PatchRequestSettings;

    fn deref(&self) -> &Self::Target {
        &self.settings
    }
}

/// Patch parameters without the scan report — filled in after gap scan.
#[derive(Clone)]
pub struct PatchRequestSettings {
    /// Drop already-equivalent gaps (mutual/ambient silence) from the fill plan before decode/patch
    /// (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate). Off by default.
    pub skip_equivalent_gaps: bool,
    /// Measure the written fill's loudest bin against its A shoulders and record it on the outcome
    /// (`crate::domain::fill_level`). Record-only; never changes a verdict.
    pub measure_fill_level: bool,
    /// Unresolved `--only-gaps` / `--skip-gaps` intent; resolved in `run_repair` once the report exists.
    pub gap_selection: GapSelectionMode,
    pub normalize_fill: bool,
    pub normalize_window_secs: f64,
    pub max_fill_gain_db: f64,
    /// Minimum normalized Pearson correlation at each gap seam (pre and post). Regions below
    /// this threshold on either seam are skipped.
    pub min_fill_correlation: f32,
    /// Extra B audio extracted on each side of the mapped gap window for boundary alignment.
    pub fill_align_margin_secs: f64,
    /// Maximum slide (seconds) applied when searching for the best B fill position.
    pub max_fill_align_adjustment_secs: f64,
    /// How far (seconds) to search in B for A's pre-gap border before local alignment.
    pub fill_border_search_secs: f64,
    /// Minimum border template length (seconds) for discovery/correlation on short gaps.
    pub min_border_discovery_secs: f64,
    /// A-side only: skip this much audio (seconds) immediately adjacent to the dropout when
    /// building border templates (avoids corrupted seam audio on A).
    pub border_standoff_secs: f64,
    /// Gaps at or below this length (seconds) pass when mean(pre, post) correlation meets the
    /// threshold instead of requiring both seams individually.
    pub short_gap_mean_correlation_secs: f64,
    /// How far B fill length may differ from A's scanned gap when locating the post-border
    /// (end-search / `max_fill` only).
    pub fill_length_slack_secs: f64,
    /// Extra B haystack tail beyond the refined mapped end (before `max` with
    /// `fill_align_margin_secs`). Extract / `pad_tail` only — not the end-search range.
    pub fill_extract_tail_slack_secs: f64,
    /// Seam correlation window (seconds) for fine align slide search and the fill gate.
    pub fill_seam_search_secs: f64,
    /// Seconds of A audio on each side of the gap used to build the structure signature.
    pub gap_signature_context_secs: f64,
    /// Bin width (milliseconds) for active/silent structure signatures.
    pub gap_signature_bin_ms: u64,
    /// Minimum active/silent pattern match score (0–1) at each seam before waveform gate.
    pub min_structure_match_score: f32,
    /// Both structure seam scores must meet this to skip the waveform Pearson gate.
    pub strong_structure_trust: f64,
    /// When true, always run the waveform Pearson seam gate.
    pub disable_structure_trust: bool,
    /// In the waveform gate path, soften Pearson threshold when structure scores meet this.
    pub partial_structure_waveform_soften: f64,
    /// Peak-amplitude floor for per-frame silence checks during gap refinement (matches scan).
    pub absolute_silence_rms: f32,
    /// How to map each gap on A to B (`recommended` vs drift-interpolated clip offsets).
    pub fill_offset_mode: crate::domain::FillOffsetMode,
    /// When waveform post-seam correlation fails, try extending the gap end on A in small steps.
    pub gap_end_extend_on_post_seam_fail: bool,
    /// When waveform pre-seam correlation fails, try extending the gap start on A in small steps.
    pub gap_start_extend_on_pre_seam_fail: bool,
    /// Maximum gap-end extension when retrying a failed post seam (milliseconds).
    pub gap_end_extend_max_ms: u64,
    /// Step size for gap-end extension retries (milliseconds).
    pub gap_end_extend_step_ms: u64,
    /// For short gaps, allow patch when mean(pre, post) fails but either seam meets the threshold.
    pub short_gap_one_strong_seam_fallback: bool,
    /// Gap-fill placement after structure match (`gate` legacy vs `fit` waveform search).
    pub fill_mode: crate::domain::FillMode,
    /// Unified fit structure weight (fit mode only).
    pub fill_fit_structure_weight: f64,
    /// Unified fit waveform weight (fit mode only).
    pub fill_fit_waveform_weight: f64,
    /// Scales distance-from-nominal penalty in unified fit structure scoring (1.0 = default).
    pub fill_fit_nominal_bias_scale: f64,
    /// Distance-from-nominal penalty scale applied when the resolved signature is energy
    /// (mode-coupled bias; defaults lower than the base scale).
    pub fill_fit_energy_nominal_bias_scale: f64,
    /// Scales late-start penalty when structure search starts after the nominal map (1.0 = default).
    pub fill_fit_late_start_penalty_scale: f64,
    /// Fit mode marginal patch band below `min_fill_correlation` (Phase C).
    pub fill_marginal_margin: f32,
    /// Fit mode hard waveform skip floor (Phase C).
    pub fill_absolute_floor: f32,
    /// Fit mode repeat-at-seam penalty weight (Phase D; 0 = off).
    pub fill_repeat_penalty_weight: f64,
    /// Lever 1 (§2.5): FFT seam band in the unified start-search refine (perf; on by default).
    pub fft_seam_search: bool,
    /// Lever 1b(b): FFT repeat-window band in the same refine (perf; on by default; `--no-fft-repeat-band` opts out).
    pub fft_repeat_band: bool,
    /// Minimum seam score for a pass-1 patch to become an offset anchor.
    pub fill_anchor_min_correlation: f32,
    /// Exclude structure-trusted gate patches from the anchor table.
    pub fill_anchor_exclude_structure_trusted: bool,
    /// Max `|align_adjustment|` as a fraction of `fill_border_search_secs` for anchors.
    pub fill_anchor_max_adjustment_frac: f64,
    /// Fit mode: soft penalty in unified search for B candidates far from anchor-predicted start (0 = off).
    pub fill_anchor_search_prior_weight: f64,
    /// `anchored_retry` pass 2: re-run fit-mode marginal pass-1 patches with anchored offset; keep pass 2 only when `High`.
    pub fill_anchor_retry_marginal: bool,
    /// Structure signature representation for gap fill search.
    pub gap_signature_mode: crate::domain::GapSignatureMode,
    /// Effective repair profile for verbose logging.
    pub profile: crate::domain::RepairProfile,
    /// Fit mode boundary search policy.
    pub fit_boundary_search: crate::domain::FitBoundarySearch,
    /// Editorial seam anchor search mode (fit mode only).
    pub anchor_seam_mode: crate::domain::AnchorSeamMode,
    pub max_anchor_bracket_secs: f64,
    pub max_anchors_per_side: usize,
    pub anchor_seam_min_prominence: f32,
    pub anchor_seam_min_match_pearson: f32,
    pub anchor_seam_min_xcorr_peak: f32,
    pub anchor_seam_xcorr_ambiguous_band: f32,
    /// **A3 dual-fit repair** (flag-gated). When on, a gap the seam gate *skips* (bracket-exhausted) gets a
    /// fallback attempt: independent per-shoulder fit + interior-trim fill, validated by the unchanged gate
    /// (§5.2). Off by default ⇒ existing bracket-search path byte-identical (D6).
    pub dual_fit: bool,
    pub residual_gate: crate::domain::ResidualGateMode,
    pub residual_floor_ok_db: f64,
    pub residual_headroom_margin_db: f64,
    pub residual_lag_secs: f64,
}

impl PatchRequestSettings {
    /// Attach the scan report, resolving [`Self::gap_selection`] against it.
    ///
    /// Policy moves in whole — there is no per-field copy list to keep in sync when a knob is
    /// added. Selection validation (bounds, duplicates, empty non-vacuous lists) happens here so
    /// callers cannot silently ignore `GapSelectionMode::Only` / `Skip`.
    pub fn into_request(self, report: GapReport) -> Result<PatchAudioRequest, String> {
        let selection =
            crate::domain::gap_fill::resolve_gap_selection(&self.gap_selection, &report)?;
        Ok(PatchAudioRequest {
            report,
            settings: self,
            // Report-only residual measurement is opt-in; callers set it on the request directly.
            measure_residual: false,
            gap_selection: selection,
        })
    }
}
