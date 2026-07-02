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
    /// Residual cancellation at the decision seam (the strong same-source confirm). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub residual: Option<ResidualInfo>,
    /// Seam recovery / encoding-robust envelope / level at the decision seam — diagnoses *why* a
    /// waveform seam is dead (mis-alignment vs cross-encoding vs quiet). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seam_probe: Option<SeamProbeFingerprint>,
    /// Does donor B actually carry audio across the hole? Energy/continuity of B over the gap-mapped span
    /// — the donor half of the fill predicate (§3/§4). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_interior: Option<DonorInterior>,
    /// Donor occupancy at the **nominal** geometry `b_mapped` span (NO per-shoulder lag) — the
    /// registration-independent sibling of `donor_interior`, so it dodges the aliased-lag confound the
    /// aligned span inherits. `silence_fraction ≈ 1` ⇒ B is quiet at the same program time as A's gap ⇒
    /// program-quiet, not a fillable dropout (D11). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_interior_nominal: Option<DonorInterior>,
    /// Symmetric B-side level profile over the nominal `b_mapped` span + context — the counterpart to
    /// `levels` (A), computed by the same `level_profile` logic. Lets "quiet in both masters ⇒ not a
    /// dropout" compare B's gap level against B's *own* noise floor (D11). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub b_levels: Option<LevelProfile>,
    /// First-class splice summary: the per-side registration step + peaks/uniqueness the repair predicate
    /// reads directly (instead of re-deriving from `baseline_lag`). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub splice: Option<SpliceSummary>,
    /// Wide (100 ms-bin) envelope segment-identity confirmer at the decision seam — its peak lag should
    /// agree with the fine-waveform lag (§3.6a cross-scale check). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub wide_envelope: Option<WideEnvelopeFingerprint>,
    /// Dual-fit repair viability at the per-shoulder placement — the gate-equivalent seam score for a
    /// length-reconciled fill (ledger C3/C7). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub splice_dualfit: Option<SpliceDualfit>,
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

/// Span args for [`level_profile`]: the gap window and the surrounding context window (in frames).
struct LevelProfileSpan {
    gap_start: usize,
    gap_end: usize,
    context_start: usize,
    context_end: usize,
}

/// Build a [`LevelProfile`] over `[context_start, context_end)` (binned), with `noise_floor` taken from the
/// context bins *outside* `[gap_start, gap_end)` and `gap_floor` the loudest bin *inside* it. `bin_rms(f,
/// end)` returns the mono RMS (linear) over `[f, end)`. Shared by the A-side (interleaved downmix) and the
/// symmetric B-side (mono) paths so the two profiles are computed by *identical* logic (D11) and cannot drift.
fn level_profile(bin_rms: impl Fn(usize, usize) -> f32, span: LevelProfileSpan, bin_frames: usize, bin_ms: u32) -> LevelProfile {
    let mut profile_db = Vec::new();
    let mut context_bins_db = Vec::new();
    let mut f = span.context_start;
    while f < span.context_end {
        let end = (f + bin_frames).min(span.context_end);
        let db = to_db(bin_rms(f, end));
        profile_db.push(db);
        if f < span.gap_start || f >= span.gap_end {
            context_bins_db.push(db);
        }
        f = end;
    }
    let gap_floor_db = {
        let mut mx = SILENCE_FLOOR_DB;
        let mut g = span.gap_start;
        while g < span.gap_end {
            let end = (g + bin_frames).min(span.gap_end);
            mx = mx.max(to_db(bin_rms(g, end)));
            g = end;
        }
        mx
    };
    LevelProfile {
        bin_ms,
        speech_peak_db: profile_db.iter().copied().fold(SILENCE_FLOOR_DB, f32::max),
        noise_floor_db: median(context_bins_db),
        gap_floor_db,
        floor_db: SILENCE_FLOOR_DB,
        profile_db,
    }
}

/// Mono RMS (linear) over `b_mono[start..end]` — the B-side accessor for [`level_profile`] / nominal donor.
fn mono_slice_rms(b_mono: &[f64], start: usize, end: usize) -> f32 {
    let end = end.min(b_mono.len());
    if start >= end {
        return 0.0;
    }
    let s = &b_mono[start..end];
    ((s.iter().map(|v| v * v).sum::<f64>() / s.len() as f64).sqrt()) as f32
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

/// Seam diagnostic at the **decision (throat) placement**, one per side. Built to separate *why* a
/// waveform seam is dead: **recovery** (does sample-level Pearson come back under a fine lag → residual
/// mis-alignment, fixable) vs **encoding-robust** envelope agreement (same content present despite the
/// raw waveform differing → cross-encoding) vs **level/SNR** (is the seam just too quiet to score?).
/// All sample-level / fine-bin measurements over the ~`fill_seam_search_secs` seam border window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SeamProbe {
    /// Sample-level waveform Pearson at lag 0 — the gate's decision metric (≈ `seams.baseline`).
    pub waveform_r: f64,
    /// Best waveform Pearson over a fine ±lag search (`fine_max_lag_ms`); recovery ⇒ mis-alignment.
    pub recovered_r: f64,
    pub recovered_lag_ms: f64,
    /// **R2** — band-limited (~300 Hz) waveform Pearson at lag 0: cross-codec-robust (drops the
    /// high-frequency detail codecs alter). High while `waveform_r` low ⇒ validator mismatch candidate.
    pub bandlimited_r: f64,
    /// **R4** — magnitude-spectrum correlation: phase- and small-shift-invariant cross-codec-robust score.
    pub spectrum_r: f64,
    /// Correlation of the fine (~10 ms-bin) RMS envelope over the same window — encoding- and
    /// small-shift-robust (≈ structure-at-seam; the R1-vs-R5 redundancy check).
    pub envelope_r: f64,
    /// Seam-window level (dBFS) and SNR vs the gap floor — is the seam energetic enough to score?
    pub rms_db: f64,
    pub snr_db: f64,
}

/// Per-side seam probes at the decision placement (mono; one entry per measured side).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SeamProbeFingerprint {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre: Option<SeamProbe>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post: Option<SeamProbe>,
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
    /// **Uniqueness guard:** the highest *competing* local-maximum correlation away from the main peak.
    /// A value near `peak_r` means the match is ambiguous — periodic content (tones, music) peaks at
    /// many lags, so a high `peak_r` may be a false same-source positive. `peak_r − second_peak_r` is
    /// the margin by which the chosen lag wins. `None` for fingerprints written before this field.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub second_peak_r: Option<f64>,
    /// **Robust uniqueness:** z-score of the peak over the whole lag curve, `(peak_r − mean)/std`. Unlike
    /// `second_peak_r` (one rival), this measures how far the peak stands out from the *entire* lag
    /// landscape — the metric the §3.6a experiment found separates real registration from periodic
    /// ambiguity at a 1 s window (ambiguous ≈ 6, unique ≥ 15). `None` when the curve has no spread.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub peak_z: Option<f64>,
    /// `peak_r − second_peak_r` — prominence of the chosen lag over the tallest competing local max.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prominence: Option<f64>,
    /// `|lag(peak) − lag(second peak)|` in ms — spacing to the tallest rival. A spacing that recurs across
    /// gaps is the content's periodicity period (judge the peak *given* it); scattered ⇒ unique.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub top2_spacing_ms: Option<f64>,
    pub peak_lag_samples: i64,
    /// Parabolic-interpolated (possibly fractional) lag of the peak.
    pub frac_lag_samples: f64,
    pub frac_lag_ms: f64,
    /// **Search-exhausted guard:** the integer peak sits at (or within [`LAG_EDGE_TOL_MS`] of) the edge of
    /// the searched lag range, so the true optimum may lie *beyond* `±max_lag`. `frac_lag_ms` / `peak_r`
    /// are then a lower bound clipped by the window, not the real registration — GIGO for `splice.step_ms`
    /// (ledger A5/C6). `None` for fingerprints written before this field. Widen the sweep to clear it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub edge_pinned: Option<bool>,
    pub verdict: LagVerdict,
}

/// A lag peak within this many ms of the searched boundary is treated as edge-pinned (search-exhausted):
/// the parabolic vertex can't be trusted and a larger lag was never scored. See [`LagSummary::edge_pinned`].
const LAG_EDGE_TOL_MS: f64 = 2.0;

/// Search knobs for a per-shoulder lag correlation sweep (`lag_side_sweep` / [`lag_pair`]).
#[derive(Debug, Clone, Copy)]
struct LagSweepParams {
    window: usize,
    max_lag: i64,
    sample_rate: u32,
    channel: LagChannel,
}

impl LagSweepParams {
    fn win_ms(self) -> u32 {
        ((self.window as f64) * 1000.0 / f64::from(self.sample_rate.max(1))) as u32
    }

    fn max_lag_ms(self) -> u32 {
        let ml = self.max_lag.max(0) as usize;
        ((ml as f64) * 1000.0 / f64::from(self.sample_rate.max(1))) as u32
    }

    fn ml(self) -> usize {
        self.max_lag.max(0) as usize
    }
}

/// One shoulder of a [`lag_pair`] sweep: an A border template vs the B haystack around `anchor_frame`.
#[derive(Debug, Clone, Copy)]
struct LagSideSweep<'a> {
    a_border: &'a [f64],
    b_signal: &'a [f64],
    anchor_frame: usize,
    /// `true` for the pre shoulder (last `window` samples of `a_border`); `false` for post (first `window`).
    pre_shoulder: bool,
    /// Samples added to each lag bin in the returned summary (sequential post: gross map to `b_mapped_end`).
    gross_lag_shift: i64,
}

