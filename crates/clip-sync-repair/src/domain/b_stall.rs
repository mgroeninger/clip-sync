//! Scan-time B-stall operator warning (A5), with shoulder confirmation for short silences.
//!
//! Finds silent intervals on B's native timeline, inside the alignment overlap, where A is
//! **not** silent over the mapped span — a stall or corruption in B that shifts every later gap's
//! map. Silence present in both files at the same mapped time is program and is never reported.
//! Below [`LENGTH_REPORT_SECS`] a silence is reported only when its **shoulders** show the timeline
//! shifted (B's audio after the silence registers against A at the start of the mapped span, not
//! its end). Advisory only: nothing here changes a patch or skip decision. See
//! `docs/dev/archive/TEMP-b-stall-warning-plan.md` and `docs/dev/archive/TEMP-b-stall-step-confirmation-plan.md`.

use crate::domain::align::TimelineOverlap;
use crate::domain::cross_check::SilenceInterval;
use crate::domain::gap_equivalence::{pearson, window_indices};
use crate::domain::policies::BlockLevel;

/// Minimum B silence (seconds, after overlap clipping) that is a candidate. Below
/// [`LENGTH_REPORT_SECS`] a candidate reports only on a shoulder `step`; chosen by the 2026-09-15
/// shoulder census (51 pairs: both stalls `step`, 0 of 35 dropouts reported).
pub const DEFAULT_STALL_MIN_SECS: f64 = 2.0;
/// A candidate is suppressed when A's silent runs cover at least this fraction of the mapped span.
pub const B_STALL_MAX_A_SILENT_FRACTION: f64 = 0.5;
/// At or above this length a candidate is reported whatever its shoulders say (A5's shipped floor).
pub const LENGTH_REPORT_SECS: f64 = 10.0;

// Shoulder-test constants (plan § 2). Starting values; the § 5 census pins them.
const SHOULDER_SECS: f64 = 4.0;
const PRE_SEARCH_SECS: f64 = 1.0;
const PRE_MAX_LAG_SECS: f64 = 0.5;
const POST_SEARCH_PAD_SECS: f64 = 1.0;
const FLAT_TOL_SECS: f64 = 0.5;
const STEP_MIN_SECS: f64 = 1.0;
const SHOULDER_MIN_R: f64 = 0.70;
const SHOULDER_MARGIN_R: f64 = 0.10;
const SHOULDER_MIN_LIVE_BINS: usize = 8;
/// A-side envelope clamp: digital-zero blocks (`BLOCK_LEVEL_FLOOR_DB`) would otherwise dominate a
/// slice's variance. B's silent shoulder bins are masked out instead (fixed per candidate).
const ENVELOPE_FLOOR_DB: f64 = -80.0;

const TIME_EPS_SECS: f64 = 1e-6;

/// Why the shoulder test could not decide (plan § 2 rule 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShoulderReason {
    /// A shoulder falls outside the scanned B extent, the overlap, or the block levels.
    NoShoulder,
    /// Too few live bins, a flat envelope, or no evaluable A lag.
    DeadShoulder,
    /// The pre-shoulder does not register near the map, so the post-shoulder cannot be read.
    PreUnregistered,
    /// A region clears the r floor but neither beats the other by the margin.
    Ambiguous,
    /// Neither region clears the r floor.
    NoMatch,
}

/// Outcome of the shoulder test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShoulderVerdict {
    /// The timeline shifted by `shift_secs` across the silence.
    Step {
        shift_secs: f64,
    },
    /// The timeline held: program was replaced by silence.
    Flat,
    Undetermined(ShoulderReason),
}

/// Measurements behind a [`ShoulderVerdict`], for the census debug line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShoulderTest {
    pub verdict: ShoulderVerdict,
    pub r_pre: Option<f64>,
    /// `p*`: the pre-shoulder's registered lag against the cumulative map.
    pub pre_lag_secs: Option<f64>,
    pub r_flat: Option<f64>,
    pub r_step: Option<f64>,
    pub b_pre_live_bins: usize,
    pub b_post_live_bins: usize,
    /// Lags (pre and post) skipped because their A slice was not evaluable.
    pub skipped_lags: usize,
}

impl ShoulderTest {
    fn undetermined(reason: ShoulderReason) -> Self {
        Self {
            verdict: ShoulderVerdict::Undetermined(reason),
            r_pre: None,
            pre_lag_secs: None,
            r_flat: None,
            r_step: None,
            b_pre_live_bins: 0,
            b_post_live_bins: 0,
            skipped_lags: 0,
        }
    }

    fn with_verdict(mut self, verdict: ShoulderVerdict) -> Self {
        self.verdict = verdict;
        self
    }
}

/// What a reported stall's offset advance rests on (plan § 2 rule 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BStallEvidence {
    /// The shoulders measured a step; the advance is the measured shift.
    Shoulder,
    /// Long enough to report on length; the shoulders could not decide. Advance = duration.
    Length,
    /// Long enough to report on length, but the shoulders read `flat`. Advance = duration (A5);
    /// consumers of the cumulative offset must not treat it as a shift.
    LengthContradicted,
}

