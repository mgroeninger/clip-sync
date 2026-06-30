//! Licensing-safe numeric characterization of a repair gap ("gap fingerprint").
//!
//! Every field here is a number or enum — no audio samples, no transcripts — so a fingerprint can be
//! committed as a regression/calibration corpus from real (licensed) media. See
//! `docs/archive/TEMP-gap-fingerprint-plan.md` and `docs/gap-fingerprint.md`.
//!
//! **P0 (this file):** the serde schema plus the lag-correlation probe
//! ([`lag_correlation_curve`] / [`summarize_lag_curve`]). The builder that fills these structs from
//! decoded PCM + the per-gap gate path lands in P1.

use serde::{Deserialize, Serialize};

use clip_sync::normalized_correlation;

use crate::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorSeamParams, AnchorSource,
};
use crate::domain::gap_energy::energy_bins;
use crate::domain::gap_fill_fit::{
    match_gap_fill_unified_in_b, UnifiedFillSearchInput, UnifiedFitWeights, WaveformSeamContext,
};
use crate::domain::gap_signature::{build_gap_signature, GapSignatureMode};
use crate::domain::gap_structure::StructureMatchParams;
use crate::domain::policies::{
    border_templates_for_gap, border_templates_per_channel_for_gap,
    interleaved_to_channels, interleaved_to_mono, refine_gap_frames, seam_channel_diagnostics,
    GapBorderSpec, RefinedGapFrames, SeamPlacement, SeamTemplates,
};

// ---------------------------------------------------------------------------------------------
// Corpus envelope
// ---------------------------------------------------------------------------------------------

/// One decoded source file's identity + non-identifying metadata. `id` is a stable strided digest of
/// the **decoded** audio: a remux / lossless re-container yields the same `id`, a different encoding
/// (codec / bitrate / partial clip) a different one. Contains no path or title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSource {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub codec: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: f64,
}

/// The scan recipe a corpus entry was produced under, so two entries are known-comparable. Fields the
/// `GapReport` doesn't carry stay `None` until the bin path fills them from config.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScanRecipe {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub min_gap_ms: Option<u64>,
    pub silence_peak_fraction: f32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub absolute_silence_rms: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scan_block_ms: Option<u64>,
}

/// Non-identifying provenance for a corpus: the A/B file identities (pair = entry identity), the scan
/// recipe, and the gap count. No titles, no paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceMeta {
    pub a_source: FileSource,
    pub b_source: FileSource,
    pub scan_recipe: ScanRecipe,
    pub gap_count: usize,
}

fn fnv_feed(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Stable 64-bit FNV-1a digest (hex) over `(sample_rate, channels, frame_count)` + a strided subset
/// of the decoded interleaved PCM. Deterministic across runs/versions; identical decodes → same id,
/// different encodings → different id. Not a cryptographic hash; capped sampling keeps it fast on long
/// tracks.
pub fn source_id(samples: &[f32], sample_rate: u32, channels: u16) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    fnv_feed(&mut h, &sample_rate.to_le_bytes());
    fnv_feed(&mut h, &channels.to_le_bytes());
    fnv_feed(&mut h, &(samples.len() as u64).to_le_bytes());
    let stride = (samples.len() / 200_000).max(1);
    let mut i = 0;
    while i < samples.len() {
        fnv_feed(&mut h, &samples[i].to_bits().to_le_bytes());
        i += stride;
    }
    format!("{h:016x}")
}

fn file_source(samples: &[f32], sample_rate: u32, channels: u16) -> FileSource {
    let ch = u32::from(channels.max(1));
    FileSource {
        id: source_id(samples, sample_rate, channels),
        container: None,
        codec: None,
        sample_rate,
        channels,
        duration_secs: samples.len() as f64 / f64::from(ch) / f64::from(sample_rate.max(1)),
    }
}

impl ScanRecipe {
    /// What the `GapReport` reliably carries; the bin path overwrites the rest from config.
    pub(crate) fn from_report(report: &crate::domain::GapReport) -> Self {
        Self {
            min_gap_ms: None,
            silence_peak_fraction: report.silence_peak_fraction,
            absolute_silence_rms: None,
            scan_block_ms: Some(report.scan_block_ms),
        }
    }
}

/// Top-level corpus: provenance + per-gap fingerprints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapCorpus {
    pub source: SourceMeta,
    pub gaps: Vec<GapFingerprint>,
}

/// How much detail a gap fingerprint carries (cost tier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailTier {
    /// Cheap: geometry + intrinsic A-side + baseline scores + outcome.
    Summary,
    /// Adds feasible brackets, per-bracket failure stages, and the lag fingerprint.
    Full,
}

// ---------------------------------------------------------------------------------------------
// Per-gap fingerprint
// ---------------------------------------------------------------------------------------------

/// One gap's numeric characterization. Pairwise fields (`structure`, `seams`, `lag`, `outcome`) are
/// `None` when characterizing A alone; `lag` and per-bracket detail require [`DetailTier::Full`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapFingerprint {
    pub index: usize,
    pub tier: DetailTier,
    pub sample_rate: u32,
    pub channels: u16,
    pub geometry: GapGeometry,
    pub levels: LevelProfile,
    pub silence: SilenceProfile,
    pub contour: ContourInfo,
    pub anchors: AnchorSet,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub brackets: Vec<BracketInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub structure: Option<StructureScores>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seams: Option<SeamScores>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lag: Option<LagFingerprint>,
    /// Lag at the **decision placement** — the structure-slid throat (zero-move) seam the gate scores
    /// for patch/skip — as opposed to `lag`, measured at the best-structure *editorial bracket* (a
    /// diagnostic placement that can sit far from the throat). This is the registration-relevant lag.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub baseline_lag: Option<LagFingerprint>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<GateOutcome>,
}

/// Gap edges on A (reported + refined) and the mapped B fill window (when B is present).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapGeometry {
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    pub a_refined_start_secs: f64,
    pub a_refined_end_secs: f64,
    pub duration_secs: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub b_mapped_start_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub b_mapped_end_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fill_offset_secs: Option<f64>,
}

/// RMS level envelope (dBFS) across the gap's pre/post context, plus salient summary levels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelProfile {
    pub bin_ms: u32,
    /// Per-bin RMS in dBFS across pre-context → post-context (silence floored, see `floor_db`).
    pub profile_db: Vec<f32>,
    /// dBFS value substituted for true-silent bins (so the vector has no `-inf`).
    pub floor_db: f32,
    pub speech_peak_db: f32,
    pub noise_floor_db: f32,
    pub gap_floor_db: f32,
}

/// The relative silence-test quantities that decide whether a noisy collar is walked off the seam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SilenceProfile {
    /// RMS/peak ratio of the collar immediately adjacent to the gap.
    pub collar_rms_peak_ratio: f32,
    /// True when the collar reads as active (ratio ≥ `silence_peak_fraction`) — i.e. NOT walked off.
    pub collar_above_relative_floor: bool,
    pub silence_peak_fraction: f32,
}

/// Energy-envelope contour flags that gate anchor-seam eligibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContourInfo {
    pub has_anchor_seam_contour: bool,
    pub pre_flatness: f32,
    pub post_flatness: f32,
}

/// How an anchor frame was chosen (mirrors `domain::gap_anchor_seam::AnchorSource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorSourceKind {
    ScanRefined,
    EnergyPeak,
    BoolTransition,
}

/// One editorial anchor candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchorPoint {
    pub time_secs: f64,
    pub source: AnchorSourceKind,
    pub prominence: f32,
    pub rms_db: f32,
}

/// Anchor candidates on each side of the gap.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnchorSet {
    pub pre: Vec<AnchorPoint>,
    pub post: Vec<AnchorPoint>,
}

/// One feasible editorial bracket and its scores (full tier).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BracketInfo {
    pub pre_time_secs: f64,
    pub post_time_secs: f64,
    pub span_secs: f64,
    pub move_frames: usize,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub structure_pre: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub structure_post: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seam_pre: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seam_post: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub failure_stage: Option<FailureStage>,
}

/// Which gate stage rejected a bracket (mirrors the W5 `failure_stage` taxonomy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    StructureAlign,
    StructureFloor,
    WaveformFloor,
    Residual,
}

/// Baseline structure-tier correlations at the throat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructureScores {
    pub baseline_pre: f64,
    pub baseline_post: f64,
}