/// Same-master confirmation at the decision seam: how deeply B cancels A (least-squares residual, dB)
/// versus the measured noise floor. `chosen_*_db ≤ floor_*_db` with `informative` ⇒ genuine same source
/// (the strong test, beyond mere correlation). A shallow residual above the floor ⇒ B differs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResidualInfo {
    pub chosen_pre_db: f64,
    pub chosen_post_db: f64,
    pub floor_pre_db: f64,
    pub floor_post_db: f64,
    /// The noise floor established cancellation on every measured side — the residual is interpretable.
    pub informative: bool,
}

/// **Donor-interior energy** over the B span mapped to fill the gap. The gap is a hole in A; this measures
/// whether **B carries audio there** — the donor half of the fill predicate (§3/§4). `silence_fraction` /
/// `longest_silence_ms` come from 50 ms RMS bins vs the gap floor; `continuous` ⇒ no internal sub-floor run
/// longer than `DONOR_CONTINUITY_MS`, i.e. B bridges the hole unbroken.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DonorInterior {
    pub rms_db: f64,
    pub silence_fraction: f64,
    pub longest_silence_ms: f64,
    pub continuous: bool,
}

const DONOR_BIN_MS: f64 = 50.0;
/// A donor with no internal sub-floor run longer than this is treated as continuous (bridges the gap).
const DONOR_CONTINUITY_MS: f64 = 150.0;

/// Donor-interior energy of `b_mono` over the **aligned** bridge span `[start_frame, end_frame)` (B's
/// audio that would fill A's hole). Callers pass the sequentially-registered shoulders
/// (`b_mapped_start + L_pre`, `b_mapped_end + L_post_gross`, ledger A2 sequential registration),
/// not the naive A-length span, so continuity metrics reflect the true bridge even when `D_B != D_A`.
/// `None` for an empty/over-range span.
fn donor_interior_at(b_mono: &[f64], start_frame: usize, end_frame: usize, gap_floor_db: f64, sample_rate: u32) -> Option<DonorInterior> {
    let end = end_frame.min(b_mono.len());
    if start_frame >= end {
        return None;
    }
    let span = &b_mono[start_frame..end];
    let rms = (span.iter().map(|v| v * v).sum::<f64>() / span.len() as f64).sqrt();
    let bin = ((DONOR_BIN_MS / 1000.0) * f64::from(sample_rate)).round().max(1.0) as usize;
    let floor_amp = 10f64.powf(gap_floor_db / 20.0);
    let (mut total, mut silent, mut run, mut longest) = (0usize, 0usize, 0usize, 0usize);
    for chunk in span.chunks(bin) {
        let r = (chunk.iter().map(|v| v * v).sum::<f64>() / chunk.len() as f64).sqrt();
        total += 1;
        if r < floor_amp {
            silent += 1;
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let longest_silence_ms = longest as f64 * DONOR_BIN_MS;
    Some(DonorInterior {
        rms_db: f64::from(to_db(rms as f32)),
        silence_fraction: silent as f64 / total.max(1) as f64,
        longest_silence_ms,
        continuous: longest_silence_ms < DONOR_CONTINUITY_MS,
    })
}

/// First-class splice summary, derived from the per-side `baseline_lag` (mono): the registration `step`
/// (`post_lag − pre_lag`) the length-reconciliation repair acts on, plus the per-side peak and robust
/// uniqueness (`peak_z`) the addressability predicate gates on. Promoted so the gate reads it directly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpliceSummary {
    pub step_ms: f64,
    pub pre_peak_r: f64,
    pub post_peak_r: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_peak_z: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_peak_z: Option<f64>,
    /// **`step_ms` reliability flag:** true when *either* shoulder's `baseline_lag` peak was edge-pinned
    /// (search-exhausted, [`LagSummary::edge_pinned`]). The step is `post_lag − pre_lag`, so a shoulder
    /// whose peak was clipped at the ±`max_lag` boundary makes `step_ms` GIGO (ledger A5/C6). `None` when
    /// neither shoulder carries the flag (older fingerprints).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub edge_pinned: Option<bool>,
}

/// **Dual-fit viability** — the offline repair simulation promoted into the scan (ledger C3/C7; supersedes
/// the `diag_splice_dualfit` harness, which decoded B separately and drifted from the scan). Each shoulder
/// is placed at its own `baseline_lag` (`b_mapped_start + L_pre`, `b_mapped_end + L_post_gross`) and the
/// pre/post seams are scored at lag 0 against the gate thresholds — i.e. *would a length-reconciled fill
/// pass the gate?* The trim/pad is interior, so it does not move the shoulder seams: `trim_frames`
/// (`bridge − gap`) equals the registration step in frames. Computed on the scan's own decode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpliceDualfit {
    pub pre_seam_r: f64,
    pub post_seam_r: f64,
    pub gap_frames: usize,
    pub bridge_frames: i64,
    /// `bridge − gap`: >0 ⇒ trim, <0 ⇒ pad. Equals the registration step in frames.
    pub trim_frames: i64,
    /// Does `min(pre, post)` clear both `min_fill_correlation` and `fill_absolute_floor`?
    pub gate_pass: bool,
    /// **Validator — is the step necessary?** Post seam scored at the *pre* offset (step forced to 0).
    /// If this clears the gate too, a single constant shift suffices and the reported step is a
    /// registration artifact, not a real splice. If only `post_seam_r` (own lag) passes, the step is real.
    pub post_seam_global_r: f64,
    /// **Validator — is the seam unique?** Prominence of each seam's placement peak over its best rival
    /// within ±30 ms. Low prominence ⇒ the seam correlates at many lags (periodic/alias), so a PASS is not
    /// a trustworthy registration. `None` when the sweep window is out of range.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_prom: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_prom: Option<f64>,
    /// **Alias guard** — whole-curve z-score of each seam's peak over the ±`SEAM_LOCAL_SEARCH_MS` search.
    /// A wide search that locked onto a far *periodic* rival (rather than the true registration) reads low.
    /// Periodicity-robust (unlike the ±30 ms `*_seam_prom`, which flags correct-but-periodic content — 5·g6).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_z: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_z: Option<f64>,
}

/// ± lag sweep (ms) used to gauge each dual-fit seam's peak uniqueness (the periodicity/alias guard).
const DUALFIT_SEAM_UNIQ_LAG_MS: f64 = 30.0;

/// Half-width (ms) of the per-shoulder **seam-local** lag search in [`splice_dualfit_at`], anchored on the
/// **nominal `b_mapped`** (not the gross 1 s `baseline_lag`). The seam defines its own placement, so the
/// search must cover the full registration range: a gross lag that locked onto distant content (7·g3: 1 s
/// pre lag −319 ms but the seam is at +18 ms) would otherwise clip a live seam. Set to the `baseline_lag`
/// range so anything the 1 s sweep could register, the seam sweep can too. `peak_z` on the seam curve is the
/// alias guard against a wide search locking onto a far periodic peak. Calibrate at ledger A5/C6.
const SEAM_LOCAL_SEARCH_MS: f64 = 600.0;

/// Prominence of a seam's placement peak over its tallest rival within ±`max_lag`. `b_ctx` must be laid
/// out so lag 0 aligns `a_win` at the placement (`b_ctx[max_lag .. max_lag + a_win.len()]`). Low prominence
/// ⇒ the seam matches at many lags (periodic) ⇒ the dual-fit placement is not a unique registration.
fn seam_prominence(a_win: &[f64], b_ctx: &[f64], max_lag: i64, sample_rate: u32) -> Option<f64> {
    let curve = lag_correlation_curve(a_win, b_ctx, max_lag);
    summarize_lag_curve(&curve, sample_rate, 0, 0, LagChannel::Mono).and_then(|s| s.prominence)
}

/// One side of the wide-envelope confirmer: the 100 ms-bin RMS-envelope lag peak. Its `peak_lag_ms` should
/// agree with the fine-waveform peak lag (segment identity at macro scale; §3.6a). `prominence` is the
/// margin over the tallest rival envelope peak.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EnvPeak {
    pub peak_r: f64,
    pub peak_lag_ms: f64,
    pub prominence: f64,
}

/// Pre/post wide-envelope confirmer at the decision seam.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WideEnvelopeFingerprint {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre: Option<EnvPeak>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post: Option<EnvPeak>,
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

