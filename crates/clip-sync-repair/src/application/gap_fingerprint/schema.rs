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
///
/// **Two coordinate systems live here, and the distinction is load-bearing.** `sample_rate`,
/// `channels`, and `duration_secs` describe the **decoded/analysis** PCM the corpus was measured on —
/// which is *A's* rate and layout **for both sides** when the pair is comparable (B is rate-resampled
/// to A before measurement). When [`SourceMeta::incomparable`] is
/// [`IncomparableReason::ChannelLayoutMismatch`], pairwise measurement was refused: `b_source.channels`
/// / `duration_secs` / `id` use **B's** native layout (rate may still be A's after resample), so the
/// dump does not pretend B was A-layout. The `native_*` fields are the per-side **source** readings
/// taken at probe time, before any conversion. When `native_sample_rate` disagrees with `sample_rate`
/// for B, the dump was measured on a rate-converted signal; see [`FileSource::was_resampled`]. `id`
/// and `duration_secs` are derived from the decoded PCM and must stay that way — `entry_filename`
/// builds every per-gap filename from `id`.
///
/// Provenance fields are raw container/track observations, never verdicts: what could be concluded
/// from them (lossless? clamp-reachable?) is a question for the reader, answered by grouping on
/// `codec`. See `docs/dev/archive/TEMP-fingerprint-provenance-plan.md` § 2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSource {
    pub id: String,
    /// **Reserved and never populated.** `AudioTrack` carries no container field, so filling this
    /// would need a second probe. Kept for wire compatibility with dumps that declared it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub container: Option<String>,
    /// Probe codec name (`aac` / `ac3` / `eac3` / `mp3` / `flac` / `vorbis` / `alac`, else
    /// Symphonia's `Display`). Absent on pre-provenance corpora.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub codec: Option<String>,
    /// Source sample format: `s16` | `s24` | `s32` | `f32` | `other:<bits>`. Absent when the
    /// container reported neither `bits_per_sample` nor `sample_format` (typical — not guaranteed —
    /// for lossy codecs). Stored for later: nothing in-tree interprets it today.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bit_depth: Option<String>,
    /// This side's own sample rate at probe time, before B was resampled to A. See the type docs.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub native_sample_rate: Option<u32>,
    /// This side's own channel count at probe time. `select_track_for_reference` prefers a
    /// channel-matched B track but falls back to any decodable one, so a mismatch is reachable.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub native_channels: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_audio_bitrate_bps: Option<u32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: f64,
}

impl FileSource {
    /// Was this side rate-converted before measurement? `None` when the corpus predates
    /// `native_sample_rate` — **not** `false`: "unanswerable" and "no" are different readings, and
    /// collapsing them is the defect this provenance exists to fix.
    pub fn was_resampled(&self) -> Option<bool> {
        self.native_sample_rate
            .map(|native| native != self.sample_rate)
    }
}

/// The scan recipe a corpus entry was produced under, so two entries are known-comparable.
/// `Option` fields stay for backward compat with corpora written before each knob existed; new dumps
/// fill all five from [`crate::domain::GapReport::recipe`].
///
/// Named [`CorpusScanRecipe`] so it does not collide with the domain [`crate::domain::ScanRecipe`]
/// (whose `PartialEq` means "same gap list"). JSON field names are unchanged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CorpusScanRecipe {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub min_gap_ms: Option<u64>,
    pub silence_peak_fraction: f32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub absolute_silence_rms: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scan_block_ms: Option<u64>,
    /// Effective hold (`silence_hold_blocks × scan_block_ms`), not the configured TOML value.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub silence_hold_ms: Option<u64>,
}

/// Why a fingerprint corpus refused pairwise A↔B measurement. Absent ⇒ comparable (or pre-gate
/// corpus). Present ⇒ `gaps` is empty and B must not be read at A's channel count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncomparableReason {
    /// `native_channels` disagree after track selection. Production fill already skips
    /// (`TrackLayoutMismatch`); the fingerprint path refuses rather than indexing B as A-layout.
    ChannelLayoutMismatch,
}

