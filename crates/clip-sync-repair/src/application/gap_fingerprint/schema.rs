//! Licensing-safe serde **schema** for the gap-fingerprint corpus: the per-gap / per-source
//! numeric types (no PCM samples, no transcripts) plus the decoded-audio identity digest
//! ([`source_id`]). Types + identity only — the PCM measurement loops that *fill* these structs
//! live in the sibling `measure` slice. See `docs/dev/archive/TEMP-gap-fingerprint-module-split-plan.md`.

use serde::{Deserialize, Serialize};

use super::contract::Contracted;

pub use crate::domain::lag_decision::{
    BorderCapture, EnvelopeRescue, LagChannel, LagSummary, LagVerdict, LAG_LANDMARK_MIN_PEAK_R,
    LAG_LANDMARK_MIN_PEAK_Z,
};
pub use crate::domain::policies::{ResidualUninformative, SeamFloorSource};

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
/// and `duration_secs` are derived from the decoded PCM and must stay that way — `entry_stem`
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

/// Seam-gate floors / margins the dump's `failure_stage` values were decided against — corpus-level
/// provenance mirroring [`crate::domain::GapEquivalenceThresholds`] for the equivalence gate.
///
/// Present on from-decode dumps (`characterize_gaps_from_decode`); absent on pre-2026-08-03 corpora and
/// on summary-only / refused corpora that never ran the gate. With bracket scores, this is enough to
/// re-derive `structure_floor` / `waveform_floor` (and check residual margin) without re-scoring PCM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusGateRecipe {
    pub min_structure_match_score: f32,
    pub min_fill_correlation: f32,
    pub fill_absolute_floor: f32,
    pub fill_marginal_margin: f32,
    pub short_gap_mean_correlation_secs: f64,
    pub short_gap_one_strong_seam_fallback: bool,
    pub residual_headroom_margin_db: f64,
    /// The threshold [`ResidualInfo::uninformative_pre`]'s `floor_above_ok_db` is relative to; without
    /// it that reason cannot be read as a measurement. `Option` (not a defaulted `f64`) because this
    /// struct has no other defaults, so a missing key must read as "recipe predates the field" rather
    /// than falsely asserting today's default.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub residual_floor_ok_db: Option<f64>,
    pub residual_gate: crate::domain::ResidualGateMode,
}

impl CorpusGateRecipe {
    /// Echo the patch settings the seam gate used for this dump.
    pub fn from_settings(s: &crate::application::PatchRequestSettings) -> Self {
        Self {
            min_structure_match_score: s.min_structure_match_score,
            min_fill_correlation: s.min_fill_correlation,
            fill_absolute_floor: s.fill_absolute_floor,
            fill_marginal_margin: s.fill_marginal_margin,
            short_gap_mean_correlation_secs: s.short_gap_mean_correlation_secs,
            short_gap_one_strong_seam_fallback: s.short_gap_one_strong_seam_fallback,
            residual_headroom_margin_db: s.residual_headroom_margin_db,
            residual_floor_ok_db: Some(s.residual_floor_ok_db),
            residual_gate: s.residual_gate,
        }
    }
}

/// The **probe's own** window geometry for a corpus — what `characterize_gaps_*` measured with, which
/// is not what the patch path fills with. Sibling of [`CorpusScanRecipe`] (which is gap *detection*)
/// and of [`CorpusGateRecipe`] (which echoes production's thresholds); this one is deliberately
/// separate from both so no reader can mistake a probe window for a production setting.
///
/// **Every field here is a value the probe actually used.** That rule exists because two fields once
/// broke it: `FingerprintConfig::border_standoff_secs` carried production's 0.35 while every border
/// spec on the path hardcoded 0 (it was never read, and is now deleted), and `border_secs` — renamed
/// to `noise_collar_secs` — reads as a border-template width but only ever sized the `levels`
/// noise collar, fed from `normalize_window_secs`. Do not add a field here by mirroring a
/// `RepairConfig` knob; add it only after finding the line that reads it.
///
/// Border-template *sizes* are absent on purpose: none is configured. Each measurement derives its own
/// from what it is measuring (`bin_frames * 3` for the structure probe, `window + max_lag` for
/// `lag_at_placement`), and the residual edge displacement that survives is recorded per gap as
/// [`LagSummary::border_displacement_ms`] rather than inferred from a recipe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CorpusProbeRecipe {
    /// Pre-gap noise-collar width behind `levels` (`FingerprintConfig::noise_collar_secs`). Not
    /// optional: `levels` is the one group every path measures for every gap.
    pub noise_collar_secs: f64,
    /// Lag-sweep window and half-width behind `lag_decision` — the §3.6a 1 s / 600 ms pair.
    ///
    /// **Absent when no gap in the corpus carries a `lag_decision`.** The summary-only path
    /// (`characterize_gaps`, `DetailTier::Summary`) never calls `lag_at_placement`, so quoting a sweep
    /// width there would describe a measurement that did not happen — the exact defect this type was
    /// added to close, one level up. See [`CorpusProbeRecipe::for_corpus`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lag_window_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lag_max_lag_ms: Option<u32>,
    /// Coarse envelope half-width (ms) behind `lag_decision` recenter. **Absent when no gap
    /// carries `lag_decision`.** `0` is recorded when the dump ran the fine sweep with recenter
    /// off (`--lag-envelope-max-lag-ms 0`), so an off-arm dump is not confused with a pre-field
    /// corpus (key missing).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lag_envelope_max_lag_ms: Option<u32>,
    /// Which border-capture arm swept `lag_decision` — see [`BorderCapture`]. **Absent when no gap
    /// carries `lag_decision`, and on corpora from before this field** (which are all `Narrow`
    /// *only if* they postdate the 2026-08-20 module split; older ones are `Wide`, and the file
    /// cannot tell you which — that ambiguity is exactly what this field exists to end).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub border_capture: Option<BorderCapture>,
    /// Coarse envelope **window** (seconds) behind the `lag_decision` recenter. **Absent when no
    /// gap carries `lag_decision`, and on corpora from before this field** (all of which ran the
    /// 2.0 s default). Recorded because it is *not* the window the Tier-3 `wide_envelope`
    /// diagnostic uses: that path stays pinned at its own constant, so on a raised-window dump the
    /// two lags in one file come from different searches and must not be compared.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lag_envelope_window_secs: Option<f64>,
    /// Seam window behind `splice` / `splice_dualfit` (`fill_seam_search_secs`). **Absent when no gap
    /// carries `seams`** — the summary-only path runs with `skip_baseline_placement`, which skips
    /// `place_on_b` entirely.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seam_window_secs: Option<f64>,
    /// Frozen ladder behind `post_window_ladder` (§3.1): `[250, 375, 500, 625, 750, 1000]`. **Absent
    /// unless some gap in the corpus actually carries a `post_window_ladder`** — the same
    /// evidence-not-config rule as `lag_window_secs` / `seam_window_secs` above: a flag with no gap
    /// surviving to measure it (empty selection, every gap short-circuited before the probe) must not
    /// claim a sweep that did not happen.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_window_ladder_ms: Option<[u32; 6]>,
    /// Frozen ladder behind `pre_window_ladder` (`docs/dev/archive/TEMP-dual-fit-pre-window-ladder-plan.md`
    /// §3.1): `[250, 375, 500, 625, 750, 1000]`. Same evidence-not-config rule as
    /// `post_window_ladder_ms` above.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_window_ladder_ms: Option<[u32; 6]>,
    /// Recorded rather than omitted so a reader never has to assume production's
    /// `border_standoff_secs` (0.35) applied to a measurement. Not a knob — [`CorpusProbeRecipe::for_corpus`]
    /// converts it from [`PROBE_BORDER_STANDOFF_FRAMES`](super::measure::PROBE_BORDER_STANDOFF_FRAMES),
    /// the same constant every probe `GapBorderSpec` is given, so the two cannot drift.
    pub border_standoff_secs: f64,
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
    /// Seam-gate thresholds used when scoring brackets — see [`CorpusGateRecipe`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gate_recipe: Option<CorpusGateRecipe>,
    /// The probe's own window geometry — see [`CorpusProbeRecipe`]. Absent on pre-2026-08-17 corpora,
    /// where the probe's geometry has to be recovered from the binary that wrote them.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub probe_recipe: Option<CorpusProbeRecipe>,
    /// Dotted paths of per-gap fields the emitting path left as **present structural defaults** rather
    /// than measurements — see [`NOT_MEASURED_BY_PROJECTION`] and [`PROJECTED_LAG_DECISION_FIELDS`].
    ///
    /// Only fields that still serialize a non-`Option` stand-in need an entry here. The `levels`
    /// envelope and `outcome.seam_shape` are `Option` (or skip-empty) and simply **omit** when
    /// unmeasured — absence already says it, so they are no longer listed. `silence` / `contour` /
    /// `anchors` are restored by the from-decode overlay and are not declared. Empty on a corpus
    /// whose emitting path measured everything it still emits as required scalars; absent on any
    /// corpus written before this field existed.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub not_measured: Vec<String>,
    /// Pair-level chromaprint scan alignment that mapped A onto B. Present on every corpus built
    /// from a [`crate::domain::GapReport`]; omitted on pre-overlay dumps and on hand-built / spec-only
    /// corpora that never saw a scan. Inner `None`s are unanswerable fields on that scan, not a missing
    /// group.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub alignment: Option<Contracted<CorpusScanAlignment>>,
    /// Identity of the **binary** that wrote this corpus — see [`BuildStamp`]. Absent on corpora
    /// written before 2026-09-09, and on hand-built / test corpora that never ran an emitting path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub build: Option<BuildStamp>,
}