/// `lag_correlation_curve` + `seam_local_peak` moved to the shared `domain::seam_local` so the production
/// dual-fit repair (A3) and this diagnostic scan use one implementation (no drift). Re-exported here so the
/// existing call sites / tests keep their paths.
pub use crate::domain::seam_local::{lag_correlation_curve, seam_local_peak};

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
    // Robust uniqueness from curve shape: prominence + spacing to the tallest rival, and the peak's
    // z-score over the whole curve (§3.6a — the metric that separates registration from periodicity).
    let second = secondary_peak(curve, pi);
    let n = curve.len() as f64;
    let mean = curve.iter().map(|(_, r)| *r).sum::<f64>() / n.max(1.0);
    let std = (curve.iter().map(|(_, r)| (r - mean).powi(2)).sum::<f64>() / n.max(1.0)).sqrt();
    // Edge-pin: the peak lands at (or within tol of) the boundary of the *searched* lags. Read the
    // boundary from the curve itself (`lag_correlation_curve` masks high-side lags where the window runs
    // past B), so this is the true searched extent, not the nominal `±max_lag`.
    let tol = ((LAG_EDGE_TOL_MS / 1000.0) * rate).round() as i64;
    let lo_lag = curve.first().map(|(l, _)| *l).unwrap_or(peak_lag);
    let hi_lag = curve.last().map(|(l, _)| *l).unwrap_or(peak_lag);
    let edge_pinned = (peak_lag - lo_lag) <= tol || (hi_lag - peak_lag) <= tol;
    Some(LagSummary {
        window_ms,
        max_lag_ms,
        channel,
        lag0_r: finite_corr(lag0_r),
        peak_r: finite_corr(peak_r),
        second_peak_r: second.map(|(_, r)| r),
        peak_z: (std > 1e-9).then(|| (peak_r - mean) / std),
        prominence: second.map(|(_, r)| peak_r - r),
        top2_spacing_ms: second.map(|(lag, _)| (peak_lag - lag).unsigned_abs() as f64 * 1000.0 / rate),
        peak_lag_samples: peak_lag,
        frac_lag_samples: frac_lag,
        frac_lag_ms: frac_lag * 1000.0 / rate,
        edge_pinned: Some(edge_pinned),
        verdict: classify_lag(lag0_r, frac_r, peak_lag),
    })
}

/// The tallest **competing** local maximum `(lag, r)` that is not the main peak at `peak_index`. Within
/// the main peak's lobe the curve falls off monotonically (no local maxima), so a separate local maximum
/// is a genuine rival lag — the periodicity / ambiguity signal. `None` when the peak is unrivalled.
fn secondary_peak(curve: &[(i64, f64)], peak_index: usize) -> Option<(i64, f64)> {
    let mut best: Option<(i64, f64)> = None;
    for i in 1..curve.len().saturating_sub(1) {
        if i == peak_index {
            continue;
        }
        let (lag, r) = curve[i];
        // Local maximum: ≥ both neighbours (≥ tolerates plateaus without double-counting the main lobe).
        if r >= curve[i - 1].1 && r >= curve[i + 1].1 {
            best = Some(match best {
                Some((_, br)) if br >= r => best.unwrap(),
                _ => (lag, r),
            });
        }
    }
    best
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
            lag_max_lag_ms: 600,
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

const SILENCE_FLOOR_DB: f32 = -120.0;

fn to_db(rms: f32) -> f32 {
    if rms <= 1e-9 {
        SILENCE_FLOOR_DB
    } else {
        20.0 * rms.log10()
    }
}

/// Replace a non-finite dB (e.g. residual cancellation to ~0 ⇒ `-inf`) with the silence floor, so it
/// serializes as a finite number rather than JSON `null` (which breaks strict consumers / the analyzer).
fn finite_db(db: f64) -> f64 {
    if db.is_finite() {
        db
    } else {
        f64::from(SILENCE_FLOOR_DB)
    }
}

/// Replace a non-finite correlation (`normalized_correlation` of a constant/silent window ⇒ `NaN`) with
/// `0.0` — a NaN "no signal" reads as zero correlation. Keeps the JSON finite (no `null` ⇒ no silent
/// whole-gap drop in strict consumers).
fn finite_corr(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
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

/// Inputs for [`place_on_b`] — structure+waveform unified search on the B haystack.
struct PlaceOnBInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_haystack: &'a [f32],
    b_mono: &'a [f64],
    b_ch: &'a [Vec<f64>],
    nominal_fill_start: usize,
    context_frames: usize,
    bin_frames: usize,
    search_radius_frames: usize,
    cfg: &'a FingerprintConfig,
}

/// Structure-best placement of the A bracket `refined` on the B haystack, plus the seam there.
/// Structure-dominant weights so the placement locks to the energy-best (mirrors the production
/// "structure aligns, seam read there" story). `None` if the match degenerates.
fn place_on_b(input: &PlaceOnBInput<'_>) -> Option<PlacementScores> {
    let PlaceOnBInput {
        a_samples,
        channels,
        refined,
        b_haystack,
        b_mono,
        b_ch,
        nominal_fill_start,
        context_frames,
        bin_frames,
        search_radius_frames,
        cfg,
    } = *input;
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

/// B-frame index in the decoded B haystack for the gross A→B map at `a_start_frame`
/// (`geometry.b_mapped_*` — ledger A2 / §3.7).
fn b_mapped_frame_in_haystack(
    a_start_frame: usize,
    sample_rate: f64,
    gap_offset_secs: f64,
    b_extract_start_secs: f64,
) -> usize {
    (((a_start_frame as f64 / sample_rate + gap_offset_secs - b_extract_start_secs) * sample_rate)
        .round()
        .max(0.0)) as usize
}

/// Correlate one A border shoulder against B over ±`params.max_lag` around `side.anchor_frame`.
fn lag_side_sweep(side: LagSideSweep<'_>, params: LagSweepParams) -> Option<LagSummary> {
    let ml = params.ml();
    let w = params.window.min(side.a_border.len());
    let in_range = if side.pre_shoulder {
        w >= 8 && side.anchor_frame >= w + ml
    } else {
        w >= 8 && side.anchor_frame >= ml
    };
    if !in_range {
        return None;
    }
    let (lo, hi) = if side.pre_shoulder {
        let lo = side.anchor_frame - w - ml;
        let hi = (side.anchor_frame + ml).min(side.b_signal.len());
        (lo, hi)
    } else {
        let lo = side.anchor_frame - ml;
        let hi = (side.anchor_frame + w + ml).min(side.b_signal.len());
        (lo, hi)
    };
    if hi <= lo {
        return None;
    }
    let a_win = if side.pre_shoulder {
        &side.a_border[side.a_border.len() - w..]
    } else {
        &side.a_border[..w]
    };
    let curve = lag_correlation_curve(a_win, &side.b_signal[lo..hi], params.max_lag);
    if side.gross_lag_shift == 0 {
        summarize_lag_curve(&curve, params.sample_rate, params.win_ms(), params.max_lag_ms(), params.channel)
    } else {
        let gross_curve: Vec<(i64, f64)> = curve
            .into_iter()
            .map(|(l, r)| (l + side.gross_lag_shift, r))
            .collect();
        summarize_lag_curve(
            &gross_curve,
            params.sample_rate,
            params.win_ms(),
            params.max_lag_ms(),
            params.channel,
        )
    }
}

fn lag_pair(
    a_pre: &[f64],
    a_post: &[f64],
    b_signal: &[f64],
    start_frame: usize,
    gap_frames: usize,
    params: LagSweepParams,
) -> (Option<LagSummary>, Option<LagSummary>) {
    let pre = lag_side_sweep(
        LagSideSweep {
            a_border: a_pre,
            b_signal,
            anchor_frame: start_frame,
            pre_shoulder: true,
            gross_lag_shift: 0,
        },
        params,
    );
    // Sequential per-shoulder registration (ledger A2 sequential registration): center the post
    // search on `start_frame + gap_frames + round(L_pre)`, not the naive `start_frame + gap_frames`.
    // Un-shifted centering forces `|L_pre + (D_B - D_A)|` (clip offset stacked with bridge-length
    // mismatch) into one ±max_lag window; shifting by the measured pre lag isolates the bridge mismatch
    // alone in the post search. The returned post lags are then shifted back so callers keep receiving
    // "gross" lags relative to `start_frame + gap_frames` (`b_mapped_end`) — i.e. `L_post_gross =
    // L_pre + L_post_fine` — for compatibility with existing consumers (`splice_summary_from_lag`,
    // `b_mapped_end`-relative alignment).
    let pre_shift = pre.map(|p| p.frac_lag_samples.round() as i64).unwrap_or(0);
    let post_base = (start_frame as i64 + gap_frames as i64 + pre_shift).max(0) as usize;
    let post = lag_side_sweep(
        LagSideSweep {
            a_border: a_post,
            b_signal,
            anchor_frame: post_base,
            pre_shoulder: false,
            gross_lag_shift: pre_shift,
        },
        params,
    );
    (pre, post)
}

/// Inputs for [`lag_at_placement`] — A/B windows + gate placement for a single lag fingerprint.
struct LagAtPlacementInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &'a [f64],
    b_ch: &'a [Vec<f64>],
    selected: Option<usize>,
    start_frame: usize,
    cfg: &'a FingerprintConfig,
    sample_rate: u32,
}

/// Lag fingerprint for a placement: A's kept border vs the B haystack swept over ±`max_lag`, on the
/// mono downmix **and** the gate-selected channel (where a multichannel failure lives).
fn lag_at_placement(input: &LagAtPlacementInput<'_>) -> LagFingerprint {
    let LagAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        b_ch,
        selected,
        start_frame,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    // §3.6a: uniqueness needs a ~1 s window. Discover that much border (plus the lag-search slack) so the
    // template isn't capped to the ~150 ms `bin_frames*3` used for the structure probe.
    let window = ((cfg.lag_window_secs * f64::from(sample_rate)).round() as usize).max(8);
    let max_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * f64::from(sample_rate)).round() as i64;
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: window + max_lag.max(0) as usize,
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, ch, &border_spec);

    let sweep = LagSweepParams {
        window,
        max_lag,
        sample_rate,
        channel: LagChannel::Mono,
    };

    let mut out = LagFingerprint::default();
    let mut add = |pre: Option<LagSummary>, post: Option<LagSummary>| {
        if let Some(p) = pre {
            out.pre_anchor.push(p);
        }
        if let Some(p) = post {
            out.post_anchor.push(p);
        }
    };

    let (pre, post) = lag_pair(&a_pre, &a_post, b_mono, start_frame, gap_frames, sweep);
    add(pre, post);

    if let Some(sel) = selected {
        if sel < b_ch.len() && sel < a_pre_ch.len() && sel < a_post_ch.len() && !a_pre_ch[sel].is_empty() {
            let (pre, post) = lag_pair(
                &a_pre_ch[sel],
                &a_post_ch[sel],
                &b_ch[sel],
                start_frame,
                gap_frames,
                LagSweepParams {
                    channel: LagChannel::Selected(sel),
                    ..sweep
                },
            );
            add(pre, post);
        }
    }
    out
}