/// Non-identifying provenance for a corpus: the A/B file identities (pair = entry identity), the scan
/// recipe, and the gap count. No titles, no paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceMeta {
    pub a_source: FileSource,
    pub b_source: FileSource,
    pub scan_recipe: CorpusScanRecipe,
    pub gap_count: usize,
    /// Set when pairwise characterization was refused. Optional so pre-gate corpora still parse.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub incomparable: Option<IncomparableReason>,
    /// Dotted paths of per-gap fields the emitting path **never populated**, so their serialized values
    /// are structural defaults rather than measurements — see [`NOT_MEASURED_BY_PROJECTION`].
    ///
    /// Every field named here is a non-`Option` in [`GapFingerprint`], so it cannot express "not asked":
    /// it serializes as `0.0` / `false` / `[]` / `-120.0`, which is exactly what a real measurement of a
    /// silent, flat, anchorless gap looks like. Fields that *can* go absent (`structure`, `seams`,
    /// `lag`, `second_peak_r`) need no entry — absence already says it.
    ///
    /// This exists because the 2026-07-31 39-pair corpus recorded `collar_rms_peak_ratio: 0.0` and
    /// `anchors: {pre: [], post: []}` on **802 of 802** full-tier gaps while the committed golden
    /// (`curated/01`, also full tier) carries `0.066` and five anchors per side. Nothing in the corpus
    /// distinguished the two, so a reader diffing them sees a measurement change where the truth is a
    /// path change. The provenance plan's §1.1 principle: an unanswerable corpus must **say so**.
    ///
    /// Empty on a corpus whose emitting path measures everything, and absent on any corpus written
    /// before this field existed — which is itself unambiguous, since those predate the guarantee.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub not_measured: Vec<String>,
}

/// The per-gap fields the **production projection path** (`project.rs`) leaves at their structural
/// defaults. Recorded in [`SourceMeta::not_measured`]; asserted against the emitted corpus by the
/// harness `--check`.
///
/// Grouped by why, since the groups have different futures:
/// - `levels.*` — `projected_level_profile` keeps only the two floor summaries it can recover from
///   `LevelTags`; the envelope itself is not carried. `floor_db` / `speech_peak_db` sit at
///   `SILENCE_FLOOR_DB`, which reads as a real −120 dB measurement.
/// - `silence.*` / `contour.*` / `anchors.*` — never computed on this path (`project.rs` builds them
///   from literals / `AnchorSet::default()`).
/// - `outcome.seam_shape` — a plain `String` hardcoded `String::new()` at **both** emit sites, unlike
///   its `Option` siblings `fit_path` / `signature_mode`, which correctly vanish.
///
/// `baseline_lag.*` is **not** here — it is conditional, so it lives in its own list
/// ([`PROJECTED_BASELINE_LAG_FIELDS`]) that the emitter appends only when it actually projected.
///
/// `structure` / `seams` are deliberately **not** listed: they are `Option` and omitted outright
/// (Finding F1, deferred — see `docs/dev/archive/TEMP-pipeline-perf-redesign-plan.md` §8g.4a), so the
/// corpus already states their absence in the only way that matters.
///
/// **Scope: [`DetailTier::Full`] gaps only** — and note the inversion. A gap the path *measured* is
/// rebuilt wholesale by `spec_to_fingerprint_summary`, which drops these fields; a gap it *could not*
/// measure keeps its initial `build_gap_fingerprint` values and still carries them for real. So the
/// unmeasured fields survive exactly on the gaps that failed. On the 39-pair corpus that is 802 gaps
/// stripped and 27 (all head gaps) intact.
pub const NOT_MEASURED_BY_PROJECTION: &[&str] = &[
    "levels.bin_ms",
    "levels.profile_db",
    "levels.floor_db",
    "levels.speech_peak_db",
    "silence.collar_rms_peak_ratio",
    "silence.collar_above_relative_floor",
    "silence.silence_peak_fraction",
    "contour.has_anchor_seam_contour",
    "contour.pre_flatness",
    "contour.post_flatness",
    "anchors.pre",
    "anchors.post",
    "outcome.seam_shape",
];

