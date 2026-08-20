//! `config` slice of the gap_fingerprint measure folder split.






/// Default coarse envelope half-width (ms) that recenters the ±`lag_max_lag_ms` fine sweep.
/// `0` disables recenter. Probe-only — not Apply's `max_lag_blocks`.
pub const DEFAULT_LAG_ENVELOPE_MAX_LAG_MS: u32 = 2000;
/// Thresholds + window sizes the builder needs (a media-free subset of `RepairConfig`).
#[derive(Debug, Clone, Copy)]
pub struct FingerprintConfig {
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
    pub gap_signature_context_secs: f64,
    pub gap_signature_bin_ms: u64,
    pub fill_seam_search_secs: f64,
    pub fill_border_search_secs: f64,
    pub fill_align_margin_secs: f64,
    /// B haystack tail slack (extract / `pad_tail`); see `RepairConfig.fill_extract_tail_slack_secs`.
    pub fill_extract_tail_slack_secs: f64,
    /// Pre-gap **noise-collar** width for the `levels` RMS/peak context — *not* a border-template
    /// size. No border template on this path is configured: each measurement sizes its own from what
    /// it is measuring (`bin_frames * 3` for structure, `window + max_lag` for
    /// [`lag_at_placement`]). Fed from `RepairConfig.normalize_window_secs`, which is why it does not
    /// share production's `border_secs` name — it never held that value.
    pub noise_collar_secs: f64,
    pub max_anchor_bracket_secs: f64,
    pub max_anchors_per_side: usize,
    pub anchor_seam_min_prominence: f32,
    pub min_structure_match_score: f64,
    pub min_fill_correlation: f32,
    pub fill_absolute_floor: f32,
    pub max_refine_secs: f64,
    pub lag_max_lag_ms: u32,
    /// Coarse per-shoulder envelope half-width (ms) that recenters the ±`lag_max_lag_ms` fine
    /// sweep. Default [`DEFAULT_LAG_ENVELOPE_MAX_LAG_MS`]. `0` disables recenter (nominal
    /// `b_mapped`, no `envelope_*` on the wire). Probe-only — not Apply's `max_lag_blocks`.
    pub lag_envelope_max_lag_ms: u32,
    /// Window (seconds) for the lag-uniqueness measurement. §3.6a froze this at **1 s** — it separates
    /// real registration from periodic ambiguity (250 ms is ambiguous on quiet produced audio, where a
    /// rival local max is nearly as tall; 1 s breaks the tie). Drives `peak_z` / `prominence`.
    pub lag_window_secs: f64,
}
impl Default for FingerprintConfig {
    fn default() -> Self {
        Self {
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            gap_signature_context_secs: 3.0,
            gap_signature_bin_ms: 50,
            fill_seam_search_secs: 0.25,
            fill_border_search_secs: 10.0,
            fill_align_margin_secs: 1.0,
            fill_extract_tail_slack_secs: 0.05,
            // 1.0 is a TEST default, not the probe's operating value: every real run goes through
            // `from_request`, which feeds `normalize_window_secs` (production default 5.0). Do not read
            // this as "the collar the corpus used" — that is `probe_recipe.noise_collar_secs`.
            noise_collar_secs: 1.0,
            max_anchor_bracket_secs: 5.0,
            max_anchors_per_side: 5,
            anchor_seam_min_prominence: 0.0,
            min_structure_match_score: 0.55,
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
            max_refine_secs: 0.75,
            lag_max_lag_ms: 600,
            lag_envelope_max_lag_ms: DEFAULT_LAG_ENVELOPE_MAX_LAG_MS,
            lag_window_secs: 1.0,
        }
    }
}
/// Decoded windows + geometry for one gap. `b_haystack` absent ⇒ A-only (no pairwise fields).
pub struct GapInputs<'a> {
    pub a_samples: &'a [f32],
    pub b_haystack: Option<&'a [f32]>,
    pub channels: usize,
    pub sample_rate: u32,
    pub reported_start_frame: usize,
    pub reported_end_frame: usize,
    /// Start of the decoded B haystack (seconds); `0.0` if the full B track is passed as the
    /// haystack. Used to map A-relative B times into haystack frame coordinates.
    pub b_extract_start_secs: f64,
    /// B-minus-A offset (seconds) for this gap, from the report mapping / resolved alignment.
    /// The builder refines A internally and maps each A boundary to B via this offset.
    pub gap_offset_secs: f64,
    pub config: FingerprintConfig,
}
impl FingerprintConfig {
    /// Mirror the run's patch request so the builder enumerates the **same** brackets the gate does
    /// (matching `bin_frames` / `context` / `max_bracket` / `min_prominence` is what lets the oracle
    /// pass align per-bracket by frame).
    pub(crate) fn from_request(
        request: &crate::application::PatchAudioRequest,
        silence_peak_fraction: f32,
        lag_envelope_max_lag_ms: u32,
    ) -> Self {
        const GAP_EDGE_REFINE_SECS: f64 = 0.75;
        Self {
            silence_peak_fraction,
            absolute_silence_rms: request.absolute_silence_rms,
            gap_signature_context_secs: request.gap_signature_context_secs,
            gap_signature_bin_ms: request.gap_signature_bin_ms,
            fill_seam_search_secs: request.fill_seam_search_secs,
            fill_border_search_secs: request.fill_border_search_secs,
            fill_align_margin_secs: request.fill_align_margin_secs,
            fill_extract_tail_slack_secs: request.fill_extract_tail_slack_secs,
            noise_collar_secs: request.normalize_window_secs,
            max_anchor_bracket_secs: request.max_anchor_bracket_secs,
            max_anchors_per_side: request.max_anchors_per_side,
            anchor_seam_min_prominence: request.anchor_seam_min_prominence,
            min_structure_match_score: f64::from(request.min_structure_match_score),
            min_fill_correlation: request.min_fill_correlation,
            fill_absolute_floor: request.fill_absolute_floor,
            max_refine_secs: GAP_EDGE_REFINE_SECS,
            // `b_mapped_start`/`b_mapped_end` share a single rigid per-gap offset (gap_offset_secs) —
            // pre/post are NOT independently registered. The full-corpus rescan showed real steps up
            // to 322 ms from that nominal, so ±200 ms clips genuine peaks off the search edge and
            // manufactures false one-sided-dead/decorrelated verdicts. 600 ms matches the width already
            // validated offline (SPLICE_EXP_FINE_LAG_MS=600, §1b-i/§3.6).
            lag_max_lag_ms: 600,
            lag_envelope_max_lag_ms,
            lag_window_secs: 1.0,
        }
    }
}