/// Which build wrote a corpus.
///
/// `a_source` / `b_source` identify the *media* and [`CorpusProbeRecipe`] identifies one axis of the
/// *probe geometry*; this identifies the *code*. Two arms of a comparison taken across a code change
/// were previously indistinguishable except by inferring it from the numbers.
///
/// **Scope: the commit, not the constants.** A build-time default that moved between two runs is not
/// visible here beyond the commit that moved it. Reading a difference in behaviour off two differing
/// stamps is still inference — the stamp tells you the builds differ, and that is all it claims.
///
/// Captured when `clip-sync-repair` was compiled, not when it ran. See this crate's `build.rs` for the
/// staleness window on [`Self::dirty`] and for why an absent git is not an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildStamp {
    /// Short `HEAD` hash at compile time. `None` when the build had no git metadata to read, which is
    /// **not** the same as a clean tree at an unknown commit — it means the question was unanswerable.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub commit: Option<String>,
    /// The working tree carried uncommitted changes when this crate was compiled. Read `false`
    /// alongside an absent `commit` as "unknown", not as "clean": the no-git path cannot tell.
    pub dirty: bool,
    /// Crate version. Static across most builds, so it separates two runs only across a version bump —
    /// it is here because it is the one axis that survives when git is unavailable.
    pub version: String,
}

impl BuildStamp {
    /// The stamp for the running binary, from the environment `build.rs` set at compile time.
    pub fn current() -> Self {
        let commit = env!("CLIP_SYNC_BUILD_COMMIT");
        Self {
            commit: (!commit.is_empty()).then(|| commit.to_string()),
            dirty: env!("CLIP_SYNC_BUILD_DIRTY") == "1",
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Licensing-safe projection of [`crate::domain::ScanAlignment`] plus the optional silence-structure
/// cross-check. No paths, no decode-skip counts, no B-window timestamps (those duplicate
/// [`Self::start_overlap`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusScanAlignment {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recommended_offset_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub offset_drift_secs: Option<f64>,
    pub offsets_consistent: bool,
    pub start_aligned: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub end_aligned: Option<bool>,
    /// Scan ran query-reference localization. This is **not** the mapped-region fill gate
    /// ([`crate::domain::GapReport::limit_fill_to_mapped_region`]), which the CLI can turn off
    /// independently.
    pub query_reference_mode: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub start_overlap: Option<CorpusTimelineOverlap>,
    pub clips: Vec<CorpusAlignedClip>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub silence_cross_check: Option<CorpusSilenceCrossCheck>,
}

impl CorpusScanAlignment {
    /// Project the report's scan alignment. Never copies `video_a` / `video_b`.
    pub(crate) fn from_report(report: &crate::domain::GapReport) -> Self {
        let a = &report.alignment;
        Self {
            recommended_offset_secs: a.recommended_offset_secs,
            offset_drift_secs: a.offset_drift_secs,
            offsets_consistent: a.offsets_consistent,
            start_aligned: a.start_aligned,
            end_aligned: a.end_aligned,
            query_reference_mode: a.query_reference_mode,
            start_overlap: a.start_overlap.map(CorpusTimelineOverlap::from),
            clips: a.clips.iter().map(CorpusAlignedClip::from).collect(),
            silence_cross_check: report
                .gap_offset_agreement
                .as_ref()
                .map(CorpusSilenceCrossCheck::from),
        }
    }
}

/// Shared-timeline overlap times only — B windows on [`CorpusAlignedClip`] are omitted as duplicates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CorpusTimelineOverlap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    pub video_b_start_secs: f64,
    pub video_b_end_secs: f64,
    pub shared_length_secs: f64,
}

impl From<crate::domain::TimelineOverlap> for CorpusTimelineOverlap {
    fn from(o: crate::domain::TimelineOverlap) -> Self {
        Self {
            video_a_start_secs: o.video_a_start_secs,
            video_a_end_secs: o.video_a_end_secs,
            video_b_start_secs: o.video_b_start_secs,
            video_b_end_secs: o.video_b_end_secs,
            shared_length_secs: o.shared_length_secs,
        }
    }
}

/// One A-side clip window on the scan map. `window_*` is A's interval; B times live on
/// [`CorpusScanAlignment::start_overlap`]. Serde lives here so domain [`crate::domain::ClipRole`]
/// does not grow a dump contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusAlignedClip {
    pub role: CorpusClipRole,
    pub window_start_secs: f64,
    pub window_end_secs: f64,
    pub aligned: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub offset_secs: Option<f64>,
    pub confidence: f32,
}

impl From<&crate::domain::AlignedClip> for CorpusAlignedClip {
    fn from(c: &crate::domain::AlignedClip) -> Self {
        Self {
            role: CorpusClipRole::from(c.role),
            window_start_secs: c.window_start_secs,
            window_end_secs: c.window_end_secs,
            aligned: c.aligned,
            offset_secs: c.offset_secs,
            confidence: c.confidence,
        }
    }
}

/// Wire form of [`crate::domain::ClipRole`]. Snake_case so the dump does not emit `Start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorpusClipRole {
    Start,
    Interior,
    End,
}

impl From<crate::domain::ClipRole> for CorpusClipRole {
    fn from(role: crate::domain::ClipRole) -> Self {
        match role {
            crate::domain::ClipRole::Start => Self::Start,
            crate::domain::ClipRole::Interior => Self::Interior,
            crate::domain::ClipRole::End => Self::End,
        }
    }
}

/// Silence-structure vs chromaprint offset comparison from [`crate::domain::GapOffsetAgreement`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusSilenceCrossCheck {
    pub silence_based_offset_secs: f64,
    pub alignment_offset_secs: f64,
    pub delta_secs: f64,
    pub agrees: bool,
}

impl From<&crate::domain::GapOffsetAgreement> for CorpusSilenceCrossCheck {
    fn from(a: &crate::domain::GapOffsetAgreement) -> Self {
        Self {
            silence_based_offset_secs: a.silence_based_offset_secs,
            alignment_offset_secs: a.alignment_offset_secs,
            delta_secs: a.delta_secs,
            agrees: a.agrees,
        }
    }
}

/// Unconditional unmeasured-field paths for the production projection rebuild.
///
/// Empty since 2026-08-03: the `levels` envelope and `outcome.seam_shape` are omitted (`Option` /
/// skip-empty) rather than written as zeros / `−120` / `""`. `silence` / `contour` / `anchors` are
/// restored by the from-decode overlay (still `Option`; absence means pre-overlay / never pass 1).
/// The only remaining fabricated stand-ins are the conditional [`PROJECTED_LAG_DECISION_FIELDS`].
///
/// Kept as a named constant so emitters and `--check` still share one list, and so older tests that
/// assert "production declares exactly this" keep compiling against the empty claim.
pub const NOT_MEASURED_BY_PROJECTION: &[&str] = &[];

/// The `lag_decision` shoulder fields `projected_lag_entry` fabricates — appended to
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
pub const PROJECTED_LAG_DECISION_FIELDS: &[&str] = &[
    "lag_decision.window_ms",
    "lag_decision.max_lag_ms",
    "lag_decision.peak_lag_samples",
    "lag_decision.frac_lag_samples",
    "lag_decision.lag0_r",
    "lag_decision.verdict",
];