/// The `baseline_lag` shoulder fields `projected_lag_entry` fabricates — appended to
/// [`SourceMeta::not_measured`] **only when some gap was actually projected**.
///
/// Conditional because, unlike the lists above, this one has a path that answers it.
/// `lag_at_placement` sweeps ±`lag_max_lag_ms` at `b_mapped` on the from-decode path and the caller
/// hands the result over via [`MeasuredDetail`], so those dumps carry a real row and must **not**
/// declare it. The oracle path projects from a `GapRepairSpec` alone — four registration scalars, no
/// PCM — and cannot recover the rest, so it fabricates and must.
///
/// What gets fabricated: `window_ms` / `max_lag_ms` / `peak_lag_samples` / `frac_lag_samples` at `0`,
/// `lag0_r` as a second copy of `peak_r`, and `verdict` hardcoded [`crate::domain::seam_local::LagVerdict::TimingOffset`].
/// The last two are the ones that misled readers. `lag0_r == peak_r` reads as "this shoulder peaks
/// exactly at zero lag" — textbook perfect registration — on every gap in the file, while the true
/// lag-0 correlation is carried nowhere; and `verdict` reads as a classification while being a
/// constant, so "the whole corpus is `timing_offset`, none decorrelated" describes the function
/// rather than the media. Both were read at face value before this declaration existed.
///
/// [`MeasuredDetail`]: super::project::MeasuredDetail
pub const PROJECTED_BASELINE_LAG_FIELDS: &[&str] = &[
    "baseline_lag.window_ms",
    "baseline_lag.max_lag_ms",
    "baseline_lag.peak_lag_samples",
    "baseline_lag.frac_lag_samples",
    "baseline_lag.lag0_r",
    "baseline_lag.verdict",
];

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

/// Wire form of [`BitDepth`]. **The token set is a contract** — `is_lossy()`-style readers and any
/// future census parse these strings, and a rename would silently reinterpret every corpus already on
/// disk. `other:<bits>` keeps the width `BitDepth::Other` carries rather than collapsing it.
/// (`BitDepth` derives no serde, and adding it to an upstream `clip-sync` domain type for a
/// diagnostic artifact is not worth it — hence a local `match`.)
pub(crate) fn bit_depth_str(depth: clip_sync::BitDepth) -> String {
    use clip_sync::BitDepth;
    match depth {
        BitDepth::Int16 => "s16".into(),
        BitDepth::Int24 => "s24".into(),
        BitDepth::Int32 => "s32".into(),
        BitDepth::Float32 => "f32".into(),
        BitDepth::Other(bits) => format!("other:{bits}"),
    }
}

/// Build one side's [`FileSource`]. `sample_rate` / `channels` are the **decoded/analysis** values
/// (A's, for both sides); `descriptor` carries this side's own source readings and is `None` for the
/// media-free callers that have no `AudioTrack` to describe.
pub(crate) fn file_source(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    descriptor: Option<&crate::application::patch_audio::SourceDescriptor>,
) -> FileSource {
    let ch = u32::from(channels.max(1));
    FileSource {
        id: source_id(samples, sample_rate, channels),
        container: None,
        codec: descriptor.map(|d| d.codec.clone()),
        bit_depth: descriptor.and_then(|d| d.bit_depth).map(bit_depth_str),
        native_sample_rate: descriptor.map(|d| d.native_sample_rate),
        native_channels: descriptor.map(|d| d.native_channels),
        source_audio_bitrate_bps: descriptor.and_then(|d| d.bitrate_bps),
        sample_rate,
        channels,
        duration_secs: samples.len() as f64 / f64::from(ch) / f64::from(sample_rate.max(1)),
    }
}

