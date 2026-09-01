//! Whole-program A↔B level relationship, measured as a **paired per-block difference**.
//!
//! Plan: `docs/dev/archive/TEMP-ab-program-levels-plan.md`. Capture only — nothing here feeds a gain.
//!
//! # Why paired
//!
//! The obvious design (summarize A, summarize B, subtract) compares two *independent* level
//! distributions, which is only a level comparison when the two files have similar silence duty
//! cycles and similar content mix. Neither holds here: **A is the damaged file**, so its dropouts
//! add mass at the bottom of its histogram and depress every rank statistic, and B is frequently a
//! different edit — which is *why* it is the donor. Both differences would surface as a phantom
//! level offset.
//!
//! Pairing removes them. Each sample is the same audio measured twice: content mix cancels,
//! dropouts and leading/trailing pads drop out of the pairing instead of biasing it (so no head/tail
//! trim rule is needed at all), and regions where B holds other material become outliers that the
//! median discards while [`AbLevelOffset::spread_db`] rises to show it happened.
//!
//! # What the offset includes
//!
//! Master/mix level differences and **decoder gain** (AC-3 dialnorm, DRC, decode-path scaling) —
//! the latter deliberately, since a fill inherits it. A channel-count mismatch is a *confound*:
//! [`block_rms_db`](crate::domain::policies) is RMS over interleaved samples, so a stereo/mono pair
//! carries a correlation-dependent constant indistinguishable from master gain. That case still
//! reports, with [`AbLevelOffset::channels_comparable`] false.

use serde::Serialize;

use crate::domain::level_stats::{median_db, median_of_sorted, percentile_of_sorted, sort_db};
use crate::domain::policies::{absolute_silence_floor_db, BlockLevel, BLOCK_LEVEL_FLOOR_DB};

/// Tolerance for the steadiness filter — a block is paired only when both its A-side neighbours sit
/// within this many dB of it. Unvalidated starting point (no corpus run yet); nothing behavioural
/// keys off it, a wrong value only widens `spread_db`.
pub const STEADY_TOLERANCE_DB: f64 = 3.0;

/// Minimum accepted pairs before an offset is reported at all. Below it the summary abstains rather
/// than inventing an offset from a handful of blocks.
pub const MIN_PAIRS: usize = 50;

/// Minimum accepted pairs before a collar offset may be **applied as a gain**.
///
/// **Necessary, not sufficient** — set by the 51-pair / 1073-gap corpus census, arm 4
/// (`docs/dev/archive/TEMP-local-paired-gain-plan.md` § 4b Result 4, `W` = 4.0 on the shoulder-corrected
/// map). 711 gaps (66.3%) clear it; raising it to 16 moves the median residual only 0.136 → 0.117
/// dB and costs 167 gaps, so the extra strictness buys nothing.
///
/// What it does **not** do is dispose of the residual tail, which is what it was originally
/// expected to carry. At `W` = 4.0 the largest residual in the corpus (9.347 dB) is read off 26
/// pairs and does not move at any floor. Reading the eleven worst collars by hand says why: eight
/// are one-sided, and every two-sided one disagrees between its shoulders by 3.9–9.6 dB. Sparsity
/// was only ever a proxy for dispersion, and [`MAX_LOCAL_SE_DB`] is the axis that actually
/// separates the tail. Both gates are required at apply; neither substitutes for the other.
///
/// **Deliberately not read by [`collar_level_offset`].** A floor inside the estimator would censor
/// the dump at exactly the value the census exists to choose — the reading would stop at the rail
/// and the rail would look like the answer, which is how `max_fill_gain_db` survived two policy
/// changes at a number nobody picked (`settled.md` § E) before being deleted on 2026-08-31. The
/// estimator measures; this is policy, checked where a gain is applied —
/// [`collar_residual_gain`](crate::domain::collar_residual_gain), since 2026-08-31.
pub const MIN_LOCAL_PAIRS: usize = 8;

/// Maximum standard error (dB) of a collar's median before it may be **applied as a gain**, paired
/// with the requirement that both shoulders contributed at least one pair.
///
/// **The axis is the finding; the threshold is fitted to this corpus.** Arm 4 (§ 4b Result 4) put
/// the corpus's entire residual tail behind one dispersion axis: over 1073 gaps, admitting only
/// collars with `n_pairs >= MIN_LOCAL_PAIRS`, both sides present, and `se_db() <= 1.0` leaves a
/// maximum reading of 1.573 dB against 9.347 unadmitted, and takes 12 of the 14 readings over 1 dB
/// with it while costing 21 of 583 gaps — a 57% hit rate against a 0.4% base rate. That ratio is
/// what distinguishes a gate finding broken collars from one censoring large true corrections, and
/// it is checkable: a genuine 6 dB offset measured on a clean collar has a low SE and is **kept**.
/// The four corpus collars over 6 dB have SEs of 1.86 / 4.21 / 2.38 / 3.93 and side disagreements
/// of 4.4–7.0 dB. They were unreliable, not large.
///
/// **`se_db()`, `|pre_offset_db - post_offset_db|` and [`spread_db`](CollarLevelOffset::spread_db)
/// are interchangeable here** — at a fixed `n_pairs` they are the same measurement, and all three
/// cut the tail equally well on this corpus. Pick one; do not claim the other two as independent
/// confirmation of it.
pub const MAX_LOCAL_SE_DB: f64 = 1.0;

/// Refusal bound (dB) on the **local residual** `d_local − offset_prior`. Past it the collar is
/// reaching the wrong material and the fill is left alone — a refusal, not a clamp.
///
/// **A backstop, not the disposal.** Set from where the readings stop: on the admitted cohort
/// (§ 4b Result 4) the corpus's largest residual is 1.573 dB and p99 is 0.909, so 3.0 is roughly
/// twice the observed maximum and **fires on nothing at any value from 2.0 up**. The tail this was
/// drafted at 6.0 to catch is disposed of upstream by [`MIN_LOCAL_PAIRS`] together with
/// [`MAX_LOCAL_SE_DB`]; what is left for this bound is material 51 pairs did not produce. That is a
/// reason to set it where the readings stop and move on, not a reason to drop it — but it does mean
/// the acceptance tests belong on the admission gate, not here.
///
/// Placing it at 6.0 would have reproduced the failure mode it exists to avoid: on arm 3 that value
/// fired on 2 of 503 collars, sitting above p99, forfeiting the detection it was added for. It is
/// deliberately not inherited from `max_fill_gain_db` (12.0), which bounded an RMS ratio, was never
/// calibrated, and was deleted on 2026-08-31 with the estimator it belonged to, nor from
/// [`DEFAULT_MAX_PROGRAM_GAIN_DB`](crate::domain::DEFAULT_MAX_PROGRAM_GAIN_DB), which bounds the
/// whole-file offset rather than a residual against it.
///
/// Read by [`collar_residual_gain`](crate::domain::collar_residual_gain) since 2026-08-31 — the
/// apply half of that plan, which landed as its own Change after the census above.
pub const MAX_LOCAL_RESIDUAL_DB: f64 = 3.0;

/// Minimum pairs for a per-chunk median to be emitted.
pub const MIN_CHUNK_PAIRS: usize = 20;

/// Width of the per-chunk buckets on A's timeline (seconds).
pub const CHUNK_SECS: f64 = 600.0;

/// Fraction of the nominal block a level must span to be measured at all.
///
/// `SilenceRunScanner::feed` closes each decode bucket with whatever frames remain, so the last
/// block of a bucket can be a sliver — one frame, in the limit — whose RMS is not an estimate of
/// anything. Under the default recipe (10 s buckets defined by frame count, 100 ms blocks) that is
/// one runt at end of file; the window-fallback scan path, whose buckets are cut by time rather
/// than frame count, can produce one per bucket. Half a block keeps a 99 ms remainder, which is a
/// real measurement, and drops the slivers.
pub const MIN_BLOCK_FRACTION: f64 = 0.5;

/// Slack when deciding whether two blocks abut in time. Block edges are computed from frame counts
/// (`start + frames / rate`) while bucket edges come from the requested duration, so consecutive
/// blocks can miss abutting by a fraction of a sample. Well under a block, far above that error.
const NEIGHBOUR_SLACK_SECS: f64 = 0.001;

/// Active-level threshold used when the recipe's absolute silence floor is disabled (`0.0`).
const FALLBACK_ACTIVE_DB: f64 = -60.0;

/// Knobs for [`paired_level_offset`] / [`program_level_context`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgramLevelParams {
    /// Blocks at or below this dB are not program content. An **absolute** threshold, deliberately
    /// not the scanner's `silent` flag — see [`active_threshold_db`].
    pub active_db: f64,
    pub steady_tolerance_db: f64,
    pub min_pairs: usize,
    pub min_chunk_pairs: usize,
    pub chunk_secs: f64,
    /// Shortest block that may be measured — see [`MIN_BLOCK_FRACTION`]. `0.0` disables the filter.
    pub min_block_secs: f64,
}