/// The same declaration as written by dumps predating the 2026-08-07 `baseline_lag` → `lag_decision`
/// rename.
///
/// Unlike a struct field, these paths are **data**: they are strings inside `source.not_measured` on
/// every already-committed corpus, and a serde alias cannot reach them. A reader that only knows the
/// new spelling silently stops recognizing an old dump's declaration — and the failure mode is the
/// bad direction, since an unrecognized declaration reads as "this field was measured".
pub const LEGACY_PROJECTED_LAG_DECISION_FIELDS: &[&str] = &[
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

/// One gap's numeric characterization. Pairwise fields (`structure`, `seams`, the two lag groups, `outcome`) are
/// `None` when characterizing A alone; `lag_editorial` and per-bracket detail require
/// [`DetailTier::Full`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapFingerprint {
    pub index: usize,
    pub tier: DetailTier,
    pub sample_rate: u32,
    pub channels: u16,
    pub geometry: GapGeometry,
    pub levels: LevelProfile,
    /// Collar / relative-silence test. Omitted by the spec projector; restored by the from-decode
    /// overlay. `None` means unmeasured (pre-overlay dump, or never reached pass 1).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub silence: Option<SilenceProfile>,
    /// Anchor-seam contour flags. Omitted by the spec projector; restored by the from-decode overlay.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub contour: Option<ContourInfo>,
    /// Editorial anchors. Omitted by the spec projector; restored by the from-decode overlay.
    /// `Some` including `{ pre: [], post: [] }` means measured, none found; `None` means unmeasured.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub anchors: Option<AnchorSet>,
    /// Constellation `L`-histogram lock test (TEMP-constellation-fingerprint-plan.md §0/§5). Omitted
    /// by the spec projector; restored by the from-decode overlay. `None` means unmeasured (pairwise +
    /// Full tier only).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub constellation: Option<ConstellationSet>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub brackets: Vec<BracketInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub structure: Option<StructureScores>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seams: Option<SeamScores>,
    /// Lag at the best-structure **editorial bracket** — a *diagnostic* placement that can sit far
    /// from the throat the gate actually scores. Nothing decides on it; prefer `lag_decision` for
    /// anything registration-shaped.
    ///
    /// Wire key was `lag` before 2026-08-07; old corpora still deserialize via the serde alias. The
    /// bare name read as "the lag of this gap", which is what made it the wrong one to reach for.
    #[serde(alias = "lag", skip_serializing_if = "Option::is_none", default)]
    pub lag_editorial: Option<Contracted<LagFingerprint>>,
    /// Lag at the **decision placement** — the geometry `b_mapped` nominal the gate registers on for
    /// patch/skip (`lag_at_placement`, ±`lag_max_lag_ms`) — as opposed to `lag_editorial`, measured at
    /// the best-structure *editorial bracket* / structure throat (a diagnostic placement that can sit
    /// far from it). This is the registration-relevant lag.
    ///
    /// **Not the throat.** `residual` is the group measured at the gate's structure throat; this one
    /// is deliberately *not* slid there (`measure.rs`: "Decision-seam lag at `b_mapped` nominal … not
    /// structure throat"), and conflating the two is the misreading `_contract` exists to stop.
    ///
    /// Wire key was `baseline_lag` before 2026-08-07; old corpora still deserialize via the serde
    /// alias. "Baseline" named the *placement pass* that produced it, not the role, and readers took
    /// it for a reference/before value.
    #[serde(
        alias = "baseline_lag",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub lag_decision: Option<Contracted<LagFingerprint>>,
    /// Residual cancellation at the gate's **structure throat** (the strong same-source confirm) — the
    /// one group that is *not* read at `b_mapped`. Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub residual: Option<Contracted<ResidualInfo>>,
    /// Seam recovery / encoding-robust envelope / level at the **`b_mapped`** registration — diagnoses
    /// *why* a waveform seam is dead (mis-alignment vs cross-encoding vs quiet). Full tier, gate path.
    /// Read at a different placement than `residual`, so their numbers do not compare.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seam_probe: Option<Contracted<SeamProbeFingerprint>>,
    /// Does donor B actually carry audio across the hole? Energy/continuity of B over the **aligned**
    /// gap-mapped span (shoulders at their own lags) — the donor half of the fill predicate (§3/§4).
    /// Full tier, gate path.
    ///
    /// Wire key was `donor_interior` before 2026-08-07; old corpora still deserialize via the serde
    /// alias. The bare name read as "the donor interior", which hid that it is one of *two* spans and
    /// that this one carries the registration confound `donor_interior_nominal` exists to dodge.
    #[serde(
        alias = "donor_interior",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub donor_interior_aligned: Option<Contracted<DonorInterior>>,
    /// Donor occupancy at the **nominal** geometry `b_mapped` span (NO per-shoulder lag) — the
    /// registration-independent sibling of `donor_interior_aligned`, so it dodges the aliased-lag
    /// confound the aligned span inherits. `silence_fraction ≈ 1` ⇒ B is quiet at the same program time
    /// as A's gap ⇒ program-quiet, not a fillable dropout (D11). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_interior_nominal: Option<Contracted<DonorInterior>>,
    /// Symmetric B-side level profile over the nominal `b_mapped` span + context — the counterpart to
    /// `levels` (A), computed by the same `level_profile` logic. Lets "quiet in both masters ⇒ not a
    /// dropout" compare B's gap level against B's *own* noise floor (D11). Full tier, gate path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub b_levels: Option<LevelProfile>,
    /// First-class splice summary: the per-side registration step + peaks/uniqueness the repair predicate
    /// reads directly (instead of re-deriving from `lag_decision`). Full tier, gate path.
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
    /// Post-seam window ladder (`docs/dev/archive/TEMP-dual-fit-post-window-ladder-plan.md`) — same-start
    /// re-locks of the **post** seam at `{250, 375, 500, 625, 750, 1000}` ms, envelope off, ±600 ms.
    /// Sibling of `splice_dualfit`, not nested in it: `splice_dualfit` stays the 250 ms
    /// production-predictor (which may envelope-rescue); this is a fixed-geometry probe curve.
    /// `--fingerprint-post-window-ladder` only (does not require `--fingerprint-diagnostics`); absent
    /// on every other dump, including old corpora.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_window_ladder: Option<Vec<PostWindowLadderRung>>,
    /// `--fingerprint-pre-window-ladder` only (does not require `--fingerprint-diagnostics`); absent
    /// on every other dump, including old corpora.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_window_ladder: Option<Vec<PreWindowLadderRung>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<GateOutcome>,
    /// Gap-equivalence classification (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate): does this gap need patching?
    /// Derived from the **silence character** — A's gap RMS vs the recording's noise floor (dropout vs room
    /// tone) + donor silence (is B occupied) — the signals that separate real dropouts from mutual/program
    /// silence. Silent-core A RMS and floor, interleaved reduction, `scan_block_ms` bins — the same
    /// definitions the scan gate uses, since F15 + I1.
    ///
    /// **Diagnostic only, and not a reference.** Nothing in the plan or patch path reads this field —
    /// `equivalence_production` is the verdict production acts on. It was documented as "the fine reference"
    /// until 2026-07-30; that framing was wrong, because the differences from the scan path then biased
    /// *this* side toward `drop` (whole-span `gap_floor_db` inflated donor silence; the ±3 s / 50 ms noise
    /// floor read lower). Those terms are fixed, and I1 removed the 50 ms binning outright — so do not
    /// call this the "fine" side either; the axis is production vs **diagnostic**, not resolution.
    /// What survives is the ±2.0 s vs ±3.0 s noise-floor context window (median 0.606 dB, the one
    /// remaining one-signed residual, in the safe direction) and ~1 block of donor-window alignment
    /// (mixed sign). Measured divergence: 1.7 % of gaps, never in the dangerous direction — but that
    /// population was measured 2026-07-30, before I1/I3, and is not a current rate.
    /// See `docs/dev/gap-fingerprint.md` § *`equivalence_diagnostic` vs `equivalence_production`*.
    ///
    /// Wire key was `equivalence` before 2026-08-07; old corpora still deserialize via the serde alias.
    #[serde(
        alias = "equivalence",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub equivalence_diagnostic:
        Option<Contracted<crate::domain::gap_equivalence::GapEquivalenceVerdict>>,
    /// The **production** equivalence verdict for the same gap — the scan-block gate the scan
    /// report carries (`GapReport::gap_equivalence`; block size is the `scan_block_ms` recipe knob, not a
    /// constant), copied in so one `--gap-fingerprints` run holds both readings per gap for calibration
    /// (the `equivalence-calibration` tool diffs `equivalence_diagnostic` vs this). This is the
    /// **authoritative** verdict — what `skip_equivalent_gaps` acts on. They are **not** two granularities
    /// of one measurement — see that tool's header for the five ways their definitions differ, and compare
    /// `gap_floor_db` on the two verdicts before concluding either is wrong.
    /// `None` when the scan did not classify the gap.
    ///
    /// Wire key was `scan_equivalence` before 2026-08-07; old corpora still deserialize via the serde alias.
    #[serde(
        alias = "scan_equivalence",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub equivalence_production:
        Option<Contracted<crate::domain::gap_equivalence::GapEquivalenceVerdict>>,
}

impl GapFingerprint {
    /// The authoritative verdict, without its `_contract` wrapper — what production acted on.
    ///
    /// Prefer this over reaching through [`Contracted::value`] at read sites: the wrapper is a wire
    /// concern, and analysis code should not care whether reader metadata is attached.
    pub fn equivalence_production_verdict(
        &self,
    ) -> Option<&crate::domain::gap_equivalence::GapEquivalenceVerdict> {
        self.equivalence_production.as_deref()
    }

    /// The diagnostic second opinion, without its `_contract` wrapper. Gates nothing — see
    /// `docs/dev/gap-fingerprint.md` § *`equivalence_diagnostic` vs `equivalence_production`*.
    pub fn equivalence_diagnostic_verdict(
        &self,
    ) -> Option<&crate::domain::gap_equivalence::GapEquivalenceVerdict> {
        self.equivalence_diagnostic.as_deref()
    }

    // C2 accessors. Same rule as the equivalence pair: **read sites should not know the wrapper
    // exists.** `Contracted<T>` derefs, so most call sites compile either way — but a file that
    // reaches through `.value` teaches the next reader that the wrapper is their problem, and a
    // half-converted file is how that spreads. Prefer these everywhere.

    /// The decision registration, unwrapped — the lag production actually gates on.
    pub fn lag_decision_fingerprint(&self) -> Option<&LagFingerprint> {
        self.lag_decision.as_deref()
    }

    /// The Tier-3 editorial lag, unwrapped. A *diagnostic* placement: never compare it against
    /// [`Self::lag_decision_fingerprint`].
    pub fn lag_editorial_fingerprint(&self) -> Option<&LagFingerprint> {
        self.lag_editorial.as_deref()
    }

    /// Donor occupancy over the lag-adjusted bridge, unwrapped.
    pub fn donor_interior_aligned_span(&self) -> Option<&DonorInterior> {
        self.donor_interior_aligned.as_deref()
    }

    /// Donor occupancy at nominal program time, unwrapped — the registration-independent read.
    pub fn donor_interior_nominal_span(&self) -> Option<&DonorInterior> {
        self.donor_interior_nominal.as_deref()
    }

    /// Same-source cancellation at the throat, unwrapped.
    pub fn residual_info(&self) -> Option<&ResidualInfo> {
        self.residual.as_deref()
    }

    /// Tier-3 seam diagnosis, unwrapped. No gate reads this.
    pub fn seam_probe_metrics(&self) -> Option<&SeamProbeFingerprint> {
        self.seam_probe.as_deref()
    }
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
///
/// Envelope fields (`bin_ms` / `profile_db` / `floor_db` / `speech_peak_db`) are `Option` / skip-empty so
/// the production projection path can **omit** them rather than emit `0` / `[]` / `−120` placeholders.
/// `noise_floor_db` / `gap_floor_db` remain required — those are the decision floors the corpus reader
/// consumes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelProfile {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bin_ms: Option<u32>,
    /// Per-bin RMS in dBFS across pre-context → post-context (silence floored, see `floor_db`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub profile_db: Vec<f32>,
    /// dBFS value substituted for true-silent bins (so the vector has no `-inf`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub floor_db: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub speech_peak_db: Option<f32>,
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

/// One kept onset from the domain picker, wire-shaped for a constellation side
/// (TEMP-constellation-fingerprint-plan.md §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationPoint {
    pub t_seam_ms: f64,
    pub time_secs: f64,
    pub prominence: f32,
}

/// One shoulder side's searched span + kept points (§5 "both sides" block).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationSide {
    pub span_start_secs: f64,
    pub span_end_secs: f64,
    pub edge_pinned: bool,
    pub n: usize,
    /// Prominence-desc, capped at 12 (matches `PickerParams::first_candidate`'s `cap`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub points: Vec<ConstellationPoint>,
}