/// Baseline waveform seam correlations, per-channel and selected channels (the gate's view).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeamScores {
    pub baseline_pre: f64,
    pub baseline_post: f64,
    pub selected_channels: Vec<usize>,
    pub per_channel: Vec<(f64, f64)>,
    pub mono_pre: f64,
    pub mono_post: f64,
}

/// Lag fingerprint at the pre/post speech anchors — the timing-offset vs decorrelation probe.
/// Each vector holds one [`LagSummary`] per measured signal (mono downmix + the gate-selected
/// channel, which is where a multichannel failure actually lives).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LagFingerprint {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub pre_anchor: Vec<LagSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub post_anchor: Vec<LagSummary>,
}

/// Which signal the lag curve was measured on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LagChannel {
    Mono,
    /// The gate-selected channel index.
    Selected(usize),
}

/// Verdict from a lag curve (see `docs/archive/TEMP-gap-fingerprint-plan.md` §4 thresholds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LagVerdict {
    /// A shift recovers strong correlation — recoverable timing offset (read `frac_lag_ms`).
    TimingOffset,
    /// No shift recovers correlation — sources genuinely differ.
    Decorrelated,
    /// Partial; weak shared content.
    Ambiguous,
}

/// Parabolic-interpolated peak of a lag-correlation sweep plus a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LagSummary {
    pub window_ms: u32,
    pub max_lag_ms: u32,
    pub channel: LagChannel,
    /// Correlation at lag 0 — what the seam currently sees.
    pub lag0_r: f64,
    /// Best correlation at an integer lag.
    pub peak_r: f64,
    pub peak_lag_samples: i64,
    /// Parabolic-interpolated (possibly fractional) lag of the peak.
    pub frac_lag_samples: f64,
    pub frac_lag_ms: f64,
    pub verdict: LagVerdict,
}

/// Final gate decision tags for the gap (mirrors the stdout gap tags).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateOutcome {
    pub plan_kind: String,
    pub tier: String,
    pub seam_shape: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fit_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub skip_reason: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Lag-correlation probe
// ---------------------------------------------------------------------------------------------

/// Normalized (mean-centered, variance-normalized) Pearson between `a` and a same-length slice of
/// `b_ctx`, for every integer `lag` in `[-max_lag, max_lag]`. Lag 0 aligns `a` with
/// `b_ctx[max_lag .. max_lag + a.len()]`; `b_ctx` must therefore span `a.len() + 2*max_lag` samples
/// (shorter contexts simply yield a truncated curve). Returns `(lag, r)` pairs.
///
/// This is exactly the seam's [`clip_sync::normalized_correlation`] (lag-0 Pearson) swept over
/// integer shifts: it answers "is there a lag at which A and B agree?", which a single lag-0 seam
/// score cannot.
pub fn lag_correlation_curve(a: &[f64], b_ctx: &[f64], max_lag: i64) -> Vec<(i64, f64)> {
    let n = a.len();
    if n == 0 || max_lag < 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity((2 * max_lag + 1) as usize);
    for lag in -max_lag..=max_lag {
        let base = max_lag + lag;
        if base < 0 {
            continue;
        }
        let base = base as usize;
        if base + n > b_ctx.len() {
            continue;
        }
        out.push((lag, normalized_correlation(a, &b_ctx[base..base + n])));
    }
    out
}

/// Summarize a lag curve: lag-0 value, integer peak, parabolic-interpolated (fractional) peak, and a
/// [`LagVerdict`]. `None` for an empty curve.
pub fn summarize_lag_curve(
    curve: &[(i64, f64)],
    sample_rate: u32,
    window_ms: u32,
    max_lag_ms: u32,
    channel: LagChannel,
) -> Option<LagSummary> {
    if curve.is_empty() {
        return None;
    }
    let lag0_r = curve
        .iter()
        .find(|(l, _)| *l == 0)
        .map(|(_, r)| *r)
        .unwrap_or(f64::NAN);
    let (pi, &(peak_lag, peak_r)) = curve
        .iter()
        .enumerate()
        .max_by(|(_, (_, x)), (_, (_, y))| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))?;

    // Parabolic vertex through the integer peak and its two neighbours (when interior).
    let (frac_lag, frac_r) = if pi > 0 && pi < curve.len() - 1 {
        let y0 = curve[pi - 1].1;
        let y1 = peak_r;
        let y2 = curve[pi + 1].1;
        let denom = y0 - 2.0 * y1 + y2;
        if denom.abs() > 1e-12 {
            let xstar = (y0 - y2) / (2.0 * denom);
            let peak = y1 - (y2 - y0) * (y2 - y0) / (8.0 * denom);
            (peak_lag as f64 + xstar, peak)
        } else {
            (peak_lag as f64, peak_r)
        }
    } else {
        (peak_lag as f64, peak_r)
    };

    let rate = f64::from(sample_rate).max(1.0);
    Some(LagSummary {
        window_ms,
        max_lag_ms,
        channel,
        lag0_r,
        peak_r,
        peak_lag_samples: peak_lag,
        frac_lag_samples: frac_lag,
        frac_lag_ms: frac_lag * 1000.0 / rate,
        verdict: classify_lag(lag0_r, frac_r, peak_lag),
    })
}

/// Verdict thresholds (see plan §4). `peak` is the parabolic-interpolated peak correlation.
fn classify_lag(lag0_r: f64, peak: f64, peak_lag: i64) -> LagVerdict {
    let recoverable = peak >= 0.5;
    let offset_away = peak_lag.abs() > 1 || (peak - lag0_r) > 0.2;
    if recoverable && offset_away {
        LagVerdict::TimingOffset
    } else if peak < 0.3 {
        LagVerdict::Decorrelated
    } else {
        LagVerdict::Ambiguous
    }
}

// ---------------------------------------------------------------------------------------------
// Builder — assembles a fingerprint from decoded windows using public domain functions only
// (no patch-path coupling). Mirrors `tests/diag_anchor_quiet_gap.rs`.
// ---------------------------------------------------------------------------------------------

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
    pub fill_length_slack_secs: f64,
    pub border_secs: f64,
    pub border_standoff_secs: f64,
    pub max_anchor_bracket_secs: f64,
    pub max_anchors_per_side: usize,
    pub anchor_seam_min_prominence: f32,
    pub min_structure_match_score: f64,
    pub min_fill_correlation: f32,
    pub fill_marginal_margin: f32,
    pub fill_absolute_floor: f32,
    pub max_refine_secs: f64,
    pub lag_max_lag_ms: u32,
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
            fill_length_slack_secs: 0.05,
            border_secs: 1.0,
            border_standoff_secs: 0.35,
            max_anchor_bracket_secs: 5.0,
            max_anchors_per_side: 5,
            anchor_seam_min_prominence: 0.0,
            min_structure_match_score: 0.55,
            min_fill_correlation: 0.35,
            fill_marginal_margin: 0.08,
            fill_absolute_floor: 0.12,
            max_refine_secs: 0.75,
            lag_max_lag_ms: 200,
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

const SILENCE_FLOOR_DB: f32 = -120.0;

fn to_db(rms: f32) -> f32 {
    if rms <= 1e-9 {
        SILENCE_FLOOR_DB
    } else {
        20.0 * rms.log10()
    }
}

/// Mono-downmix RMS over a frame range.
fn mono_rms(samples: &[f32], channels: usize, start: usize, end: usize) -> f32 {
    let ch = channels.max(1);
    let total = samples.len() / ch;
    let start = start.min(total);
    let end = end.min(total);
    if end <= start {
        return 0.0;
    }
    let mut sum_sq = 0f64;
    for f in start..end {
        let mut acc = 0f64;
        for c in 0..ch {
            acc += samples[f * ch + c] as f64;
        }
        let v = acc / ch as f64;
        sum_sq += v * v;
    }
    (sum_sq / (end - start) as f64).sqrt() as f32
}