impl Default for ProgramLevelParams {
    fn default() -> Self {
        Self {
            active_db: FALLBACK_ACTIVE_DB,
            steady_tolerance_db: STEADY_TOLERANCE_DB,
            min_pairs: MIN_PAIRS,
            min_chunk_pairs: MIN_CHUNK_PAIRS,
            chunk_secs: CHUNK_SECS,
            min_block_secs: 0.0,
        }
    }
}

impl ProgramLevelParams {
    /// Params for a scan recipe: its absolute silence amplitude sets the active threshold, its
    /// nominal block size sets the runt filter.
    pub fn for_recipe(absolute_silence_rms: f32, scan_block_secs: f64) -> Self {
        Self {
            active_db: active_threshold_db(absolute_silence_rms),
            min_block_secs: scan_block_secs.max(0.0) * MIN_BLOCK_FRACTION,
            ..Self::default()
        }
    }
}

/// The dB threshold separating program content from quiet, from the recipe's absolute silence
/// amplitude (≈ −60 dBFS by default).
///
/// This is **not** the scanner's `silent` flag, and the difference matters.
/// [`is_silent_interleaved`](crate::domain::policies::is_silent_interleaved) fires when either the
/// peak is below the absolute floor **or** `RMS < peak × silence_peak_fraction` (default `0.01`).
/// The second branch is unbounded in level — a block with peak 0.5 and RMS 0.004 reads *silent* at
/// −48 dBFS. It exists to catch codec noise around isolated transients (clock ticks, drum beats,
/// speech pauses), which is exactly this corpus's awkward material. Gating levels on it would admit
/// −48 dB blocks into a "quiet" population and let a transient-only passage read as a pad. The
/// absolute threshold sorts such a block into *active*, where it belongs, and the steadiness filter
/// then rejects it for being transient-adjacent.
pub fn active_threshold_db(absolute_silence_rms: f32) -> f64 {
    absolute_silence_floor_db(absolute_silence_rms).unwrap_or(FALLBACK_ACTIVE_DB)
}

/// Per-chunk median offset over a fixed bucket of A's timeline — the answer to *is the offset
/// constant?* A differently-normalized reel, per-segment loudness normalization or varying decoder
/// DRC shows up here and is invisible in a single whole-file median.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AbLevelOffsetChunk {
    pub start_secs: f64,
    pub end_secs: f64,
    pub offset_db: f64,
    pub n_pairs: usize,
}

/// Whole-program A↔B level relationship. Sign convention: **B minus A** (positive ⇒ B is louder).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AbLevelOffset {
    /// Median paired difference (dB).
    pub offset_db: f64,
    /// Median absolute deviation of the paired differences about `offset_db`.
    ///
    /// *Does one scalar exist?* Tight ⇒ a real master offset. Wide or multimodal ⇒ dynamics or
    /// compression differ, or the alignment is wrong. This — not `n_pairs` — is the gate any later
    /// gain policy must consult.
    pub spread_db: f64,
    /// Nearest-rank deciles of the paired differences: asymmetry and bimodality a single MAD hides.
    pub p10_db: f64,
    pub p90_db: f64,
    pub n_pairs: usize,
    /// A blocks above the active threshold, **before** the steadiness filter. Same predicate over
    /// the same timeline as [`ProgramLevelContext::n_active`] on the A side, so on a scan report the
    /// two are equal — a cross-check, not a second quantity.
    pub n_active: usize,
    /// A blocks that were active *and* steady — the pairing denominator. Two ratios fall out:
    /// `n_pairs / n_steady` is B-coverage yield (low ⇒ B covered little of A), and
    /// `n_steady / n_active` is what the steadiness filter cost — the only way to tell from the
    /// emitted fields whether [`STEADY_TOLERANCE_DB`] is set sanely for this material.
    pub n_steady: usize,
    pub first_pair_secs: f64,
    pub last_pair_secs: f64,
    /// False when A and B channel counts differ — the offset is then not a pure gain (see module
    /// docs). Reported anyway; consumers must check it.
    pub channels_comparable: bool,
    /// Per-chunk medians, ascending. Empty when fewer than two chunks cleared `min_chunk_pairs`
    /// (one chunk says nothing `offset_db` does not already say).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub chunks: Vec<AbLevelOffsetChunk>,
}

/// The same paired **B minus A** measurement as [`AbLevelOffset`], restricted to a collar of A
/// blocks either side of one gap — the hole's own master relationship, to be read against the
/// whole-file one rather than instead of it.
///
/// Plan: `docs/dev/archive/TEMP-local-paired-gain-plan.md`, closed 2026-08-31. Stamped
/// unconditionally at scan — measurement never depended on `normalize_fill` — and read on the apply
/// side by [`collar_residual_gain`](crate::domain::collar_residual_gain) since that date. No
/// `chunks` — a 4 s collar is one chunk by construction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CollarLevelOffset {
    /// Median paired difference over the **pooled** collar. This — not either side alone — is what
    /// a later gain reads.
    pub offset_db: f64,
    /// Median absolute deviation about `offset_db`. Over ~10 samples this is not the same statistic
    /// as the whole-file MAD over ~500, which is why it is not read directly: the apply-side trust
    /// gates are [`MIN_LOCAL_PAIRS`] on the count and [`MAX_LOCAL_SE_DB`] on
    /// [`se_db()`](Self::se_db), which folds this together with that count.
    pub spread_db: f64,
    /// Accepted pairs. **No floor is applied here** — see [`MIN_LOCAL_PAIRS`] for why the estimator
    /// reports a thin collar rather than hiding it. A consumer that turns this into a gain owes its
    /// own minimum.
    pub n_pairs: usize,
    /// Collar A blocks above the active threshold, **before** the steadiness filter, and those that
    /// then survived it. Unlike the whole-file pair, these two are the reason a collar came back
    /// thin: `n_steady / n_active` is what steadiness cost (widen the collar), `n_pairs / n_steady`
    /// is B-coverage yield (the collar is fine, B is not). Without them a thin `n_pairs` names no
    /// cause and the census cannot say which knob to turn.
    pub n_active: usize,
    pub n_steady: usize,
    /// Accepted pairs whose A block starts before the gap / after it.
    ///
    /// Emitted as provenance — is this a one-sided collar? — and then promoted: *both sides
    /// present* is half the apply-side admission test (§ 4b Result 4), because eight of the eleven
    /// worst collars in the corpus are one-sided. The other half is [`MAX_LOCAL_SE_DB`].
    pub n_pre: usize,
    pub n_post: usize,
    /// Median of the pre-side pairs alone; `None` when that side contributed none.
    ///
    /// **Provenance, not a straddle detector — and not a gate.** It was specified as the latter
    /// (`settled.md` § D: *"pre and post disagreeing by ≈ the pair's map step"*, corpus base rate
    /// **one** gap) and the 2026-08-27 corpus refuted that reading. Disagreement is a pure function
    /// of [`spread_db`](Self::spread_db): over well-populated two-sided collars, **zero of 76** with
    /// a spread under 0.5 dB disagreed by more than 1 dB, against 12 of 14 above 2.0 dB, with no
    /// dependence on gap length at all. It is two small medians drawn from unsteady material, which
    /// `spread_db` already reports. The one real straddle in the corpus (31/g7) reads 2.61 dB at
    /// n = 25 — mid-pack among the noise, because at that spread the per-side uncertainty is the
    /// same size as the step being hunted. Detecting a map step in the level domain needs quieter
    /// shoulders than real holes have. Still worth emitting as provenance; nothing refuses on it.
    pub pre_offset_db: Option<f64>,
    /// Median of the post-side pairs alone; `None` when that side contributed none.
    pub post_offset_db: Option<f64>,
    /// A-timeline extent of the accepted pairs, same convention as [`AbLevelOffset`].
    pub first_pair_secs: f64,
    pub last_pair_secs: f64,
    /// Per-gap correction applied to the nominal A→B map before pairing, in ms, or `None` when
    /// this collar was read at the nominal map because no trusted registration was available.
    ///
    /// **Provenance for the map itself, and it has to be here.** Every other field is a reading
    /// *through* a mapping; without this one, a corrected collar and a nominal one are
    /// indistinguishable in the dump, and the census cannot ask the question it exists to ask.
    /// `Some(0.0)` and `None` are different facts — "the fit was believed and said zero" versus
    /// "there was no fit to believe" — so this is an `Option`, not a defaulted `0.0`.
    pub map_lag_ms: Option<f64>,
}