/// `reason` (§5, §0 vocabulary). `Unmeasured` covers two distinct domain situations, both "the test
/// didn't run": a shoulder window that doesn't exist to attempt (file-boundary A/B window, never
/// reaches `evaluate_l_histogram`), and `domain::LockReason::RedrawInfeasible` (the null had no legal
/// arrangement — the uniqueness test needs a legal redraw to compare against; without one there's no
/// verdict, which is `unmeasured`, not `not_unique`). Every other `LockReason` variant maps 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstellationReason {
    Locked,
    NoTrain,
    NotUnique,
    /// The histogram mode landed on the edge of the *searched B span*, not on a `ConstellationSide`'s
    /// own truncated window — independent of `ConstellationSide.edge_pinned` (a side's window hitting
    /// a file/haystack boundary). The two can disagree in either direction: a well-inside-the-file
    /// shoulder can still edge-pin here if the true lag sits outside `max_lag_ms`, and a
    /// file-boundary-truncated side can still lock cleanly.
    EdgePinned,
    Unmeasured,
}

/// One shoulder's A/B sides plus the `L`-histogram test (§5 "the test" block).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationShoulder {
    pub a: ConstellationSide,
    pub b: ConstellationSide,
    pub n_a: usize,
    pub n_b: usize,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hist_peak_lag_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hist_peak_votes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hist_second_votes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub redraw_p95_votes: Option<usize>,
    pub reason: ConstellationReason,
}

/// Group name `constellation` (§5). Recipe-stamp fields live once per gap, not per shoulder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationSet {
    pub bin_ms: f64,
    pub min_spacing_ms: f64,
    pub context_secs: f64,
    pub max_lag_ms: u32,
    pub vote_bin_ms: f64,
    pub redraw_trials: usize,
    pub redraw_percentile: f64,
    /// Not `Option`: every gap that reaches this fn gets both shoulders — a not-attempted shoulder
    /// (file-boundary window, undecodable B) is `ConstellationReason::Unmeasured`, not a missing
    /// struct.
    pub pre: ConstellationShoulder,
    pub post: ConstellationShoulder,
}

/// One feasible editorial bracket and its scores (full tier).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BracketInfo {
    pub pre_time_secs: f64,
    pub post_time_secs: f64,
    pub span_secs: f64,
    pub move_frames: usize,
    /// Structure-match scores at the gate placement. Populated on
    /// [`FailureStage::StructureFloor`] (and on paths that record structure for every bracket);
    /// **not** stuffed into [`Self::seam_pre`] / [`Self::seam_post`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub structure_pre: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub structure_post: Option<f64>,
    /// Waveform Pearson at the gate placement. Present on pass and on
    /// [`FailureStage::WaveformFloor`] / [`FailureStage::Residual`]; absent on
    /// [`FailureStage::StructureFloor`] / [`FailureStage::StructureAlign`].
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
    /// Residual headroom margin (dB) from `SeamGateFailure::ResidualHeadroomExceeded` when
    /// [`Self::failure_stage`] is [`FailureStage::Residual`]. Absent otherwise (and on dumps that
    /// predate this field). Compare against [`CorpusGateRecipe::residual_headroom_margin_db`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub residual_margin_db: Option<f64>,
}

/// Progress score for "closest failing bracket" selection: structure scores on
/// [`FailureStage::StructureFloor`], waveform seam scores otherwise. `NEG_INFINITY` when the
/// relevant pair is missing (e.g. [`FailureStage::StructureAlign`]).
pub fn bracket_failure_progress(b: &BracketInfo) -> f64 {
    match b.failure_stage {
        Some(FailureStage::StructureFloor) => match (b.structure_pre, b.structure_post) {
            (Some(a), Some(c)) => a.min(c),
            _ => f64::NEG_INFINITY,
        },
        _ => match (b.seam_pre, b.seam_post) {
            (Some(a), Some(c)) => a.min(c),
            _ => f64::NEG_INFINITY,
        },
    }
}

/// Failing bracket that got furthest (highest [`bracket_failure_progress`]) — the gap-level
/// fingerprint skip label. `None` when every bracket passed or none were scored.
pub fn closest_bracket_failure_stage(brackets: &[BracketInfo]) -> Option<FailureStage> {
    brackets
        .iter()
        .filter(|b| b.failure_stage.is_some())
        .max_by(|x, y| {
            bracket_failure_progress(x)
                .partial_cmp(&bracket_failure_progress(y))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|b| b.failure_stage)
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

impl FailureStage {
    /// Stable dump / analyzer label; matches the serde `snake_case` representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StructureAlign => "structure_align",
            Self::StructureFloor => "structure_floor",
            Self::WaveformFloor => "waveform_floor",
            Self::Residual => "residual",
        }
    }
}

/// **F14 — the fingerprint analogue of production's `dual_fit_eligible`** (`patch_audio/region/dual_fit.rs`).
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
/// Models the accept conditions of `domain::dual_fit::dual_fit_at_nominal` (same conjunction
/// `gap_repair_spec::classify_bracket_exhausted_skip` uses for the `SilenceSplice` cell). Conditions
/// 2–3 are the two *lock-level* rejections, now owned by `dual_fit_at_nominal` /
/// `domain::seam_locks` rather than by `try_dual_fit` itself:
///
/// 1. the bracket failure class is dual-fit-eligible ([`brackets_dual_fit_eligible`]);
/// 2. `gate_pass` — both shoulders clear the seam floors (`SeamGateFloors::passes`; seam
///    correlations are finite by construction, so there is no NaN case here);
/// 3. shoulders do **not** cross — `bridge_frames > 0` (production declines when
///    `b_post_seam <= b_pre_seam`);
/// 4. the **aligned donor bridges** the hole (`continuous`);
/// 5. the **nominal donor is not program-quiet**.
///
/// Does **not** require the step to be real (dropped 2026-09-01,
/// docs/dev/archive/TEMP-seam-lock-rigid-fill-plan.md §3.5): `try_dual_fit` assembles a trim-0 fill on a
/// rigid map exactly as it does an interior-trim fill on a real step, so `step_real` is a report
/// axis on the result, not an admission gate.
///
/// Dropping the rest makes this over-promise badly: a program-quiet gap has high seam correlation
/// and a dead donor, a crossed-shoulder bridge has nothing to splice, and a NaN `post_global` is
/// still `try_dual_fit`'s hard decline (§3.1 — unmeasured is not "a constant shift suffices"),
/// not a case the dropped `step_real` conjunct is around to catch anymore. The curated
/// `04_program_quiet` fixture pins the donor case.
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

    let program_quiet =
        nominal.silence_fraction >= crate::domain::donor::PROGRAM_QUIET_SILENCE_FRAC;

    Some(
        brackets_dual_fit_eligible(input.brackets)
            && df.gate_pass
            && df.bridge_frames > 0
            && df.post_seam_global_r.is_finite()
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

/// Seam diagnostic at the **`b_mapped` registration** (the post side shifted by the measured pre lag),
/// one per side — *not* the structure throat `residual` uses. Built to separate *why* a
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

/// Per-side seam probes at the `b_mapped` registration (mono; one entry per measured side).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SeamProbeFingerprint {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre: Option<SeamProbe>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post: Option<SeamProbe>,
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
///
/// Per-side dB values are `None` when that side was unmeasured / non-finite / silence-floor sentinel
/// (`−120`) — never emit a fabricated −120 as if it were a cancellation depth.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResidualInfo {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_residual_db_opt"
    )]
    pub chosen_pre_db: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_residual_db_opt"
    )]
    pub chosen_post_db: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_residual_db_opt"
    )]
    pub floor_pre_db: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_residual_db_opt"
    )]
    pub floor_post_db: Option<f64>,
    /// Where each side's floor probe found its reference window — [`SeamFloorSource::Border`] (the
    /// immediate border window was usable), [`Walked`](SeamFloorSource::Walked) (it was quiet, so the
    /// reference came from further out), or [`None`](SeamFloorSource::None) (no usable window at all).
    ///
    /// **This is the disambiguator for an absent `floor_*_db`.** The dB goes `None` on the wire for
    /// two unrelated reasons — the probe never found a reference (`source: none`, `residual_db` is
    /// `NaN`), or it found one and `residual_db_opt` mapped a −120 / non-finite reading away. Without
    /// the source those are the same hole, and a reader cannot tell "we could not measure here" from
    /// "we measured and it cancelled to silence". `informative` does not close the gap either: it is
    /// derived from a *different channel* than these scalars (see below).
    ///
    /// `Option` only so dumps written before 2026-08-03 still deserialize; any path that emits a
    /// residual at all knows the source, so a current dump always carries both.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub floor_source_pre: Option<SeamFloorSource>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub floor_source_post: Option<SeamFloorSource>,
    /// The noise floor established cancellation on every *governing* side — the residual is interpretable.
    ///
    /// **Do not try to recompute this from the four dB scalars above; on multichannel media it is not
    /// derivable from them.** `SeamResidualVerdict::from_channel_residuals` deliberately reads two
    /// different channels: the scalars summarize the **worst-headroom** channel
    /// (`side_worst_headroom_summary`), while `informative` follows the **best-cancelling (min-floor)**
    /// channel (shared measuredness: sourced-NaN ignored like unmeasured), so that a noisy surround
    /// cannot flip the same-master regime off (Non-goal §2). Both are intentional and neither is wrong —
    /// but they describe different channels, so `floor_pre_db > −15 dB` alongside `informative: true`
    /// is a consistent reading, not a contradiction. The per-channel breakdown that would reconcile
    /// them is not in the dump; it is only logged (`log_residual_channel_breakdown`).
    pub informative: bool,
    /// Per-side reason the floor is unusable — `no_reference_window` (never found one),
    /// `probe_non_finite` (found one, the fit produced nothing), `floor_above_ok_db` (measured, and
    /// above the threshold — a measurement, not an abstention).
    ///
    /// Stored rather than derived: `informative: false` alone conflates all three, and
    /// `floor_above_ok_db` is relative to `residual_floor_ok_db`, which is a setting — see
    /// [`CorpusGateRecipe::residual_floor_ok_db`], where the run's threshold is recorded.
    ///
    /// `Option` only so dumps written before 2026-08-05 still deserialize.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub uninformative_pre: Option<ResidualUninformative>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub uninformative_post: Option<ResidualUninformative>,
    /// Placement slide and lag radius, carried so a replayed verdict can answer
    /// `beyond_lag_reach()` the way production did.
    ///
    /// Before these existed the projection path reconstructed every verdict with `0`/`0`, which made
    /// `beyond_lag_reach()` structurally `false` on replay — so the gate's own abstention could never
    /// be seen in a dump, and `residual_band` disagreed with production on exactly those gaps.
    /// `None` means the dump predates the fields; replay then falls back to the old `0`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub placement_slide_frames: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_lag_frames: Option<i64>,
}