/// `~10 ms`-bin RMS envelope — the encoding-/small-shift-robust representation for the seam probe.
fn fine_rms_envelope(x: &[f64], bin: usize) -> Vec<f64> {
    let bin = bin.max(1);
    x.chunks(bin)
        .map(|c| (c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).sqrt())
        .collect()
}

/// RMS of the **energy-weighted** downmix over interleaved frames `[lo, hi)`. Each channel is weighted by
/// its own energy, so a loud center isn't diluted by quiet surrounds/LFE the way a straight `1/N` mix is
/// (which buried these 5.1 seams 13–15 dB and over-flagged them "quiet" — §3.6a froze level on this mix).
/// Correlation stays on mono; only the level/SNR uses this. `0.0` for an empty/over-range span.
fn weighted_downmix_rms(samples: &[f32], channels: usize, lo: usize, hi: usize) -> f64 {
    let ch = channels.max(1);
    let total_frames = samples.len() / ch;
    let hi = hi.min(total_frames);
    if hi <= lo {
        return 0.0;
    }
    let n = (hi - lo) as f64;
    let mut ms = vec![0.0f64; ch];
    for f in lo..hi {
        let base = f * ch;
        for (c, m) in ms.iter_mut().enumerate() {
            let s = samples.get(base + c).copied().unwrap_or(0.0) as f64;
            *m += s * s;
        }
    }
    let total: f64 = ms.iter().sum();
    if total <= f64::EPSILON {
        return 0.0;
    }
    let weights: Vec<f64> = ms.iter().map(|e| e / total).collect();
    let mut acc = 0.0;
    for f in lo..hi {
        let base = f * ch;
        let y: f64 = weights
            .iter()
            .enumerate()
            .map(|(c, w)| samples.get(base + c).copied().unwrap_or(0.0) as f64 * w)
            .sum();
        acc += y * y;
    }
    (acc / n).sqrt()
}

/// One side's [`SeamProbe`]: waveform Pearson at lag 0, recovery over ±`fine_max_lag`, encoding-robust
/// envelope correlation, and level/SNR. Correlation is on the mono `a_win`; the level/SNR is taken from
/// `level_rms` (the energy-weighted-downmix RMS at the raw seam, computed by the caller). `b_ctx` spans
/// `±fine_max_lag` around the seam (lag 0 ⇒ `b_ctx[fine_max_lag .. + a_win.len()]`).
fn seam_probe_side(a_win: &[f64], b_ctx: &[f64], level_rms: f64, fine_max_lag: i64, sample_rate: u32, fine_bin: usize, gap_floor_db: f64) -> Option<SeamProbe> {
    let w = a_win.len();
    let ml = fine_max_lag.max(0) as usize;
    let rate = f64::from(sample_rate).max(1.0);
    if w < 8 || ml + w > b_ctx.len() {
        return None;
    }
    let curve = lag_correlation_curve(a_win, b_ctx, fine_max_lag);
    if curve.is_empty() {
        return None;
    }
    let waveform_r = curve.iter().find(|(l, _)| *l == 0).map(|(_, r)| *r).unwrap_or(f64::NAN);
    let &(peak_lag, recovered_r) = curve
        .iter()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
    let b0 = &b_ctx[ml..ml + w];
    let envelope_r =
        normalized_correlation(&fine_rms_envelope(a_win, fine_bin), &fine_rms_envelope(b0, fine_bin));
    let bandlimited_r = crate::domain::seam_robust::bandlimited_pearson(
        a_win, b0, sample_rate, crate::domain::seam_robust::BANDLIMITED_CUTOFF_HZ,
    );
    let spectrum_r = crate::domain::seam_robust::spectrum_correlation(a_win, b0);
    let rms_db = f64::from(to_db(level_rms as f32));
    Some(SeamProbe {
        waveform_r: finite_corr(waveform_r),
        recovered_r: finite_corr(recovered_r),
        recovered_lag_ms: peak_lag as f64 * 1000.0 / rate,
        bandlimited_r: finite_corr(bandlimited_r),
        spectrum_r: finite_corr(spectrum_r),
        envelope_r: finite_corr(envelope_r),
        rms_db,
        snr_db: rms_db - gap_floor_db,
    })
}

const SEAM_PROBE_FINE_LAG_MS: f64 = 25.0;
const SEAM_PROBE_ENV_BIN_MS: f64 = 10.0;

/// Pre/post [`SeamProbe`]s at a placement (mono). Built at **`b_mapped`** registration to diagnose a dead
/// waveform seam: recovery (mis-alignment) vs encoding-robust envelope (cross-encoding) vs level/SNR.
/// `post_shift_frames` is the measured pre-side lag (rounded, from `baseline_lag`'s mono pre entry) —
/// the same sequential-registration shift `lag_pair` applies (ledger A2 sequential registration),
/// so the post probe isn't centered on the un-shifted `start_frame + gap_frames` while `baseline_lag`'s
/// post search is. The post fine-lag half-width is also raised to `cfg.lag_max_lag_ms` (from the ±25 ms
/// `SEAM_PROBE_FINE_LAG_MS`) since, even after shifting, the residual search still needs to cover the
/// bridge-length mismatch (`D_B - D_A`), not just fine sub-frame jitter. `recovered_lag_ms` is reported
/// gross-relative (shifted back by `post_shift_frames`) to stay comparable with `baseline_lag`.
struct SeamProbeAtPlacementInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &'a [f64],
    start_frame: usize,
    post_shift_frames: i64,
    bin_frames: usize,
    cfg: &'a FingerprintConfig,
    sample_rate: u32,
    gap_floor_db: f64,
}

fn seam_probe_at_placement(input: &SeamProbeAtPlacementInput<'_>) -> SeamProbeFingerprint {
    let SeamProbeAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        start_frame,
        post_shift_frames,
        bin_frames,
        cfg,
        sample_rate,
        gap_floor_db,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: bin_frames * 3,
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let window = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
    let fine_max_lag = ((SEAM_PROBE_FINE_LAG_MS / 1000.0) * rate).round() as i64;
    let fine_bin = ((SEAM_PROBE_ENV_BIN_MS / 1000.0) * rate).round().max(1.0) as usize;
    let ml = fine_max_lag.max(0) as usize;
    let post_max_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * rate).round() as i64;
    let post_ml = post_max_lag.max(0) as usize;

    let pre = (|| {
        let w = window.min(a_pre.len());
        if w < 8 || start_frame < w + ml {
            return None;
        }
        let lo = start_frame - w - ml;
        let hi = (start_frame + ml).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        // Level over the raw seam span (the w frames ending at the gap), on the energy-weighted downmix.
        let level_rms = weighted_downmix_rms(a_samples, ch, start_frame - w, start_frame);
        seam_probe_side(&a_pre[a_pre.len() - w..], &b_mono[lo..hi], level_rms, fine_max_lag, sample_rate, fine_bin, gap_floor_db)
    })();
    let post = (|| {
        let w = window.min(a_post.len());
        let post_base = (start_frame as i64 + gap_frames as i64 + post_shift_frames).max(0) as usize;
        if w < 8 || post_base < post_ml {
            return None;
        }
        let lo = post_base - post_ml;
        let hi = (post_base + w + post_ml).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        // Level over the raw seam span (the w frames starting at the gap end), energy-weighted downmix.
        let level_rms = weighted_downmix_rms(a_samples, ch, post_base, post_base + w);
        seam_probe_side(&a_post[..w], &b_mono[lo..hi], level_rms, post_max_lag, sample_rate, fine_bin, gap_floor_db)
    })();
    let post_shift_ms = post_shift_frames as f64 * 1000.0 / rate;
    let post = post.map(|mut sp| {
        sp.recovered_lag_ms += post_shift_ms;
        sp
    });
    SeamProbeFingerprint { pre, post }
}

/// Inputs for [`splice_dualfit_at`] — the A/B PCM plus the **nominal** `b_mapped` gap-start frame. The seam
/// search re-anchors on nominal (not the gross `baseline_lag`), so it needs only the geometry anchor; the
/// pre shoulder butts at `b_mapped_start`, the post at `b_mapped_start + gap_frames`.
struct SpliceDualfitInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &'a [f64],
    /// Nominal geometry `b_mapped` gap-start frame (no lag adjustment).
    b_mapped_start: usize,
    cfg: &'a FingerprintConfig,
    sample_rate: u32,
}