impl CollarLevelOffset {
    /// Standard error (dB) of [`offset_db`](Self::offset_db), from the MAD and the pair count.
    ///
    /// **Derived, never stored.** A serialized field could disagree with the `spread_db` and
    /// `n_pairs` sitting beside it in the same object; this cannot. It is also the one expression
    /// the census script, the apply-side admission gate and the plan must not drift apart on, so it
    /// lives here rather than being rewritten at each call site.
    ///
    /// Both constants are normal-theory — 1.4826 takes MAD to sigma, 1.2533 takes sigma to the SE
    /// of a *median* rather than a mean. The collar population is small and not guaranteed normal,
    /// so read the result as an order of magnitude, not a confidence interval. That is enough for
    /// its job: [`MAX_LOCAL_SE_DB`] separates collars reading tenths of a dB from ones reading
    /// several, and nothing depends on the boundary being exact.
    ///
    /// Zero pairs yields `None`, not infinity — the caller already has to handle an absent collar,
    /// and a sentinel here would compare as "dispersed" in one direction and "clean" in the other
    /// depending on the sign convention someone assumed.
    #[must_use]
    pub fn se_db(&self) -> Option<f64> {
        if self.n_pairs == 0 {
            return None;
        }
        Some(1.2533 * 1.4826 * self.spread_db / (self.n_pairs as f64).sqrt())
    }
}

/// Per-side level context. Kept because it is the only thing available when B cannot be mapped, and
/// because A's own floor is independently useful.
///
/// **Not a comparison instrument** — never subtract two of these. That is the design this module
/// replaced; see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ProgramLevelContext {
    /// Median dB of active blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mid_db: Option<f64>,
    /// Median dB of blocks between digital zero and the active threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor_db: Option<f64>,
    pub n_active: usize,
}

/// Everything this module stamps on a scan report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProgramLevels {
    /// A-side context. Always present when A was scanned with level retention.
    pub a: ProgramLevelContext,
    /// B-side context. `None` when B produced no level timeline — which includes a readable B with
    /// no alignment offset, since the B scan only runs when both are available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<ProgramLevelContext>,
    /// The paired offset. `None` when B was not scanned or too few pairs survived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<AbLevelOffset>,
    /// False when A and B channel counts differ — see [`AbLevelOffset::channels_comparable`], which
    /// carries the same fact for consumers already reading it there.
    ///
    /// Hoisted here because it is computed from track compatibility *independently* of whether the
    /// whole-file pairing cleared [`MIN_PAIRS`], while [`Self::offset`] is `None` whenever it did
    /// not. Read off the offset object alone, a pair whose channels differ **and** whose pairing
    /// abstained is indistinguishable from one that merely ran out of pairs — so a consumer
    /// explaining an absent measurement would name the wrong cause.
    pub channels_comparable: bool,
}

/// Per-side median levels above and below the active threshold. Digital zeros (`rms_db` at or below
/// [`BLOCK_LEVEL_FLOOR_DB`]) are excluded from both populations.
pub fn program_level_context(
    levels: &[BlockLevel],
    params: &ProgramLevelParams,
) -> ProgramLevelContext {
    let mut active = Vec::new();
    let mut quiet = Vec::new();
    for level in levels {
        if !is_measurable(level, params.min_block_secs) {
            continue;
        }
        if level.rms_db > params.active_db {
            active.push(level.rms_db);
        } else if level.rms_db > BLOCK_LEVEL_FLOOR_DB {
            quiet.push(level.rms_db);
        }
    }
    ProgramLevelContext {
        n_active: active.len(),
        mid_db: median_db(active),
        floor_db: median_db(quiet),
    }
}

/// Measure the paired A↔B level offset. `offset_secs` maps A's timeline onto B's (`b = a + offset`),
/// `b_scanned_end_secs` bounds the reviewed B prefix.
///
/// `None` means no bound was recorded — in the scan path that only happens when B fed no buckets,
/// which also leaves `b_levels` empty, so nothing pairs regardless. It is therefore vacuous here
/// rather than a rejection, unlike `b_range_fully_scanned`, whose occupancy caller must fail closed.
///
/// The bound is a *tail* bound only. Nothing rejects a window mapped before B's start: time before
/// zero is not unscanned B, it does not exist, and the part of such a window that overlaps B was
/// measured like any other. See step 4.
///
/// Returns `None` when fewer than `params.min_pairs` blocks paired.
pub fn paired_level_offset(
    a_levels: &[BlockLevel],
    b_levels: &[BlockLevel],
    offset_secs: f64,
    b_scanned_end_secs: Option<f64>,
    channels_comparable: bool,
    params: &ProgramLevelParams,
) -> Option<AbLevelOffset> {
    let collected = collect_pairs(
        a_levels,
        b_levels,
        offset_secs,
        b_scanned_end_secs,
        params,
        0..a_levels.len(),
        &|_| true,
    );
    let pairs = collected.pairs;

    if pairs.len() < params.min_pairs {
        return None;
    }

    // The guard above proves the population is non-empty, so the sorted-slice estimators apply
    // directly — no `Option` to unwrap, and one sort instead of four.
    debug_assert!(!pairs.is_empty());
    let summary = summarize_pairs(&pairs);
    Some(AbLevelOffset {
        offset_db: summary.offset_db,
        spread_db: summary.spread_db,
        p10_db: summary.p10_db,
        p90_db: summary.p90_db,
        n_pairs: pairs.len(),
        n_active: collected.n_active,
        n_steady: collected.n_steady,
        first_pair_secs: pairs[0].0,
        last_pair_secs: pairs[pairs.len() - 1].0,
        channels_comparable,
        chunks: chunk_offsets(&pairs, params),
    })
}

/// The collar around one gap: `[gap_start − collar, gap_start)` ∪ `(gap_end, gap_end + collar]`.
#[derive(Clone, Copy, Debug)]
pub struct CollarSpan {
    pub gap_start_secs: f64,
    pub gap_end_secs: f64,
    pub collar_secs: f64,
}