/// Map a residual dB to wire form: non-finite and the `SILENCE_FLOOR_DB` (−120) sentinel become `None`.
pub fn residual_db_opt(db: f64) -> Option<f64> {
    if db.is_finite() && (db - f64::from(SILENCE_FLOOR_DB)).abs() > 1e-9 {
        Some(db)
    } else {
        None
    }
}

fn de_residual_db_opt<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    // Accept JSON `null`, absent (via default), bare number, and map −120 / non-finite → None so old
    // dumps that wrote the silence-floor sentinel read as "unmeasured side".
    Ok(Option::<f64>::deserialize(d)?.and_then(residual_db_opt))
}

/// `DonorInterior` + `donor_interior_at` moved to the shared `domain::donor` so the production dual-fit
/// repair (A3) and this scan use one occupancy implementation. Re-exported for the existing call sites.
pub use crate::domain::donor::{donor_interior_at, DonorInterior};

/// First-class splice summary, derived from the per-side `lag_decision` (mono): the registration `step`
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
    /// **`step_ms` reliability flag:** true when *either* shoulder's `lag_decision` peak was edge-pinned
    /// (search-exhausted, [`LagSummary::edge_pinned`]). The step is `post_lag − pre_lag`, so a shoulder
    /// whose peak was clipped at the ±`max_lag` boundary makes `step_ms` GIGO (ledger A5/C6). `None` when
    /// neither shoulder carries the flag (older fingerprints).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub edge_pinned: Option<bool>,
}

/// **Dual-fit viability** — the offline repair simulation promoted into the scan (ledger C3/C7; supersedes
/// the `diag_splice_dualfit` harness, which decoded B separately and drifted from the scan). Each shoulder
/// is placed with `seam_local_peak` re-anchored on nominal `b_mapped` (pre at `b_mapped_start`, post at
/// `b_mapped_start + gap_frames`, ±`SEAM_LOCAL_SEARCH_MS`) — **not** on the gross 1 s `lag_decision` —
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
    /// **Experiment, `TEMP-phat-seam-scoring-plan.md`.** Phase-transform (GCC-PHAT) equivalent of
    /// `pre_seam_r`/`pre_seam_prom`/`pre_seam_z`, computed over the exact same ±30 ms seam-uniqueness
    /// window (own peak, own prominence over its best rival, own whole-window z-score) but with the
    /// cross-power spectrum whitened by phase instead of Pearson-normalized. **Diagnostic-only: nothing
    /// gates on this.** Not comparable numerically to `pre_seam_r` (phase-only normalization has no
    /// `[-1, 1]` scale) — compare curve shape (prominence, z), not magnitude. `None` when the sweep
    /// window is out of range, same as the Pearson triplet; also `None` on any dump written before this
    /// field existed (not the same as a measured, uninformative reading).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_phat_r: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_phat_r: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_phat_prom: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_phat_prom: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_phat_z: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_phat_z: Option<f64>,
    /// **Same-lag agreement, `TEMP-phat-seam-scoring-plan.md`.** Fractional-interpolated peak lag (ms)
    /// of each curve's own chosen peak — the Pearson pair mirrors `pre_seam_r`'s own search, the PHAT
    /// pair mirrors `pre_seam_phat_r`'s. Lets a reader check whether PHAT sharpens the *same* lag
    /// Pearson already favors (agreement) or promotes a different one (disagreement) — prominence alone
    /// can't distinguish those, since the two curves aren't on a comparable scale. `None` under the same
    /// conditions as the corresponding `*_r`/`*_prom`/`*_z` field.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_lag_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_lag_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_phat_lag_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_phat_lag_ms: Option<f64>,
    /// **Envelope rescue** — the coarse wide-lag envelope moved this shoulder off its nominal center
    /// before the fine search re-ran there. A shoulder only becomes a candidate when it reads below
    /// `min_fill_correlation` at nominal, so `true` means "this seam's `r` was earned somewhere the
    /// ±600 ms sweep alone could not reach". `None` on dumps written before 2026-08-28, when this
    /// probe had no envelope at all — not the same thing as `Some(false)`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_rescued: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_rescued: Option<bool>,
    /// The **accepted** search's peak sat on the boundary of the lags actually scored, so a larger lag
    /// was never tried. Read as "these lags are a floor, not a measurement" — **not** as "the true peak
    /// lies beyond reach": a flat noise curve peaks mid-sweep and reads `false`. For reach, read
    /// `*_seam_z`. `None` on pre-2026-08-28 dumps.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_seam_edge_pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_seam_edge_pinned: Option<bool>,
    /// A non-positive `bridge_frames` exists only because both seams clamped at B frame 0; unclamped
    /// they still leave donor material. Distinguishes a manufactured abut from a real one, which is why
    /// production's `shoulders_abut` skip carries the same tag. `None` on pre-2026-08-28 dumps.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seams_clamped: Option<bool>,
}

/// One rung of the post-window ladder probe (`docs/dev/archive/TEMP-dual-fit-post-window-ladder-plan.md`
/// §3.2) — a same-start re-lock of the **post** seam at `window_ms`, envelope off, raw
/// `mono(refined_end .. refined_end + window_ms)` vs `B` around nominal `b_mapped_end` ±600 ms.
/// Omitted (not recorded, not a fake `0`) when the window runs off A or the B search is OOB.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PostWindowLadderRung {
    /// The ladder rung this row measured — one of `{250, 375, 500, 625, 750, 1000}`.
    pub window_ms: u32,
    /// `seam_local_peak` of this rung's template against `B` at its own best lag.
    #[serde(
        serialize_with = "ser_nan_as_null",
        deserialize_with = "de_null_as_nan"
    )]
    pub post_r: f64,
    pub post_lag_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_z: Option<f64>,
    /// Prominence of this rung's placement peak over its best rival within ±30 ms — same helper
    /// `splice_dualfit`'s `post_seam_prom` uses. `None` when the sweep window is out of range.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_prom: Option<f64>,
    /// This rung's template vs `B` at the (envelope-off, 250 ms) pre seam's own offset — is the step
    /// necessary? `NaN` when that window is OOB.
    #[serde(
        serialize_with = "ser_nan_as_null",
        deserialize_with = "de_null_as_nan"
    )]
    pub post_global_r: f64,
    /// `post_r` vs `post_global_r + DUALFIT_STEP_REAL_MARGIN` (0.15), the same `partial_cmp` refusal
    /// `try_dual_fit` uses. `None` only when that comparison is unordered (a NaN operand).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub step_real: Option<bool>,
    /// This rung's post lag minus the envelope-off 250 ms pre lag (both vs their own nominals).
    pub lag_minus_pre_ms: f64,
    /// This rung's post lag minus the 250 ms rung's post lag. `None` when the 250 ms rung itself was
    /// omitted (window OOB).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lag_minus_w250_ms: Option<f64>,
    /// RMS of this rung's A template, for confirming (or not) that level does not predict `window_ms`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_post_rms_db: Option<f32>,
}