fn mono_peak(samples: &[f32], channels: usize, start: usize, end: usize) -> f32 {
    let ch = channels.max(1);
    let total = samples.len() / ch;
    let (start, end) = (start.min(total), end.min(total));
    let mut peak = 0f32;
    for f in start..end {
        let mut acc = 0f64;
        for c in 0..ch {
            acc += samples[f * ch + c] as f64;
        }
        peak = peak.max((acc / ch as f64).abs() as f32);
    }
    peak
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return SILENCE_FLOOR_DB;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// `1.0 - peak-normalized range` of a gated-energy envelope: 1.0 = perfectly flat, →0 = strong contour.
fn flatness(bins: &[f32]) -> f32 {
    if bins.is_empty() {
        return 1.0;
    }
    let peak = bins.iter().copied().fold(0.0f32, f32::max);
    if peak <= f32::EPSILON {
        return 1.0;
    }
    let min = bins.iter().copied().fold(f32::INFINITY, f32::min);
    (1.0 - (peak - min) / peak).clamp(0.0, 1.0)
}

fn structure_params_for(cfg: &FingerprintConfig, gap_frames: usize, bin_frames: usize, search_radius_frames: usize, slack: usize) -> StructureMatchParams {
    StructureMatchParams {
        gap_frames,
        bin_frames: bin_frames.max(1),
        search_radius_frames,
        fill_length_slack_frames: slack,
        // Bounded sample polish — NOT `slack`/`bin_frames`: a large value makes the unified search's
        // fine-polish loop run multi-second exhaustive scans per candidate (see gap_structure docs).
        max_fine_adjustment_frames: crate::domain::gap_structure::structure_fine_polish_frames(bin_frames),
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_silence_rms: cfg.absolute_silence_rms,
    }
}

/// Owned result of a structure+waveform placement on B at one A bracket.
struct PlacementScores {
    start_frame: usize,
    structure_pre: f64,
    structure_post: f64,
    seam_pre: f64,
    seam_post: f64,
    per_channel: Vec<(f64, f64)>,
    selected_channels: Vec<usize>,
    mono_pre: f64,
    mono_post: f64,
}

/// Structure-best placement of the A bracket `refined` on the B haystack, plus the seam there.
/// Structure-dominant weights so the placement locks to the energy-best (mirrors the production
/// "structure aligns, seam read there" story). `None` if the match degenerates.
#[allow(clippy::too_many_arguments)]
fn place_on_b(
    a_samples: &[f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_haystack: &[f32],
    b_mono: &[f64],
    b_ch: &[Vec<f64>],
    nominal_fill_start: usize,
    context_frames: usize,
    bin_frames: usize,
    search_radius_frames: usize,
    cfg: &FingerprintConfig,
) -> Option<PlacementScores> {
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.checked_sub(refined.start_frame)?;
    if gap_frames == 0 {
        return None;
    }
    let border_frames = bin_frames * 3;
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, ch, &border_spec);
    let pre_window = a_pre.len().max(1);
    let post_window = a_post.len().max(1);
    let templates = SeamTemplates {
        a_pre: &a_pre,
        a_post: &a_post,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        b_mono,
        b_ch,
    };
    let waveform = WaveformSeamContext {
        templates: &templates,
        gap_frames,
        pre_window,
        post_window,
        b_total_frames: b_mono.len(),
        repeat_window_frames: bin_frames.max(1),
        repeat_penalty_weight: 0.0,
    };
    let structure_params = structure_params_for(cfg, gap_frames, bin_frames, search_radius_frames, bin_frames);
    let signature = build_gap_signature(
        a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        context_frames,
        &structure_params,
        GapSignatureMode::Energy,
    );
    let nominal_fill_end = nominal_fill_start + gap_frames;
    let weights = UnifiedFitWeights {
        structure_weight: 1.0,
        waveform_weight: 0.0,
        nominal_bias_scale: 0.0,
        late_start_penalty_scale: 0.0,
    };
    let matched = match_gap_fill_unified_in_b(
        &UnifiedFillSearchInput {
            signature: &signature,
            b_samples: b_haystack,
            channels,
            waveform: &waveform,
            nominal_fill_start,
            nominal_fill_end,
        },
        &structure_params,
        weights,
    )?;
    let start = matched.alignment.start_frame;
    let diag = seam_channel_diagnostics(
        &templates,
        SeamPlacement { start, gap_frames, pre_window, post_window },
    );
    Some(PlacementScores {
        start_frame: start,
        structure_pre: matched.structure_pre,
        structure_post: matched.structure_post,
        seam_pre: matched.alignment.pre_correlation,
        seam_post: matched.alignment.post_correlation,
        per_channel: diag.per_channel,
        selected_channels: diag.selected,
        mono_pre: diag.mono.0,
        mono_post: diag.mono.1,
    })
}

/// Pre/post lag summaries of one A border vs one B signal at a placement.
#[allow(clippy::too_many_arguments)]
fn lag_pair(
    a_pre: &[f64],
    a_post: &[f64],
    b_signal: &[f64],
    start_frame: usize,
    gap_frames: usize,
    window: usize,
    max_lag: i64,
    channel: LagChannel,
    sample_rate: u32,
) -> (Option<LagSummary>, Option<LagSummary>) {
    let ml = max_lag.max(0) as usize;
    let win_ms = ((window as f64) * 1000.0 / f64::from(sample_rate.max(1))) as u32;
    let max_lag_ms = ((ml as f64) * 1000.0 / f64::from(sample_rate.max(1))) as u32;
    let pre = (|| {
        let w = window.min(a_pre.len());
        if w < 8 || start_frame < w + ml {
            return None;
        }
        let hi = (start_frame + ml).min(b_signal.len());
        let lo = start_frame - w - ml;
        if hi <= lo {
            return None;
        }
        let curve = lag_correlation_curve(&a_pre[a_pre.len() - w..], &b_signal[lo..hi], max_lag);
        summarize_lag_curve(&curve, sample_rate, win_ms, max_lag_ms, channel)
    })();
    let post = (|| {
        let w = window.min(a_post.len());
        let post_base = start_frame + gap_frames;
        if w < 8 || post_base < ml {
            return None;
        }
        let lo = post_base - ml;
        let hi = (post_base + w + ml).min(b_signal.len());
        if hi <= lo {
            return None;
        }
        let curve = lag_correlation_curve(&a_post[..w], &b_signal[lo..hi], max_lag);
        summarize_lag_curve(&curve, sample_rate, win_ms, max_lag_ms, channel)
    })();
    (pre, post)
}

/// Lag fingerprint for a placement: A's kept border vs the B haystack swept over ±`max_lag`, on the
/// mono downmix **and** the gate-selected channel (where a multichannel failure lives).
#[allow(clippy::too_many_arguments)]
fn lag_at_placement(
    a_samples: &[f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &[f64],
    b_ch: &[Vec<f64>],
    selected: Option<usize>,
    start_frame: usize,
    bin_frames: usize,
    cfg: &FingerprintConfig,
    sample_rate: u32,
) -> LagFingerprint {
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: bin_frames * 3,
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, ch, &border_spec);
    let window = ((cfg.fill_seam_search_secs * f64::from(sample_rate)).round() as usize).max(8);
    let max_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * f64::from(sample_rate)).round() as i64;

    let mut out = LagFingerprint::default();
    let mut add = |pre: Option<LagSummary>, post: Option<LagSummary>| {
        if let Some(p) = pre {
            out.pre_anchor.push(p);
        }
        if let Some(p) = post {
            out.post_anchor.push(p);
        }
    };

    let (pre, post) = lag_pair(&a_pre, &a_post, b_mono, start_frame, gap_frames, window, max_lag, LagChannel::Mono, sample_rate);
    add(pre, post);

    if let Some(sel) = selected {
        if sel < b_ch.len() && sel < a_pre_ch.len() && sel < a_post_ch.len() && !a_pre_ch[sel].is_empty() {
            let (pre, post) = lag_pair(&a_pre_ch[sel], &a_post_ch[sel], &b_ch[sel], start_frame, gap_frames, window, max_lag, LagChannel::Selected(sel), sample_rate);
            add(pre, post);
        }
    }
    out
}

fn classify_bracket_stage(
    structure_pre: f64,
    structure_post: f64,
    seam_pre: f64,
    seam_post: f64,
    cfg: &FingerprintConfig,
) -> Option<FailureStage> {
    if structure_pre.min(structure_post) < cfg.min_structure_match_score {
        return Some(FailureStage::StructureFloor);
    }
    if seam_pre.min(seam_post) < f64::from(cfg.fill_absolute_floor) {
        return Some(FailureStage::WaveformFloor);
    }
    None
}