/// The same measurement over a collar of A blocks around one gap — see [`CollarLevelOffset`].
///
/// The collar is `[gap_start − collar_secs, gap_start)` ∪ `(gap_end, gap_end + collar_secs]` on
/// **A's scan timeline** (never patch-refined edges: 100 ms blocks make tens-of-ms refinement
/// noise). A block joins it by *overlap*, and any block overlapping the gap core is excluded
/// outright.
///
/// Nothing special-cases the core beyond that. A is inactive inside it, so those blocks never pair;
/// and the first active block *abutting* the core fails the steadiness filter, which is the walk-off
/// this estimator wants — the same job `border_standoff` does at patch time.
///
/// One-sided collars are valid, not errors: a gap at the head or tail of the file, or one whose
/// other side is unsteady, still reports from the surviving side.
///
/// `map_lag_secs` corrects the nominal A→B map for **this gap** and is recorded as
/// [`CollarLevelOffset::map_lag_ms`]. Pass the shoulder-registered lag whenever one is trusted
/// (`DonorRegistration::trusted_lag_secs`) and `None` otherwise: a collar is ~10 samples where the
/// whole-program reading is ~500, so it has nothing to out-vote a mapping error, and the corpus says
/// the nominal map is off by >=100 ms at most dropouts.
///
/// Returns `None` only when the collar cannot be *located* (a non-finite or inverted bound, a
/// non-positive width) or when **nothing paired at all** — never because too few pairs survived.
/// A thin collar is a measurement, and [`MIN_LOCAL_PAIRS`] is applied by whoever turns one into a
/// gain; keeping the floor here would censor the very distribution that sets it.
pub fn collar_level_offset(
    a_levels: &[BlockLevel],
    b_levels: &[BlockLevel],
    span: CollarSpan,
    offset_secs: f64,
    map_lag_secs: Option<f64>,
    b_scanned_end_secs: Option<f64>,
    params: &ProgramLevelParams,
) -> Option<CollarLevelOffset> {
    let CollarSpan {
        gap_start_secs,
        gap_end_secs,
        collar_secs,
    } = span;
    // Written as a positive predicate so a NaN bound falls out here rather than through a negated
    // comparison, which NaN passes.
    let bounds_ok = collar_secs.is_finite()
        && collar_secs > 0.0
        && gap_start_secs.is_finite()
        && gap_end_secs.is_finite()
        && gap_end_secs >= gap_start_secs;
    if !bounds_ok {
        return None;
    }
    let pre_lo = gap_start_secs - collar_secs;
    let post_hi = gap_end_secs + collar_secs;
    let overlaps =
        |level: &BlockLevel, lo: f64, hi: f64| level.start_secs < hi && level.end_secs > lo;
    let in_collar = |level: &BlockLevel| {
        (overlaps(level, pre_lo, gap_start_secs) || overlaps(level, gap_end_secs, post_hi))
            && !overlaps(level, gap_start_secs, gap_end_secs)
    };

    // Only the collar's own index window may emit pairs — a gap must not cost a walk of the whole
    // timeline. Blocks are ascending and non-overlapping, so both ends binary-search. The core
    // sits inside the window and is rejected by `in_collar`, not by skipping indices, because
    // `is_steady` must still be able to *see* it (below).
    let first = a_levels.partition_point(|level| level.end_secs <= pre_lo);
    let last = a_levels.partition_point(|level| level.start_secs < post_hi);

    // The predicate gates **pairing**, never the timeline: `collect_pairs` keeps the full
    // `a_levels` slice so `is_steady` reads block `i`'s real time-adjacent neighbours. Hand it a
    // pre-sliced collar instead and the property above inverts — the block abutting the core loses
    // its core-side neighbour, an absent neighbour is permissive, and the fade block that walk-off
    // exists to reject pairs.
    // The whole-program offset is one scalar over the entire runtime and is measurably wrong at an
    // individual gap: over the 2026-08-27 corpus it sat at the shoulder-correlation peak on only 42%
    // of repairable dropouts, off by >=100 ms on 59%, out to 400 ms. A 100 ms error pairs a syllable
    // in A against the space between syllables in B, and the dB difference between two *different
    // moments* is not a level relationship at all — it lands in `spread_db` and drags `offset_db`.
    // `donor_registration` already fits that lag, on the shoulders, in the same scan.
    //
    // Sign matches `register_donor_window`: the lag is what B's window moves by, positive later in B.
    let collected = collect_pairs(
        a_levels,
        b_levels,
        offset_secs + map_lag_secs.unwrap_or(0.0),
        b_scanned_end_secs,
        params,
        first..last,
        &in_collar,
    );
    let pairs = collected.pairs;
    // Empty, not "thin": the summary below takes a median and reads both ends of the vec, and a
    // caller cannot act on a collar that measured nothing anyway.
    if pairs.is_empty() {
        return None;
    }

    // Accepted blocks never overlap the core, so an A start before the gap is a pre block.
    let side_median = |before: bool| {
        median_db(
            pairs
                .iter()
                .filter(|&&(start, _)| (start < gap_start_secs) == before)
                .map(|&(_, d)| d)
                .collect(),
        )
    };
    let n_pre = pairs
        .iter()
        .filter(|&&(start, _)| start < gap_start_secs)
        .count();

    let summary = summarize_pairs(&pairs);
    Some(CollarLevelOffset {
        offset_db: summary.offset_db,
        spread_db: summary.spread_db,
        n_pairs: pairs.len(),
        n_active: collected.n_active,
        n_steady: collected.n_steady,
        n_pre,
        n_post: pairs.len() - n_pre,
        pre_offset_db: side_median(true),
        post_offset_db: side_median(false),
        first_pair_secs: pairs[0].0,
        last_pair_secs: pairs[pairs.len() - 1].0,
        map_lag_ms: map_lag_secs.map(|secs| secs * 1000.0),
    })
}

/// Accepted pairs plus the two filter counters, from one walk of `emit`.
struct PairCollection {
    /// `(A block start, B dB − A dB)`, ascending in A — so chunking and per-side binning can
    /// bucket by A's timeline without a second pass.
    pairs: Vec<(f64, f64)>,
    n_active: usize,
    n_steady: usize,
}

/// Median / MAD / deciles of a **non-empty** pair population.
struct PairSummary {
    offset_db: f64,
    spread_db: f64,
    p10_db: f64,
    p90_db: f64,
}

fn summarize_pairs(pairs: &[(f64, f64)]) -> PairSummary {
    debug_assert!(!pairs.is_empty());
    let mut diffs: Vec<f64> = pairs.iter().map(|&(_, d)| d).collect();
    sort_db(&mut diffs);
    let offset_db = median_of_sorted(&diffs);
    let mut deviations: Vec<f64> = diffs.iter().map(|d| (d - offset_db).abs()).collect();
    sort_db(&mut deviations);
    PairSummary {
        offset_db,
        spread_db: median_of_sorted(&deviations),
        p10_db: percentile_of_sorted(&diffs, 0.1),
        p90_db: percentile_of_sorted(&diffs, 0.9),
    }
}

/// The pairing walk shared by [`paired_level_offset`] and [`collar_level_offset`].
///
/// `emit` bounds which **indices** may produce a pair and `accept` which of those blocks may; both
/// are restrictions on *pairing only*. `a_levels` is always the whole timeline, because
/// [`is_steady`] resolves neighbours out of it by time, and a neighbour it cannot see counts as
/// absent — which is permissive, and would admit exactly the transient-adjacent blocks the filter
/// exists to drop.
fn collect_pairs(
    a_levels: &[BlockLevel],
    b_levels: &[BlockLevel],
    offset_secs: f64,
    b_scanned_end_secs: Option<f64>,
    params: &ProgramLevelParams,
    emit: std::ops::Range<usize>,
    accept: &dyn Fn(&BlockLevel) -> bool,
) -> PairCollection {
    let mut n_active = 0usize;
    let mut n_steady = 0usize;
    let mut pairs: Vec<(f64, f64)> = Vec::new();

    for i in emit {
        let a = &a_levels[i];
        if !accept(a) {
            continue;
        }
        // 0: a full-length block — a bucket-tail sliver is not a measurement.
        // 1: active on A. Counted before step 2 so `n_steady / n_active` exposes what the
        // steadiness filter cost.
        if !is_measurable(a, params.min_block_secs) || a.rms_db <= params.active_db {
            continue;
        }
        n_active += 1;
        // 2: steady — both time-adjacent neighbours within tolerance. A neighbour that does not
        // abut (a file edge, or the far side of a timeline hole) is permissive.
        if !is_steady(a_levels, i, params) {
            continue;
        }
        n_steady += 1;

        // 3: map onto B and take the block with the largest time overlap. Max-overlap rather than
        // interpolating the two straddled blocks: the steadiness filter has already guaranteed the
        // A neighbourhood is flat, so the candidates differ by little.
        let (b_lo, b_hi) = (a.start_secs + offset_secs, a.end_secs + offset_secs);
        // 4: fail closed past the reviewed B prefix. Deliberately *not* `b_range_fully_scanned`,
        // which is occupancy's rule: it also demands `b_lo >= 0.0`, and that is the wrong question
        // here. Occupancy asks "may I fill from this window", where time before B's start must fail
        // closed. Pairing asks "is there a B block to difference against" — and for a window
        // straddling zero the overlapping part *was* scanned, so it is no different from the
        // interior half-block straddle that step 3 already accepts. A window mapped wholly before
        // zero needs no rule: `max_overlap_block` finds nothing and returns `None`.
        if !b_scanned_end_secs.is_none_or(|end| b_hi <= end) {
            continue;
        }
        let Some(b) = max_overlap_block(b_levels, b_lo, b_hi) else {
            continue;
        };
        // 5: full-length and active on B, same rules as A.
        if !is_measurable(b, params.min_block_secs) || b.rms_db <= params.active_db {
            continue;
        }
        // 6.
        pairs.push((a.start_secs, b.rms_db - a.rms_db));
    }

    PairCollection {
        pairs,
        n_active,
        n_steady,
    }
}

/// A block long enough for its RMS to mean something — see [`MIN_BLOCK_FRACTION`].
fn is_measurable(level: &BlockLevel, min_block_secs: f64) -> bool {
    level.end_secs - level.start_secs >= min_block_secs
}

/// Both **time-adjacent, measurable** neighbours of `i` within `steady_tolerance_db`.
///
/// Adjacency is checked in time, not in index: the timeline can carry holes (a decode
/// discontinuity, the same reason [`max_overlap_block`] binary-searches rather than dividing), and
/// across a hole the neighbouring *index* is not the neighbouring *audio*.
///
/// A neighbour that does not abut, or is too short to measure ([`is_measurable`]), counts as
/// **absent**, which is permissive: absence of a neighbour is not evidence of a transient, and a
/// sliver's RMS is not evidence of anything. Were slivers allowed to vote, every bucket-tail runt
/// would take its preceding full block down with it. Absent neighbours also cover the two
/// file-boundary blocks.
fn is_steady(levels: &[BlockLevel], i: usize, params: &ProgramLevelParams) -> bool {
    let here = &levels[i];
    let counts = |other: &BlockLevel, abuts: bool| {
        abuts
            && is_measurable(other, params.min_block_secs)
            && (other.rms_db - here.rms_db).abs() > params.steady_tolerance_db
    };
    let prev_breaks = i
        .checked_sub(1)
        .map(|prev| &levels[prev])
        .is_some_and(|prev| {
            counts(
                prev,
                (here.start_secs - prev.end_secs).abs() <= NEIGHBOUR_SLACK_SECS,
            )
        });
    let next_breaks = levels.get(i + 1).is_some_and(|next| {
        counts(
            next,
            (next.start_secs - here.end_secs).abs() <= NEIGHBOUR_SLACK_SECS,
        )
    });
    !prev_breaks && !next_breaks
}

