//! Licensing-safe serde **schema** for the gap-fingerprint corpus: the per-gap / per-source
//! numeric types (no PCM samples, no transcripts) plus the decoded-audio identity digest
//! ([`source_id`]). Types + identity only — the PCM measurement loops that *fill* these structs
//! live in the sibling `measure` slice. See `docs/dev/archive/TEMP-gap-fingerprint-module-split-plan.md`.

use serde::{Deserialize, Serialize};

/// dBFS sentinel substituted for true-silent RMS so level vectors carry no `-inf` (the value
/// [`LevelProfile::floor_db`] reports). Shared by the schema projection and the PCM measure path.
pub(crate) const SILENCE_FLOOR_DB: f32 = -120.0;

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

pub(crate) fn file_source(samples: &[f32], sample_rate: u32, channels: u16) -> FileSource {
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
    /// Gap-equivalence classification (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate): does this gap need patching?
    /// Derived from the **silence character** — A's gap RMS vs the recording's noise floor (dropout vs room
    /// tone) + donor silence (is B occupied) — the signals that separate real dropouts from mutual/program
    /// silence. This is the **fine reference**: sample-level A RMS + fine-bin noise floor + 50 ms donor bins,
    /// on the full decode. Emitted for tuning/categorizing; the production plan-time drop is a later (v1) step.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub equivalence: Option<crate::domain::gap_equivalence::GapEquivalenceVerdict>,
    /// The **coarse production** equivalence verdict for the same gap — the 250 ms scan-block gate the scan
    /// report carries (`GapReport::gap_equivalence`), copied in so one `--gap-fingerprints` run holds both
    /// granularities per gap for calibration (the `equivalence-calibration` tool diffs `equivalence` vs this).
    /// `None` when the scan did not classify the gap.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scan_equivalence: Option<crate::domain::gap_equivalence::GapEquivalenceVerdict>,
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
    /// B frame where the placement starts, and the fill length the end sweep chose.
    ///
    /// The end search's nominal is this bracket's refined span (`span_secs` /
    /// `params.gap_frames` = post−pre), **not** the original silent-run gap.
    /// `fill_frames` differs from that span by up to `fill_length_slack_secs` (default 1.0 s;
    /// end-search only). Haystack tail is sized by `fill_extract_tail_slack_secs`.
    /// Comparing `fill_frames` to the original gap conflates **anchor widening** with end-search
    /// excursion — the Phase B denominator trap; see docs/dev/archive/TEMP-fill-placement-axis-plan.md.
    /// On the dump path this is the *only* projection of the end search's decision.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub start_frame: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fill_frames: Option<usize>,
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
    #[serde(deserialize_with = "de_null_as_nan")]
    pub baseline_pre: f64,
    #[serde(deserialize_with = "de_null_as_nan")]
    pub baseline_post: f64,
}

/// Read a `Vec<(f64, f64)>` where a dead channel's Pearson was non-finite and serialized as JSON `null`
/// (same reason as [`de_null_as_nan`]); map each `null` back to `NaN` so a corpus fingerprint round-trips.
fn de_pairs_null_as_nan<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<(f64, f64)>, D::Error> {
    let raw: Vec<(Option<f64>, Option<f64>)> = Vec::deserialize(d)?;
    Ok(raw
        .into_iter()
        .map(|(a, b)| (a.unwrap_or(f64::NAN), b.unwrap_or(f64::NAN)))
        .collect())
}

/// Baseline waveform seam correlations, per-channel and selected channels (the gate's view).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeamScores {
    #[serde(deserialize_with = "de_null_as_nan")]
    pub baseline_pre: f64,
    #[serde(deserialize_with = "de_null_as_nan")]
    pub baseline_post: f64,
    pub selected_channels: Vec<usize>,
    #[serde(deserialize_with = "de_pairs_null_as_nan")]
    pub per_channel: Vec<(f64, f64)>,
    #[serde(deserialize_with = "de_null_as_nan")]
    pub mono_pre: f64,
    #[serde(deserialize_with = "de_null_as_nan")]
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

