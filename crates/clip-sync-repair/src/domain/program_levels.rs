//! Whole-program A↔B level relationship, measured as a **paired per-block difference**.
//!
//! Plan: `docs/dev/TEMP-ab-program-levels-plan.md`. Capture only — nothing here feeds a gain.
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

use crate::domain::cross_check::b_range_fully_scanned;
use crate::domain::level_stats::{median_db, median_of_sorted, percentile_of_sorted, sort_db};
use crate::domain::policies::{absolute_silence_floor_db, BlockLevel, BLOCK_LEVEL_FLOOR_DB};

/// Tolerance for the steadiness filter — a block is paired only when both its A-side neighbours sit
/// within this many dB of it. Unvalidated starting point (no corpus run yet); nothing behavioural
/// keys off it, a wrong value only widens `spread_db`.
pub const STEADY_TOLERANCE_DB: f64 = 3.0;

/// Minimum accepted pairs before an offset is reported at all. Below it the summary abstains rather
/// than inventing an offset from a handful of blocks.
pub const MIN_PAIRS: usize = 50;

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
/// which also leaves `b_levels` empty, so nothing pairs regardless. Unlike
/// [`b_range_fully_scanned`], `None` is therefore *not* treated as a rejection here; a mapped window
/// starting before B's timeline still is.
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
    let mut n_active = 0usize;
    let mut n_steady = 0usize;
    // (A block start, difference) so chunking can bucket by A's timeline without a second pass.
    let mut pairs: Vec<(f64, f64)> = Vec::new();

    for (i, a) in a_levels.iter().enumerate() {
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
        // 4: fail closed outside the reviewed B prefix, through the same helper the
        // donor-occupancy read uses — which also rejects a window mapped before B's start.
        let reviewed = match b_scanned_end_secs {
            Some(_) => b_range_fully_scanned(b_lo, b_hi, b_scanned_end_secs),
            // No bound recorded. `b_range_fully_scanned` would reject outright; here that would
            // make the whole estimator unusable without a bound, so only the prefix half is
            // vacuous and the start-of-timeline half still applies.
            None => b_lo >= 0.0,
        };
        if !reviewed {
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

    if pairs.len() < params.min_pairs {
        return None;
    }

    // The guard above proves the population is non-empty, so the sorted-slice estimators apply
    // directly — no `Option` to unwrap, and one sort instead of four.
    debug_assert!(!pairs.is_empty());
    let mut diffs: Vec<f64> = pairs.iter().map(|&(_, d)| d).collect();
    sort_db(&mut diffs);
    let offset_db = median_of_sorted(&diffs);
    let mut deviations: Vec<f64> = diffs.iter().map(|d| (d - offset_db).abs()).collect();
    sort_db(&mut deviations);
    Some(AbLevelOffset {
        offset_db,
        spread_db: median_of_sorted(&deviations),
        p10_db: percentile_of_sorted(&diffs, 0.1),
        p90_db: percentile_of_sorted(&diffs, 0.9),
        n_pairs: pairs.len(),
        n_active,
        n_steady,
        first_pair_secs: pairs[0].0,
        last_pair_secs: pairs[pairs.len() - 1].0,
        channels_comparable,
        chunks: chunk_offsets(&pairs, params),
    })
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
            "three map before B's start and are dropped whole, not partially paired"
        );
        assert!(
            (got.first_pair_secs - 3.0 * BLOCK_SECS).abs() < 1e-9,
            "{got:?}"
        );
    }

    #[test]
    fn a_window_straddling_b_start_is_dropped_not_partially_paired() {
        // Half a block of negative offset: A block 0 maps to [-0.05, 0.05) — `max_overlap_block`
        // would happily pair it against B block 0 on the overlapping half. The bound rejects it.
        let a = timeline(0.0, &flat(20, -30.0));
        let b = timeline(0.0, &flat(20, -24.0));

        let got =
            paired_level_offset(&a, &b, -0.5 * BLOCK_SECS, None, true, &params()).expect("offset");

        assert_eq!(
            got.n_pairs, 19,
            "block 0 straddles B's start and is dropped"
        );
        assert!((got.first_pair_secs - BLOCK_SECS).abs() < 1e-9, "{got:?}");
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
}