/// One B silence, mapped onto A. Reported, suppressed, and unconfirmed candidates carry this shape.
///
/// A behaviourless reporting record, so it serializes directly as `scan.b_stalls[]`
/// (`docs/json-output.md` § Revision procedure): renaming a field is a JSON contract change.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize)]
pub struct BStallCandidate {
    /// B silent interval, clipped to the overlap.
    pub b_start_secs: f64,
    pub b_end_secs: f64,
    pub duration_secs: f64,
    /// `b_* − offset_used_secs`.
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    /// Pair offset plus the offset advance ([`Self::shift_secs`]) of every earlier **reported** stall.
    pub offset_used_secs: f64,
    /// Fraction of `[a_start, a_end)` covered by A silent runs.
    pub a_silent_fraction: f64,
    /// Offset advance this stall applies to later candidates; `0.0` when not reported.
    pub shift_secs: f64,
    /// `None` only on candidates that were not reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<BStallEvidence>,
    /// Why the shoulders could not decide; set on [`BStallEvidence::Length`] only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict_reason: Option<ShoulderReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_lag_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_r: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flat_r: Option<f64>,
    /// Full shoulder measurements for the debug line; `None` on A-silent suppressions.
    #[serde(skip)]
    pub shoulder: Option<ShoulderTest>,
}

/// What a real scan stamps on [`crate::domain::GapReport::b_stall`]: the floor it ran with and the
/// stalls it reported (empty when none, or when a § 2 rule 0 precondition was missing).
///
/// The floor lives here rather than on [`crate::domain::ScanRecipe`], whose contract is
/// `PartialEq` ⇔ same gap list; a warning floor cannot move a gap boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct BStallReport {
    pub stall_min_secs: f64,
    pub stalls: Vec<BStallCandidate>,
}

/// Result of the detector. `suppressed` (A silent over the mapped span) and `unconfirmed` (short,
/// shoulders did not read `step`) exist for the `tracing::debug` census lines only; neither is
/// reported to the operator.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BStallScan {
    pub reported: Vec<BStallCandidate>,
    pub suppressed: Vec<BStallCandidate>,
    pub unconfirmed: Vec<BStallCandidate>,
}

/// B's side of the scan as the detector reads it.
#[derive(Debug, Clone, Copy)]
pub struct BStallBScan<'a> {
    /// Silence intervals on B's native timeline.
    pub intervals: &'a [SilenceInterval],
    /// B block levels; empty disables the shoulder test (every short candidate is unconfirmed).
    pub levels: &'a [BlockLevel],
    /// `None` means B was not reviewed, so nothing is a candidate.
    pub scanned_end_secs: Option<f64>,
    pub truncated: bool,
}

/// Detect B stalls (A5 plan § 2 rules 1–4; step-confirmation plan § 2).
///
/// `b.intervals` and `a_runs` are silence intervals on each file's native timeline. `a_runs` must
/// be **every** A silent run the scanner produced, not the mutual-silence subset the offset
/// cross-check uses — a subset under-counts A silence and over-reports stalls. `offset_secs` is the
/// pair map (`b = a + offset`).
///
/// When the B scan truncated, an interval reaching the scanned end is skipped (a stopped scan is
/// not evidence of silence); on a completed scan an interval may end exactly at the scanned end.
pub fn detect_b_stalls(
    b: &BStallBScan<'_>,
    a_runs: &[SilenceInterval],
    a_levels: &[BlockLevel],
    offset_secs: f64,
    overlap: &TimelineOverlap,
    stall_min_secs: f64,
) -> BStallScan {
    let mut scan = BStallScan::default();
    let Some(scanned_end) = b.scanned_end_secs else {
        return scan;
    };

    let mut sorted = b.intervals.to_vec();
    sorted.sort_by(|x, y| x.start_secs.total_cmp(&y.start_secs));

    let mut cumulative_offset = offset_secs;
    for interval in sorted {
        let b0 = interval.start_secs.max(overlap.video_b_start_secs);
        let b1 = interval.end_secs.min(overlap.video_b_end_secs);
        let duration = b1 - b0;
        if duration + TIME_EPS_SECS < stall_min_secs {
            continue;
        }
        if b.truncated && b1 + TIME_EPS_SECS >= scanned_end {
            continue;
        }

        let a0 = b0 - cumulative_offset;
        let a1 = b1 - cumulative_offset;
        let mut candidate = BStallCandidate {
            b_start_secs: b0,
            b_end_secs: b1,
            duration_secs: duration,
            a_start_secs: a0,
            a_end_secs: a1,
            offset_used_secs: cumulative_offset,
            a_silent_fraction: covered_fraction(a_runs, a0, a1),
            ..BStallCandidate::default()
        };
        if candidate.a_silent_fraction >= B_STALL_MAX_A_SILENT_FRACTION {
            scan.suppressed.push(candidate);
            continue;
        }

        let shoulder = shoulder_test(
            a_levels,
            b.levels,
            b0,
            b1,
            cumulative_offset,
            overlap,
            scanned_end,
        );
        candidate.shoulder = Some(shoulder);
        candidate.pre_lag_secs = shoulder.pre_lag_secs;
        candidate.step_r = shoulder.r_step;
        candidate.flat_r = shoulder.r_flat;

        let long = duration + TIME_EPS_SECS >= LENGTH_REPORT_SECS;
        let decision = match shoulder.verdict {
            ShoulderVerdict::Step { shift_secs } => Some((shift_secs, BStallEvidence::Shoulder)),
            ShoulderVerdict::Flat if long => Some((duration, BStallEvidence::LengthContradicted)),
            ShoulderVerdict::Undetermined(reason) if long => {
                candidate.verdict_reason = Some(reason);
                Some((duration, BStallEvidence::Length))
            }
            _ => None,
        };
        match decision {
            Some((advance, evidence)) => {
                candidate.shift_secs = advance;
                candidate.evidence = Some(evidence);
                cumulative_offset += advance;
                scan.reported.push(candidate);
            }
            None => scan.unconfirmed.push(candidate),
        }
    }
    scan
}