/// Build a fingerprint for one gap from decoded windows.
pub fn build_gap_fingerprint(index: usize, inputs: &GapInputs<'_>, tier: DetailTier) -> GapFingerprint {
    let cfg = &inputs.config;
    let ch = inputs.channels.max(1);
    let rate = f64::from(inputs.sample_rate).max(1.0);
    let total_a = inputs.a_samples.len() / ch;

    let bin_frames = ((cfg.gap_signature_bin_ms as f64 / 1000.0) * rate).round().max(1.0) as usize;
    let context_frames = (cfg.gap_signature_context_secs * rate).round() as usize;
    let border_frames = (cfg.border_secs * rate).round() as usize;
    let max_refine_frames = (cfg.max_refine_secs * rate).round() as usize;

    let refined = refine_gap_frames(
        inputs.a_samples,
        ch,
        inputs.reported_start_frame.min(total_a),
        inputs.reported_end_frame.min(total_a),
        cfg.silence_peak_fraction,
        cfg.absolute_silence_rms,
        max_refine_frames,
    );
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);

    let geometry = GapGeometry {
        a_start_secs: inputs.reported_start_frame as f64 / rate,
        a_end_secs: inputs.reported_end_frame as f64 / rate,
        a_refined_start_secs: refined.start_frame as f64 / rate,
        a_refined_end_secs: refined.end_frame as f64 / rate,
        duration_secs: gap_frames as f64 / rate,
        b_mapped_start_secs: inputs
            .b_haystack
            .map(|_| refined.start_frame as f64 / rate + inputs.gap_offset_secs),
        b_mapped_end_secs: inputs
            .b_haystack
            .map(|_| refined.end_frame as f64 / rate + inputs.gap_offset_secs),
        fill_offset_secs: inputs.b_haystack.map(|_| inputs.gap_offset_secs),
    };

    // --- intrinsic (A-side) ---
    let pre_start = refined.start_frame.saturating_sub(context_frames);
    let post_end = (refined.end_frame + context_frames).min(total_a);
    let mut profile_db = Vec::new();
    let mut context_bins_db = Vec::new();
    let mut f = pre_start;
    while f < post_end {
        let end = (f + bin_frames).min(post_end);
        let db = to_db(mono_rms(inputs.a_samples, ch, f, end));
        profile_db.push(db);
        if f < refined.start_frame || f >= refined.end_frame {
            context_bins_db.push(db);
        }
        f = end;
    }
    let gap_floor_db = {
        let mut mx = SILENCE_FLOOR_DB;
        let mut g = refined.start_frame;
        while g < refined.end_frame {
            let end = (g + bin_frames).min(refined.end_frame);
            mx = mx.max(to_db(mono_rms(inputs.a_samples, ch, g, end)));
            g = end;
        }
        mx
    };
    let levels = LevelProfile {
        bin_ms: cfg.gap_signature_bin_ms as u32,
        speech_peak_db: profile_db.iter().copied().fold(SILENCE_FLOOR_DB, f32::max),
        noise_floor_db: median(context_bins_db),
        gap_floor_db,
        floor_db: SILENCE_FLOOR_DB,
        profile_db,
    };

    let collar_start = refined.start_frame.saturating_sub(border_frames);
    let collar_rms = mono_rms(inputs.a_samples, ch, collar_start, refined.start_frame);
    let collar_peak = mono_peak(inputs.a_samples, ch, collar_start, refined.start_frame);
    let collar_ratio = if collar_peak > 0.0 { collar_rms / collar_peak } else { 0.0 };
    let silence = SilenceProfile {
        collar_rms_peak_ratio: collar_ratio,
        collar_above_relative_floor: collar_ratio >= cfg.silence_peak_fraction,
        silence_peak_fraction: cfg.silence_peak_fraction,
    };

    let sig_params = structure_params_for(cfg, gap_frames.max(1), bin_frames, 0, 0);
    let signature = build_gap_signature(
        inputs.a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        context_frames,
        &sig_params,
        GapSignatureMode::Energy,
    );
    let pre_env = energy_bins(inputs.a_samples, ch, pre_start, refined.start_frame, bin_frames.max(1), cfg.silence_peak_fraction, cfg.absolute_silence_rms);
    let post_env = energy_bins(inputs.a_samples, ch, refined.end_frame, post_end, bin_frames.max(1), cfg.silence_peak_fraction, cfg.absolute_silence_rms);
    let contour = ContourInfo {
        has_anchor_seam_contour: signature.has_anchor_seam_contour(),
        pre_flatness: flatness(&pre_env),
        post_flatness: flatness(&post_env),
    };

    let bracket_params = AnchorSeamParams {
        context_frames,
        max_anchors_per_side: cfg.max_anchors_per_side,
        max_bracket_frames: (cfg.max_anchor_bracket_secs * rate).round().max(1.0) as usize,
        min_prominence: cfg.anchor_seam_min_prominence,
        structure: structure_params_for(cfg, gap_frames.max(1), bin_frames, 0, 0),
    };
    let candidates = list_anchor_candidates_a(inputs.a_samples, ch, refined, &bracket_params);
    let map_anchor = |c: &crate::domain::gap_anchor_seam::AnchorCandidate| AnchorPoint {
        time_secs: c.frame as f64 / rate,
        source: match c.source {
            AnchorSource::ScanRefined => AnchorSourceKind::ScanRefined,
            AnchorSource::EnergyPeak => AnchorSourceKind::EnergyPeak,
            AnchorSource::BoolTransition => AnchorSourceKind::BoolTransition,
        },
        prominence: c.prominence,
        rms_db: to_db((c.rms.exp() - 1.0).max(0.0)),
    };
    let anchors = AnchorSet {
        pre: candidates.pre.iter().map(map_anchor).collect(),
        post: candidates.post.iter().map(map_anchor).collect(),
    };
    let raw_brackets = list_feasible_anchor_brackets(&candidates, refined, &bracket_params);

    // --- pairwise (B present) ---
    let mut structure = None;
    let mut seams = None;
    let mut lag = None;
    let mut baseline_lag = None;
    // Throat (decision) placement, captured from the baseline `place_on_b` for the decision-seam lag.
    let mut baseline_placement: Option<(usize, Option<usize>)> = None;
    let mut brackets: Vec<BracketInfo> = raw_brackets
        .iter()
        .map(|b| BracketInfo {
            pre_time_secs: b.pre.frame as f64 / rate,
            post_time_secs: b.post.frame as f64 / rate,
            span_secs: (b.post.frame.saturating_sub(b.pre.frame)) as f64 / rate,
            move_frames: b.move_frames,
            structure_pre: None,
            structure_post: None,
            seam_pre: None,
            seam_post: None,
            failure_stage: None,
        })
        .collect();

    if let Some(b_haystack) = inputs.b_haystack {
        let b_mono = interleaved_to_mono(b_haystack, ch);
        let b_ch = interleaved_to_channels(b_haystack, ch);
        let search_radius_frames = ((cfg.fill_border_search_secs.max(cfg.fill_align_margin_secs)) * rate).round() as usize;
        // Each A boundary maps to its own B nominal: a_time + gap_offset, in haystack frame coords.
        let gap_offset_secs = inputs.gap_offset_secs;
        let nominal_of = |a_start_frame: usize| -> usize {
            (((a_start_frame as f64 / rate + gap_offset_secs - inputs.b_extract_start_secs) * rate)
                .round()
                .max(0.0)) as usize
        };

        if let Some(base) = place_on_b(inputs.a_samples, ch, refined, b_haystack, &b_mono, &b_ch, nominal_of(refined.start_frame), context_frames, bin_frames, search_radius_frames, cfg) {
            baseline_placement = Some((base.start_frame, base.selected_channels.first().copied()));
            structure = Some(StructureScores { baseline_pre: base.structure_pre, baseline_post: base.structure_post });
            seams = Some(SeamScores {
                baseline_pre: base.seam_pre,
                baseline_post: base.seam_post,
                selected_channels: base.selected_channels,
                per_channel: base.per_channel,
                mono_pre: base.mono_pre,
                mono_post: base.mono_post,
            });
        }

        if tier == DetailTier::Full {
            // Decision-seam lag: at the structure-slid throat placement (the seam the gate decides on).
            if let Some((start_frame, selected)) = baseline_placement {
                baseline_lag = Some(lag_at_placement(inputs.a_samples, ch, refined, &b_mono, &b_ch, selected, start_frame, bin_frames, cfg, inputs.sample_rate));
            }
            // Per-bracket scoring; remember the best-structure energy-peak bracket's placement for lag.
            let mut best: Option<(f64, usize, RefinedGapFrames, Option<usize>)> = None;
            for (i, br) in raw_brackets.iter().enumerate() {
                let refined_b = br.refined;
                if let Some(p) = place_on_b(inputs.a_samples, ch, refined_b, b_haystack, &b_mono, &b_ch, nominal_of(refined_b.start_frame), context_frames, bin_frames, search_radius_frames, cfg) {
                    brackets[i].structure_pre = Some(p.structure_pre);
                    brackets[i].structure_post = Some(p.structure_post);
                    brackets[i].seam_pre = Some(p.seam_pre);
                    brackets[i].seam_post = Some(p.seam_post);
                    brackets[i].failure_stage = classify_bracket_stage(p.structure_pre, p.structure_post, p.seam_pre, p.seam_post, cfg);
                    let energy_pair = br.pre.source == AnchorSource::EnergyPeak && br.post.source == AnchorSource::EnergyPeak;
                    let smin = p.structure_pre.min(p.structure_post);
                    if energy_pair && best.is_none_or(|(bs, ..)| smin > bs) {
                        best = Some((smin, p.start_frame, refined_b, p.selected_channels.first().copied()));
                    }
                } else {
                    brackets[i].failure_stage = Some(FailureStage::StructureAlign);
                }
            }
            if let Some((_, start_frame, refined_b, selected)) = best {
                lag = Some(lag_at_placement(inputs.a_samples, ch, refined_b, &b_mono, &b_ch, selected, start_frame, bin_frames, cfg, inputs.sample_rate));
            }
        }
    }

    if tier != DetailTier::Full {
        brackets.clear();
    }

    GapFingerprint {
        index,
        tier,
        sample_rate: inputs.sample_rate,
        channels: inputs.channels as u16,
        geometry,
        levels,
        silence,
        contour,
        anchors,
        brackets,
        structure,
        seams,
        lag,
        baseline_lag,
        outcome: None,
    }
}

