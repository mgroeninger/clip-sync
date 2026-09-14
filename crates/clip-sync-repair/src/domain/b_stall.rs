//! Scan-time B-stall operator warning (A5).
//!
//! Finds long silent intervals on B's native timeline, inside the alignment overlap, where A is
//! **not** silent over the mapped span — a stall or corruption in B that shifts every later gap's
//! map. Silence present in both files at the same mapped time is program and is never reported.
//! Advisory only: nothing here changes a patch or skip decision. See
//! `docs/dev/archive/TEMP-b-stall-warning-plan.md`.

use crate::domain::align::TimelineOverlap;
use crate::domain::cross_check::SilenceInterval;

/// Minimum B silence (seconds, after overlap clipping) that is a candidate. Starting value; the
/// plan's § 5 census picks the shipped default.
pub const DEFAULT_STALL_MIN_SECS: f64 = 10.0;
/// A candidate is suppressed when A's silent runs cover at least this fraction of the mapped span.
pub const B_STALL_MAX_A_SILENT_FRACTION: f64 = 0.5;

const TIME_EPS_SECS: f64 = 1e-6;

/// One long B silence, mapped onto A. Both reported and suppressed candidates carry this shape.
///
/// A behaviourless reporting record, so it serializes directly as `scan.b_stalls[]`
/// (`docs/json-output.md` § Revision procedure): renaming a field is a JSON contract change.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct BStallCandidate {
    /// B silent interval, clipped to the overlap.
    pub b_start_secs: f64,
    pub b_end_secs: f64,
    pub duration_secs: f64,
    /// `b_* − offset_used_secs`.
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    /// Pair offset plus the durations of every earlier **reported** stall.
    pub offset_used_secs: f64,
    /// Fraction of `[a_start, a_end)` covered by A silent runs.
    pub a_silent_fraction: f64,
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

/// Result of the detector. `suppressed` exists for the `tracing::debug` census line only; it is
/// not reported to the operator.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BStallScan {
    pub reported: Vec<BStallCandidate>,
    pub suppressed: Vec<BStallCandidate>,
}

/// Detect B stalls (plan § 2 rules 1–4).
///
/// `b_intervals` and `a_runs` are silence intervals on each file's native timeline (sorted by
/// start, as `SilenceRunScanner` produces). `a_runs` must be **every** A silent run the scanner
/// produced, not the mutual-silence subset the offset cross-check uses — a subset under-counts A
/// silence and over-reports stalls. `offset_secs` is the pair map (`b = a + offset`).
///
/// A missing `b_scanned_end_secs` means B was not reviewed, so nothing is a candidate. When the B
/// scan truncated, an interval reaching the scanned end is skipped (a stopped scan is not evidence
/// of silence); on a completed scan an interval may end exactly at the scanned end.
pub fn detect_b_stalls(
    b_intervals: &[SilenceInterval],
    a_runs: &[SilenceInterval],
    offset_secs: f64,
    overlap: &TimelineOverlap,
    b_scanned_end_secs: Option<f64>,
    b_scan_truncated: bool,
    stall_min_secs: f64,
) -> BStallScan {
    let mut scan = BStallScan::default();
    let Some(scanned_end) = b_scanned_end_secs else {
        return scan;
    };

    let mut sorted = b_intervals.to_vec();
    sorted.sort_by(|x, y| x.start_secs.total_cmp(&y.start_secs));

    let mut cumulative_offset = offset_secs;
    for interval in sorted {
        let b0 = interval.start_secs.max(overlap.video_b_start_secs);
        let b1 = interval.end_secs.min(overlap.video_b_end_secs);
        if b1 - b0 + TIME_EPS_SECS < stall_min_secs {
            continue;
        }
        if b_scan_truncated && b1 + TIME_EPS_SECS >= scanned_end {
            continue;
        }

        let a0 = b0 - cumulative_offset;
        let a1 = b1 - cumulative_offset;
        let candidate = BStallCandidate {
            b_start_secs: b0,
            b_end_secs: b1,
            duration_secs: b1 - b0,
            a_start_secs: a0,
            a_end_secs: a1,
            offset_used_secs: cumulative_offset,
            a_silent_fraction: covered_fraction(a_runs, a0, a1),
        };
        if candidate.a_silent_fraction < B_STALL_MAX_A_SILENT_FRACTION {
            cumulative_offset += candidate.duration_secs;
            scan.reported.push(candidate);
        } else {
            scan.suppressed.push(candidate);
        }
    }
    scan
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

    fn detect(b: &[SilenceInterval], a: &[SilenceInterval]) -> BStallScan {
        detect_b_stalls(
            b,
            a,
            OFFSET,
            &overlap(),
            SCANNED_END,
            false,
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
    }

    #[test]
    fn mutual_silence_suppressed() {
        let scan = detect(&[interval(100.0, 160.0)], &[interval(98.0, 158.0)]);
        assert!(scan.reported.is_empty());
        assert_eq!(scan.suppressed.len(), 1);
        assert_eq!(scan.suppressed[0].a_silent_fraction, 1.0);
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
        let scan = detect(&[interval(100.0, 109.9)], &[]);
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
        let run = |truncated: bool| {
            detect_b_stalls(
                &b,
                &[],
                OFFSET,
                &overlap(),
                Some(160.0),
                truncated,
                DEFAULT_STALL_MIN_SECS,
            )
        };

        let truncated = run(true);
        assert_eq!(truncated.reported.len(), 1, "earlier interval still counts");
        assert_eq!(truncated.reported[0].b_start_secs, 40.0);

        let completed = run(false);
        assert_eq!(
            completed.reported.len(),
            2,
            "a completed scan may end exactly at an interval's end"
        );

        let unscanned = detect_b_stalls(
            &b,
            &[],
            OFFSET,
            &overlap(),
            None,
            false,
            DEFAULT_STALL_MIN_SECS,
        );
        assert_eq!(unscanned, BStallScan::default());
    }

    #[test]
    fn silence_clipped_to_overlap_before_length_test() {
        // Overlap on B ends at 1002.
        let long = detect(&[interval(950.0, 1060.0)], &[]);
        assert_eq!(long.reported.len(), 1);
        assert_eq!(long.reported[0].b_end_secs, 1002.0);
        assert_eq!(long.reported[0].duration_secs, 52.0);

        let short_inside = detect(&[interval(995.0, 1060.0)], &[]);
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
}