/// Plan § 2 rules 1–5 for one candidate `[b0, b1)` read through `offset`.
fn shoulder_test(
    a: &[BlockLevel],
    b: &[BlockLevel],
    b0: f64,
    b1: f64,
    offset: f64,
    overlap: &TimelineOverlap,
    scanned_end: f64,
) -> ShoulderTest {
    use ShoulderReason::{Ambiguous, DeadShoulder, NoMatch, NoShoulder, PreUnregistered};

    // Rule 1: both shoulders inside the scanned B extent, the overlap, and the levels.
    let (Some(b_first), Some(b_last), false) = (b.first(), b.last(), a.is_empty()) else {
        return ShoulderTest::undetermined(NoShoulder);
    };
    let block = b_first.end_secs - b_first.start_secs;
    let pre_lo = b0 - SHOULDER_SECS;
    let post_hi = b1 + SHOULDER_SECS;
    let inside = block > 0.0
        && pre_lo + TIME_EPS_SECS >= overlap.video_b_start_secs
        && pre_lo + TIME_EPS_SECS >= b_first.start_secs
        && post_hi <= overlap.video_b_end_secs + TIME_EPS_SECS
        && post_hi <= scanned_end + TIME_EPS_SECS
        && post_hi <= b_last.end_secs + TIME_EPS_SECS;
    let (true, Some((pre0, pre1)), Some((post0, post1))) = (
        inside,
        window_indices(b, pre_lo, b0),
        window_indices(b, b1, post_hi),
    ) else {
        return ShoulderTest::undetermined(NoShoulder);
    };
    let b_pre = &b[pre0..pre1];
    let b_post = &b[post0..post1];

    // Rule 2: dead B shoulder.
    let mut test = ShoulderTest::undetermined(DeadShoulder);
    test.b_pre_live_bins = live_bins(b_pre);
    test.b_post_live_bins = live_bins(b_post);
    if test.b_pre_live_bins < SHOULDER_MIN_LIVE_BINS
        || test.b_post_live_bins < SHOULDER_MIN_LIVE_BINS
        || !live_envelope_varies(b_pre)
        || !live_envelope_varies(b_post)
    {
        return test;
    }

    let blocks = |secs: f64| (secs / block).round() as isize;

    // Rule 3: pre-shoulder registration. Slice index = end index − len + lag.
    let pre_base = a_index_at(a, b0 - offset) - b_pre.len() as isize;
    let pre_reach = blocks(PRE_SEARCH_SECS);
    let mut pre_peak: Option<(isize, f64)> = None;
    for p in -pre_reach..=pre_reach {
        match correlate(b_pre, a, pre_base + p) {
            Some(r) if pre_peak.is_none_or(|(_, best)| r > best) => pre_peak = Some((p, r)),
            Some(_) => {}
            None => test.skipped_lags += 1,
        }
    }
    let Some((p_star, r_pre)) = pre_peak else {
        return test;
    };
    let p_star_secs = p_star as f64 * block;
    test.r_pre = Some(r_pre);
    test.pre_lag_secs = Some(p_star_secs);
    if r_pre < SHOULDER_MIN_R || p_star_secs.abs() > PRE_MAX_LAG_SECS + TIME_EPS_SECS {
        return test.with_verdict(ShoulderVerdict::Undetermined(PreUnregistered));
    }

    // Rule 4: post-shoulder search over shift `s`; A slice starts at `b1 − offset − s + p*`.
    let post_base = a_index_at(a, b1 - offset) + p_star;
    let mut step: Option<(f64, f64)> = None;
    for s in blocks(-POST_SEARCH_PAD_SECS)..=blocks(b1 - b0 + POST_SEARCH_PAD_SECS) {
        let shift = s as f64 * block;
        let in_flat = shift.abs() <= FLAT_TOL_SECS + TIME_EPS_SECS;
        let in_step = shift + TIME_EPS_SECS >= STEP_MIN_SECS;
        if !in_flat && !in_step {
            continue;
        }
        let Some(r) = correlate(b_post, a, post_base - s) else {
            test.skipped_lags += 1;
            continue;
        };
        if in_flat && test.r_flat.is_none_or(|best| r > best) {
            test.r_flat = Some(r);
        }
        if in_step && step.is_none_or(|(_, best)| r > best) {
            step = Some((shift, r));
        }
    }
    test.r_step = step.map(|(_, r)| r);

    // Rule 5: verdict. A region with no evaluable lag reads −1.
    if test.r_flat.is_none() && step.is_none() {
        return test;
    }
    let r_flat = test.r_flat.unwrap_or(-1.0);
    let verdict = match step {
        Some((shift_secs, r_step))
            if r_step >= SHOULDER_MIN_R && r_step - r_flat >= SHOULDER_MARGIN_R =>
        {
            ShoulderVerdict::Step { shift_secs }
        }
        _ => {
            let r_step = test.r_step.unwrap_or(-1.0);
            if r_flat >= SHOULDER_MIN_R && r_flat - r_step >= SHOULDER_MARGIN_R {
                ShoulderVerdict::Flat
            } else if r_flat < SHOULDER_MIN_R && r_step < SHOULDER_MIN_R {
                ShoulderVerdict::Undetermined(NoMatch)
            } else {
                ShoulderVerdict::Undetermined(Ambiguous)
            }
        }
    };
    test.with_verdict(verdict)
}