impl FingerprintConfig {
    /// Mirror the run's patch request so the builder enumerates the **same** brackets the gate does
    /// (matching `bin_frames` / `context` / `max_bracket` / `min_prominence` is what lets the oracle
    /// pass align per-bracket by frame).
    pub(crate) fn from_request(
        request: &crate::application::PatchAudioRequest,
        silence_peak_fraction: f32,
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
            fill_length_slack_secs: request.fill_length_slack_secs,
            border_secs: request.normalize_window_secs,
            border_standoff_secs: request.border_standoff_secs,
            max_anchor_bracket_secs: request.max_anchor_bracket_secs,
            max_anchors_per_side: request.max_anchors_per_side,
            anchor_seam_min_prominence: request.anchor_seam_min_prominence,
            min_structure_match_score: f64::from(request.min_structure_match_score),
            min_fill_correlation: request.min_fill_correlation,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
            max_refine_secs: GAP_EDGE_REFINE_SECS,
            lag_max_lag_ms: 200,
        }
    }
}

/// Map a gate failure to the fingerprint's `failure_stage` + the seam scores it carries.
fn stage_of(
    failure: &crate::application::patch_region::SeamGateFailure,
) -> (FailureStage, Option<f64>, Option<f64>) {
    use crate::application::patch_region::SeamGateFailure as F;
    match failure {
        F::StructureAlignmentFailed => (FailureStage::StructureAlign, None, None),
        F::StructureBelowThreshold { pre, post } => (FailureStage::StructureFloor, Some(*pre), Some(*post)),
        F::WaveformBelowThreshold { pre, post, .. } => (FailureStage::WaveformFloor, Some(*pre), Some(*post)),
        F::ResidualHeadroomExceeded { pre, post, .. } => (FailureStage::Residual, Some(*pre), Some(*post)),
    }
}

fn anchor_params_from_gate(
    cfg: &crate::application::patch_region::SeamGateConfig,
    baseline: RefinedGapFrames,
) -> AnchorSeamParams {
    let gap_frames = baseline.end_frame.saturating_sub(baseline.start_frame);
    AnchorSeamParams {
        context_frames: cfg.context_frames,
        max_anchors_per_side: cfg.max_anchors_per_side,
        max_bracket_frames: (cfg.max_anchor_bracket_secs * f64::from(cfg.sample_rate)).round().max(1.0) as usize,
        min_prominence: cfg.anchor_seam_min_prominence,
        structure: StructureMatchParams {
            gap_frames,
            bin_frames: cfg.bin_frames.max(1),
            search_radius_frames: cfg.search_radius_frames,
            fill_length_slack_frames: cfg.fill_length_slack_frames,
            max_fine_adjustment_frames: crate::domain::gap_structure::structure_fine_polish_frames(cfg.bin_frames),
            silence_peak_fraction: cfg.silence_peak_fraction,
            absolute_silence_rms: cfg.absolute_silence_rms,
        },
    }
}