impl CorpusScanRecipe {
    /// Echo the domain recipe that produced the report (all five knobs).
    pub(crate) fn from_report(report: &crate::domain::GapReport) -> Self {
        Self {
            min_gap_ms: Some(report.recipe.min_gap_ms()),
            silence_peak_fraction: report.recipe.silence_peak_fraction(),
            absolute_silence_rms: Some(report.recipe.absolute_silence_rms()),
            scan_block_ms: Some(report.recipe.scan_block_ms()),
            silence_hold_ms: Some(report.recipe.silence_hold_ms()),
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
    /// silence. Silent-core A RMS and floor, interleaved reduction, `scan_block_ms` bins — the same
    /// definitions the scan gate uses, since F15 + I1.
    ///
    /// **Diagnostic only, and not a reference.** Nothing in the plan or patch path reads this field —
    /// `scan_equivalence` is the verdict production acts on. It was documented as "the fine reference"
    /// until 2026-07-30; that framing was wrong, because the differences from the scan path then biased
    /// *this* side toward `drop` (whole-span `gap_floor_db` inflated donor silence; the ±3 s / 50 ms noise
    /// floor read lower). Those terms are fixed, and I1 removed the 50 ms binning outright — so do not
    /// call this the "fine" side either; the axis is production vs **diagnostic**, not resolution.
    /// What survives is the ±2.0 s vs ±3.0 s noise-floor context window (median 0.606 dB, the one
    /// remaining one-signed residual, in the safe direction) and ~1 block of donor-window alignment
    /// (mixed sign). Measured divergence: 1.7 % of gaps, never in the dangerous direction — but that
    /// population was measured 2026-07-30, before I1/I3, and is not a current rate.
    /// See `docs/dev/gap-fingerprint.md` § *`equivalence` vs `scan_equivalence`*.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub equivalence: Option<crate::domain::gap_equivalence::GapEquivalenceVerdict>,
    /// The **coarse production** equivalence verdict for the same gap — the scan-block gate the scan
    /// report carries (`GapReport::gap_equivalence`; block size is the `scan_block_ms` recipe knob, not a
    /// constant), copied in so one `--gap-fingerprints` run holds both readings per gap for calibration
    /// (the `equivalence-calibration` tool diffs `equivalence` vs this). They are **not** two granularities
    /// of one measurement — see that tool's header for the five ways their definitions differ, and compare
    /// `gap_floor_db` on the two verdicts before concluding either is wrong.
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

/// **F14 — the fingerprint analogue of production's `dual_fit_eligible`** (`patch_audio/region.rs`).
///
/// Production carries one `SeamGateFailure` per region and excludes only `StructureAlignmentFailed`,
/// which `select_joint_fit_winner_with_residual` emits exactly when no candidate recorded a failure —
/// nothing was ever scored, so there is nothing to rescue. The per-bracket equivalent is "some bracket
/// failed at a stage past structure alignment". Zero brackets ⇒ nothing scored ⇒ not eligible.
///
/// Also correct on the *synthesized* brackets the corpus projection builds: `synth_brackets` gives
/// filler rows `StructureAlign` and puts the real stage on the closest bracket, so this reduces there
/// to "the closest failure was past structure alignment" — the same question, since the closest
/// bracket is by definition the one that got furthest.
pub fn brackets_dual_fit_eligible(brackets: &[BracketInfo]) -> bool {
    brackets
        .iter()
        .any(|b| matches!(b.failure_stage, Some(stage) if stage != FailureStage::StructureAlign))
}

/// The measurements [`dual_fit_rescue_flag`] needs, so the two call sites pass the same set and neither
/// can quietly omit one. `donor_aligned` is the registered bridge; `donor_nominal` is the
/// registration-independent program-quiet read.
pub struct DualFitRescueInput<'a> {
    /// Bracket-gate `any_ok`.
    pub patched: bool,
    pub brackets: &'a [BracketInfo],
    pub splice_dualfit: Option<&'a SpliceDualfit>,
    pub donor_aligned: Option<&'a DonorInterior>,
    pub donor_nominal: Option<&'a DonorInterior>,
}

/// **F14 — would production's dual-fit rescue this gap?** The single definition both the from-decode
/// dump and the corpus projection use, so the two paths cannot drift. See
/// [`GateOutcome::dual_fit_rescue`] for what the value means and its limits.
///
/// Models the accept conditions of `domain::dual_fit::try_dual_fit` (same conjunction
/// `gap_repair_spec::classify_bracket_exhausted_skip` uses for the `SilenceSplice` cell):
///
/// 1. the bracket failure class is dual-fit-eligible ([`brackets_dual_fit_eligible`]);
/// 2. `gate_pass` — both shoulders clear the seam floors (NaN-aware: matches production's
///    `smin < floor` form, not `smin >= floor`);
/// 3. shoulders do **not** cross — `bridge_frames > 0` (production declines when
///    `b_post_seam <= b_pre_seam`);
/// 4. the **step is real** — `post_seam_r` beats `post_seam_global_r` by
///    `DUALFIT_STEP_REAL_MARGIN`, via the same `partial_cmp` production uses (so a non-finite
///    global — OOB window or zero-variance silence — declines, never over-promises);
/// 5. the **aligned donor bridges** the hole (`continuous`);
/// 6. the **nominal donor is not program-quiet**.
///
/// Dropping 3–6 makes this over-promise badly: a program-quiet gap has high seam correlation and a
/// dead donor, and a crossed-shoulder / NaN-global case can still clear a naive `post − global ≥
/// margin` arithmetic. The curated `04_program_quiet` fixture pins the donor case.
///
/// `None` means "no claim": a patched gap never reaches `skip_or_dual_fit`, and a gap missing any input
/// isn't measurable. Never a defaulted `false`.
pub fn dual_fit_rescue_flag(input: &DualFitRescueInput<'_>) -> Option<bool> {
    if input.patched {
        return None;
    }
    let df = input.splice_dualfit?;
    let aligned = input.donor_aligned?;
    let nominal = input.donor_nominal?;

    // Mirror `try_dual_fit`: decline when `partial_cmp` is None (NaN) or Less.
    let step_real = df
        .post_seam_r
        .partial_cmp(&(df.post_seam_global_r + crate::domain::dual_fit::DUALFIT_STEP_REAL_MARGIN))
        .is_some_and(|ord| ord != std::cmp::Ordering::Less);
    let program_quiet =
        nominal.silence_fraction >= crate::domain::donor::PROGRAM_QUIET_SILENCE_FRAC;

    Some(
        brackets_dual_fit_eligible(input.brackets)
            && df.gate_pass
            && df.bridge_frames > 0
            && step_real
            && aligned.continuous
            && !program_quiet,
    )
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
/// tolerance). Pair with [`ser_nan_as_null`] so `NaN` round-trips as `null`.
fn de_null_as_nan<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
}

/// Emit JSON `null` for non-finite f64 (serde_json rejects bare NaN/Inf). Inverse of [`de_null_as_nan`].
fn ser_nan_as_null<S: serde::Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if v.is_finite() {
        s.serialize_f64(*v)
    } else {
        s.serialize_none()
    }
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
    /// Seam Pearson; non-finite (zero-variance / silence) serializes as JSON `null` — keep raw so
    /// [`dual_fit_rescue_flag`] can mirror production's NaN decisions (F14).
    #[serde(
        serialize_with = "ser_nan_as_null",
        deserialize_with = "de_null_as_nan"
    )]
    pub pre_seam_r: f64,
    #[serde(
        serialize_with = "ser_nan_as_null",
        deserialize_with = "de_null_as_nan"
    )]
    pub post_seam_r: f64,
    pub gap_frames: usize,
    pub bridge_frames: i64,
    /// `bridge − gap`: >0 ⇒ trim, <0 ⇒ pad. Equals the registration step in frames.
    pub trim_frames: i64,
    /// Does `min(pre, post)` clear both floors? Computed with production's `smin < floor` form so a
    /// NaN `smin` does **not** fail the gate (IEEE: `NaN < x` is false).
    pub gate_pass: bool,
    /// **Validator — is the step necessary?** Post seam scored at the *pre* offset (step forced to 0).
    /// If this clears the gate too, a single constant shift suffices and the reported step is a
    /// registration artifact, not a real splice. If only `post_seam_r` (own lag) passes, the step is real.
    /// Non-finite when the global window is OOB or zero-variance — same as `try_dual_fit`.
    #[serde(
        serialize_with = "ser_nan_as_null",
        deserialize_with = "de_null_as_nan"
    )]
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
    /// **Would production's dual-fit rescue this bracket-gate skip?** (F14.) `tier` is contractually the
    /// bracket-gate `any_ok` result and nothing else — see `characterize_gaps_from_decode`, "does NOT run
    /// the production patch gate". But production, after the same class of failure, calls `skip_or_dual_fit`
    /// and may still patch, so a `tier: "skip"` fingerprint can correspond to a *patched* production gap.
    /// This field records that second disposition **beside** `tier` instead of overloading it.
    ///
    /// `Some(true)` ⇒ no bracket passed, at least one bracket was actually *scored* (the fingerprint
    /// analogue of production's `dual_fit_eligible`: anything but `StructureAlignmentFailed`), and
    /// `splice_dualfit.gate_pass` is true. `Some(false)` ⇒ a skip that dual-fit would not rescue.
    /// `None` ⇒ not applicable (the gate patched it) or not measurable (no `splice_dualfit`).
    ///
    /// **Predictive, not observed.** Assumes `--dual-fit` is on (the fingerprint has no request flag)
    /// and reflects `try_dual_fit`'s accept conditions over dump measurements — production still
    /// re-validates the assembled seams and can decline. A-border construction matches production
    /// (`mono(refined ± w)` in `splice_dualfit_at`; F14). Read as "dual-fit rescue candidate", not
    /// "this was patched".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dual_fit_rescue: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `bit_depth` token set is pinned: consumers stratify on these strings, so a rename is a
    /// schema break. `Other(bits)` must stay parseable back to its bit count.
    #[test]
    fn bit_depth_tokens_are_pinned() {
        use clip_sync::BitDepth;
        assert_eq!(bit_depth_str(BitDepth::Int16), "s16");
        assert_eq!(bit_depth_str(BitDepth::Int24), "s24");
        assert_eq!(bit_depth_str(BitDepth::Int32), "s32");
        assert_eq!(bit_depth_str(BitDepth::Float32), "f32");
        assert_eq!(bit_depth_str(BitDepth::Other(20)), "other:20");
    }

    #[test]
    fn was_resampled_distinguishes_no_from_unanswerable() {
        let src = |native: Option<u32>| FileSource {
            id: "0000000000000000".into(),
            container: None,
            codec: None,
            bit_depth: None,
            native_sample_rate: native,
            native_channels: None,
            source_audio_bitrate_bps: None,
            sample_rate: 48_000,
            channels: 2,
            duration_secs: 1.0,
        };
        assert_eq!(src(Some(44_100)).was_resampled(), Some(true));
        assert_eq!(src(Some(48_000)).was_resampled(), Some(false));
        // Pre-provenance corpus: unanswerable, and must not read as "not resampled".
        assert_eq!(src(None).was_resampled(), None);
    }

    /// Provenance fields are omitted, not null, when absent — pre-Track-A corpora must round-trip
    /// byte-identically through the current schema.
    #[test]
    fn absent_provenance_is_omitted_from_json() {
        let json = serde_json::to_string(&file_source(&[0.0; 4], 48_000, 2, None)).unwrap();
        for field in [
            "codec",
            "bit_depth",
            "native_sample_rate",
            "native_channels",
            "source_audio_bitrate_bps",
        ] {
            assert!(!json.contains(field), "{field} must be absent: {json}");
        }
    }

    #[test]
    fn descriptor_populates_provenance() {
        use crate::application::SourceDescriptor;
        let d = SourceDescriptor {
            codec: "aac".into(),
            bit_depth: Some(clip_sync::BitDepth::Int16),
            native_sample_rate: 44_100,
            native_channels: 2,
            bitrate_bps: Some(192_000),
        };
        let fs = file_source(&[0.0; 4], 48_000, 2, Some(&d));
        assert_eq!(fs.codec.as_deref(), Some("aac"));
        assert_eq!(fs.bit_depth.as_deref(), Some("s16"));
        assert_eq!(fs.native_sample_rate, Some(44_100));
        assert_eq!(fs.native_channels, Some(2));
        assert_eq!(fs.source_audio_bitrate_bps, Some(192_000));
        // Measured at A's rate, so this side was rate-converted.
        assert_eq!(fs.was_resampled(), Some(true));
    }

    #[test]
    fn incomparable_reason_omitted_when_absent_and_round_trips() {
        let meta = SourceMeta {
            a_source: file_source(&[0.0; 4], 48_000, 2, None),
            b_source: file_source(&[0.0; 2], 48_000, 1, None),
            scan_recipe: CorpusScanRecipe::default(),
            gap_count: 0,
            incomparable: None,
            not_measured: Vec::new(),
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(
            !json.contains("incomparable"),
            "absent incomparable must omit key: {json}"
        );
        // Same rule for `not_measured`: an empty list is a claim ("this path measured everything"), and
        // a corpus written before the field existed cannot make it. Omit rather than emit `[]`.
        assert!(
            !json.contains("not_measured"),
            "empty not_measured must omit key: {json}"
        );

        let refused = SourceMeta {
            incomparable: Some(IncomparableReason::ChannelLayoutMismatch),
            ..meta
        };
        let round: SourceMeta =
            serde_json::from_str(&serde_json::to_string(&refused).unwrap()).unwrap();
        assert_eq!(
            round.incomparable,
            Some(IncomparableReason::ChannelLayoutMismatch)
        );
    }

    fn bracket(failure_stage: Option<FailureStage>) -> BracketInfo {
        BracketInfo {
            pre_time_secs: 0.0,
            post_time_secs: 0.0,
            span_secs: 0.0,
            move_frames: 0,
            structure_pre: None,
            structure_post: None,
            seam_pre: None,
            seam_post: None,
            start_frame: None,
            fill_frames: None,
            failure_stage,
        }
    }

    fn splice_dualfit(gate_pass: bool) -> SpliceDualfit {
        SpliceDualfit {
            pre_seam_r: 0.9,
            post_seam_r: 0.9,
            gap_frames: 1000,
            bridge_frames: 1000,
            trim_frames: 0,
            gate_pass,
            // 0.9 − 0.1 = 0.8 ≥ DUALFIT_STEP_REAL_MARGIN ⇒ the step is real.
            post_seam_global_r: 0.1,
            pre_seam_prom: None,
            post_seam_prom: None,
            pre_seam_z: None,
            post_seam_z: None,
        }
    }

    fn donor(silence_fraction: f64, continuous: bool) -> DonorInterior {
        DonorInterior {
            rms_db: -30.0,
            silence_fraction,
            longest_silence_ms: 0.0,
            continuous,
            basis: None,
        }
    }

    /// A rescuable gap: scored-but-failed brackets, seams pass, step real, donor bridges, B occupied.
    fn rescuable<'a>(
        brackets: &'a [BracketInfo],
        df: &'a SpliceDualfit,
        aligned: &'a DonorInterior,
        nominal: &'a DonorInterior,
    ) -> DualFitRescueInput<'a> {
        DualFitRescueInput {
            patched: false,
            brackets,
            splice_dualfit: Some(df),
            donor_aligned: Some(aligned),
            donor_nominal: Some(nominal),
        }
    }

    /// F14 — the fingerprint's dual-fit prediction must mirror production's `dual_fit_eligible`:
    /// `StructureAlignmentFailed` (nothing scored) is the one class that cannot be rescued.
    #[test]
    fn dual_fit_rescue_mirrors_production_eligibility() {
        let scored = [bracket(Some(FailureStage::WaveformFloor))];
        let unscored = [bracket(Some(FailureStage::StructureAlign))];
        let pass = splice_dualfit(true);
        let fail = splice_dualfit(false);
        let occupied = donor(0.0, true);

        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &pass, &occupied, &occupied)),
            Some(true)
        );
        // Same brackets, seam gate fails ⇒ a skip that stays a skip.
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &fail, &occupied, &occupied)),
            Some(false)
        );
        // Nothing was ever scored: there is no shoulder pair to fit, so a passing seam gate must NOT
        // be reported as a rescue — this is the production carve-out.
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&unscored, &pass, &occupied, &occupied)),
            Some(false)
        );
        // Mixed: one bracket got past structure alignment ⇒ eligible (production records the failure
        // of the candidate that got furthest, not the first one tried).
        let mixed = [
            bracket(Some(FailureStage::StructureAlign)),
            bracket(Some(FailureStage::Residual)),
        ];
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&mixed, &pass, &occupied, &occupied)),
            Some(true)
        );
    }

    /// The seam gate alone is NOT the accept condition — `try_dual_fit` also requires non-crossed
    /// shoulders, a real step, a continuous aligned donor, and a non-quiet nominal donor. Each must
    /// independently veto, or the flag over-promises repair coverage on exactly the gaps production
    /// declines (the curated `04_program_quiet` regression).
    #[test]
    fn dual_fit_rescue_requires_every_try_dual_fit_condition() {
        let scored = [bracket(Some(FailureStage::WaveformFloor))];
        let pass = splice_dualfit(true);
        let occupied = donor(0.0, true);

        // Step not real: the post seam at the pre lag is just as good ⇒ a rigid single-lag map already
        // explains it, so there is no splice to fit.
        let mut rigid = splice_dualfit(true);
        rigid.post_seam_global_r = rigid.post_seam_r;
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &rigid, &occupied, &occupied)),
            Some(false)
        );

        // Shoulders crossed / collapsed (`b_post_seam <= b_pre_seam`) — production declines; a
        // ±600 ms lag pair can invert a 500 ms min-gap bridge.
        let mut crossed = splice_dualfit(true);
        crossed.bridge_frames = 0;
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &crossed, &occupied, &occupied)),
            Some(false)
        );
        crossed.bridge_frames = -100;
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &crossed, &occupied, &occupied)),
            Some(false)
        );

        // Non-finite global (OOB window or zero-variance silence): production's `partial_cmp` is
        // None ⇒ decline. Arithmetic `post − 0.0 ≥ margin` would wrongly pass.
        let mut nan_global = splice_dualfit(true);
        nan_global.post_seam_global_r = f64::NAN;
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &nan_global, &occupied, &occupied)),
            Some(false)
        );

        // Aligned donor has an internal hole — nothing continuous to bridge with.
        let broken = donor(0.4, false);
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &pass, &broken, &occupied)),
            Some(false)
        );

        // Program-quiet at nominal: B is silent at the same program time. Seams correlate beautifully
        // (silence against silence), which is exactly why gate_pass alone is not enough.
        let quiet = donor(crate::domain::donor::PROGRAM_QUIET_SILENCE_FRAC, true);
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &pass, &occupied, &quiet)),
            Some(false)
        );
    }

    /// The `None` cases are "no claim", never a defaulted `false` — a patched gap never reaches
    /// `skip_or_dual_fit`, and an unmeasured one has nothing to report.
    #[test]
    fn dual_fit_rescue_absent_rather_than_false_when_inapplicable() {
        let scored = [bracket(Some(FailureStage::WaveformFloor))];
        let pass = splice_dualfit(true);
        let occupied = donor(0.0, true);

        // The bracket gate patched it: dual-fit is never consulted.
        let mut patched = rescuable(&scored, &pass, &occupied, &occupied);
        patched.patched = true;
        assert_eq!(dual_fit_rescue_flag(&patched), None);

        // Missing any measurement (lean dump / older corpus) ⇒ unknown, not "no".
        let mut no_df = rescuable(&scored, &pass, &occupied, &occupied);
        no_df.splice_dualfit = None;
        assert_eq!(dual_fit_rescue_flag(&no_df), None);

        let mut no_donor = rescuable(&scored, &pass, &occupied, &occupied);
        no_donor.donor_nominal = None;
        assert_eq!(dual_fit_rescue_flag(&no_donor), None);

        // No brackets at all ⇒ nothing scored; a definite "would not rescue" once measured.
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&[], &pass, &occupied, &occupied)),
            Some(false)
        );
    }

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
                basis: None,
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
                dual_fit_rescue: None,
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