fn live_bins(levels: &[BlockLevel]) -> usize {
    levels.iter().filter(|l| !l.silent).count()
}

fn live_envelope_varies(levels: &[BlockLevel]) -> bool {
    let live = levels
        .iter()
        .filter(|l| !l.silent)
        .map(|l| clamp_db(l.rms_db));
    let (lo, hi) = live.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    hi - lo > 1e-9
}

fn clamp_db(db: f64) -> f64 {
    db.max(ENVELOPE_FLOOR_DB)
}

/// Index of the first A block whose centre is at or after `t` (a slice boundary at time `t`).
fn a_index_at(a: &[BlockLevel], t: f64) -> isize {
    a.partition_point(|l| (l.start_secs + l.end_secs) / 2.0 < t) as isize
}

/// Pearson r of a B shoulder against the A slice starting at index `a_start`, over the pairs where
/// the B bin is live. `None` when the lag is not evaluable (plan § 2 *Evaluable lag*).
fn correlate(b_shoulder: &[BlockLevel], a: &[BlockLevel], a_start: isize) -> Option<f64> {
    let start = usize::try_from(a_start).ok()?;
    let slice = a.get(start..start.checked_add(b_shoulder.len())?)?;
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    let mut a_live = 0;
    for (bl, al) in b_shoulder.iter().zip(slice).filter(|(bl, _)| !bl.silent) {
        xs.push(clamp_db(bl.rms_db));
        ys.push(clamp_db(al.rms_db));
        a_live += usize::from(!al.silent);
    }
    if a_live < SHOULDER_MIN_LIVE_BINS {
        return None;
    }
    pearson(&xs, &ys)
}

/// For each gap A start, the index of the last reported stall whose `a_start_secs` is at or
/// before it (plan § 2 rule 5). `stalls` must be in detector order.
pub fn attribute_gaps_to_b_stalls(
    gap_a_start_secs: &[f64],
    stalls: &[BStallCandidate],
) -> Vec<Option<usize>> {
    gap_a_start_secs
        .iter()
        .map(|&a_start| {
            let after = stalls.partition_point(|s| s.a_start_secs <= a_start);
            after.checked_sub(1)
        })
        .collect()
}