/// One rung of the pre-window ladder probe (`docs/dev/archive/TEMP-dual-fit-pre-window-ladder-plan.md`
/// §3.2) — a same-end re-lock of the **pre** seam at `window_ms`, envelope off, raw
/// `mono(refined_start − window_ms .. refined_start)` vs `B` around nominal `b_mapped_start` ±600 ms.
/// Omitted (not recorded, not a fake `0`) when the window runs off A or the B search is OOB.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PreWindowLadderRung {
    /// The ladder rung this row measured — one of `{250, 375, 500, 625, 750, 1000}`.
    pub window_ms: u32,
    /// `seam_local_peak` of this rung's template against `B` at its own best lag.
    #[serde(
        serialize_with = "ser_nan_as_null",
        deserialize_with = "de_null_as_nan"
    )]
    pub pre_r: f64,
    pub pre_lag_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_z: Option<f64>,
    /// Prominence of this rung's placement peak over its best rival within ±30 ms — same helper
    /// `splice_dualfit`'s `pre_seam_prom` uses. `None` when the sweep window is out of range.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_prom: Option<f64>,
    /// This rung's template vs `B` at the true post-lock conjunct (the envelope-off, 250 ms post
    /// seam's *actual measured* offset, gap length back) — is the step necessary? Deliberately NOT
    /// the nominal lag-0 anchor `post_window_ladder`'s `post_global_r` shortcuts to (plan §0 anchor
    /// note). `NaN` when that window is OOB.
    #[serde(
        serialize_with = "ser_nan_as_null",
        deserialize_with = "de_null_as_nan"
    )]
    pub pre_global_r: f64,
    /// `pre_r` vs `pre_global_r + DUALFIT_STEP_REAL_MARGIN` (0.15), the same `partial_cmp` refusal
    /// `try_dual_fit` uses. `None` only when that comparison is unordered (a NaN operand).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub step_real: Option<bool>,
    /// This rung's pre lag minus the envelope-off 250 ms post lag (both vs their own nominals).
    pub lag_minus_post_ms: f64,
    /// This rung's pre lag minus the 250 ms rung's pre lag. `None` when the 250 ms rung itself was
    /// omitted (window OOB).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lag_minus_w250_ms: Option<f64>,
    /// RMS of this rung's A template, for confirming (or not) that level does not predict `window_ms`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_pre_rms_db: Option<f32>,
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
    /// Seam-shape label. `None` on the production projection path (was hardcoded `""`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seam_shape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fit_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature_mode: Option<String>,
    /// On `tier: "skip"`: closest failing bracket's [`FailureStage`] (`structure_align` /
    /// `structure_floor` / `waveform_floor` / `residual`) — the same reduction as analyzer
    /// `closest_failure_stage`. **Not** a production [`crate::domain::GapPatchSkipReason`]: the
    /// fingerprint path enumerates brackets (`any_ok`) and does not run the production patch gate.
    /// `None` on patch, or when no bracket recorded a failure stage. Legacy dumps may still say
    /// `correlation_below_threshold` (placeholder from before this was fingerprint-native).
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
    /// and reflects `dual_fit_at_nominal`'s accept conditions over dump measurements — production still
    /// re-validates the assembled seams and can decline. A-border construction matches production
    /// (`mono(refined ± w)` in `splice_dualfit_at`; F14). Read as "dual-fit rescue candidate", not
    /// "this was patched".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dual_fit_rescue: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::super::contract::{
        stamp, DONOR_INTERIOR_ALIGNED_CONTRACT, RESIDUAL_CONTRACT, SEAM_PROBE_CONTRACT,
    };
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

    /// The stamp answers "which build", so the two ways it can say "I don't know" must stay
    /// distinguishable from a real answer: an absent `commit` is *unanswerable*, and `dirty: false`
    /// beside it is not a claim that the tree was clean. Pinned because a later reader that treats a
    /// missing commit as "some unknown clean build" would draw exactly the wrong conclusion.
    #[test]
    fn build_stamp_reports_the_compiled_crate_and_survives_a_round_trip() {
        let stamp = BuildStamp::current();
        assert_eq!(stamp.version, env!("CARGO_PKG_VERSION"));
        if let Some(commit) = stamp.commit.as_deref() {
            assert!(
                !commit.is_empty() && commit.chars().all(|c| c.is_ascii_hexdigit()),
                "a present commit must be a hash, not a git error message: {commit:?}"
            );
        }
        let json = serde_json::to_string(&stamp).expect("serialize");
        assert_eq!(
            serde_json::from_str::<BuildStamp>(&json).expect("deserialize"),
            stamp
        );
        // `dirty` is not `skip_serializing_if`: false must be written, so a reader can tell
        // "stamped, clean" from "no stamp at all".
        assert!(
            json.contains("\"dirty\""),
            "dirty must always serialize: {json}"
        );
    }

    /// Corpora written before the stamp existed must keep parsing, and an unstamped corpus must not
    /// grow an empty `build` key that would read as an answer.
    #[test]
    fn absent_build_stamp_omits_the_key_and_parses_from_a_corpus_without_it() {
        let meta = SourceMeta {
            a_source: file_source(&[0.0; 4], 48_000, 2, None),
            b_source: file_source(&[0.0; 2], 48_000, 1, None),
            scan_recipe: CorpusScanRecipe::default(),
            gap_count: 0,
            incomparable: None,
            gate_recipe: None,
            probe_recipe: None,
            not_measured: Vec::new(),
            alignment: None,
            build: None,
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        assert!(
            !json.contains("build"),
            "absent build must omit the key: {json}"
        );
        let back: SourceMeta = serde_json::from_str(&json).expect("a pre-stamp corpus must parse");
        assert_eq!(back.build, None);
    }

    #[test]
    fn incomparable_reason_omitted_when_absent_and_round_trips() {
        let meta = SourceMeta {
            a_source: file_source(&[0.0; 4], 48_000, 2, None),
            b_source: file_source(&[0.0; 2], 48_000, 1, None),
            scan_recipe: CorpusScanRecipe::default(),
            gap_count: 0,
            incomparable: None,
            gate_recipe: None,
            probe_recipe: None,
            not_measured: Vec::new(),
            alignment: None,
            build: None,
        };
        assert!(
            !serde_json::to_string(&meta)
                .unwrap()
                .contains("gate_recipe"),
            "absent gate_recipe must omit key"
        );
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
        let value = serde_json::to_value(&meta).expect("source json");
        assert!(
            value.get("alignment").is_none(),
            "absent alignment must omit key"
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
            residual_margin_db: None,
        }
    }

    #[test]
    fn bracket_failure_progress_uses_structure_on_structure_floor() {
        let mut b = bracket(Some(FailureStage::StructureFloor));
        b.structure_pre = Some(0.4);
        b.structure_post = Some(0.5);
        b.seam_pre = Some(0.99); // must be ignored for structure_floor
        b.seam_post = Some(0.99);
        assert!((bracket_failure_progress(&b) - 0.4).abs() < 1e-12);

        let mut w = bracket(Some(FailureStage::WaveformFloor));
        w.seam_pre = Some(0.2);
        w.seam_post = Some(0.3);
        assert!((bracket_failure_progress(&w) - 0.2).abs() < 1e-12);
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
            pre_seam_phat_r: None,
            post_seam_phat_r: None,
            pre_seam_phat_prom: None,
            post_seam_phat_prom: None,
            pre_seam_phat_z: None,
            post_seam_phat_z: None,
            pre_seam_lag_ms: None,
            post_seam_lag_ms: None,
            pre_seam_phat_lag_ms: None,
            post_seam_phat_lag_ms: None,
            pre_seam_rescued: None,
            post_seam_rescued: None,
            pre_seam_edge_pinned: None,
            post_seam_edge_pinned: None,
            seams_clamped: None,
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

    /// The seam gate alone is NOT the accept condition — production also requires non-crossed
    /// shoulders (the caller-owned lock check in `dual_fit_at_nominal`), a real step, a continuous aligned donor, and a non-quiet nominal donor. Each must
    /// independently veto, or the flag over-promises repair coverage on exactly the gaps production
    /// declines (the curated `04_program_quiet` regression).
    #[test]
    fn dual_fit_rescue_requires_every_try_dual_fit_condition() {
        let scored = [bracket(Some(FailureStage::WaveformFloor))];
        let pass = splice_dualfit(true);
        let occupied = donor(0.0, true);

        // Step not real: the post seam at the pre lag is just as good ⇒ a rigid single-lag map
        // explains it. Since 2026-09-01 (§3.5) this still rescues: `try_dual_fit` assembles a
        // trim-0 fill off the rigid map rather than declining.
        let mut rigid = splice_dualfit(true);
        rigid.post_seam_global_r = rigid.post_seam_r;
        assert_eq!(
            dual_fit_rescue_flag(&rescuable(&scored, &rigid, &occupied, &occupied)),
            Some(true)
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
                bin_ms: Some(50),
                profile_db: vec![-45.0, -24.0, -45.0],
                floor_db: Some(-120.0),
                speech_peak_db: Some(-22.0),
                noise_floor_db: -45.0,
                gap_floor_db: -120.0,
            },
            silence: Some(SilenceProfile {
                collar_rms_peak_ratio: 0.42,
                collar_above_relative_floor: true,
                silence_peak_fraction: 0.01,
            }),
            contour: Some(ContourInfo {
                has_anchor_seam_contour: true,
                pre_flatness: 0.1,
                post_flatness: 0.1,
            }),
            anchors: Some(AnchorSet {
                pre: vec![AnchorPoint {
                    time_secs: 831.3,
                    source: AnchorSourceKind::EnergyPeak,
                    prominence: 0.0075,
                    rms_db: -24.0,
                }],
                post: vec![],
            }),
            constellation: Some(ConstellationSet {
                bin_ms: 10.0,
                min_spacing_ms: 10.0,
                context_secs: 3.0,
                max_lag_ms: 2000,
                vote_bin_ms: 30.0,
                redraw_trials: 100,
                redraw_percentile: 95.0,
                pre: ConstellationShoulder {
                    a: ConstellationSide {
                        span_start_secs: 829.3,
                        span_end_secs: 832.304,
                        edge_pinned: false,
                        n: 1,
                        points: vec![ConstellationPoint {
                            t_seam_ms: -120.0,
                            time_secs: 832.184,
                            prominence: 0.02,
                        }],
                    },
                    b: ConstellationSide {
                        span_start_secs: 821.5,
                        span_end_secs: 828.752,
                        edge_pinned: false,
                        n: 0,
                        points: vec![],
                    },
                    n_a: 1,
                    n_b: 0,
                    hist_peak_lag_ms: None,
                    hist_peak_votes: None,
                    hist_second_votes: None,
                    redraw_p95_votes: None,
                    reason: ConstellationReason::NoTrain,
                },
                post: ConstellationShoulder {
                    // Zero-width: this is the "not attempted" case (the post window ran off the end
                    // of the file), not "attempted but empty" — a real `Unmeasured` shoulder's spans
                    // collapse to a point rather than spanning the nominal context/search width.
                    a: ConstellationSide {
                        span_start_secs: 837.133,
                        span_end_secs: 837.133,
                        edge_pinned: true,
                        n: 0,
                        points: vec![],
                    },
                    b: ConstellationSide {
                        span_start_secs: 831.581,
                        span_end_secs: 831.581,
                        edge_pinned: true,
                        n: 0,
                        points: vec![],
                    },
                    n_a: 0,
                    n_b: 0,
                    hist_peak_lag_ms: None,
                    hist_peak_votes: None,
                    hist_second_votes: None,
                    redraw_p95_votes: None,
                    reason: ConstellationReason::Unmeasured,
                },
            }),
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
            lag_editorial: None,
            lag_decision: None,
            residual: stamp(
                Some(ResidualInfo {
                    chosen_pre_db: Some(-42.0),
                    chosen_post_db: Some(-38.0),
                    floor_pre_db: Some(-40.0),
                    floor_post_db: Some(-39.0),
                    floor_source_pre: Some(SeamFloorSource::Border),
                    floor_source_post: Some(SeamFloorSource::Border),
                    informative: true,
                    uninformative_pre: None,
                    uninformative_post: None,
                    placement_slide_frames: Some(0),
                    max_lag_frames: Some(0),
                }),
                RESIDUAL_CONTRACT,
            ),
            seam_probe: stamp(
                Some(SeamProbeFingerprint {
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
                SEAM_PROBE_CONTRACT,
            ),
            donor_interior_aligned: stamp(
                Some(DonorInterior {
                    rms_db: -28.0,
                    silence_fraction: 0.0,
                    longest_silence_ms: 0.0,
                    continuous: true,
                    basis: None,
                }),
                DONOR_INTERIOR_ALIGNED_CONTRACT,
            ),
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
            post_window_ladder: None,
            pre_window_ladder: None,
            donor_interior_nominal: None,
            b_levels: None,
            outcome: Some(GateOutcome {
                plan_kind: "fillable".into(),
                tier: "hard_skip".into(),
                seam_shape: Some("symmetric_weak".into()),
                fit_path: Some("baseline_only".into()),
                signature_mode: Some("energy".into()),
                skip_reason: Some("boundary correlation below threshold".into()),
                dual_fit_rescue: None,
            }),
            equivalence_diagnostic: None,
            equivalence_production: None,
        };
        let json = serde_json::to_string(&fp).expect("serialize");
        let back: GapFingerprint = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(fp, back);
        // Absent optionals stay out of the wire form.
        assert!(!json.contains("\"lag_editorial\""));
        assert!(!json.contains("\"lag_decision\""));
        assert!(!json.contains("\"brackets\""));
        assert!(!json.contains("\"keep_path_shift\""));
    }

    /// R1 rename: pre-2026-08-07 corpora wrote `equivalence` / `scan_equivalence`. Those keys must
    /// still deserialize (serde aliases), and must re-serialize under the **new** names only — the
    /// rename is not dual-write, so a round-tripped old dump carries no old key.
    #[test]
    fn legacy_equivalence_keys_deserialize_and_reserialize_renamed() {
        use crate::domain::gap_equivalence::GapEquivalenceClass;

        let old = r#"{
            "index": 0,
            "tier": "summary",
            "sample_rate": 48000,
            "channels": 2,
            "levels": {"noise_floor_db": -60.0, "gap_floor_db": -70.0},
            "geometry": {
                "a_start_secs": 1.0,
                "a_end_secs": 2.0,
                "a_refined_start_secs": 1.0,
                "a_refined_end_secs": 2.0,
                "duration_secs": 1.0
            },
            "equivalence": {"class": "repairable_dropout", "drop": false},
            "scan_equivalence": {"class": "shared_silence", "drop": true}
        }"#;
        let fp: GapFingerprint = serde_json::from_str(old).expect("legacy keys deserialize");
        assert_eq!(
            fp.equivalence_diagnostic_verdict().map(|v| v.class),
            Some(GapEquivalenceClass::RepairableDropout)
        );
        assert_eq!(
            fp.equivalence_production_verdict().map(|v| v.class),
            Some(GapEquivalenceClass::SharedSilence)
        );

        let json = serde_json::to_string(&fp).expect("serialize");
        assert!(json.contains("\"equivalence_diagnostic\""));
        assert!(json.contains("\"equivalence_production\""));
        assert!(!json.contains("\"scan_equivalence\""));
        // `equivalence_*` contains `equivalence`, so check for the bare old key exactly.
        assert!(!json.contains("\"equivalence\""));

        // New keys round-trip unchanged.
        let back: GapFingerprint = serde_json::from_str(&json).expect("new keys deserialize");
        assert_eq!(fp, back);
    }

    /// R2 rename: pre-2026-08-07 corpora wrote `lag` / `baseline_lag`. Same contract as R1's — old
    /// keys deserialize via alias, new keys only on the way out.
    ///
    /// The two halves are asserted separately on purpose: `lag` and `baseline_lag` alias onto
    /// *different* fields, and a copy-paste that pointed both aliases at one field would still parse
    /// and still round-trip — it would just silently merge the editorial and decision placements,
    /// which is precisely the confusion this rename exists to end.
    #[test]
    fn legacy_lag_keys_deserialize_and_reserialize_renamed() {
        let old = r#"{
            "index": 0,
            "tier": "full",
            "sample_rate": 48000,
            "channels": 2,
            "geometry": {
                "a_start_secs": 1.0,
                "a_end_secs": 2.0,
                "a_refined_start_secs": 1.0,
                "a_refined_end_secs": 2.0,
                "duration_secs": 1.0
            },
            "levels": {"noise_floor_db": -60.0, "gap_floor_db": -70.0},
            "lag": {"pre_anchor": [{"window_ms": 250, "max_lag_ms": 200, "lag0_r": 0.1,
                     "peak_r": 0.91, "channel": "mono", "peak_lag_samples": -100, "frac_lag_samples": -100.0,
                     "frac_lag_ms": -2.08, "verdict": "timing_offset"}], "post_anchor": []},
            "baseline_lag": {"pre_anchor": [{"window_ms": 1000, "max_lag_ms": 600, "lag0_r": 0.2,
                     "peak_r": 0.77, "channel": "mono", "peak_lag_samples": -50, "frac_lag_samples": -50.0,
                     "frac_lag_ms": -1.04, "verdict": "timing_offset"}], "post_anchor": []}
        }"#;
        let fp: GapFingerprint = serde_json::from_str(old).expect("legacy keys deserialize");

        // Distinct placements must stay distinct — `peak_r` is the discriminator.
        assert_eq!(
            fp.lag_editorial
                .as_ref()
                .and_then(|l| l.pre_anchor.first())
                .map(|s| s.peak_r),
            Some(0.91),
            "`lag` must land on lag_editorial"
        );
        assert_eq!(
            fp.lag_decision
                .as_ref()
                .and_then(|l| l.pre_anchor.first())
                .map(|s| s.peak_r),
            Some(0.77),
            "`baseline_lag` must land on lag_decision"
        );

        let json = serde_json::to_string(&fp).expect("serialize");
        assert!(json.contains("\"lag_editorial\""));
        assert!(json.contains("\"lag_decision\""));
        assert!(!json.contains("\"baseline_lag\""));
        // `lag_*` contains `lag`, so check for the bare old key exactly (the R1 substring trap).
        assert!(!json.contains("\"lag\""));

        let back: GapFingerprint = serde_json::from_str(&json).expect("new keys deserialize");
        assert_eq!(fp, back);
    }

    /// R3 rename: pre-2026-08-07 corpora wrote `donor_interior` for the **aligned** span.
    ///
    /// The trap here is the reverse of R1/R2's. `donor_interior_nominal` *starts with* the old key,
    /// and its sibling was **not** renamed — so an old dump carries both `donor_interior` and
    /// `donor_interior_nominal` side by side. serde matches whole keys, so the alias cannot swallow
    /// the nominal one, but a fixture rewrite done by naive substring replace absolutely can. Both
    /// spans are given distinct `rms_db` so a merge or a swap fails loudly rather than round-tripping.
    #[test]
    fn legacy_donor_interior_key_deserializes_without_swallowing_its_nominal_sibling() {
        let old = r#"{
            "index": 0,
            "tier": "full",
            "sample_rate": 48000,
            "channels": 2,
            "geometry": {
                "a_start_secs": 1.0,
                "a_end_secs": 2.0,
                "a_refined_start_secs": 1.0,
                "a_refined_end_secs": 2.0,
                "duration_secs": 1.0
            },
            "levels": {"noise_floor_db": -60.0, "gap_floor_db": -70.0},
            "donor_interior": {"rms_db": -40.0, "silence_fraction": 0.02,
                               "longest_silence_ms": 12.0, "continuous": true},
            "donor_interior_nominal": {"rms_db": -80.0, "silence_fraction": 0.97,
                                       "longest_silence_ms": 940.0, "continuous": false}
        }"#;
        let fp: GapFingerprint = serde_json::from_str(old).expect("legacy key deserializes");

        assert_eq!(
            fp.donor_interior_aligned.as_ref().map(|d| d.rms_db),
            Some(-40.0),
            "`donor_interior` must land on the aligned span"
        );
        assert_eq!(
            fp.donor_interior_nominal.as_ref().map(|d| d.rms_db),
            Some(-80.0),
            "the nominal sibling must survive the alias untouched"
        );

        let json = serde_json::to_string(&fp).expect("serialize");
        assert!(json.contains("\"donor_interior_aligned\""));
        assert!(json.contains("\"donor_interior_nominal\""));
        // `donor_interior_*` contains `donor_interior`, so check for the bare old key exactly.
        assert!(!json.contains("\"donor_interior\""));

        let back: GapFingerprint = serde_json::from_str(&json).expect("new keys deserialize");
        assert_eq!(fp, back);
    }

    /// C2 wrapped six groups in [`Contracted`], whose `#[serde(flatten)]` deserializes through
    /// serde's buffered `Content` instead of each struct's own field visitor. That is a real change in
    /// how the bytes are read, so it is pinned rather than assumed: an old corpus (no `_contract`
    /// anywhere) must come back with every value intact, `None` contracts, and — since these dumps are
    /// not dual-write — no `_contract` on re-serialization either.
    ///
    /// `lag_editorial` and `seam_probe` are covered *here* rather than in
    /// `tests/equivalence_divergence.rs`: both are Tier-3, so no committed fixture carries them.
    #[test]
    fn contract_wrapped_groups_round_trip_a_contract_free_corpus() {
        let old = r#"{
            "index": 0,
            "tier": "full",
            "sample_rate": 48000,
            "channels": 2,
            "geometry": {
                "a_start_secs": 1.0,
                "a_end_secs": 2.0,
                "a_refined_start_secs": 1.0,
                "a_refined_end_secs": 2.0,
                "duration_secs": 1.0
            },
            "levels": {"noise_floor_db": -60.0, "gap_floor_db": -70.0},
            "lag": {"pre_anchor": [{"window_ms": 1000, "max_lag_ms": 600,
                                    "channel": {"selected": 1},
                                    "lag0_r": 0.02, "peak_r": 0.97, "peak_lag_samples": -12,
                                    "frac_lag_samples": -12.5, "frac_lag_ms": -0.26,
                                    "verdict": "timing_offset"}],
                    "post_anchor": []},
            "seam_probe": {"pre": {"waveform_r": 0.04, "recovered_r": 0.95,
                                   "recovered_lag_ms": -8.0, "bandlimited_r": 0.8,
                                   "spectrum_r": 0.85, "envelope_r": 0.9,
                                   "rms_db": -30.0, "snr_db": 20.0}}
        }"#;
        let fp: GapFingerprint = serde_json::from_str(old).expect("contract-free corpus");

        // Values survive the buffered read, reached through the accessors read sites should use.
        // `channel` is deliberately the **newtype** variant: `Content` buffering resolves an
        // externally-tagged enum differently from a plain unit variant, and `"mono"` alone would
        // leave that path unexercised.
        assert_eq!(
            fp.lag_editorial_fingerprint()
                .map(|l| (l.pre_anchor[0].peak_r, l.pre_anchor[0].channel)),
            Some((0.97, LagChannel::Selected(1)))
        );
        assert_eq!(
            fp.seam_probe_metrics()
                .and_then(|p| p.pre.as_ref())
                .map(|p| p.snr_db),
            Some(20.0)
        );
        // Absent on the wire ⇒ absent in the wrapper. Never fabricated on read.
        assert!(fp
            .lag_editorial
            .as_ref()
            .is_some_and(|w| w.contract.is_none()));
        assert!(fp.seam_probe.as_ref().is_some_and(|w| w.contract.is_none()));

        let json = serde_json::to_string(&fp).expect("serialize");
        assert!(
            !json.contains("_contract"),
            "reading an old corpus must not invent contracts: {json}"
        );
        let back: GapFingerprint = serde_json::from_str(&json).expect("re-read");
        assert_eq!(fp, back);
    }

    /// The dotted paths in [`PROJECTED_LAG_DECISION_FIELDS`] are strings the compiler cannot check
    /// against the struct they describe, and they are written into `source.not_measured` where a
    /// stale spelling reads as "this field **was** measured". Pin each one to a key the type
    /// actually emits.
    ///
    /// [`LEGACY_PROJECTED_LAG_DECISION_FIELDS`] is checked the opposite way: its entries must
    /// *disagree* only in the prefix. It exists to be recognized on old corpora, so it has to stay a
    /// faithful transcription of what those dumps wrote — not track the current spelling.
    #[test]
    fn projected_lag_decision_paths_name_keys_the_type_emits() {
        // Deserialize-then-reserialize so both halves come from the types themselves: the group key
        // from `GapFingerprint`, the field keys from `LagSummary`.
        let fp: GapFingerprint = serde_json::from_str(
            r#"{
            "index": 0,
            "tier": "full",
            "sample_rate": 48000,
            "channels": 2,
            "geometry": {
                "a_start_secs": 1.0,
                "a_end_secs": 2.0,
                "a_refined_start_secs": 1.0,
                "a_refined_end_secs": 2.0,
                "duration_secs": 1.0
            },
            "levels": {"noise_floor_db": -60.0, "gap_floor_db": -70.0},
            "lag_decision": {"pre_anchor": [{"window_ms": 1000, "max_lag_ms": 600, "lag0_r": 0.2,
                     "peak_r": 0.77, "channel": "mono", "peak_lag_samples": -50, "frac_lag_samples": -50.0,
                     "frac_lag_ms": -1.04, "verdict": "timing_offset"}], "post_anchor": []}
        }"#,
        )
        .expect("parse");
        let wire = serde_json::to_value(&fp).expect("serialize");
        let group = wire
            .get("lag_decision")
            .expect("the group key the dotted paths are rooted at");
        let row = &group["pre_anchor"][0];

        for path in PROJECTED_LAG_DECISION_FIELDS {
            let (g, field) = path.split_once('.').expect("dotted path");
            assert_eq!(g, "lag_decision", "{path}: unexpected group");
            assert!(
                row.get(field).is_some(),
                "{path}: `{field}` is not a key a shoulder row emits"
            );
        }
        assert_eq!(
            LEGACY_PROJECTED_LAG_DECISION_FIELDS.len(),
            PROJECTED_LAG_DECISION_FIELDS.len(),
            "the legacy transcription must cover the same fields"
        );
        for (legacy, current) in LEGACY_PROJECTED_LAG_DECISION_FIELDS
            .iter()
            .zip(PROJECTED_LAG_DECISION_FIELDS)
        {
            assert_eq!(
                legacy.strip_prefix("baseline_lag."),
                current.strip_prefix("lag_decision."),
                "{legacy} / {current}: same field, different prefix"
            );
        }
    }

    #[test]
    fn residual_silence_floor_sentinel_deserializes_as_none() {
        let r: ResidualInfo = serde_json::from_str(
            r#"{"chosen_pre_db":-120.0,"chosen_post_db":null,"floor_pre_db":-40.0,"floor_post_db":-39.0,"informative":false}"#,
        )
        .expect("parse");
        assert_eq!(r.chosen_pre_db, None);
        assert_eq!(r.chosen_post_db, None);
        assert_eq!(r.floor_pre_db, Some(-40.0));
        assert_eq!(r.floor_post_db, Some(-39.0));
        let wire = serde_json::to_string(&r).expect("serialize");
        assert!(!wire.contains("-120"));
        assert!(!wire.contains("chosen_pre_db"));
        assert!(!wire.contains("chosen_post_db"));
        // The pre-2026-08-03 payload above carries no floor source at all, and must still parse.
        assert_eq!(r.floor_source_pre, None);
        assert_eq!(r.floor_source_post, None);
    }

    /// The floor source reaches the wire in snake_case and survives a round trip.
    ///
    /// The case that motivates the field: `floor_post_db` is absent *because the probe found no
    /// reference window*, which on the wire is indistinguishable from a suppressed −120 reading
    /// unless `floor_source_post: "none"` is there to say so.
    #[test]
    fn floor_source_round_trips_and_disambiguates_an_absent_floor() {
        let r = ResidualInfo {
            chosen_pre_db: Some(-42.0),
            chosen_post_db: None,
            floor_pre_db: Some(-40.0),
            floor_post_db: None,
            floor_source_pre: Some(SeamFloorSource::Walked),
            floor_source_post: Some(SeamFloorSource::None),
            informative: true,
            uninformative_pre: None,
            uninformative_post: Some(ResidualUninformative::NoReferenceWindow),
            placement_slide_frames: Some(0),
            max_lag_frames: Some(4410),
        };
        let wire = serde_json::to_string(&r).expect("serialize");
        assert!(
            wire.contains(r#""floor_source_pre":"walked""#),
            "walked floor must name itself on the wire: {wire}"
        );
        assert!(
            wire.contains(r#""floor_source_post":"none""#),
            "an unmeasured side must say so rather than just omitting the dB: {wire}"
        );
        let back: ResidualInfo = serde_json::from_str(&wire).expect("parse");
        assert_eq!(back, r);
    }

    /// The abstention reason and the lag reach reach the wire, and a dump that predates them still
    /// parses — landing on today's behaviour (`None` / `0`) rather than failing.
    #[test]
    fn abstention_reason_and_reach_round_trip_and_stay_back_compatible() {
        let r = ResidualInfo {
            chosen_pre_db: Some(-42.0),
            chosen_post_db: Some(-30.0),
            floor_pre_db: Some(-40.0),
            floor_post_db: Some(-10.0),
            floor_source_pre: Some(SeamFloorSource::Border),
            floor_source_post: Some(SeamFloorSource::Border),
            informative: false,
            uninformative_pre: None,
            uninformative_post: Some(ResidualUninformative::FloorAboveOkDb),
            placement_slide_frames: Some(9_000),
            max_lag_frames: Some(4_410),
        };
        let wire = serde_json::to_string(&r).expect("serialize");
        assert!(
            wire.contains(r#""uninformative_post":"floor_above_ok_db""#),
            "the reason must name itself on the wire: {wire}"
        );
        // A side with a usable floor says nothing rather than writing a "fine" reason.
        assert!(!wire.contains("uninformative_pre"), "{wire}");
        assert_eq!(
            serde_json::from_str::<ResidualInfo>(&wire).expect("parse"),
            r
        );

        // Pre-2026-08-05 payload: no reason, no reach.
        let old: ResidualInfo = serde_json::from_str(
            r#"{"chosen_pre_db":-42.0,"chosen_post_db":-41.0,"floor_pre_db":-40.0,"floor_post_db":-40.0,"informative":true}"#,
        )
        .expect("parse");
        assert_eq!(old.uninformative_pre, None);
        assert_eq!(old.uninformative_post, None);
        assert_eq!(old.placement_slide_frames, None);
        assert_eq!(old.max_lag_frames, None);
    }

    /// The threshold `floor_above_ok_db` is relative to is recorded, and a recipe written before it
    /// existed still parses.
    #[test]
    fn gate_recipe_carries_the_floor_ok_threshold_and_stays_back_compatible() {
        let json = r#"{"min_structure_match_score":0.5,"min_fill_correlation":0.35,"fill_absolute_floor":0.2,"fill_marginal_margin":0.05,"short_gap_mean_correlation_secs":1.5,"short_gap_one_strong_seam_fallback":true,"residual_headroom_margin_db":6.0,"residual_gate":"veto"}"#;
        let old: CorpusGateRecipe = serde_json::from_str(json).expect("parse");
        assert_eq!(
            old.residual_floor_ok_db, None,
            "a missing key must read as 'recipe predates the field', not as today's default"
        );

        let current = CorpusGateRecipe {
            residual_floor_ok_db: Some(-15.0),
            ..old
        };
        let wire = serde_json::to_string(&current).expect("serialize");
        assert!(wire.contains(r#""residual_floor_ok_db":-15.0"#), "{wire}");
        assert_eq!(
            serde_json::from_str::<CorpusGateRecipe>(&wire).expect("parse"),
            current
        );
    }
}