/// Characterize the gaps in `select` (empty ⇒ **all** gaps) at **full** authoritative detail (the bin
/// path). Each gap gets its per-bracket `failure_stage` + seam, baseline seam, outcome, and lag from
/// the production gate (`oracle_*`) — **N + 2** unified searches per gap (N brackets via
/// `oracle_score_fit_candidate`, the baseline via the zero-move bracket, and one `place_on_b` for the
/// lag placement). Only the selected gaps are built; unselected gaps are never characterized.
pub(crate) fn characterize_gaps_with_gate(
    report: &crate::domain::GapReport,
    a_pcm: &clip_sync::MultiChannelPcm,
    b_samples_full: &[f32],
    request: &crate::application::PatchAudioRequest,
    select: &[usize],
    progress: &dyn clip_sync::ProgressReporter,
) -> GapCorpus {
    use crate::application::patch_region::{oracle_build_fit_cache, oracle_score_fit_candidate};

    let sample_rate = a_pcm.sample_rate;
    let channels = a_pcm.channels as usize;
    let cfg = FingerprintConfig::from_request(request, report.silence_peak_fraction);
    // Build only the selected gaps (summary base: baseline structure/seam, no per-bracket, no lag).
    let mut corpus = characterize_gaps(report, &a_pcm.samples, b_samples_full, sample_rate, channels, &cfg, select);

    let gate_cfg = crate::application::patch_region::SeamGateConfig::from_repair(
        request,
        sample_rate,
        channels,
        report.silence_peak_fraction,
    );
    let rate = f64::from(sample_rate).max(1.0);
    let ch = channels.max(1);
    let b_total = b_samples_full.len() / ch;
    let max_refine_frames = (cfg.max_refine_secs * rate).round() as usize;
    let context_frames = (cfg.gap_signature_context_secs * rate).round() as usize;
    let bin_frames = ((cfg.gap_signature_bin_ms as f64 / 1000.0) * rate).round().max(1.0) as usize;
    let search_radius_frames = ((cfg.fill_border_search_secs.max(cfg.fill_align_margin_secs)) * rate).round() as usize;
    let pad_lead = cfg.gap_signature_context_secs + cfg.fill_border_search_secs + cfg.fill_align_margin_secs;
    let pad_tail = cfg.gap_signature_context_secs
        + cfg.fill_length_slack_secs.max(cfg.fill_align_margin_secs)
        + cfg.fill_border_search_secs
        + cfg.fill_align_margin_secs;

    let total_gaps = corpus.gaps.len() as u64;
    for (gn, fp) in corpus.gaps.iter_mut().enumerate() {
        progress.progress("fingerprint-gap", gn as u64 + 1, total_gaps);
        let i = fp.index;
        let gap = &report.gaps[i];
        let Some(b_start) = gap.video_b_start_secs else { continue };
        let refined = refine_gap_frames(
            &a_pcm.samples,
            ch,
            (gap.video_a_start_secs * rate) as usize,
            (gap.video_a_end_secs * rate) as usize,
            cfg.silence_peak_fraction,
            cfg.absolute_silence_rms,
            max_refine_frames,
        );
        let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
        if gap_frames == 0 {
            continue;
        }
        let gap_offset = b_start - gap.video_a_start_secs;
        let b_end = gap.video_b_end_secs.unwrap_or(gap.video_a_end_secs);
        let lo = (((b_start - pad_lead).max(0.0) * rate) as usize).min(b_total);
        let hi = (((b_end + pad_tail) * rate).ceil() as usize).min(b_total);
        if hi <= lo {
            continue;
        }
        let b_slice = &b_samples_full[lo * ch..hi * ch];
        let b_extract_start_secs = lo as f64 / rate;
        let geom = crate::application::patch_region::derive_seam_gate_geometry(
            &gate_cfg,
            a_pcm,
            b_slice,
            b_extract_start_secs,
            refined.start_frame as f64 / rate + gap_offset,
            refined.end_frame as f64 / rate + gap_offset,
            gap_frames,
            None,
        );
        let params = crate::application::patch_region::SeamGateParams { cfg: &gate_cfg, geom };
        let cache = oracle_build_fit_cache(&params);
        fp.tier = DetailTier::Full;

        // Per-bracket authoritative seam + failure_stage (gate enumeration). The zero-move bracket is
        // the throat; its score becomes the baseline seam (consistent with the brackets and ~the
        // production throat — unlike a separate non-anchor baseline, whose unbiased search drifts to a
        // different placement and reads a divergent value).
        let anchor_params = anchor_params_from_gate(&gate_cfg, refined);
        let candidates = list_anchor_candidates_a(&a_pcm.samples, ch, refined, &anchor_params);
        let brackets = list_feasible_anchor_brackets(&candidates, refined, &anchor_params);
        let mut any_ok = false;
        let mut best_energy: Option<(f64, RefinedGapFrames)> = None;
        let mut infos = Vec::with_capacity(brackets.len());
        let bracket_total = brackets.len() as u64;
        for (bn, br) in brackets.iter().enumerate() {
            progress.progress("fingerprint-scoring", bn as u64 + 1, bracket_total);
            let (seam_pre, seam_post, stage) =
                match oracle_score_fit_candidate(&params, &cache, br.refined, refined, true) {
                    Ok((pre, post, _, _)) => {
                        any_ok = true;
                        (Some(pre), Some(post), None)
                    }
                    Err(f) => {
                        let (stage, pre, post) = stage_of(&f);
                        (pre, post, Some(stage))
                    }
                };
            if br.refined == refined {
                if let Some(s) = &mut fp.seams {
                    if let Some(p) = seam_pre {
                        s.baseline_pre = p;
                    }
                    if let Some(p) = seam_post {
                        s.baseline_post = p;
                    }
                }
            }
            infos.push(BracketInfo {
                pre_time_secs: br.pre.frame as f64 / rate,
                post_time_secs: br.post.frame as f64 / rate,
                span_secs: br.post.frame.saturating_sub(br.pre.frame) as f64 / rate,
                move_frames: br.move_frames,
                structure_pre: None,
                structure_post: None,
                seam_pre,
                seam_post,
                failure_stage: stage,
            });
            if br.pre.source == AnchorSource::EnergyPeak && br.post.source == AnchorSource::EnergyPeak {
                let smin = match (seam_pre, seam_post) {
                    (Some(a), Some(b)) => a.min(b),
                    _ => f64::NEG_INFINITY,
                };
                if best_energy.is_none_or(|(bs, _)| smin > bs) {
                    best_energy = Some((smin, br.refined));
                }
            }
        }
        fp.brackets = infos;
        let patched = any_ok;
        fp.outcome = Some(GateOutcome {
            plan_kind: "fillable".into(),
            tier: if patched { "patch".into() } else { "skip".into() },
            seam_shape: String::new(),
            fit_path: None,
            signature_mode: None,
            skip_reason: (!patched).then(|| "gate skipped".into()),
        });

        // Lag fingerprints — `b_mono`/`b_ch` shared by both placements.
        let b_mono = interleaved_to_mono(b_slice, ch);
        let b_ch = interleaved_to_channels(b_slice, ch);
        let nominal_of = |start: usize| {
            (((start as f64 / rate + gap_offset - b_extract_start_secs) * rate)
                .round()
                .max(0.0)) as usize
        };

        // Decision-seam lag (#2): at the structure-slid THROAT placement — the seam the gate decides
        // patch/skip on under baseline_only — not the moved best-energy bracket the diagnostic `lag`
        // measures. This is the registration-relevant lag.
        if let Some(p) = place_on_b(&a_pcm.samples, ch, refined, b_slice, &b_mono, &b_ch, nominal_of(refined.start_frame), context_frames, bin_frames, search_radius_frames, &cfg) {
            fp.baseline_lag = Some(lag_at_placement(&a_pcm.samples, ch, refined, &b_mono, &b_ch, p.selected_channels.first().copied(), p.start_frame, bin_frames, &cfg, sample_rate));
        }

        // Diagnostic lag: one placement search at the best (highest-seam) speech bracket.
        if let Some((_, refined_b)) = best_energy {
            if let Some(p) = place_on_b(&a_pcm.samples, ch, refined_b, b_slice, &b_mono, &b_ch, nominal_of(refined_b.start_frame), context_frames, bin_frames, search_radius_frames, &cfg) {
                fp.lag = Some(lag_at_placement(&a_pcm.samples, ch, refined_b, &b_mono, &b_ch, p.selected_channels.first().copied(), p.start_frame, bin_frames, &cfg, sample_rate));
            }
        }
    }
    corpus
}

/// Build A-side **summary** fingerprints for the gaps in `select` (empty ⇒ all) against decoded
/// full A/B PCM: geometry + levels + contour + anchors + a baseline structure/seam per gap. The
/// authoritative gate detail (brackets / `failure_stage` / lag / outcome) is layered on by
/// [`characterize_gaps_with_gate`]. A gap with no B mapping is characterized A-only.
pub fn characterize_gaps(
    report: &crate::domain::GapReport,
    a_samples: &[f32],
    b_samples: &[f32],
    sample_rate: u32,
    channels: usize,
    cfg: &FingerprintConfig,
    select: &[usize],
) -> GapCorpus {
    let rate = f64::from(sample_rate).max(1.0);
    let ch = channels.max(1);
    let b_total = b_samples.len() / ch;
    // Per-gap B haystack pad: context + border search + margin/slack on each side (mirrors
    // `prepare_region_patch`). Bounds the unified search so it does not build a timeline over all of B.
    let pad_lead = cfg.gap_signature_context_secs + cfg.fill_border_search_secs + cfg.fill_align_margin_secs;
    let pad_tail = cfg.gap_signature_context_secs
        + cfg.fill_border_search_secs
        + cfg.fill_length_slack_secs.max(cfg.fill_align_margin_secs)
        + cfg.fill_align_margin_secs;

    let take_all = select.is_empty();
    let gaps = report
        .gaps
        .iter()
        .enumerate()
        .filter(|(i, _)| take_all || select.contains(i))
        .map(|(i, gap)| {
            let has_b = gap.video_b_start_secs.is_some();
            let gap_offset_secs = gap
                .video_b_start_secs
                .map(|b0| b0 - gap.video_a_start_secs)
                .unwrap_or(0.0);

            let (b_haystack, b_extract_start_secs) = if has_b {
                let b_start = gap.video_b_start_secs.unwrap_or(gap.video_a_start_secs);
                let b_end = gap.video_b_end_secs.unwrap_or(gap.video_a_end_secs);
                let extract_start = (b_start - pad_lead).max(0.0);
                let extract_end = b_end + pad_tail;
                let lo = ((extract_start * rate) as usize).min(b_total);
                let hi = ((extract_end * rate).ceil() as usize).min(b_total);
                if hi > lo {
                    (Some(&b_samples[lo * ch..hi * ch]), lo as f64 / rate)
                } else {
                    (None, 0.0)
                }
            } else {
                (None, 0.0)
            };

            let inputs = GapInputs {
                a_samples,
                b_haystack,
                channels,
                sample_rate,
                reported_start_frame: (gap.video_a_start_secs * rate) as usize,
                reported_end_frame: (gap.video_a_end_secs * rate) as usize,
                b_extract_start_secs,
                gap_offset_secs,
                config: *cfg,
            };
            build_gap_fingerprint(i, &inputs, DetailTier::Summary)
        })
        .collect();

    GapCorpus {
        source: SourceMeta {
            a_source: file_source(a_samples, sample_rate, channels as u16),
            b_source: file_source(b_samples, sample_rate, channels as u16),
            scan_recipe: ScanRecipe::from_report(report),
            gap_count: report.gaps.len(),
        },
        gaps,
    }
}

fn detail_tier_str(t: DetailTier) -> &'static str {
    match t {
        DetailTier::Summary => "summary",
        DetailTier::Full => "full",
    }
}

fn lag_verdict_str(v: LagVerdict) -> &'static str {
    match v {
        LagVerdict::TimingOffset => "timing_offset",
        LagVerdict::Decorrelated => "decorrelated",
        LagVerdict::Ambiguous => "ambiguous",
    }
}