/// The block sharing the most time with `[lo, hi)`. Blocks are ascending and non-overlapping, so a
/// binary search finds the first candidate; a decoded-timeline hole can leave gaps between blocks,
/// which is why this never derives an index by division.
fn max_overlap_block(levels: &[BlockLevel], lo: f64, hi: f64) -> Option<&BlockLevel> {
    if hi <= lo {
        return None;
    }
    let first = levels.partition_point(|b| b.end_secs <= lo);
    let mut best: Option<(&BlockLevel, f64)> = None;
    for level in &levels[first..] {
        if level.start_secs >= hi {
            break;
        }
        let overlap = level.end_secs.min(hi) - level.start_secs.max(lo);
        if overlap > 0.0 && best.is_none_or(|(_, best_overlap)| overlap > best_overlap) {
            best = Some((level, overlap));
        }
    }
    best.map(|(level, _)| level)
}

/// Median offset per fixed bucket of A's timeline. Emitted only when at least two buckets clear
/// `min_chunk_pairs` — a single chunk repeats `offset_db`.
fn chunk_offsets(pairs: &[(f64, f64)], params: &ProgramLevelParams) -> Vec<AbLevelOffsetChunk> {
    if params.chunk_secs <= 0.0 {
        return Vec::new();
    }
    let mut chunks: Vec<AbLevelOffsetChunk> = Vec::new();
    let mut bucket: Vec<f64> = Vec::new();
    let mut current: Option<i64> = None;

    // `pairs` is built in A order, so buckets are contiguous runs.
    for &(start_secs, diff) in pairs {
        let index = (start_secs / params.chunk_secs).floor() as i64;
        if current != Some(index) {
            flush_chunk(&mut chunks, current, &mut bucket, params);
            current = Some(index);
        }
        bucket.push(diff);
    }
    flush_chunk(&mut chunks, current, &mut bucket, params);

    if chunks.len() < 2 {
        return Vec::new();
    }
    chunks
}