/// Dual-fit viability: score the pre/post seams at lag 0 with each shoulder at its own baseline lag, over
/// the gate's own `fill_seam_search_secs` window, and gate on `min(pre, post)`. The interior trim/pad does
/// not touch the shoulder seams, so this is exactly "would the length-reconciled fill pass?" — the C3/C7
/// question, on the scan's decode. `None` when either seam window falls out of range.
fn splice_dualfit_at(input: &SpliceDualfitInput<'_>) -> Option<SpliceDualfit> {
    let SpliceDualfitInput {
        a_samples,
        channels,
        refined,
        b_mono,
        b_mapped_start,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    let window = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: window,
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);

    let w_pre = window.min(a_pre.len());
    let w_post = window.min(a_post.len());
    let max_lag = (((SEAM_LOCAL_SEARCH_MS / 1000.0) * rate).round() as usize).max(1);

    // Seam-local viability, **re-anchored on nominal `b_mapped`**: the fill places each shoulder at the lag
    // that maximizes ITS own seam, so search ±`max_lag` around the nominal shoulder (pre butts at
    // `b_mapped_start`, post at `b_mapped_start + gap_frames`) and take the peak. Anchoring on the gross 1 s
    // `baseline_lag` (the prior behavior) clipped seams whose lag diverges far from the 1 s peak — e.g. 7·g3,
    // pre 1 s lag −319 ms but the seam is at +18 ms, outside any narrow window around the gross placement.
    let b_pre_nominal = b_mapped_start;
    let b_post_nominal = b_mapped_start + gap_frames;
    let pre_start = b_pre_nominal.checked_sub(w_pre)?;
    let (pre_seam_r, pre_lag, pre_seam_z) =
        seam_local_peak(&a_pre[a_pre.len() - w_pre..], b_mono, pre_start, max_lag)?;
    let (post_seam_r, post_lag, post_seam_z) =
        seam_local_peak(&a_post[..w_post], b_mono, b_post_nominal, max_lag)?;

    let pre_seam_r = finite_corr(pre_seam_r);
    let post_seam_r = finite_corr(post_seam_r);
    // The seam-local shoulder placements (nominal ± the per-seam search) define the bridge + step.
    let b_pre_seam = (b_pre_nominal as i64 + pre_lag).max(0) as usize;
    let b_post_seam = (b_post_nominal as i64 + post_lag).max(0) as usize;
    let bridge_frames = b_post_seam as i64 - b_pre_seam as i64;
    let smin = pre_seam_r.min(post_seam_r);
    let gate_pass =
        smin >= f64::from(cfg.min_fill_correlation) && smin >= f64::from(cfg.fill_absolute_floor);

    // Validator 1 — is the step necessary? Post seam at the PRE shoulder's seam-local lag (step forced 0):
    // if the post seam also clears there, one constant shift fixes both ⇒ registration artifact, not a splice.
    let b_post_global = b_pre_seam + gap_frames;
    let post_seam_global_r = (w_post >= 8 && b_post_global + w_post <= b_mono.len())
        .then(|| finite_corr(normalized_correlation(&a_post[..w_post], &b_mono[b_post_global..b_post_global + w_post])))
        .unwrap_or(f64::NAN);

    // Validator 2 — is each seam a unique (non-periodic) match? Prominence of the placement peak over its
    // best rival within ±30 ms.
    let ml = ((DUALFIT_SEAM_UNIQ_LAG_MS / 1000.0) * rate).round() as i64;
    let mlu = ml.max(0) as usize;
    let pre_seam_prom = (w_pre >= 8 && b_pre_seam >= w_pre + mlu && b_pre_seam + mlu <= b_mono.len())
        .then(|| seam_prominence(&a_pre[a_pre.len() - w_pre..], &b_mono[b_pre_seam - w_pre - mlu..b_pre_seam + mlu], ml, sample_rate))
        .flatten();
    let post_seam_prom = (w_post >= 8 && b_post_seam >= mlu && b_post_seam + w_post + mlu <= b_mono.len())
        .then(|| seam_prominence(&a_post[..w_post], &b_mono[b_post_seam - mlu..b_post_seam + w_post + mlu], ml, sample_rate))
        .flatten();

    Some(SpliceDualfit {
        pre_seam_r,
        post_seam_r,
        gap_frames,
        bridge_frames,
        trim_frames: bridge_frames - gap_frames as i64,
        gate_pass,
        post_seam_global_r,
        pre_seam_prom,
        post_seam_prom,
        pre_seam_z,
        post_seam_z,
    })
}

/// §3.6a frozen wide-envelope confirmer: 100 ms RMS bins, 2 s seam window, ±400 ms lag on the envelope.
const WIDE_ENV_BIN_MS: f64 = 100.0;
const WIDE_ENV_WINDOW_SECS: f64 = 2.0;
const WIDE_ENV_MAX_LAG_MS: f64 = 400.0;

/// Mono [`LagSummary`] for one side of a [`LagFingerprint`].
fn mono_lag_side<'a>(lag: &'a LagFingerprint, pre: bool) -> Option<&'a LagSummary> {
    let entries = if pre { &lag.pre_anchor } else { &lag.post_anchor };
    entries.iter().find(|s| s.channel == LagChannel::Mono)
}

/// First-class splice summary from decision-seam `baseline_lag` (mono): step + per-side peaks / `peak_z`.
pub fn splice_summary_from_lag(lag: &LagFingerprint) -> Option<SpliceSummary> {
    let pre = mono_lag_side(lag, true)?;
    let post = mono_lag_side(lag, false)?;
    // `step` spans both shoulders, so either shoulder being search-exhausted taints it. `None` only when
    // neither side recorded the flag (pre-edge-pin fingerprints).
    let edge_pinned = match (pre.edge_pinned, post.edge_pinned) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(false) || b.unwrap_or(false)),
    };
    Some(SpliceSummary {
        step_ms: post.frac_lag_ms - pre.frac_lag_ms,
        pre_peak_r: pre.peak_r,
        post_peak_r: post.peak_r,
        pre_peak_z: pre.peak_z,
        post_peak_z: post.peak_z,
        edge_pinned,
    })
}

/// One side of the wide-envelope segment-identity confirmer: bucketed RMS envelope lag peak + prominence.
fn wide_envelope_side(
    a_win: &[f64],
    b_wave_ctx: &[f64],
    sample_rate: u32,
    env_bin: usize,
    wide_lag_samples: usize,
) -> Option<EnvPeak> {
    if a_win.len() < env_bin.max(8) {
        return None;
    }
    let rate = f64::from(sample_rate).max(1.0);
    let ea = fine_rms_envelope(a_win, env_bin);
    let eb = fine_rms_envelope(b_wave_ctx, env_bin);
    if ea.len() < 3 {
        return None;
    }
    let env_max_lag = (wide_lag_samples / env_bin.max(1)) as i64;
    if env_max_lag < 1 {
        return None;
    }
    let curve = lag_correlation_curve(&ea, &eb, env_max_lag);
    if curve.is_empty() {
        return None;
    }
    let (pi, &(peak_lag_bin, peak_r)) = curve
        .iter()
        .enumerate()
        .max_by(|(_, (_, x)), (_, (_, y))| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))?;
    let prominence = secondary_peak(&curve, pi).map(|(_, r)| peak_r - r).unwrap_or(0.0);
    let peak_lag_ms = peak_lag_bin as f64 * env_bin as f64 * 1000.0 / rate;
    Some(EnvPeak {
        peak_r: finite_corr(peak_r),
        peak_lag_ms,
        prominence: finite_corr(prominence),
    })
}

/// Pre/post wide-envelope confirmers at **`b_mapped`** registration — cross-scale check vs `baseline_lag`.
/// `post_shift_frames` mirrors `lag_pair`'s sequential centering (ledger A2): the post window is centered on `start_frame + gap_frames + post_shift_frames`, and its
/// search half-width is raised to `cfg.lag_max_lag_ms` (aligned with `baseline_lag`, not the frozen
/// ±400 ms `WIDE_ENV_MAX_LAG_MS`) so it can still resolve the bridge-length mismatch after shifting.
/// `peak_lag_ms` is reported gross-relative for comparability with `baseline_lag`.
struct WideEnvelopeAtPlacementInput<'a> {
    a_samples: &'a [f32],
    channels: usize,
    refined: RefinedGapFrames,
    b_mono: &'a [f64],
    start_frame: usize,
    post_shift_frames: i64,
    cfg: &'a FingerprintConfig,
    sample_rate: u32,
}