fn hms(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}-{:02}-{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Headline tag for a gap's filename: the lag verdict if measured, else the gate outcome, else `na`.
fn entry_verdict(gap: &GapFingerprint) -> String {
    gap.lag
        .as_ref()
        .and_then(|l| l.pre_anchor.first().or_else(|| l.post_anchor.first()))
        .map(|s| lag_verdict_str(s.verdict).to_string())
        .or_else(|| gap.outcome.as_ref().map(|o| o.tier.clone()))
        .unwrap_or_else(|| "na".to_string())
}

/// `<a8>_<b4>_t<hh-mm-ss>_g<idx>_<tier>_<verdict>.json` — non-leaking, sortable, classifiable.
fn entry_filename(source: &SourceMeta, gap: &GapFingerprint) -> String {
    let a8: String = source.a_source.id.chars().take(8).collect();
    let b4: String = source.b_source.id.chars().take(4).collect();
    format!(
        "{a8}_{b4}_t{}_g{:03}_{}_{}.json",
        hms(gap.geometry.a_refined_start_secs),
        gap.index,
        detail_tier_str(gap.tier),
        entry_verdict(gap),
    )
}

#[derive(Serialize)]
struct ManifestEntry {
    file: String,
    index: usize,
    a_start_secs: f64,
    tier: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lag_verdict: Option<String>,
}

#[derive(Serialize)]
struct Manifest<'a> {
    a_id: &'a str,
    b_id: &'a str,
    scan_recipe: &'a ScanRecipe,
    gap_count: usize,
    entries: Vec<ManifestEntry>,
}