fn flush_chunk(
    chunks: &mut Vec<AbLevelOffsetChunk>,
    index: Option<i64>,
    bucket: &mut Vec<f64>,
    params: &ProgramLevelParams,
) {
    let Some(index) = index else {
        bucket.clear();
        return;
    };
    let n_pairs = bucket.len();
    let offset = (n_pairs >= params.min_chunk_pairs)
        .then(|| median_db(std::mem::take(bucket)))
        .flatten();
    bucket.clear();
    if let Some(offset_db) = offset {
        let start_secs = index as f64 * params.chunk_secs;
        chunks.push(AbLevelOffsetChunk {
            start_secs,
            end_secs: start_secs + params.chunk_secs,
            offset_db,
            n_pairs,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK_SECS: f64 = 0.1;

    fn params() -> ProgramLevelParams {
        ProgramLevelParams {
            min_pairs: 4,
            min_chunk_pairs: 2,
            min_block_secs: BLOCK_SECS * MIN_BLOCK_FRACTION,
            ..ProgramLevelParams::default()
        }
    }

    /// Blocks of `BLOCK_SECS` starting at `start_secs`, one per dB value.
    fn timeline(start_secs: f64, dbs: &[f64]) -> Vec<BlockLevel> {
        dbs.iter()
            .enumerate()
            .map(|(i, &rms_db)| BlockLevel {
                start_secs: start_secs + i as f64 * BLOCK_SECS,
                end_secs: start_secs + (i + 1) as f64 * BLOCK_SECS,
                rms_db,
                // Never read by this module — the absolute threshold is the level gate.
                silent: rms_db <= -60.0,
            })
            .collect()
    }

    fn flat(n: usize, db: f64) -> Vec<f64> {
        vec![db; n]
    }

    fn offset_of(a: &[BlockLevel], b: &[BlockLevel], offset_secs: f64) -> AbLevelOffset {
        paired_level_offset(a, b, offset_secs, None, true, &params())
            .expect("expected a paired offset")
    }

    #[test]
    fn constant_gain_is_recovered_with_zero_spread() {
        let a = timeline(0.0, &flat(20, -30.0));
        let b = timeline(0.0, &flat(20, -24.0));

        let got = offset_of(&a, &b, 0.0);

        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
        assert!(got.spread_db.abs() < 1e-9, "{got:?}");
        assert_eq!(got.n_pairs, 20);
        assert_eq!(got.n_active, 20);
        assert_eq!(
            got.n_steady, 20,
            "a flat timeline loses nothing to steadiness"
        );
        assert!(got.channels_comparable);
    }

    #[test]
    fn fractional_block_offset_still_recovers_the_gain() {
        let a = timeline(0.0, &flat(20, -30.0));
        // B shifted by 1.34 blocks: every A block straddles two B blocks.
        let b = timeline(0.134, &flat(20, -24.0));

        let got = offset_of(&a, &b, 0.134);

        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn a_side_dropout_does_not_move_the_offset() {
        let undamaged = timeline(0.0, &flat(20, -30.0));
        let b = timeline(0.0, &flat(20, -24.0));
        let mut damaged_dbs = flat(20, -30.0);
        // A eight-block dropout at the digital-zero rail — the shape the two-summary design would
        // have read as A being quieter overall.
        for db in damaged_dbs.iter_mut().skip(6).take(8) {
            *db = BLOCK_LEVEL_FLOOR_DB;
        }
        let damaged = timeline(0.0, &damaged_dbs);

        let clean = offset_of(&undamaged, &b, 0.0);
        let with_dropout = offset_of(&damaged, &b, 0.0);

        assert!(
            (with_dropout.offset_db - clean.offset_db).abs() < 1e-9,
            "dropout moved the offset: {with_dropout:?} vs {clean:?}"
        );
        assert!(with_dropout.n_pairs < clean.n_pairs, "{with_dropout:?}");
    }

    #[test]
    fn pads_of_different_lengths_need_no_trim_rule() {
        let mut a_dbs = flat(3, -95.0);
        a_dbs.extend(flat(20, -30.0));
        a_dbs.extend(flat(2, -95.0));
        let mut b_dbs = flat(9, -95.0);
        b_dbs.extend(flat(20, -24.0));
        b_dbs.extend(flat(7, -95.0));

        let a = timeline(0.0, &a_dbs);
        // B's content starts 6 blocks later; the map carries that, the pads never pair.
        let b = timeline(0.0, &b_dbs);

        let got = offset_of(&a, &b, 6.0 * BLOCK_SECS);

        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
        // 18, not 20: the content blocks abutting a pad are transient-adjacent by construction, so
        // the steadiness filter drops them. That is the filter working, and it is why no separate
        // head/tail trim rule is needed.
        assert_eq!(got.n_pairs, 18);
    }

    #[test]
    fn a_mismatched_b_half_raises_spread_without_moving_the_offset() {
        let a = timeline(0.0, &flat(40, -30.0));
        let mut b_dbs = flat(20, -24.0);
        // Second half is other material: alternating levels, still "active".
        for i in 0..20 {
            b_dbs.push(if i % 2 == 0 { -12.0 } else { -44.0 });
        }
        let b = timeline(0.0, &b_dbs);

        let got = offset_of(&a, &b, 0.0);

        assert!(
            (got.offset_db - 6.0).abs() < 12.0,
            "offset should still track the matching half: {got:?}"
        );
        assert!(
            got.spread_db > 5.0,
            "mismatched material should show as spread: {got:?}"
        );
    }

    #[test]
    fn chunks_expose_a_non_constant_offset() {
        let a = timeline(0.0, &flat(40, -30.0));
        let mut b_dbs = flat(20, -28.0); // +2 dB
        b_dbs.extend(flat(20, -22.0)); // +8 dB
        let b = timeline(0.0, &b_dbs);

        let params = ProgramLevelParams {
            chunk_secs: 20.0 * BLOCK_SECS,
            ..params()
        };
        let got = paired_level_offset(&a, &b, 0.0, None, true, &params).expect("offset");

        assert!((got.offset_db - 5.0).abs() < 3.001, "{got:?}");
        assert_eq!(got.chunks.len(), 2, "{:?}", got.chunks);
        assert!((got.chunks[0].offset_db - 2.0).abs() < 1e-9, "{got:?}");
        assert!((got.chunks[1].offset_db - 8.0).abs() < 1e-9, "{got:?}");
        assert!(got.chunks[0].start_secs < got.chunks[1].start_secs);
    }

    #[test]
    fn a_single_chunk_is_not_emitted() {
        let a = timeline(0.0, &flat(20, -30.0));
        let b = timeline(0.0, &flat(20, -24.0));

        assert!(offset_of(&a, &b, 0.0).chunks.is_empty());
    }

    #[test]
    fn steadiness_filter_drops_transient_adjacent_blocks() {
        // A alternating loud/quiet run: every block differs from its neighbours by 20 dB, so a
        // half-block map error would turn registration error into level error.
        let a_dbs: Vec<f64> = (0..40)
            .map(|i| if i % 2 == 0 { -20.0 } else { -40.0 })
            .collect();
        let a = timeline(0.0, &a_dbs);
        let b = timeline(0.0, &a_dbs.iter().map(|db| db + 6.0).collect::<Vec<_>>());

        assert!(
            paired_level_offset(&a, &b, 0.0, None, true, &params()).is_none(),
            "transient-adjacent blocks must not pair"
        );

        // The same material with the filter relaxed does pair — proving the rejection is the
        // filter's doing, not the mapping's.
        let relaxed = ProgramLevelParams {
            steady_tolerance_db: 30.0,
            ..params()
        };
        let got = paired_level_offset(&a, &b, 0.0, None, true, &relaxed).expect("offset");
        assert_eq!(got.n_pairs, 40);
        assert_eq!(
            got.n_steady, 40,
            "nothing is transient-adjacent at 30 dB tolerance"
        );
    }

    #[test]
    fn the_steadiness_cost_is_visible_in_the_counters() {
        // Flat material with one isolated transient: the spike and both its neighbours fail
        // steadiness, so three of forty active blocks never reach the pairing.
        let mut a_dbs = flat(40, -30.0);
        a_dbs[20] = -10.0;
        let a = timeline(0.0, &a_dbs);
        let b = timeline(0.0, &a_dbs.iter().map(|db| db + 6.0).collect::<Vec<_>>());

        let got = offset_of(&a, &b, 0.0);

        assert_eq!(
            got.n_active, 40,
            "the spike is active — it is not a quiet block"
        );
        assert_eq!(got.n_steady, 37, "spike + both neighbours rejected");
        assert_eq!(got.n_pairs, 37, "B rejected none of them");
        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn b_prefix_bound_is_fail_closed() {
        let a = timeline(0.0, &flat(20, -30.0));
        let b = timeline(0.0, &flat(20, -24.0));

        // Only the first ten B blocks were reviewed.
        let got = paired_level_offset(&a, &b, 0.0, Some(10.0 * BLOCK_SECS), true, &params())
            .expect("offset");

        assert_eq!(got.n_pairs, 10);
        assert_eq!(got.n_active, 20, "denominator counts all active A blocks");
        assert_eq!(
            got.n_steady, 20,
            "the bound is a B-side rejection, not an A-side one"
        );
        assert!(
            (got.last_pair_secs - 9.0 * BLOCK_SECS).abs() < 1e-9,
            "{got:?}"
        );
    }

    #[test]
    fn a_bucket_tail_sliver_is_not_measured() {
        // A full timeline plus the kind of runt `feed` leaves at the end of a bucket: one frame's
        // worth of audio, at a level nothing else in the file is near.
        let mut a = timeline(0.0, &flat(20, -30.0));
        let last_end = a[19].end_secs;
        a.push(BlockLevel {
            start_secs: last_end,
            end_secs: last_end + 1.0 / 48_000.0,
            rms_db: 0.0,
            silent: false,
        });
        let mut b = timeline(0.0, &flat(20, -24.0));
        let b_last_end = b[19].end_secs;
        b.push(BlockLevel {
            start_secs: b_last_end,
            end_secs: b_last_end + 1.0 / 48_000.0,
            rms_db: 0.0,
            silent: false,
        });

        let got = offset_of(&a, &b, 0.0);

        assert_eq!(got.n_active, 20, "the sliver never reaches the active test");
        assert_eq!(got.n_pairs, 20);
        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
        // The same sliver is excluded from the per-side context.
        let context = program_level_context(&a, &params());
        assert_eq!(context.n_active, 20);

        // Disable the filter and the sliver costs twice: it enters the context as if it were a
        // measurement, and — being 30 dB from its only neighbour — it takes the last real block out
        // of the pairing with it.
        let unfiltered = ProgramLevelParams {
            min_block_secs: 0.0,
            ..params()
        };
        assert_eq!(program_level_context(&a, &unfiltered).n_active, 21);
        let got = paired_level_offset(&a, &b, 0.0, None, true, &unfiltered).expect("offset");
        assert_eq!(got.n_active, 21);
        assert_eq!(got.n_steady, 19, "the sliver and its neighbour both fail");
    }

    #[test]
    fn steadiness_does_not_compare_across_a_timeline_hole() {
        // Two flat runs at very different levels with a multi-second hole between them: index
        // neighbours across the hole differ by 20 dB, time neighbours do not exist.
        let mut a = timeline(0.0, &flat(10, -40.0));
        a.extend(timeline(30.0, &flat(10, -20.0)));
        let b: Vec<BlockLevel> = a
            .iter()
            .map(|level| BlockLevel {
                rms_db: level.rms_db + 6.0,
                ..*level
            })
            .collect();

        let got = offset_of(&a, &b, 0.0);

        assert_eq!(
            got.n_steady, 20,
            "the blocks either side of the hole are hole-edges, not transient-adjacent"
        );
        assert_eq!(got.n_pairs, 20);
        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn a_negative_offset_drops_the_blocks_that_map_before_b_starts() {
        // `b = a + offset`: a negative offset means A's head has no counterpart in B (A started
        // first). The first three A blocks map to negative B time.
        let a = timeline(0.0, &flat(20, -30.0));
        let b = timeline(0.0, &flat(20, -24.0));

        let got = paired_level_offset(&a, &b, -3.0 * BLOCK_SECS, None, true, &params())
            .expect("the mapped remainder still pairs");

        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
        assert_eq!(got.n_active, 20, "every A block is still active");
        assert_eq!(
            got.n_pairs, 17,
            "three map wholly before B's start, so `max_overlap_block` finds no candidate"
        );
        assert!(
            (got.first_pair_secs - 3.0 * BLOCK_SECS).abs() < 1e-9,
            "{got:?}"
        );
    }

    #[test]
    fn a_window_straddling_b_start_is_paired_on_its_overlap() {
        // Half a block of negative offset: A block 0 maps to [-0.05, 0.05), overlapping B block 0
        // by half. This is the one case where occupancy's rule and the pairing rule disagree, and
        // the pairing rule is the right one — the overlapping half of B was scanned, and the
        // registration error is no worse than the interior straddle that is accepted everywhere
        // else. Integer-block offsets cannot exercise this: there the window is wholly outside.
        let a = timeline(0.0, &flat(20, -30.0));
        let b = timeline(0.0, &flat(20, -24.0));

        let got =
            paired_level_offset(&a, &b, -0.5 * BLOCK_SECS, None, true, &params()).expect("offset");

        assert_eq!(got.n_pairs, 20, "every block finds an overlapping B block");
        assert!(got.first_pair_secs.abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn too_few_pairs_abstains() {
        let a = timeline(0.0, &flat(3, -30.0));
        let b = timeline(0.0, &flat(3, -24.0));

        assert!(paired_level_offset(&a, &b, 0.0, None, true, &params()).is_none());
        assert!(paired_level_offset(&[], &[], 0.0, None, true, &params()).is_none());
    }

    #[test]
    fn all_inactive_timelines_abstain() {
        let a = timeline(0.0, &flat(40, -95.0));
        let b = timeline(0.0, &flat(40, -95.0));

        assert!(paired_level_offset(&a, &b, 0.0, None, true, &params()).is_none());
    }

    #[test]
    fn channel_mismatch_is_recorded_not_suppressed() {
        let a = timeline(0.0, &flat(20, -30.0));
        let b = timeline(0.0, &flat(20, -24.0));

        let got = paired_level_offset(&a, &b, 0.0, None, false, &params())
            .expect("offset still reported");

        assert!(!got.channels_comparable);
    }

    #[test]
    fn context_splits_on_the_absolute_threshold_and_excludes_digital_zero() {
        let mut dbs = flat(4, -30.0);
        dbs.extend(flat(3, -80.0));
        dbs.push(BLOCK_LEVEL_FLOOR_DB);
        // A transient-only block: `silent` by the scanner's peak-fraction branch, but −48 dB is
        // program level, so it must count as active.
        dbs.push(-48.0);
        let levels = timeline(0.0, &dbs);

        let got = program_level_context(&levels, &params());

        assert_eq!(got.n_active, 5);
        assert_eq!(got.mid_db, Some(-30.0));
        assert_eq!(got.floor_db, Some(-80.0));
    }

    #[test]
    fn context_abstains_per_population() {
        let all_active = program_level_context(&timeline(0.0, &flat(4, -20.0)), &params());
        assert_eq!(all_active.floor_db, None);
        assert_eq!(all_active.mid_db, Some(-20.0));

        let empty = program_level_context(&[], &params());
        assert_eq!(empty.mid_db, None);
        assert_eq!(empty.floor_db, None);
        assert_eq!(empty.n_active, 0);
    }

    #[test]
    fn active_threshold_follows_the_recipe() {
        assert!((active_threshold_db(0.001_007) + 59.94).abs() < 0.05);
        // Disabled absolute floor falls back rather than admitting digital zeros.
        assert_eq!(active_threshold_db(0.0), FALLBACK_ACTIVE_DB);
    }

    // ---- collar (per-gap) pairing ----------------------------------------------------------

    /// A flat A timeline with a digital-zero hole over `[gap_from, gap_to)` blocks.
    fn holed(n: usize, program_db: f64, gap_from: usize, gap_to: usize) -> Vec<f64> {
        let mut dbs = flat(n, program_db);
        for db in dbs.iter_mut().take(gap_to).skip(gap_from) {
            *db = BLOCK_LEVEL_FLOOR_DB;
        }
        dbs
    }

    fn collar_of(
        a: &[BlockLevel],
        b: &[BlockLevel],
        gap: (f64, f64),
        collar_secs: f64,
    ) -> Option<CollarLevelOffset> {
        collar_level_offset(
            a,
            b,
            CollarSpan {
                gap_start_secs: gap.0,
                gap_end_secs: gap.1,
                collar_secs,
            },
            0.0,
            None,
            None,
            &params(),
        )
    }

    /// `collar_of` with an explicit per-gap map correction.
    fn collar_at_lag(
        a: &[BlockLevel],
        b: &[BlockLevel],
        gap: (f64, f64),
        collar_secs: f64,
        map_lag_secs: Option<f64>,
    ) -> Option<CollarLevelOffset> {
        collar_level_offset(
            a,
            b,
            CollarSpan {
                gap_start_secs: gap.0,
                gap_end_secs: gap.1,
                collar_secs,
            },
            0.0,
            map_lag_secs,
            None,
            &params(),
        )
    }

    #[test]
    fn a_collar_reads_both_sides_and_never_the_core() {
        // 10 s of program at 100 ms blocks with a 1 s hole at 4.0 s.
        let a = timeline(0.0, &holed(100, -30.0, 40, 50));
        let b = timeline(0.0, &flat(100, -30.0));

        let got = collar_of(&a, &b, (4.0, 5.0), 2.0).expect("both collars pair");

        assert!(got.offset_db.abs() < 1e-9, "{got:?}");
        // 20 blocks a side, less the one abutting the hole — it fails steadiness against the rail,
        // which is the walk-off this estimator wants and `border_standoff`'s job at patch time.
        assert_eq!((got.n_pre, got.n_post), (19, 19), "{got:?}");
        assert_eq!(got.n_pairs, 38);
        // Both collars are active throughout; only the two blocks abutting the rail lose steadiness.
        assert_eq!((got.n_active, got.n_steady), (40, 38), "{got:?}");
        // Nothing from inside the hole, and nothing from beyond the collar.
        assert!((got.first_pair_secs - 2.0).abs() < 1e-9, "{got:?}");
        assert!((got.last_pair_secs - 6.9).abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn a_constant_gain_is_recovered_locally_at_any_hole_length() {
        let b = timeline(0.0, &flat(100, -24.0));
        for (from, to) in [(40usize, 42usize), (40, 50), (40, 70)] {
            let a = timeline(0.0, &holed(100, -30.0, from, to));
            let gap = (from as f64 * BLOCK_SECS, to as f64 * BLOCK_SECS);

            let got = collar_of(&a, &b, gap, 2.0).expect("both collars pair");

            assert!((got.offset_db - 6.0).abs() < 1e-9, "{from}..{to}: {got:?}");
        }
    }

    /// The defect this estimator replaces: a quiet passage whose *reference window* reaches louder
    /// material reads the wrong level relationship. Here A and B agree exactly over the quiet
    /// passage and disagree by 12 dB outside it — a collar that stays inside the passage must read
    /// 0, and one wide enough to leave it must not.
    #[test]
    fn the_collar_reads_the_passage_it_sits_in() {
        // Quiet passage over [2.0, 7.0); loud program either side. Hole at [4.4, 5.0).
        let mut a_dbs = flat(120, -20.0);
        for db in a_dbs.iter_mut().take(70).skip(20) {
            *db = -52.0;
        }
        let mut b_dbs = a_dbs.clone();
        // B is 12 dB up on A everywhere *outside* the passage.
        for db in b_dbs.iter_mut().take(20) {
            *db += 12.0;
        }
        for db in b_dbs.iter_mut().skip(70) {
            *db += 12.0;
        }
        for db in a_dbs.iter_mut().take(50).skip(44) {
            *db = BLOCK_LEVEL_FLOOR_DB;
        }
        let (a, b) = (timeline(0.0, &a_dbs), timeline(0.0, &b_dbs));

        let local = collar_of(&a, &b, (4.4, 5.0), 2.0).expect("both collars pair");
        assert!(local.offset_db.abs() < 1e-9, "local collar: {local:?}");
        assert!(local.spread_db.abs() < 1e-9, "local collar: {local:?}");

        let wide = collar_of(&a, &b, (4.4, 5.0), 5.0).expect("both collars pair");
        assert!(
            wide.offset_db > 6.0,
            "a 5 s reach leaves the passage and reads the loud material: {wide:?}"
        );
    }

    #[test]
    fn a_leading_edge_gap_reports_from_one_side() {
        let a = timeline(0.0, &holed(100, -30.0, 0, 10));
        let b = timeline(0.0, &flat(100, -24.0));

        let got =
            collar_of(&a, &b, (0.0, 1.0), 2.0).expect("the post collar alone clears the floor");

        assert_eq!(got.n_pre, 0);
        assert_eq!(got.n_post, 19);
        assert_eq!(got.pre_offset_db, None, "an empty side has no median");
        assert!((got.post_offset_db.expect("post pairs") - 6.0).abs() < 1e-9);
        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
    }

    /// The floor lives at the apply site, not here. If the estimator hid thin collars, the census
    /// that is supposed to *choose* `MIN_LOCAL_PAIRS` could never see the distribution below it.
    #[test]
    fn a_thin_collar_is_reported_not_hidden() {
        let a = timeline(0.0, &holed(100, -30.0, 0, 10));
        let b = timeline(0.0, &flat(100, -24.0));

        // 0.5 s of one-sided collar is five blocks, four after the walk-off against the rail.
        let got = collar_of(&a, &b, (0.0, 1.0), 0.5).expect("a thin collar still reports");

        assert_eq!(got.n_pairs, 4);
        assert!(
            got.n_pairs < MIN_LOCAL_PAIRS,
            "the sample must sit under the floor to be a test of it"
        );
        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
    }

    /// The counters name *why* a collar came back thin — the question a bare `n_pairs` leaves open.
    /// Both halves below accept exactly 19 pairs and differ only in which filter spent the other
    /// 19, which is the difference between "widen the collar" and "the collar is fine, B is not".
    #[test]
    fn the_counters_separate_an_unsteady_collar_from_an_uncovered_one() {
        // Steadiness ate the pre side: alternating 6 dB steps leave no block with two flat
        // neighbours. Every block is still active, so the loss is invisible in `n_active`.
        let mut jittery = holed(100, -30.0, 40, 50);
        for (i, db) in jittery.iter_mut().enumerate().take(40).skip(20) {
            if i % 2 == 0 {
                *db += 6.0;
            }
        }
        let unsteady = collar_of(
            &timeline(0.0, &jittery),
            &timeline(0.0, &flat(100, -24.0)),
            (4.0, 5.0),
            2.0,
        )
        .expect("the post side still pairs");

        // B never dropped a pair here, so the whole shortfall is the steadiness filter.
        assert_eq!(unsteady.n_active, 40, "{unsteady:?}");
        assert_eq!(
            (unsteady.n_steady, unsteady.n_pairs),
            (19, 19),
            "{unsteady:?}"
        );

        // Same yield, other cause: A is clean throughout, but B is silent where the pre collar maps.
        let mut b_dbs = flat(100, -24.0);
        for db in b_dbs.iter_mut().take(40).skip(20) {
            *db = BLOCK_LEVEL_FLOOR_DB;
        }
        let uncovered = collar_of(
            &timeline(0.0, &holed(100, -30.0, 40, 50)),
            &timeline(0.0, &b_dbs),
            (4.0, 5.0),
            2.0,
        )
        .expect("the post side still pairs");

        assert_eq!(uncovered.n_pairs, 19, "same yield as the unsteady collar");
        assert_eq!(
            (uncovered.n_active, uncovered.n_steady),
            (40, 38),
            "{uncovered:?}"
        );
    }

    /// A collar where **nothing** pairs is still `None`, so the counters cannot explain a total
    /// abstention — only a thin one. Pinned so the limit is a decision rather than a surprise: an
    /// `offset_db` over zero pairs does not exist, and `Option`-ing it to carry two counters would
    /// push the emptiness check onto every future consumer.
    #[test]
    fn a_collar_that_pairs_nothing_reports_no_counters() {
        let mut jittery = holed(100, -30.0, 40, 50);
        for (i, db) in jittery.iter_mut().enumerate() {
            if i % 2 == 0 && *db > BLOCK_LEVEL_FLOOR_DB {
                *db += 6.0;
            }
        }

        assert_eq!(
            collar_of(
                &timeline(0.0, &jittery),
                &timeline(0.0, &flat(100, -24.0)),
                (4.0, 5.0),
                2.0
            ),
            None
        );
    }

    /// **Pins the slicing hazard.** `is_steady` resolves neighbours out of the slice it is handed,
    /// and a neighbour it cannot see counts as *absent*, which is permissive. Build the collar over
    /// a pre-sliced timeline and the block abutting the hole loses its core-side neighbour, reads
    /// steady, and pairs — so the fade blocks walk-off exists to reject would be measured. Every
    /// other collar test here passes under that bug; this one does not.
    #[test]
    fn the_block_abutting_the_hole_is_judged_against_the_core() {
        let mut a_dbs = holed(100, -30.0, 40, 50);
        // A two-block fade into the hole. Block 39's only disqualifying neighbour is the core:
        // block 38 sits at the same level, so nothing on the collar side rejects it.
        a_dbs[38] = -40.0;
        a_dbs[39] = -40.0;
        let a = timeline(0.0, &a_dbs);
        let b = timeline(0.0, &flat(100, -24.0));

        let got = collar_of(&a, &b, (4.0, 5.0), 2.0).expect("both collars pair");

        // 20 pre blocks less three: 37 (its neighbour 38 breaks), 38 (its neighbour 37 breaks) and
        // 39 — rejected *only* because `is_steady` can see the rail at block 40.
        assert_eq!(got.n_pre, 17, "{got:?}");
        assert_eq!(got.n_post, 19, "{got:?}");
        assert!((got.offset_db - 6.0).abs() < 1e-9, "{got:?}");
    }

    /// Per-side medians are a **detector** (`settled.md` § D, plan § 3a): they make a map straddle
    /// visible on the scan JSON. Nothing refuses on them — `offset_db` stays the pooled median.
    #[test]
    fn per_side_medians_are_reported_without_gating() {
        let a = timeline(0.0, &holed(100, -30.0, 40, 50));
        // B holds a different level after the hole — what one side of a map step looks like in the
        // level domain.
        let mut b_dbs = flat(100, -30.0);
        for db in b_dbs.iter_mut().skip(50) {
            *db = -24.0;
        }
        let b = timeline(0.0, &b_dbs);

        let got = collar_of(&a, &b, (4.0, 5.0), 2.0).expect("both collars pair");

        assert!(
            (got.pre_offset_db.expect("pre pairs")).abs() < 1e-9,
            "{got:?}"
        );
        assert!(
            (got.post_offset_db.expect("post pairs") - 6.0).abs() < 1e-9,
            "{got:?}"
        );
        // Still reported, and still the pooled median of an evenly split population.
        assert_eq!(got.n_pairs, 38);
        assert!(
            got.offset_db == 0.0 || got.offset_db == 6.0,
            "pooled median is one of the two observed levels, not an average: {got:?}"
        );
    }

    #[test]
    fn an_unmeasurable_collar_is_none_not_a_panic() {
        let a = timeline(0.0, &holed(100, -30.0, 40, 50));
        let b = timeline(0.0, &flat(100, -30.0));

        assert_eq!(collar_of(&a, &b, (4.0, 5.0), 0.0), None, "zero width");
        assert_eq!(collar_of(&a, &b, (5.0, 4.0), 2.0), None, "inverted gap");
        assert_eq!(collar_of(&a, &b, (f64::NAN, 5.0), 2.0), None, "NaN bound");
        assert_eq!(collar_of(&[], &b, (4.0, 5.0), 2.0), None, "no A timeline");
        assert_eq!(collar_of(&a, &[], (4.0, 5.0), 2.0), None, "no B timeline");
    }

    // ---- per-gap map correction ---------------------------------------------------------------

    /// A ramp makes misregistration arithmetic instead of noise: at `slope` dB per block, pairing
    /// A against B shifted by `k` blocks biases every difference by exactly `k * slope`. Real
    /// material turns that bias into scatter (the corpus reads it as `spread_db`), but a ramp pins
    /// the magnitude *and* the sign, which scatter cannot.
    const RAMP_SLOPE_DB: f64 = 0.8;
    const RAMP_BASE_DB: f64 = -55.0;

    fn ramp(n: usize, shift_blocks: i64, gain_db: f64) -> Vec<f64> {
        (0..n)
            .map(|i| RAMP_BASE_DB + (i as i64 - shift_blocks) as f64 * RAMP_SLOPE_DB + gain_db)
            .collect()
    }

    /// **The reason the collar takes a lag at all.** B is the same program 6 dB up, but its true
    /// correspondence with A is three blocks later than the global map claims. Read at the nominal
    /// map, the estimator confidently reports the wrong gain; read at the registered lag it is
    /// exact. Both readings have zero spread, so nothing downstream could have told them apart.
    #[test]
    fn a_misregistered_map_biases_the_reading_and_the_lag_removes_it() {
        const SHIFT: i64 = 3;
        let mut a_dbs = ramp(55, 0, 0.0);
        for db in a_dbs.iter_mut().take(30).skip(25) {
            *db = BLOCK_LEVEL_FLOOR_DB;
        }
        let a = timeline(0.0, &a_dbs);
        let b = timeline(0.0, &ramp(55, SHIFT, 6.0));
        let gap = (25.0 * BLOCK_SECS, 30.0 * BLOCK_SECS);

        let nominal = collar_at_lag(&a, &b, gap, 2.0, None).expect("pairs at the nominal map");
        let bias = SHIFT as f64 * RAMP_SLOPE_DB;
        assert!(
            (nominal.offset_db - (6.0 - bias)).abs() < 1e-9,
            "a {SHIFT}-block error biases the reading by exactly {bias} dB: {nominal:?}"
        );
        assert!(
            nominal.spread_db.abs() < 1e-9,
            "and it is a bias, not scatter — nothing here looks wrong: {nominal:?}"
        );

        let lag_secs = SHIFT as f64 * BLOCK_SECS;
        let fixed = collar_at_lag(&a, &b, gap, 2.0, Some(lag_secs)).expect("pairs at the lag");
        assert!(
            (fixed.offset_db - 6.0).abs() < 1e-9,
            "the registered lag recovers the real gain: {fixed:?}"
        );
        // Sign, pinned: `register_donor_window`'s lag is what B's window moves by, positive later
        // in B. Correct it the other way and the error doubles instead of cancelling.
        let wrong_way =
            collar_at_lag(&a, &b, gap, 2.0, Some(-lag_secs)).expect("pairs at the wrong sign");
        assert!(
            (wrong_way.offset_db - (6.0 - 2.0 * bias)).abs() < 1e-9,
            "negating the lag doubles the error: {wrong_way:?}"
        );
    }

    /// `None` (no trusted fit, read at nominal) and `Some(0.0)` (fit trusted, and it said zero) are
    /// different facts about how the number was obtained, and the census separates its cohorts on
    /// exactly that. Identical readings must still be distinguishable in the dump.
    #[test]
    fn the_map_a_collar_was_read_through_is_recorded() {
        let a = timeline(0.0, &holed(100, -30.0, 40, 50));
        let b = timeline(0.0, &flat(100, -24.0));
        let gap = (4.0, 5.0);

        let unregistered = collar_at_lag(&a, &b, gap, 2.0, None).expect("pairs");
        let registered = collar_at_lag(&a, &b, gap, 2.0, Some(0.0)).expect("pairs");

        assert_eq!(unregistered.map_lag_ms, None);
        assert_eq!(registered.map_lag_ms, Some(0.0));
        assert!(
            (unregistered.offset_db - registered.offset_db).abs() < 1e-9,
            "a zero correction is not a different measurement, only a different provenance"
        );

        let moved = collar_at_lag(&a, &b, gap, 2.0, Some(0.3)).expect("pairs");
        assert_eq!(moved.map_lag_ms, Some(300.0), "recorded in ms");
    }
}