fn wide_envelope_at_placement(input: &WideEnvelopeAtPlacementInput<'_>) -> WideEnvelopeFingerprint {
    let WideEnvelopeAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        start_frame,
        post_shift_frames,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    let window = ((WIDE_ENV_WINDOW_SECS * rate).round() as usize).max(8);
    let wide_lag = ((WIDE_ENV_MAX_LAG_MS / 1000.0) * rate).round() as usize;
    let post_wide_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * rate).round() as usize;
    let env_bin = ((WIDE_ENV_BIN_MS / 1000.0) * rate).round().max(1.0) as usize;
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: (window + wide_lag).max(window + post_wide_lag),
        border_standoff_frames: 0,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_rms_floor: cfg.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);

    let pre = (|| {
        let w = window.min(a_pre.len());
        if w < env_bin || start_frame < w + wide_lag {
            return None;
        }
        let lo = start_frame.saturating_sub(w + wide_lag);
        let hi = (start_frame + wide_lag).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        wide_envelope_side(
            &a_pre[a_pre.len() - w..],
            &b_mono[lo..hi],
            sample_rate,
            env_bin,
            wide_lag,
        )
    })();
    let post = (|| {
        let w = window.min(a_post.len());
        let post_base = (start_frame as i64 + gap_frames as i64 + post_shift_frames).max(0) as usize;
        if w < env_bin || post_base < post_wide_lag {
            return None;
        }
        let lo = post_base.saturating_sub(post_wide_lag);
        let hi = (post_base + w + post_wide_lag).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        wide_envelope_side(&a_post[..w], &b_mono[lo..hi], sample_rate, env_bin, post_wide_lag)
    })();
    let post_shift_ms = post_shift_frames as f64 * 1000.0 / rate;
    let post = post.map(|mut ep| {
        ep.peak_lag_ms += post_shift_ms;
        ep
    });
    WideEnvelopeFingerprint { pre, post }
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
    let levels = level_profile(
        |f, end| mono_rms(inputs.a_samples, ch, f, end),
        LevelProfileSpan {
            gap_start: refined.start_frame,
            gap_end: refined.end_frame,
            context_start: pre_start,
            context_end: post_end,
        },
        bin_frames,
        cfg.gap_signature_bin_ms as u32,
    );

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
    // Per-bracket scoring in Full tier; Summary tier only needs baseline structure/seam at `b_mapped`.
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
        let b_mapped_start = |a_start_frame: usize| -> usize {
            b_mapped_frame_in_haystack(
                a_start_frame,
                rate,
                gap_offset_secs,
                inputs.b_extract_start_secs,
            )
        };

        if let Some(base) = place_on_b(&PlaceOnBInput {
            a_samples: inputs.a_samples,
            channels: ch,
            refined,
            b_haystack,
            b_mono: &b_mono,
            b_ch: &b_ch,
            nominal_fill_start: b_mapped_start(refined.start_frame),
            context_frames,
            bin_frames,
            search_radius_frames,
            cfg,
        }) {
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
            // Decision-seam lag at `b_mapped` nominal + ±600 ms sweep (ledger A2) — not structure throat.
            baseline_lag = Some(lag_at_placement(&LagAtPlacementInput {
                a_samples: inputs.a_samples,
                channels: ch,
                refined,
                b_mono: &b_mono,
                b_ch: &b_ch,
                selected: None,
                start_frame: b_mapped_start(refined.start_frame),
                cfg,
                sample_rate: inputs.sample_rate,
            }));
            // Per-bracket scoring; remember the best-structure energy-peak bracket's placement for lag.
            let mut best: Option<(f64, usize, RefinedGapFrames, Option<usize>)> = None;
            for (i, br) in raw_brackets.iter().enumerate() {
                let refined_b = br.refined;
                if let Some(p) = place_on_b(&PlaceOnBInput {
                    a_samples: inputs.a_samples,
                    channels: ch,
                    refined: refined_b,
                    b_haystack,
                    b_mono: &b_mono,
                    b_ch: &b_ch,
                    nominal_fill_start: b_mapped_start(refined_b.start_frame),
                    context_frames,
                    bin_frames,
                    search_radius_frames,
                    cfg,
                }) {
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
                lag = Some(lag_at_placement(&LagAtPlacementInput {
                    a_samples: inputs.a_samples,
                    channels: ch,
                    refined: refined_b,
                    b_mono: &b_mono,
                    b_ch: &b_ch,
                    selected,
                    start_frame,
                    cfg,
                    sample_rate: inputs.sample_rate,
                }));
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
        residual: None,
        seam_probe: None,
        donor_interior: None,
        donor_interior_nominal: None,
        b_levels: None,
        splice: None,
        wide_envelope: None,
        splice_dualfit: None,
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
            // `b_mapped_start`/`b_mapped_end` share a single rigid per-gap offset (gap_offset_secs) —
            // pre/post are NOT independently registered. The full-corpus rescan showed real steps up
            // to 322 ms from that nominal, so ±200 ms clips genuine peaks off the search edge and
            // manufactures false one-sided-dead/decorrelated verdicts. 600 ms matches the width already
            // validated offline (SPLICE_EXP_FINE_LAG_MS=600, §1b-i/§3.6).
            lag_max_lag_ms: 600,
            lag_window_secs: 1.0,
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
/// path). Each gap gets its per-bracket `failure_stage` + seam, `b_mapped` registration metrics, outcome,
/// and diagnostic lag from the production gate (`oracle_*`) — **N + 1** unified searches per gap (N
/// brackets via `oracle_score_fit_candidate`, plus optional `place_on_b` for the best-energy diagnostic
/// `lag` field). Only the selected gaps are built; unselected gaps are never characterized.
pub(crate) fn characterize_gaps_with_gate(
    report: &crate::domain::GapReport,
    a_pcm: &clip_sync::MultiChannelPcm,
    b_samples_full: &[f32],
    request: &crate::application::PatchAudioRequest,
    select: &[usize],
    progress: &dyn clip_sync::ProgressReporter,
) -> GapCorpus {
    use crate::application::patch_region::{
        oracle_build_fit_cache, oracle_measure_residual, oracle_score_fit_candidate,
        oracle_throat_structure_frame,
    };

    let sample_rate = a_pcm.sample_rate;
    let channels = a_pcm.channels as usize;
    let cfg = FingerprintConfig::from_request(request, report.silence_peak_fraction);
    // Build only the selected gaps (summary base: baseline structure/seam, no per-bracket, no lag).
    let mut corpus = characterize_gaps(report, &a_pcm.samples, b_samples_full, sample_rate, channels, &cfg, select);

    let mut gate_cfg = crate::application::patch_region::SeamGateConfig::from_repair(
        request,
        sample_rate,
        channels,
        report.silence_peak_fraction,
    );
    // Force the same-source residual probe on for every gap (the fingerprint records it; the gate
    // would otherwise only measure it when the residual gate is active or DEBUG logging is on).
    gate_cfg.measure_residual = true;
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
        let b_mapped_start = b_mapped_frame_in_haystack(
            refined.start_frame,
            rate,
            gap_offset,
            b_extract_start_secs,
        );
        let b_mapped_bracket = |refined_b: RefinedGapFrames| {
            b_mapped_frame_in_haystack(refined_b.start_frame, rate, gap_offset, b_extract_start_secs)
        };

        // Registration metrics at `b_mapped` nominal (ledger A2 / §3.7) — stable gross map + ±600 ms lag
        // sweep, sequentially centered (ledger A2 sequential registration). Correlation/uniqueness
        // is mono (frozen, §3.6a).
        fp.baseline_lag = Some(lag_at_placement(&LagAtPlacementInput {
            a_samples: &a_pcm.samples,
            channels: ch,
            refined,
            b_mono: &b_mono,
            b_ch: &b_ch,
            selected: None,
            start_frame: b_mapped_start,
            cfg: &cfg,
            sample_rate,
        }));
        // L_pre / L_post_gross (mono) drive the same sequential post centering in seam_probe/wide_envelope,
        // and the aligned bridge span for donor_interior — see lag_pair's doc comment for the derivation.
        let pre_shift_frames = fp
            .baseline_lag
            .as_ref()
            .and_then(|l| mono_lag_side(l, true))
            .map(|s| s.frac_lag_samples.round() as i64)
            .unwrap_or(0);
        let post_gross_frames = fp
            .baseline_lag
            .as_ref()
            .and_then(|l| mono_lag_side(l, false))
            .map(|s| s.frac_lag_samples.round() as i64);
        fp.seam_probe = Some(seam_probe_at_placement(&SeamProbeAtPlacementInput {
            a_samples: &a_pcm.samples,
            channels: ch,
            refined,
            b_mono: &b_mono,
            start_frame: b_mapped_start,
            post_shift_frames: pre_shift_frames,
            bin_frames,
            cfg: &cfg,
            sample_rate,
            gap_floor_db: f64::from(fp.levels.gap_floor_db),
        }));
        {
            let b_pre_aligned = (b_mapped_start as i64 + pre_shift_frames).max(0) as usize;
            // Fall back to the naive A-length span when the post lag search found no peak, rather than
            // dropping donor_interior entirely.
            let b_post_aligned = post_gross_frames
                .map(|g| (b_mapped_start as i64 + gap_frames as i64 + g).max(0) as usize)
                .unwrap_or(b_mapped_start + gap_frames);
            fp.donor_interior =
                donor_interior_at(&b_mono, b_pre_aligned, b_post_aligned, f64::from(fp.levels.gap_floor_db), sample_rate);
            // Registration-independent B occupancy + symmetric B level profile at the NOMINAL geometry
            // b_mapped span (no per-shoulder lag) — the "quiet in both masters ⇒ not a dropout" signals (D11).
            let b_gap_end = b_mapped_start + gap_frames;
            fp.donor_interior_nominal =
                donor_interior_at(&b_mono, b_mapped_start, b_gap_end, f64::from(fp.levels.gap_floor_db), sample_rate);
            fp.b_levels = Some(level_profile(
                |f, end| mono_slice_rms(&b_mono, f, end),
                LevelProfileSpan {
                    gap_start: b_mapped_start,
                    gap_end: b_gap_end,
                    context_start: b_mapped_start.saturating_sub(context_frames),
                    context_end: (b_gap_end + context_frames).min(b_mono.len()),
                },
                bin_frames,
                cfg.gap_signature_bin_ms as u32,
            ));
            // Dual-fit viability: the seam search re-anchors on nominal `b_mapped` (not the gross lag), so it
            // runs regardless of whether the gross post lag resolved — each seam finds its own placement.
            fp.splice_dualfit = splice_dualfit_at(&SpliceDualfitInput {
                a_samples: &a_pcm.samples,
                channels: ch,
                refined,
                b_mono: &b_mono,
                b_mapped_start,
                cfg: &cfg,
                sample_rate,
            });
        }
        fp.wide_envelope = Some(wide_envelope_at_placement(&WideEnvelopeAtPlacementInput {
            a_samples: &a_pcm.samples,
            channels: ch,
            refined,
            b_mono: &b_mono,
            start_frame: b_mapped_start,
            post_shift_frames: pre_shift_frames,
            cfg: &cfg,
            sample_rate,
        }));
        if let Some(ref bl) = fp.baseline_lag {
            fp.splice = splice_summary_from_lag(bl);
        }

        // Residual stays at the gate's structure throat (same-source cancellation as the gate scores).
        if let Some(throat_frame) = oracle_throat_structure_frame(&params, &cache, refined) {
            fp.residual = oracle_measure_residual(&params, &cache, refined, throat_frame).map(|v| ResidualInfo {
                chosen_pre_db: finite_db(v.chosen_pre_db),
                chosen_post_db: finite_db(v.chosen_post_db),
                floor_pre_db: finite_db(v.floor_pre_db),
                floor_post_db: finite_db(v.floor_post_db),
                informative: v.informative,
            });
        }

        // Diagnostic lag: one placement search at the best (highest-seam) speech bracket.
        if let Some((_, refined_b)) = best_energy {
            if let Some(p) = place_on_b(&PlaceOnBInput {
                a_samples: &a_pcm.samples,
                channels: ch,
                refined: refined_b,
                b_haystack: b_slice,
                b_mono: &b_mono,
                b_ch: &b_ch,
                nominal_fill_start: b_mapped_bracket(refined_b),
                context_frames,
                bin_frames,
                search_radius_frames,
                cfg: &cfg,
            }) {
                fp.lag = Some(lag_at_placement(&LagAtPlacementInput {
                    a_samples: &a_pcm.samples,
                    channels: ch,
                    refined: refined_b,
                    b_mono: &b_mono,
                    b_ch: &b_ch,
                    selected: p.selected_channels.first().copied(),
                    start_frame: p.start_frame,
                    cfg: &cfg,
                    sample_rate,
                }));
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
    fn lag_summary_flags_competing_peak_for_periodic_curve() {
        // Unique peak: one hump, monotonic falloff → no rival local maximum.
        let unique: Vec<(i64, f64)> = (-5..=5).map(|l| (l, 1.0 - 0.1 * (l as f64).abs())).collect();
        let s = summarize_lag_curve(&unique, 48_000, 10, 1, LagChannel::Mono).expect("summary");
        assert!(
            s.second_peak_r.map_or(true, |r| r < s.peak_r - 0.3),
            "a unique peak should have no strong rival: {:?}",
            s.second_peak_r
        );

        // Periodic-like: two humps of similar height → a rival near peak_r (low uniqueness margin).
        let periodic = vec![
            (-4, 0.2), (-3, 0.9), (-2, 0.5), (-1, 0.3), (0, 0.85), (1, 0.4), (2, 0.2), (3, 0.1), (4, 0.05),
        ];
        let s2 = summarize_lag_curve(&periodic, 48_000, 10, 1, LagChannel::Mono).expect("summary");
        assert!(
            s2.second_peak_r.is_some_and(|r| r > 0.8),
            "a periodic curve should expose a competing peak near peak_r: {:?}",
            s2.second_peak_r
        );

        // Robust uniqueness fields: the unique (monotonic) curve has NO rival → prominence `None` (best
        // case); the periodic one has a rival near the peak → low prominence. peak_z stands out further for
        // the unique curve. Spacing is the lag gap to the rival.
        assert!(s.prominence.is_none(), "a unrivalled peak has no prominence value: {:?}", s.prominence);
        assert!(s2.prominence.is_some_and(|p| p < 0.1), "periodic: low prominence {:?}", s2.prominence);
        // peak_z is computed (finite, positive) whenever the curve has spread; its discriminating power is
        // validated on real flat-floor curves in the §3.6a experiment, not these broad toy humps.
        assert!(s.peak_z.is_some_and(|z| z.is_finite() && z > 0.0), "peak_z computed: {:?}", s.peak_z);
        assert!(s2.peak_z.is_some_and(|z| z.is_finite() && z > 0.0), "peak_z computed: {:?}", s2.peak_z);
        // periodic peak at lag 0, rival at lag −3 → spacing 3 samples = 62.5 µs at 48 kHz.
        assert!(
            s2.top2_spacing_ms.is_some_and(|ms| (ms - 3.0 * 1000.0 / 48_000.0).abs() < 1e-6),
            "spacing to the rival lag: {:?}",
            s2.top2_spacing_ms
        );
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

    /// Pre-fix post search: center on `start_frame + gap_frames` with no pre-shift — stacks
    /// `L_pre + (D_B - D_A)` into one ±max_lag window (ledger A2).
    fn naive_lag_pair_post(
        a_post: &[f64],
        b_signal: &[f64],
        start_frame: usize,
        gap_frames: usize,
        params: LagSweepParams,
    ) -> Option<LagSummary> {
        lag_side_sweep(
            LagSideSweep {
                a_border: a_post,
                b_signal,
                anchor_frame: start_frame + gap_frames,
                pre_shoulder: false,
                gross_lag_shift: 0,
            },
            params,
        )
    }

    /// Regression: when `|L_pre + (D_B - D_A)|` exceeds ±max_lag but `|D_B - D_A|` alone fits,
    /// sequential `lag_pair` finds the post shoulder while naive centering does not.
    #[test]
    fn lag_pair_sequential_decouples_pre_offset_from_bridge_mismatch() {
        const RATE: u32 = 48_000;
        let w = 4_000usize;
        let max_lag = 4_000i64;
        let ml = max_lag as usize;
        let start_frame = 30_000usize;
        let gap_frames = 20_000usize; // D_A — keep post burst out of the pre search window
        let l_pre_true = 3_000i64;
        let bridge_delta = 1_500i64; // |L_pre + bridge| = 4500 > max_lag; |bridge| = 1500 <= max_lag

        let b_post_match = start_frame as i64 + l_pre_true + gap_frames as i64 + bridge_delta;
        let naive_lag_needed = b_post_match - (start_frame + gap_frames) as i64;
        assert_eq!(naive_lag_needed, l_pre_true + bridge_delta);
        assert!(naive_lag_needed.unsigned_abs() > max_lag as u64);

        let mut b_signal = vec![0.0f64; 100_000];

        let (a_pre, pre_ctx) = shifted_pair(l_pre_true as f64, w, max_lag);
        let pre_lo = start_frame.saturating_sub(w + ml);
        for (i, &v) in pre_ctx.iter().enumerate() {
            if pre_lo + i < b_signal.len() {
                b_signal[pre_lo + i] = v;
            }
        }

        let (a_post, post_ctx) = shifted_pair(bridge_delta as f64, w, max_lag);
        let seq_post_base = start_frame as i64 + gap_frames as i64 + l_pre_true;
        let post_lo = (seq_post_base - ml as i64).max(0) as usize;
        for (i, &v) in post_ctx.iter().enumerate() {
            if post_lo + i < b_signal.len() {
                b_signal[post_lo + i] = v;
            }
        }

        let sweep = LagSweepParams {
            window: w,
            max_lag,
            sample_rate: RATE,
            channel: LagChannel::Mono,
        };

        let (pre, post_seq) = lag_pair(&a_pre, &a_post, &b_signal, start_frame, gap_frames, sweep);
        let pre = pre.expect("pre shoulder");
        let post_seq = post_seq.expect("sequential post shoulder");
        assert!(
            (pre.frac_lag_samples - l_pre_true as f64).abs() < 5.0,
            "pre lag {} should land near {l_pre_true}",
            pre.frac_lag_samples
        );
        assert!(pre.peak_r > 0.95, "pre peak_r {}", pre.peak_r);

        let gross_post_expected = l_pre_true + bridge_delta;
        assert!(
            (post_seq.frac_lag_samples - gross_post_expected as f64).abs() < 5.0,
            "gross post lag {} should land near {gross_post_expected}",
            post_seq.frac_lag_samples
        );
        assert!(post_seq.peak_r > 0.95, "sequential post peak_r {}", post_seq.peak_r);

        let post_naive = naive_lag_pair_post(&a_post, &b_signal, start_frame, gap_frames, sweep)
            .expect("naive post summary");
        assert!(
            post_naive.peak_r < post_seq.peak_r - 0.15,
            "naive post should be worse than sequential (naive r {} at lag {} vs sequential r {})",
            post_naive.peak_r,
            post_naive.frac_lag_samples,
            post_seq.peak_r
        );
    }

    #[test]
    fn weighted_downmix_recovers_center_dominant_level() {
        // 6ch center-dominant 5.1 seam: loud center (idx 2), quiet L/R, silent surrounds/LFE.
        let ch = 6;
        let n = 480;
        let mut s = vec![0.0f32; n * ch];
        for f in 0..n {
            s[f * ch + 2] = (std::f64::consts::TAU * 200.0 * f as f64 / 48_000.0).sin() as f32 * 0.5;
            s[f * ch] = 0.005;
            s[f * ch + 1] = -0.005;
        }
        let weighted = weighted_downmix_rms(&s, ch, 0, n);
        let mono: Vec<f64> = s.chunks(ch).map(|fr| fr.iter().map(|&x| x as f64).sum::<f64>() / ch as f64).collect();
        let mono_rms = (mono.iter().map(|v| v * v).sum::<f64>() / mono.len() as f64).sqrt();
        // The straight 1/6 mix buries the center; the energy-weighted mix keeps it (~0.5/√2 ≈ 0.35).
        assert!(weighted > 0.2, "weighted preserves center level: {weighted}");
        assert!(weighted > mono_rms * 3.0, "weighted {weighted} ≫ straight mono {mono_rms}");
        // Over-range / empty spans are guarded.
        assert_eq!(weighted_downmix_rms(&s, ch, 10, 10), 0.0);
        assert_eq!(weighted_downmix_rms(&s, ch, n - 5, n + 100), weighted_downmix_rms(&s, ch, n - 5, n));
    }

    #[test]
    fn donor_interior_detects_bridge_vs_hole() {
        let sr = 48_000u32;
        let floor_db = -60.0;
        let half = sr as usize / 2; // 0.5 s span
        let tone: Vec<f64> =
            (0..sr as usize).map(|i| (std::f64::consts::TAU * 200.0 * i as f64 / 48_000.0).sin() * 0.3).collect();

        // Continuous donor: B carries audio across the whole span → bridges the hole.
        let d = donor_interior_at(&tone, 0, half, floor_db, sr).expect("donor");
        assert!(d.continuous && d.silence_fraction < 0.05, "bridged donor: {d:?}");
        assert!(d.rms_db > floor_db);

        // Donor with its OWN hole: 250 ms of silence inside the span breaks continuity.
        let mut holed = tone.clone();
        for v in holed.iter_mut().take(half).skip(sr as usize / 4) {
            *v = 0.0;
        }
        let d2 = donor_interior_at(&holed, 0, half, floor_db, sr).expect("donor");
        assert!(!d2.continuous, "internal silence breaks continuity: {d2:?}");
        assert!(d2.longest_silence_ms >= DONOR_CONTINUITY_MS, "{d2:?}");

        // Empty / over-range spans guarded.
        assert!(donor_interior_at(&tone, 10, 0, floor_db, sr).is_none());
    }

    #[test]
    fn splice_summary_from_baseline_lag_mono() {
        let lag = LagFingerprint {
            pre_anchor: vec![LagSummary {
                window_ms: 1000,
                max_lag_ms: 200,
                channel: LagChannel::Mono,
                lag0_r: 0.1,
                peak_r: 0.99,
                second_peak_r: Some(0.2),
                peak_z: Some(16.0),
                prominence: Some(0.79),
                top2_spacing_ms: Some(40.0),
                peak_lag_samples: -500,
                frac_lag_samples: -500.0,
                frac_lag_ms: -10.5,
                edge_pinned: Some(false),
                verdict: LagVerdict::TimingOffset,
            }],
            post_anchor: vec![LagSummary {
                window_ms: 1000,
                max_lag_ms: 200,
                channel: LagChannel::Mono,
                lag0_r: 0.2,
                peak_r: 0.96,
                second_peak_r: Some(0.3),
                peak_z: Some(14.0),
                prominence: Some(0.66),
                top2_spacing_ms: Some(50.0),
                peak_lag_samples: -300,
                frac_lag_samples: -300.0,
                frac_lag_ms: -6.2,
                edge_pinned: Some(false),
                verdict: LagVerdict::TimingOffset,
            }],
        };
        let s = splice_summary_from_lag(&lag).expect("splice");
        assert!((s.step_ms - 4.3).abs() < 0.01, "step {}", s.step_ms);
        assert!((s.pre_peak_r - 0.99).abs() < 1e-6);
        assert!((s.post_peak_r - 0.96).abs() < 1e-6);
        assert_eq!(s.pre_peak_z, Some(16.0));
        assert_eq!(s.post_peak_z, Some(14.0));
        // Neither shoulder search-exhausted → step is trustworthy.
        assert_eq!(s.edge_pinned, Some(false));

        // One edge-pinned shoulder taints the step (either side ⇒ true).
        let mut pinned = lag.clone();
        pinned.post_anchor[0].edge_pinned = Some(true);
        assert_eq!(
            splice_summary_from_lag(&pinned).expect("splice").edge_pinned,
            Some(true),
        );

        // No shoulder carries the flag (old fingerprint) → unknown, not a false negative.
        let mut legacy = lag.clone();
        legacy.pre_anchor[0].edge_pinned = None;
        legacy.post_anchor[0].edge_pinned = None;
        assert_eq!(
            splice_summary_from_lag(&legacy).expect("splice").edge_pinned,
            None,
        );
    }

    #[test]
    fn edge_pinned_flags_boundary_peak() {
        // A curve whose maximum sits at the top boundary lag is search-exhausted (true optimum ≥ edge).
        let sr = 48_000u32;
        let curve: Vec<(i64, f64)> = (-4800..=4800).map(|l| (l, l as f64 / 4800.0)).collect();
        let s = summarize_lag_curve(&curve, sr, 1000, 100, LagChannel::Mono).expect("summary");
        assert_eq!(s.peak_lag_samples, 4800, "peak at the boundary");
        assert_eq!(s.edge_pinned, Some(true), "boundary peak is edge-pinned");

        // A curve peaking in the interior is not edge-pinned.
        let interior: Vec<(i64, f64)> =
            (-4800..=4800).map(|l| (l, -((l as f64) / 4800.0).powi(2))).collect();
        let s2 = summarize_lag_curve(&interior, sr, 1000, 100, LagChannel::Mono).expect("summary");
        assert_eq!(s2.peak_lag_samples, 0, "peak in the interior");
        assert_eq!(s2.edge_pinned, Some(false), "interior peak not edge-pinned");
    }

    #[test]
    fn wide_envelope_finds_shifted_segment_peak() {
        let rate = 48_000u32;
        let r = f64::from(rate);
        let max_lag = ((WIDE_ENV_MAX_LAG_MS / 1000.0) * r).round() as i64;
        let window = (WIDE_ENV_WINDOW_SECS * r).round() as usize;
        let env_bin = ((WIDE_ENV_BIN_MS / 1000.0) * r).round() as usize;
        // Offset must be visible at 100 ms-bin resolution (~1 bin = 100 ms).
        let offset_samples = env_bin as f64;
        let (a, b_ctx) = shifted_pair(offset_samples, window, max_lag);
        let wide_lag = max_lag as usize;
        let peak = wide_envelope_side(&a, &b_ctx, rate, env_bin, wide_lag).expect("env peak");
        assert!(peak.peak_r > 0.9, "segment match {}", peak.peak_r);
        let expected_ms = offset_samples * 1000.0 / r;
        assert!(
            (peak.peak_lag_ms - expected_ms).abs() < 50.0,
            "env peak lag {} should land near {expected_ms} ms",
            peak.peak_lag_ms
        );
        // Non-vacuous: `finite_corr` guarantees finite outputs even on a flat/degenerate envelope.
        assert!(peak.prominence.is_finite() && peak.peak_r.is_finite(), "finite: {peak:?}");
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
                    second_peak_r: Some(0.20),
                    peak_z: Some(12.5),
                    prominence: Some(0.79),
                    top2_spacing_ms: Some(40.0),
                    peak_lag_samples: -778,
                    frac_lag_samples: -778.0,
                    frac_lag_ms: -16.2,
                    edge_pinned: Some(false),
                    verdict: LagVerdict::TimingOffset,
                }],
                post_anchor: vec![],
            }),
            baseline_lag: None,
            residual: None,
            seam_probe: None,
            donor_interior: None,
            splice: None,
            wide_envelope: None,
            splice_dualfit: None,
            donor_interior_nominal: None,
            b_levels: None,
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
            residual: Some(ResidualInfo {
                chosen_pre_db: -42.0,
                chosen_post_db: -38.0,
                floor_pre_db: -40.0,
                floor_post_db: -39.0,
                informative: true,
            }),
            seam_probe: Some(SeamProbeFingerprint {
                pre: Some(SeamProbe {
                    waveform_r: 0.04,
                    recovered_r: 0.95,
                    recovered_lag_ms: -8.0,
                    bandlimited_r: 0.8,
                    spectrum_r: 0.85,
                    envelope_r: 0.9,
                    rms_db: -30.0,
                    snr_db: 20.0,
                }),
                post: None,
            }),
            donor_interior: Some(DonorInterior {
                rms_db: -28.0,
                silence_fraction: 0.0,
                longest_silence_ms: 0.0,
                continuous: true,
            }),
            splice: Some(SpliceSummary {
                step_ms: 4.2,
                pre_peak_r: 0.99,
                post_peak_r: 0.96,
                pre_peak_z: Some(15.0),
                post_peak_z: Some(14.0),
                edge_pinned: Some(false),
            }),
            wide_envelope: Some(WideEnvelopeFingerprint {
                pre: Some(EnvPeak {
                    peak_r: 0.98,
                    peak_lag_ms: -110.0,
                    prominence: 0.55,
                }),
                post: None,
            }),
            splice_dualfit: None,
            donor_interior_nominal: None,
            b_levels: None,
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