/// Write a self-contained corpus directory: the combined `corpus.json` (all gaps), one single-gap
/// [`GapCorpus`] JSON **per gap**, and a non-identifying `manifest.json`. Returns the gap count. No
/// titles/paths anywhere.
pub(crate) fn write_corpus_dir(
    corpus: &GapCorpus,
    dir: &std::path::Path,
) -> std::io::Result<usize> {
    let to_io =
        |e: serde_json::Error| std::io::Error::new(std::io::ErrorKind::Other, e);
    std::fs::create_dir_all(dir)?;
    // Combined corpus (all gaps) for quick inspection / scripting.
    let combined = std::fs::File::create(dir.join("corpus.json"))?;
    serde_json::to_writer_pretty(combined, corpus).map_err(to_io)?;

    let mut entries = Vec::with_capacity(corpus.gaps.len());
    for gap in &corpus.gaps {
        let file = entry_filename(&corpus.source, gap);
        let single = GapCorpus {
            source: corpus.source.clone(),
            gaps: vec![gap.clone()],
        };
        let f = std::fs::File::create(dir.join(&file))?;
        serde_json::to_writer_pretty(f, &single).map_err(to_io)?;
        entries.push(ManifestEntry {
            file,
            index: gap.index,
            a_start_secs: gap.geometry.a_refined_start_secs,
            tier: detail_tier_str(gap.tier),
            outcome: gap.outcome.as_ref().map(|o| o.tier.clone()),
            lag_verdict: gap
                .lag
                .as_ref()
                .and_then(|l| l.pre_anchor.first().or_else(|| l.post_anchor.first()))
                .map(|s| lag_verdict_str(s.verdict).to_string()),
        });
    }
    let manifest = Manifest {
        a_id: &corpus.source.a_source.id,
        b_id: &corpus.source.b_source.id,
        scan_recipe: &corpus.source.scan_recipe,
        gap_count: corpus.gaps.len(),
        entries,
    };
    let mf = std::fs::File::create(dir.join("manifest.json"))?;
    serde_json::to_writer_pretty(mf, &manifest).map_err(to_io)?;
    Ok(corpus.gaps.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// splitmix64 finalizer → deterministic noise in [-1, 1).
    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = (((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15))
            as u64;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    fn base_noise(seed: u64, len: usize) -> Vec<f64> {
        (0..len).map(|i| noise(seed, i)).collect()
    }

    fn lerp(base: &[f64], x: f64) -> f64 {
        if x <= 0.0 {
            return base[0];
        }
        let i = x.floor() as usize;
        if i + 1 >= base.len() {
            return base[base.len() - 1];
        }
        let f = x - i as f64;
        base[i] * (1.0 - f) + base[i + 1] * f
    }

    /// Build (a, b_ctx) where B is the shared broadband signal delayed by `offset` samples (possibly
    /// fractional), so the lag curve should peak at lag ≈ `offset`.
    fn shifted_pair(offset: f64, n: usize, max_lag: i64) -> (Vec<f64>, Vec<f64>) {
        let ml = max_lag as f64;
        let a0 = (ml + offset.abs()).ceil() as usize + 4;
        let base = base_noise(1, a0 + n + max_lag as usize + 8);
        let a: Vec<f64> = (0..n).map(|i| base[a0 + i]).collect();
        let ctx_len = n + 2 * max_lag as usize;
        let b_ctx: Vec<f64> = (0..ctx_len)
            .map(|k| lerp(&base, a0 as f64 + (k as f64 - ml - offset)))
            .collect();
        (a, b_ctx)
    }

    #[test]
    fn lag_curve_finds_integer_offset() {
        let (a, b_ctx) = shifted_pair(17.0, 4000, 64);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert_eq!(s.peak_lag_samples, 17, "peak at the true integer lag");
        assert!(s.peak_r > 0.95, "shared signal correlates ~1 at the lag, got {}", s.peak_r);
        assert!(s.lag0_r < 0.5, "lag-0 is depressed by the offset, got {}", s.lag0_r);
        assert_eq!(s.verdict, LagVerdict::TimingOffset);
    }

    #[test]
    fn lag_curve_resolves_fractional_offset() {
        let (a, b_ctx) = shifted_pair(17.4, 4000, 64);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert!(
            (s.frac_lag_samples - 17.4).abs() < 0.3,
            "parabolic lag {} should land near 17.4",
            s.frac_lag_samples
        );
        assert_eq!(s.verdict, LagVerdict::TimingOffset);
    }

    #[test]
    fn lag_curve_flat_for_decorrelated_sources() {
        let n = 4000;
        let max_lag = 64i64;
        let a = base_noise(1, n);
        let b_ctx = base_noise(2, n + 2 * max_lag as usize); // independent stream
        let curve = lag_correlation_curve(&a, &b_ctx, max_lag);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert!(
            s.peak_r < 0.3,
            "independent noise should not correlate at any lag, peak {}",
            s.peak_r
        );
        assert_eq!(s.verdict, LagVerdict::Decorrelated);
    }

    #[test]
    fn lag_curve_truncates_when_context_too_short() {
        // b_ctx shorter than a.len() + 2*max_lag → fewer than the full set of lags, no panic.
        let a = base_noise(1, 100);
        let b_ctx = base_noise(1, 120);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        assert!(curve.len() < (2 * 64 + 1));
        assert!(curve.iter().all(|(_, r)| r.is_finite()));
    }

    fn write_speech(buf: &mut [f32], start: usize, end: usize, freq: f64, amp: f32) {
        let n = (end - start) as f64;
        for f in start..end {
            let t = (f - start) as f64;
            let env = 0.5 - 0.5 * (std::f64::consts::TAU * t / n).cos();
            let s = (std::f64::consts::TAU * freq * t / 48_000.0).sin();
            buf[f] = (env * s) as f32 * amp;
        }
    }

    fn write_noise(buf: &mut [f32], start: usize, end: usize, seed: u64, amp: f32) {
        for f in start..end {
            buf[f] = noise(seed, f) as f32 * amp;
        }
    }

    #[test]
    fn builder_characterizes_noise_collar_gap() {
        let rate = 48_000u32;
        let ch = 1usize;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(5.0);
        let (sp1, n1, gap, n2, sp2) = (
            (secs(0.50), secs(0.85)),
            (secs(0.85), secs(1.85)),
            (secs(1.85), secs(3.35)),
            (secs(3.35), secs(4.35)),
            (secs(4.35), secs(4.70)),
        );
        let mut a = vec![0f32; total];
        let mut b = vec![0f32; total];
        // Same-master speech bursts (identical), decorrelated collars, B fill in the gap.
        write_speech(&mut a, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut a, sp2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, sp2.0, sp2.1, 440.0, 0.079);
        write_noise(&mut a, n1.0, n1.1, 1, 0.0056);
        write_noise(&mut b, n1.0, n1.1, 11, 0.0056);
        write_noise(&mut a, n2.0, n2.1, 3, 0.0056);
        write_noise(&mut b, n2.0, n2.1, 13, 0.0056);
        write_noise(&mut b, gap.0, gap.1, 5, 0.0056);

        let config = FingerprintConfig {
            gap_signature_context_secs: 1.5,
            fill_border_search_secs: 1.0,
            border_secs: 0.3,
            border_standoff_secs: 0.0,
            ..Default::default()
        };
        let inputs = GapInputs {
            a_samples: &a,
            b_haystack: Some(&b),
            channels: ch,
            sample_rate: rate,
            reported_start_frame: gap.0,
            reported_end_frame: gap.1,
            b_extract_start_secs: 0.0,
            gap_offset_secs: 0.0,
            config,
        };
        let fp = build_gap_fingerprint(0, &inputs, DetailTier::Full);

        assert!((fp.geometry.duration_secs - 1.5).abs() < 0.05, "duration {}", fp.geometry.duration_secs);
        assert!(
            fp.anchors.pre.iter().any(|p| p.source == AnchorSourceKind::EnergyPeak),
            "expected a pre energy-peak anchor: {:?}",
            fp.anchors.pre
        );
        assert!(fp.contour.has_anchor_seam_contour, "speech bursts give contour");

        let s = fp.structure.expect("structure present with B");
        assert!(s.baseline_pre > 0.5, "structure aligns at the throat: {}", s.baseline_pre);
        let seam = fp.seams.expect("seams present with B");
        assert!(
            seam.baseline_pre < 0.2 && seam.baseline_post < 0.2,
            "throat seam collapses in decorrelated noise: pre={} post={}",
            seam.baseline_pre,
            seam.baseline_post
        );
        assert!(
            fp.brackets.iter().any(|b| b.seam_pre.is_some_and(|v| v > 0.5) && b.seam_post.is_some_and(|v| v > 0.5)),
            "a speech-anchored bracket should score a strong seam where the throat cannot"
        );
        assert!(fp.lag.is_some(), "lag computed at the best speech bracket");
    }

    #[test]
    fn source_id_stable_and_distinguishing() {
        let a: Vec<f32> = (0..10_000).map(|i| noise(7, i) as f32).collect();
        let b: Vec<f32> = (0..10_000).map(|i| noise(8, i) as f32).collect();
        assert_eq!(source_id(&a, 48_000, 2), source_id(&a, 48_000, 2), "deterministic");
        assert_eq!(source_id(&a, 48_000, 2).len(), 16, "16 hex chars");
        assert_ne!(source_id(&a, 48_000, 2), source_id(&b, 48_000, 2), "different audio → different id");
        assert_ne!(source_id(&a, 48_000, 2), source_id(&a, 44_100, 2), "sample rate is part of identity");
        assert_ne!(source_id(&a, 48_000, 2), source_id(&a, 48_000, 6), "channels are part of identity");
    }

    fn mk_fp(index: usize, full: bool) -> GapFingerprint {
        GapFingerprint {
            index,
            tier: if full { DetailTier::Full } else { DetailTier::Summary },
            sample_rate: 48_000,
            channels: 2,
            geometry: GapGeometry {
                a_start_secs: 832.5,
                a_end_secs: 834.0,
                a_refined_start_secs: 832.3,
                a_refined_end_secs: 834.1,
                duration_secs: 1.8,
                b_mapped_start_secs: None,
                b_mapped_end_secs: None,
                fill_offset_secs: None,
            },
            levels: LevelProfile {
                bin_ms: 50,
                profile_db: vec![],
                floor_db: -120.0,
                speech_peak_db: -40.0,
                noise_floor_db: -53.0,
                gap_floor_db: -98.0,
            },
            silence: SilenceProfile {
                collar_rms_peak_ratio: 0.1,
                collar_above_relative_floor: true,
                silence_peak_fraction: 0.01,
            },
            contour: ContourInfo {
                has_anchor_seam_contour: true,
                pre_flatness: 0.0,
                post_flatness: 0.0,
            },
            anchors: AnchorSet::default(),
            brackets: vec![],
            structure: None,
            seams: None,
            lag: full.then(|| LagFingerprint {
                pre_anchor: vec![LagSummary {
                    window_ms: 250,
                    max_lag_ms: 200,
                    channel: LagChannel::Mono,
                    lag0_r: 0.02,
                    peak_r: 0.99,
                    peak_lag_samples: -778,
                    frac_lag_samples: -778.0,
                    frac_lag_ms: -16.2,
                    verdict: LagVerdict::TimingOffset,
                }],
                post_anchor: vec![],
            }),
            baseline_lag: None,
            outcome: None,
        }
    }

    #[test]
    fn write_corpus_library_emits_named_files_and_manifest() {
        let corpus = GapCorpus {
            source: SourceMeta {
                a_source: FileSource {
                    id: "aaaaaaaaaaaaaaaa".into(),
                    container: None,
                    codec: None,
                    sample_rate: 48_000,
                    channels: 2,
                    duration_secs: 1000.0,
                },
                b_source: FileSource {
                    id: "bbbbbbbbbbbbbbbb".into(),
                    container: None,
                    codec: None,
                    sample_rate: 48_000,
                    channels: 2,
                    duration_secs: 1000.0,
                },
                scan_recipe: ScanRecipe::default(),
                gap_count: 2,
            },
            gaps: vec![mk_fp(0, false), mk_fp(3, true)],
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let n = write_corpus_dir(&corpus, dir.path()).expect("write library");
        assert_eq!(n, 2);
        assert!(dir.path().join("manifest.json").exists());
        assert!(dir.path().join("corpus.json").exists());

        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|f| f.contains("g003_full_timing_offset")),
            "full gap named by verdict: {names:?}"
        );
        assert!(
            names.iter().any(|f| f.contains("g000_summary_na")),
            "summary gap named na: {names:?}"
        );
        // No leaking identifiers; per-gap files are opaque-id prefixed.
        assert!(names
            .iter()
            .all(|f| f == "manifest.json" || f == "corpus.json" || f.starts_with("aaaaaaaa_bbbb_")));

        // Each per-gap file is a self-contained single-gap corpus.
        let full = names.iter().find(|f| f.contains("g003")).unwrap();
        let parsed: GapCorpus =
            serde_json::from_reader(std::fs::File::open(dir.path().join(full)).unwrap()).unwrap();
        assert_eq!(parsed.gaps.len(), 1);
        assert_eq!(parsed.gaps[0].index, 3);
        assert_eq!(parsed.source.a_source.id, "aaaaaaaaaaaaaaaa");
    }

    #[test]
    fn fingerprint_json_round_trips() {
        let fp = GapFingerprint {
            index: 3,
            tier: DetailTier::Summary,
            sample_rate: 48_000,
            channels: 6,
            geometry: GapGeometry {
                a_start_secs: 832.5,
                a_end_secs: 834.0,
                a_refined_start_secs: 832.304,
                a_refined_end_secs: 834.133,
                duration_secs: 1.829,
                b_mapped_start_secs: Some(826.752),
                b_mapped_end_secs: Some(828.581),
                fill_offset_secs: Some(-5.552),
            },
            levels: LevelProfile {
                bin_ms: 50,
                profile_db: vec![-45.0, -24.0, -45.0],
                floor_db: -120.0,
                speech_peak_db: -22.0,
                noise_floor_db: -45.0,
                gap_floor_db: -120.0,
            },
            silence: SilenceProfile {
                collar_rms_peak_ratio: 0.42,
                collar_above_relative_floor: true,
                silence_peak_fraction: 0.01,
            },
            contour: ContourInfo {
                has_anchor_seam_contour: true,
                pre_flatness: 0.1,
                post_flatness: 0.1,
            },
            anchors: AnchorSet {
                pre: vec![AnchorPoint {
                    time_secs: 831.3,
                    source: AnchorSourceKind::EnergyPeak,
                    prominence: 0.0075,
                    rms_db: -24.0,
                }],
                post: vec![],
            },
            brackets: vec![],
            structure: Some(StructureScores {
                baseline_pre: 0.996,
                baseline_post: 0.926,
            }),
            seams: Some(SeamScores {
                baseline_pre: 0.030,
                baseline_post: 0.030,
                selected_channels: vec![2],
                per_channel: vec![(0.030, -0.050), (0.030, 0.030)],
                mono_pre: 0.0296,
                mono_post: 0.0290,
            }),
            lag: None,
            baseline_lag: None,
            outcome: Some(GateOutcome {
                plan_kind: "fillable".into(),
                tier: "hard_skip".into(),
                seam_shape: "symmetric_weak".into(),
                fit_path: Some("baseline_only".into()),
                signature_mode: Some("energy".into()),
                skip_reason: Some("boundary correlation below threshold".into()),
            }),
        };
        let json = serde_json::to_string(&fp).expect("serialize");
        let back: GapFingerprint = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(fp, back);
        // Absent optionals stay out of the wire form.
        assert!(!json.contains("\"lag\""));
        assert!(!json.contains("\"baseline_lag\""));
        assert!(!json.contains("\"brackets\""));
    }
}