/// Fraction of `[start, end)` covered by the union of `runs`.
fn covered_fraction(runs: &[SilenceInterval], start: f64, end: f64) -> f64 {
    let span = end - start;
    if span <= 0.0 {
        return 0.0;
    }
    let mut clipped: Vec<(f64, f64)> = runs
        .iter()
        .map(|r| (r.start_secs.max(start), r.end_secs.min(end)))
        .filter(|(s, e)| e > s)
        .collect();
    clipped.sort_by(|x, y| x.0.total_cmp(&y.0));

    let mut covered = 0.0;
    let mut cursor = start;
    for (s, e) in clipped {
        let s = s.max(cursor);
        if e > s {
            covered += e - s;
            cursor = e;
        }
    }
    covered / span
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policies::BLOCK_LEVEL_FLOOR_DB;

    const OFFSET: f64 = 2.0;
    const SCANNED_END: Option<f64> = Some(1100.0);

    fn interval(start: f64, end: f64) -> SilenceInterval {
        SilenceInterval {
            start_secs: start,
            end_secs: end,
        }
    }

    fn overlap() -> TimelineOverlap {
        TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: 1000.0,
            video_b_start_secs: OFFSET,
            video_b_end_secs: 1000.0 + OFFSET,
            shared_length_secs: 1000.0,
        }
    }

    fn b_scan(intervals: &[SilenceInterval], end: Option<f64>, truncated: bool) -> BStallBScan<'_> {
        BStallBScan {
            intervals,
            levels: &[],
            scanned_end_secs: end,
            truncated,
        }
    }

    fn detect(b: &[SilenceInterval], a: &[SilenceInterval]) -> BStallScan {
        detect_b_stalls(
            &b_scan(b, SCANNED_END, false),
            a,
            &[],
            OFFSET,
            &overlap(),
            DEFAULT_STALL_MIN_SECS,
        )
    }

    #[test]
    fn stall_reported_when_a_has_audio() {
        let scan = detect(&[interval(100.0, 160.0)], &[]);
        assert_eq!(scan.reported.len(), 1);
        assert!(scan.suppressed.is_empty());
        let s = scan.reported[0];
        assert_eq!(s.a_silent_fraction, 0.0);
        assert_eq!(s.offset_used_secs, 2.0);
        assert_eq!((s.a_start_secs, s.a_end_secs), (98.0, 158.0));
        assert_eq!(s.duration_secs, 60.0);
        assert_eq!(s.evidence, Some(BStallEvidence::Length));
        assert_eq!(s.verdict_reason, Some(ShoulderReason::NoShoulder));
        assert_eq!(s.shift_secs, 60.0);
    }

    #[test]
    fn mutual_silence_suppressed() {
        let scan = detect(&[interval(100.0, 160.0)], &[interval(98.0, 158.0)]);
        assert!(scan.reported.is_empty());
        assert_eq!(scan.suppressed.len(), 1);
        assert_eq!(scan.suppressed[0].a_silent_fraction, 1.0);
        assert_eq!(
            scan.suppressed[0].shoulder, None,
            "suppression precedes the test"
        );
    }

    #[test]
    fn partial_a_silence_cut_at_half() {
        // Mapped span is A [98, 158) = 60 s.
        let forty = detect(&[interval(100.0, 160.0)], &[interval(98.0, 122.0)]);
        assert_eq!(forty.reported.len(), 1, "40 % A silence reports");

        let sixty = detect(&[interval(100.0, 160.0)], &[interval(98.0, 134.0)]);
        assert!(sixty.reported.is_empty(), "60 % A silence suppresses");

        let half = detect(&[interval(100.0, 160.0)], &[interval(98.0, 128.0)]);
        assert!(half.reported.is_empty(), "exactly 0.5 suppresses");
        assert_eq!(half.suppressed[0].a_silent_fraction, 0.5);
    }

    #[test]
    fn a_silent_fraction_counts_overlapping_runs_once() {
        let scan = detect(
            &[interval(100.0, 160.0)],
            &[interval(98.0, 110.0), interval(104.0, 116.0)],
        );
        let s = scan.reported[0];
        assert!((s.a_silent_fraction - 18.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn short_silence_ignored() {
        let scan = detect(&[interval(100.0, 101.9)], &[]);
        assert_eq!(scan, BStallScan::default());
    }

    #[test]
    fn later_stall_maps_through_cumulative_offset() {
        // A is silent where B 500 lands on the cumulative map (500 − 62 = 438), not on the pair
        // map (500 − 2 = 498). Mapping with the pair offset would report; cumulative suppresses.
        let b = [interval(100.0, 160.0), interval(500.0, 530.0)];
        let a = [interval(438.0, 468.0)];
        let scan = detect(&b, &a);
        assert_eq!(scan.reported.len(), 1);
        assert_eq!(scan.suppressed.len(), 1);
        assert_eq!(scan.suppressed[0].offset_used_secs, 62.0);
        assert_eq!(scan.suppressed[0].a_start_secs, 438.0);
    }

    #[test]
    fn suppressed_candidate_does_not_advance_offset() {
        let b = [interval(100.0, 160.0), interval(500.0, 530.0)];
        let a = [interval(98.0, 158.0)];
        let scan = detect(&b, &a);
        assert_eq!(scan.suppressed.len(), 1);
        assert_eq!(scan.reported.len(), 1);
        assert_eq!(scan.reported[0].offset_used_secs, 2.0);
    }

    #[test]
    fn truncated_scan_end_is_not_a_candidate() {
        let b = [interval(40.0, 60.0), interval(100.0, 160.0)];
        let run = |end: Option<f64>, truncated: bool| {
            detect_b_stalls(
                &b_scan(&b, end, truncated),
                &[],
                &[],
                OFFSET,
                &overlap(),
                DEFAULT_STALL_MIN_SECS,
            )
        };

        let truncated = run(Some(160.0), true);
        assert_eq!(truncated.reported.len(), 1, "earlier interval still counts");
        assert_eq!(truncated.reported[0].b_start_secs, 40.0);

        let completed = run(Some(160.0), false);
        assert_eq!(
            completed.reported.len(),
            2,
            "a completed scan may end exactly at an interval's end"
        );

        assert_eq!(run(None, false), BStallScan::default());
    }

    #[test]
    fn silence_clipped_to_overlap_before_length_test() {
        // Overlap on B ends at 1002.
        let long = detect(&[interval(950.0, 1060.0)], &[]);
        assert_eq!(long.reported.len(), 1);
        assert_eq!(long.reported[0].b_end_secs, 1002.0);
        assert_eq!(long.reported[0].duration_secs, 52.0);

        let short_inside = detect(&[interval(1000.5, 1060.0)], &[]);
        assert_eq!(short_inside, BStallScan::default());
    }

    #[test]
    fn attribution_picks_latest_preceding_stall() {
        let b = [interval(100.0, 160.0), interval(500.0, 530.0)];
        let scan = detect(&b, &[]);
        assert_eq!(scan.reported.len(), 2);
        // Stall A starts: 98 and 500 − 62 = 438.
        let gaps = [50.0, 98.0, 200.0, 438.0, 900.0];
        assert_eq!(
            attribute_gaps_to_b_stalls(&gaps, &scan.reported),
            vec![None, Some(0), Some(0), Some(1), Some(1)]
        );
        assert_eq!(attribute_gaps_to_b_stalls(&gaps, &[]), vec![None; 5]);
    }

    // ---- Shoulder test (step-confirmation plan § 4). ----
    //
    // Synthetic 100 ms envelopes. A is a deterministic non-periodic dB sequence starting at t = 0;
    // B is assembled from segments of A plus silence, also starting at t = 0. B's first segment
    // starts at A block `SHOULDER_A0`, so the pair map is `b = a − SHOULDER_A0 · 0.1`.

    const BLOCK: f64 = 0.1;
    const SHOULDER_A0: usize = 20;
    const SHOULDER_OFFSET: f64 = -(SHOULDER_A0 as f64) * BLOCK;

    /// Deterministic dB in [−40, −10) with no period (LCG), so every lag but the true one decorrelates.
    fn noise_db(seed: u64, i: usize) -> f64 {
        let mut x = seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 33;
        -40.0 + 30.0 * (x % 10_000) as f64 / 10_000.0
    }

    fn level(i: usize, rms_db: f64, silent: bool) -> BlockLevel {
        BlockLevel {
            start_secs: i as f64 * BLOCK,
            end_secs: (i + 1) as f64 * BLOCK,
            rms_db,
            silent,
        }
    }

    fn silent_level(i: usize) -> BlockLevel {
        level(i, BLOCK_LEVEL_FLOOR_DB, true)
    }

    fn silence_blocks(levels: &mut [BlockLevel], range: std::ops::Range<usize>) {
        let (start, end) = (range.start, range.end);
        for (i, l) in levels.iter_mut().enumerate().take(end).skip(start) {
            *l = silent_level(i);
        }
    }

    fn a_levels(len: usize) -> Vec<BlockLevel> {
        (0..len).map(|i| level(i, noise_db(1, i), false)).collect()
    }

    #[derive(Clone, Copy)]
    enum Seg {
        /// `len` blocks copied from A starting at A block `from`.
        A { from: usize, len: usize },
        /// `len` silent blocks, listed as a B silence interval.
        Silence(usize),
        /// `len` blocks at a constant non-silent level (a flat, live envelope).
        Flat(usize),
        /// `len` blocks unrelated to A.
        Other(usize),
    }

    struct Pair {
        a: Vec<BlockLevel>,
        b: Vec<BlockLevel>,
        b_intervals: Vec<SilenceInterval>,
    }

    fn build(a: Vec<BlockLevel>, segs: &[Seg]) -> Pair {
        let mut b = Vec::new();
        let mut b_intervals = Vec::new();
        for seg in segs {
            let i0 = b.len();
            match *seg {
                Seg::A { from, len } => {
                    for k in 0..len {
                        let src = a[from + k];
                        b.push(level(i0 + k, src.rms_db, src.silent));
                    }
                }
                Seg::Silence(len) => {
                    b.extend((0..len).map(|k| silent_level(i0 + k)));
                    b_intervals.push(interval(i0 as f64 * BLOCK, (i0 + len) as f64 * BLOCK));
                }
                Seg::Flat(len) => b.extend((0..len).map(|k| level(i0 + k, -25.0, false))),
                Seg::Other(len) => {
                    b.extend((0..len).map(|k| level(i0 + k, noise_db(99, i0 + k), false)))
                }
            }
        }
        Pair { a, b, b_intervals }
    }

    fn shoulder_overlap(b_end: f64) -> TimelineOverlap {
        TimelineOverlap {
            video_a_start_secs: -SHOULDER_OFFSET,
            video_a_end_secs: b_end - SHOULDER_OFFSET,
            video_b_start_secs: 0.0,
            video_b_end_secs: b_end,
            shared_length_secs: b_end,
        }
    }

    fn run_pair(pair: &Pair, min_secs: f64) -> BStallScan {
        let b_end = pair.b.len() as f64 * BLOCK;
        run_pair_with(pair, min_secs, SHOULDER_OFFSET, Some(b_end))
    }

    fn run_pair_with(
        pair: &Pair,
        min_secs: f64,
        offset: f64,
        scanned_end: Option<f64>,
    ) -> BStallScan {
        let b_end = pair.b.len() as f64 * BLOCK;
        detect_b_stalls(
            &BStallBScan {
                intervals: &pair.b_intervals,
                levels: &pair.b,
                scanned_end_secs: scanned_end,
                truncated: false,
            },
            &[],
            &pair.a,
            offset,
            &shoulder_overlap(b_end),
            min_secs,
        )
    }

    fn verdict(c: &BStallCandidate) -> ShoulderVerdict {
        c.shoulder.expect("shoulder test ran").verdict
    }

    fn assert_step(c: &BStallCandidate, shift: f64) {
        match verdict(c) {
            ShoulderVerdict::Step { shift_secs } => {
                assert!(
                    (shift_secs - shift).abs() < 1e-6,
                    "shift {shift_secs} vs {shift}: {c:?}"
                )
            }
            other => panic!("expected step, got {other:?}: {c:?}"),
        }
    }

    #[test]
    fn shoulder_stall_reads_step_and_advances_by_measured_shift() {
        // B: 10 s of A, 3 s inserted silence, A resumes where it paused.
        let pair = build(
            a_levels(400),
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::A {
                    from: SHOULDER_A0 + 100,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        assert_eq!(scan.reported.len(), 1, "{scan:?}");
        let s = scan.reported[0];
        assert_step(&s, 3.0);
        assert_eq!(s.evidence, Some(BStallEvidence::Shoulder));
        assert!((s.shift_secs - 3.0).abs() < 1e-6);
        assert_eq!(s.verdict_reason, None);
        assert!(s.step_r.unwrap() > 0.99 && s.flat_r.unwrap() < 0.5, "{s:?}");
    }

    #[test]
    fn shoulder_dropout_reads_flat_and_is_not_reported() {
        // B: program replaced by 3 s of silence; the timeline holds.
        let pair = build(
            a_levels(400),
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::A {
                    from: SHOULDER_A0 + 130,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        assert!(scan.reported.is_empty(), "{scan:?}");
        assert_eq!(scan.unconfirmed.len(), 1);
        assert_eq!(verdict(&scan.unconfirmed[0]), ShoulderVerdict::Flat);
    }

    #[test]
    fn shoulder_stall_that_lost_program_reports_measured_not_duration() {
        // 2.8 s silence inserted, 0.3 s of A lost after it ⇒ shift 2.5, not 2.8.
        let pair = build(
            a_levels(400),
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(28),
                Seg::A {
                    from: SHOULDER_A0 + 103,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        assert_eq!(scan.reported.len(), 1, "{scan:?}");
        assert_step(&scan.reported[0], 2.5);
        assert!((scan.reported[0].shift_secs - 2.5).abs() < 1e-6);
    }

    #[test]
    fn shoulder_long_stall_search_reaches_pair_54_shape() {
        // 66.6 s silence, 0.8 s of program lost ⇒ shift 65.8.
        let pair = build(
            a_levels(1200),
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(666),
                Seg::A {
                    from: SHOULDER_A0 + 108,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        assert_eq!(scan.reported.len(), 1, "{scan:?}");
        assert_step(&scan.reported[0], 65.8);
        assert_eq!(scan.reported[0].evidence, Some(BStallEvidence::Shoulder));
    }

    #[test]
    fn shoulder_dead_post_short_unconfirmed_long_reported_on_length() {
        let segs = |silence| {
            [
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(silence),
                Seg::Flat(60),
            ]
        };
        let short = run_pair(&build(a_levels(400), &segs(30)), 2.0);
        assert!(short.reported.is_empty());
        assert_eq!(
            verdict(&short.unconfirmed[0]),
            ShoulderVerdict::Undetermined(ShoulderReason::DeadShoulder)
        );

        let long = run_pair(&build(a_levels(400), &segs(120)), 2.0);
        assert_eq!(long.reported.len(), 1);
        let s = long.reported[0];
        assert_eq!(s.evidence, Some(BStallEvidence::Length));
        assert_eq!(s.verdict_reason, Some(ShoulderReason::DeadShoulder));
        assert!((s.shift_secs - 12.0).abs() < 1e-6);
    }

    #[test]
    fn shoulder_pre_unregistered_when_map_is_already_off() {
        // B's content sits 0.8 s later on A than the map says.
        let pair = build(
            a_levels(400),
            &[
                Seg::A {
                    from: SHOULDER_A0 + 8,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::A {
                    from: SHOULDER_A0 + 108,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        let c = scan.unconfirmed[0];
        assert_eq!(
            verdict(&c),
            ShoulderVerdict::Undetermined(ShoulderReason::PreUnregistered)
        );
        assert!((c.pre_lag_secs.unwrap() - 0.8).abs() < 1e-6, "{c:?}");
    }

    #[test]
    fn shoulder_periodic_material_is_ambiguous() {
        // A repeats every 3 s, the silence is 3 s: both hypotheses match perfectly.
        let a: Vec<BlockLevel> = (0..400)
            .map(|i| level(i, noise_db(1, i % 30), false))
            .collect();
        let pair = build(
            a,
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::A {
                    from: SHOULDER_A0 + 100,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        assert!(scan.reported.is_empty());
        assert_eq!(
            verdict(&scan.unconfirmed[0]),
            ShoulderVerdict::Undetermined(ShoulderReason::Ambiguous)
        );
    }

    #[test]
    fn shoulder_cumulative_map_ignores_pre_lag_in_the_advance() {
        // Pair map is 0.3 s off (inside PRE_MAX_LAG_SECS): dropout, 3 s stall, dropout.
        let a0 = SHOULDER_A0 + 3;
        let pair = build(
            a_levels(800),
            &[
                Seg::A { from: a0, len: 100 },
                Seg::Silence(30), // dropout: A continues underneath
                Seg::A {
                    from: a0 + 130,
                    len: 100,
                },
                Seg::Silence(30), // stall
                Seg::A {
                    from: a0 + 230,
                    len: 100,
                },
                Seg::Silence(30), // dropout
                Seg::A {
                    from: a0 + 360,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        assert_eq!(scan.reported.len(), 1, "{scan:?}");
        assert_eq!(scan.unconfirmed.len(), 2);
        let stall = scan.reported[0];
        assert_step(&stall, 3.0);
        assert!((stall.pre_lag_secs.unwrap() - 0.3).abs() < 1e-6);
        assert!((stall.shift_secs - 3.0).abs() < 1e-6, "advance excludes p*");

        let [first, second] = [scan.unconfirmed[0], scan.unconfirmed[1]];
        assert_eq!(verdict(&first), ShoulderVerdict::Flat);
        assert!((first.offset_used_secs - SHOULDER_OFFSET).abs() < 1e-9);
        assert_eq!(verdict(&second), ShoulderVerdict::Flat);
        assert!((second.offset_used_secs - (SHOULDER_OFFSET + 3.0)).abs() < 1e-9);
        assert!((second.a_start_secs - (second.b_start_secs - SHOULDER_OFFSET - 3.0)).abs() < 1e-9);
    }

    #[test]
    fn shoulder_outside_scanned_end_is_no_shoulder() {
        let pair = build(
            a_levels(400),
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::A {
                    from: SHOULDER_A0 + 100,
                    len: 100,
                },
            ],
        );
        // Post-shoulder needs B up to 17 s; the scan stopped at 15 s.
        let scan = run_pair_with(&pair, 2.0, SHOULDER_OFFSET, Some(15.0));
        assert_eq!(
            verdict(&scan.unconfirmed[0]),
            ShoulderVerdict::Undetermined(ShoulderReason::NoShoulder)
        );
    }

    #[test]
    fn shoulder_without_levels_short_unconfirmed_long_on_length() {
        let b = [interval(100.0, 103.0), interval(200.0, 212.0)];
        let scan = detect_b_stalls(
            &b_scan(&b, SCANNED_END, false),
            &[],
            &a_levels(100),
            OFFSET,
            &overlap(),
            2.0,
        );
        assert_eq!(
            verdict(&scan.unconfirmed[0]),
            ShoulderVerdict::Undetermined(ShoulderReason::NoShoulder)
        );
        assert_eq!(scan.reported.len(), 1);
        assert_eq!(scan.reported[0].evidence, Some(BStallEvidence::Length));
    }

    #[test]
    fn shoulder_quiet_a_stretch_skips_lags_not_the_candidate() {
        // Pair-54 shape with 4 s of A silence mid-span: lags whose slice sits in it are skipped.
        let mut a = a_levels(1200);
        silence_blocks(&mut a, 400..440);
        let pair = build(
            a,
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(666),
                Seg::A {
                    from: SHOULDER_A0 + 108,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        let s = scan.reported[0];
        assert_step(&s, 65.8);
        assert!(s.shoulder.unwrap().skipped_lags > 0, "{s:?}");
    }

    #[test]
    fn shoulder_long_flat_is_length_contradicted() {
        let pair = build(
            a_levels(500),
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(120),
                Seg::A {
                    from: SHOULDER_A0 + 220,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        let s = scan.reported[0];
        assert_eq!(verdict(&s), ShoulderVerdict::Flat);
        assert_eq!(s.evidence, Some(BStallEvidence::LengthContradicted));
        assert_eq!(s.verdict_reason, None);
        assert!((s.shift_secs - 12.0).abs() < 1e-6);
    }

    #[test]
    fn shoulder_a_slice_past_a_end_is_skipped() {
        // A ends 4.3 s after the dropout's mapped end: flat-region lags with s < −0.3 overrun it.
        let a_len = SHOULDER_A0 + 130 + 43;
        let pair = build(
            a_levels(a_len),
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::A {
                    from: SHOULDER_A0 + 130,
                    len: 40,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        let c = scan.unconfirmed[0];
        assert_eq!(verdict(&c), ShoulderVerdict::Flat);
        assert!(c.shoulder.unwrap().skipped_lags > 0, "{c:?}");
    }

    #[test]
    fn shoulder_silent_b_pre_bins_are_masked() {
        // Half of B pre is digital zero (A has audio there). Unmasked, the 20 −120 dB bins would
        // carry the variance and r would collapse; masked, the live half registers.
        let mut pair = build(
            a_levels(400),
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::A {
                    from: SHOULDER_A0 + 100,
                    len: 100,
                },
            ],
        );
        silence_blocks(&mut pair.b, 60..80);
        let scan = run_pair(&pair, 2.0);
        assert_eq!(scan.reported.len(), 1, "{scan:?}");
        assert_step(&scan.reported[0], 3.0);
        assert_eq!(scan.reported[0].shoulder.unwrap().b_pre_live_bins, 20);
    }

    #[test]
    fn shoulder_region_without_evaluable_lag() {
        // Flat region's A slices are silent; the step region still reads the stall.
        // Stall: B's post shoulder is A from `resume` (= b0 − offset); the flat hypothesis reads
        // A from `resume + 30` (= b1 − offset). Silence A over every flat-region slice.
        let resume = SHOULDER_A0 + 100;
        let mut a = a_levels(400);
        silence_blocks(&mut a, (resume + 25)..(resume + 75));
        // B after the stall copies A (silence included), so its post shoulder keeps 25 live bins.
        let pair = build(
            a,
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::A {
                    from: resume,
                    len: 100,
                },
            ],
        );
        let scan = run_pair(&pair, 2.0);
        let c = scan.reported[0];
        assert_step(&c, 3.0);
        assert_eq!(c.flat_r, None, "no evaluable flat lag: {c:?}");

        // Every post-search slice silent and B's post unrelated to A ⇒ dead shoulder.
        let mut a = a_levels(400);
        silence_blocks(&mut a, (SHOULDER_A0 + 95)..(SHOULDER_A0 + 185));
        let pair = build(
            a,
            &[
                Seg::A {
                    from: SHOULDER_A0,
                    len: 100,
                },
                Seg::Silence(30),
                Seg::Other(100),
            ],
        );
        let scan = run_pair(&pair, 2.0);
        let c = scan.unconfirmed[0];
        assert_eq!(
            verdict(&c),
            ShoulderVerdict::Undetermined(ShoulderReason::DeadShoulder),
            "{c:?}"
        );
        assert!(
            c.pre_lag_secs.is_some(),
            "pre registered; post had nothing to read"
        );
    }
}