/// Verdict from a lag curve (see `docs/dev/archive/TEMP-gap-fingerprint-plan.md` §4 thresholds).
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
    /// **Search-exhausted guard:** the integer peak sits at (or within `LAG_EDGE_TOL_MS` of) the edge of
    /// the searched lag range, so the true optimum may lie *beyond* `±max_lag`. `frac_lag_ms` / `peak_r`
    /// are then a lower bound clipped by the window, not the real registration — GIGO for `splice.step_ms`
    /// (ledger A5/C6). `None` for fingerprints written before this field. Widen the sweep to clear it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub edge_pinned: Option<bool>,
    pub verdict: LagVerdict,
}

/// Read a residual dB that the writer emits as JSON `null` when it was non-finite (a fully-silent gap cancels
/// to ~0 ⇒ `to_db(0) = -inf`, which serde_json can't represent) back as `NaN`. Without this, deserializing a
/// corpus fingerprint into [`ResidualInfo`] fails on the whole gap (mirrors the harness `Residual` `Option`
/// tolerance). `NaN` round-trips back to `null` on re-serialization, so the reader still reads "unavailable".
fn de_null_as_nan<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
}

/// Same-master confirmation at the decision seam: how deeply B cancels A (least-squares residual, dB)
/// versus the measured noise floor. `chosen_*_db ≤ floor_*_db` with `informative` ⇒ genuine same source
/// (the strong test, beyond mere correlation). A shallow residual above the floor ⇒ B differs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResidualInfo {
    #[serde(deserialize_with = "de_null_as_nan")]
    pub chosen_pre_db: f64,
    #[serde(deserialize_with = "de_null_as_nan")]
    pub chosen_post_db: f64,
    #[serde(deserialize_with = "de_null_as_nan")]
    pub floor_pre_db: f64,
    #[serde(deserialize_with = "de_null_as_nan")]
    pub floor_post_db: f64,
    /// The noise floor established cancellation on every measured side — the residual is interpretable.
    pub informative: bool,
}

/// `DonorInterior` + `donor_interior_at` moved to the shared `domain::donor` so the production dual-fit
/// repair (A3) and this scan use one occupancy implementation. Re-exported for the existing call sites.
pub use crate::domain::donor::{donor_interior_at, DonorInterior};

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
/// is placed with `seam_local_peak` re-anchored on nominal `b_mapped` (pre at `b_mapped_start`, post at
/// `b_mapped_start + gap_frames`, ±`SEAM_LOCAL_SEARCH_MS`) — **not** on the gross 1 s `baseline_lag` —
/// and the pre/post seams are scored at those placements against the gate thresholds — i.e. *would a
/// length-reconciled fill pass the gate?* The trim/pad is interior, so it does not move the shoulder
/// seams: `trim_frames` (`bridge − gap`) equals the registration step in frames. Computed on the scan's
/// own decode.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic PRNG noise sample in [-1, 1) — local to the schema tests (the measure slice keeps
    /// its own copy for its PCM builders).
    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    #[test]
    fn source_id_stable_and_distinguishing() {
        let a: Vec<f32> = (0..10_000).map(|i| noise(7, i) as f32).collect();
        let b: Vec<f32> = (0..10_000).map(|i| noise(8, i) as f32).collect();
        assert_eq!(
            source_id(&a, 48_000, 2),
            source_id(&a, 48_000, 2),
            "deterministic"
        );
        assert_eq!(source_id(&a, 48_000, 2).len(), 16, "16 hex chars");
        assert_ne!(
            source_id(&a, 48_000, 2),
            source_id(&b, 48_000, 2),
            "different audio → different id"
        );
        assert_ne!(
            source_id(&a, 48_000, 2),
            source_id(&a, 44_100, 2),
            "sample rate is part of identity"
        );
        assert_ne!(
            source_id(&a, 48_000, 2),
            source_id(&a, 48_000, 6),
            "channels are part of identity"
        );
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
            equivalence: None,
            scan_equivalence: None,
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
