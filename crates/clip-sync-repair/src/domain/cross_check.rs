use crate::domain::align::TimelineOverlap;

use crate::domain::gap::interval_fully_within_window;
use crate::domain::gap::{Gap, GapOffsetAgreement};
use crate::domain::gap_equivalence::{GapEquivalenceClass, GapEquivalenceVerdict};
use crate::domain::policies::BlockLevel;

/// A silence interval on a single file's native timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceInterval {
    pub start_secs: f64,
    pub end_secs: f64,
}

/// Returns `true` if `[start, end]` on B's native timeline contains any non-silent audio.
///
/// Intervals must be sorted by `start_secs` (as produced by `SilenceRunScanner`). A range is
/// considered to have energy if any sub-second of it is not covered by a silence interval.
///
/// Prefer [`b_has_energy_from_levels`] for fillability: hold-bridged silence runs can span
/// real audio, so interval coverage is not a trustworthy occupancy oracle.
pub fn b_has_energy_in_range(b_intervals: &[SilenceInterval], start: f64, end: f64) -> bool {
    if start >= end {
        return false;
    }
    let mut covered_up_to = start;
    for interval in b_intervals {
        if interval.end_secs <= start {
            continue;
        }
        if interval.start_secs >= end {
            break;
        }
        if interval.start_secs > covered_up_to {
            return true;
        }
        covered_up_to = covered_up_to.max(interval.end_secs);
    }
    covered_up_to < end
}

/// Absolute B occupancy from the per-block level timeline (fillability signal).
///
/// A range has energy when any analysis block whose center falls in `[start, end)` has
/// [`BlockLevel::silent`] `false` — the scanner's peak-domain, per-channel predicate
/// (including absolute RMS floor), recorded before hold bridging. Do **not** re-threshold
/// `rms_db`: interleaved RMS is a different domain (downmix-diluted, typically 10–20 dB
/// below peak) and would raise the effective bar vs the original `is_silent_interleaved`
/// occupancy path.
///
/// Degenerate ranges and windows with no overlapping blocks return `false` (no evidence of
/// donor energy). Distinct from the equivalence gate's relative `donor_silence_fraction`:
/// this answers "is there anything to copy?", not "is this mutual/ambient quiet?".
pub fn b_has_energy_from_levels(b_levels: &[BlockLevel], start: f64, end: f64) -> bool {
    if start >= end {
        debug_assert!(
            start == end,
            "degenerate B occupancy range [{start}, {end})"
        );
        return false;
    }
    b_levels.iter().any(|b| {
        let c = (b.start_secs + b.end_secs) / 2.0;
        c >= start && c < end && !b.silent
    })
}

/// True when `[start, end]` lies entirely in the reviewed B scan prefix `[0, scanned_end]`.
///
/// Used to fail-closed absolute occupancy (and equivalence's B window) when the B walk truncated
/// or the mapped core sticks past what was measured.
pub fn b_range_fully_scanned(start: f64, end: f64, scanned_end_secs: Option<f64>) -> bool {
    match scanned_end_secs {
        Some(limit) if start < end => start >= 0.0 && end <= limit,
        _ => false,
    }
}

/// Silence on A for the mutual-silence offset cross-check.
///
/// Prefer [`GapEquivalenceClass::SharedSilence`] (donor metric) so fillability cannot poison
/// alignment. When equivalence is [`NotEvaluated`] (e.g. no A noise-floor context on a
/// file-spanning silent run), fall back to mapped `!b_has_energy` — occupancy alone, only
/// when the gate made no decision. RepairableDropout / AmbientQuiet stay excluded.
pub fn mutual_silence_intervals_from_gaps(
    gaps: &[Gap],
    gap_equivalence: &[GapEquivalenceVerdict],
) -> Vec<SilenceInterval> {
    debug_assert_eq!(
        gaps.len(),
        gap_equivalence.len(),
        "gaps and equivalence must be index-parallel"
    );
    gaps.iter()
        .zip(gap_equivalence.iter())
        .filter(|(gap, eq)| {
            if gap.video_b_start_secs.is_none() {
                return false;
            }
            match eq.class {
                GapEquivalenceClass::SharedSilence => true,
                GapEquivalenceClass::NotEvaluated => !gap.b_has_energy,
                GapEquivalenceClass::RepairableDropout | GapEquivalenceClass::AmbientQuiet => false,
            }
        })
        .map(|(gap, _)| SilenceInterval {
            start_secs: gap.video_a_start_secs,
            end_secs: gap.video_a_end_secs,
        })
        .collect()
}

/// Keep only intervals fully contained in the alignment overlap on each native timeline.
pub fn filter_intervals_for_cross_check(
    a_intervals: &[SilenceInterval],
    b_intervals: &[SilenceInterval],
    overlap: &TimelineOverlap,
) -> (Vec<SilenceInterval>, Vec<SilenceInterval>) {
    let a = a_intervals
        .iter()
        .filter(|interval| {
            interval_fully_within_window(
                interval.start_secs,
                interval.end_secs,
                overlap.video_a_start_secs,
                overlap.video_a_end_secs,
            )
        })
        .cloned()
        .collect();
    let b = b_intervals
        .iter()
        .filter(|interval| {
            interval_fully_within_window(
                interval.start_secs,
                interval.end_secs,
                overlap.video_b_start_secs,
                overlap.video_b_end_secs,
            )
        })
        .cloned()
        .collect();
    (a, b)
}

/// Compare alignment offset with a silence-structure estimate, using only overlap-contained intervals.
pub fn check_gap_offset_agreement_in_overlap(
    a_intervals: &[SilenceInterval],
    b_intervals: &[SilenceInterval],
    overlap: Option<&TimelineOverlap>,
    recommended_offset_secs: f64,
    tolerance_secs: f64,
) -> Option<GapOffsetAgreement> {
    let (a, b) = match overlap {
        Some(ov) => filter_intervals_for_cross_check(a_intervals, b_intervals, ov),
        None => (a_intervals.to_vec(), b_intervals.to_vec()),
    };
    check_gap_offset_agreement(&a, &b, recommended_offset_secs, tolerance_secs)
}

/// Compare the Chromaprint alignment offset with one derived from silence-structure overlap.
/// Returns `None` when either timeline has no silence or no candidate shift produces overlap.
pub fn check_gap_offset_agreement(
    a_intervals: &[SilenceInterval],
    b_intervals: &[SilenceInterval],
    recommended_offset_secs: f64,
    tolerance_secs: f64,
) -> Option<GapOffsetAgreement> {
    let silence_offset = silence_based_offset(a_intervals, b_intervals)?;
    let delta = (silence_offset - recommended_offset_secs).abs();
    Some(GapOffsetAgreement {
        silence_based_offset_secs: silence_offset,
        alignment_offset_secs: recommended_offset_secs,
        delta_secs: delta,
        agrees: delta <= tolerance_secs,
    })
}

/// Find the shift Δ (b_pos = a_pos + Δ) that maximises total silence-interval overlap between A
/// and B timelines. Returns `None` when no candidate shift produces any overlap.
///
/// Candidates are boundary-aligned shifts: Δ = b.start − a.start and Δ = b.end − a.end for
/// every (a, b) interval pair. This is O((N·M)²) in the number of pairs — fine for typical gap
/// counts (<<100 per timeline).
pub fn silence_based_offset(
    a_intervals: &[SilenceInterval],
    b_intervals: &[SilenceInterval],
) -> Option<f64> {
    if a_intervals.is_empty() || b_intervals.is_empty() {
        return None;
    }

    let mut candidates: Vec<f64> = Vec::with_capacity(2 * a_intervals.len() * b_intervals.len());
    for a in a_intervals {
        for b in b_intervals {
            candidates.push(b.start_secs - a.start_secs);
            candidates.push(b.end_secs - a.end_secs);
        }
    }
    candidates.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    candidates.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

    let best = candidates.into_iter().max_by(|&d1, &d2| {
        let o1 = total_overlap(a_intervals, b_intervals, d1);
        let o2 = total_overlap(a_intervals, b_intervals, d2);
        o1.partial_cmp(&o2).unwrap_or(std::cmp::Ordering::Equal)
    })?;

    if total_overlap(a_intervals, b_intervals, best) <= 0.0 {
        return None;
    }

    Some(best)
}

/// Sum of overlap lengths for all (a, b) interval pairs given offset Δ (b_on_a = b − Δ).
fn total_overlap(a: &[SilenceInterval], b: &[SilenceInterval], delta: f64) -> f64 {
    let mut sum = 0.0f64;
    for ai in a {
        for bi in b {
            // Map B interval to A's clock: [bi.start − Δ, bi.end − Δ]
            let mapped_start = bi.start_secs - delta;
            let mapped_end = bi.end_secs - delta;
            let overlap_start = ai.start_secs.max(mapped_start);
            let overlap_end = ai.end_secs.min(mapped_end);
            if overlap_end > overlap_start {
                sum += overlap_end - overlap_start;
            }
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interval(start: f64, end: f64) -> SilenceInterval {
        SilenceInterval {
            start_secs: start,
            end_secs: end,
        }
    }

    use crate::domain::gap::Gap;
    use crate::domain::policies::BLOCK_LEVEL_FLOOR_DB;

    fn level(start: f64, end: f64, rms_db: f64, silent: bool) -> BlockLevel {
        BlockLevel {
            start_secs: start,
            end_secs: end,
            rms_db,
            silent,
        }
    }

    #[test]
    fn levels_occupancy_sees_energy_inside_hold_bridged_silence_span() {
        // Hold-bridged run would cover [0, 4] as one silence interval, but block 2 is loud.
        let levels = [
            level(0.0, 1.0, BLOCK_LEVEL_FLOOR_DB, true),
            level(1.0, 2.0, BLOCK_LEVEL_FLOOR_DB, true),
            level(2.0, 3.0, -20.0, false),
            level(3.0, 4.0, BLOCK_LEVEL_FLOOR_DB, true),
        ];
        let bridged = [interval(0.0, 4.0)];
        assert!(
            !b_has_energy_in_range(&bridged, 0.0, 4.0),
            "interval coverage treats the hold-bridged span as fully silent"
        );
        assert!(
            b_has_energy_from_levels(&levels, 0.0, 4.0),
            "levels must still report the occupied block as energy"
        );
    }

    #[test]
    fn levels_occupancy_uses_scanner_silent_bit_not_rms_rethreshold() {
        // Scanner marked both silent (abs floor / peak path); rms alone would be ambiguous.
        let levels = [
            level(0.0, 1.0, BLOCK_LEVEL_FLOOR_DB, true),
            level(1.0, 2.0, -80.0, true),
        ];
        assert!(!b_has_energy_from_levels(&levels, 0.0, 2.0));
        // Same rms, silent=false → occupied (center-only dialogue diluted in interleaved RMS).
        let occupied = [level(0.0, 1.0, -55.0, false)];
        assert!(b_has_energy_from_levels(&occupied, 0.0, 1.0));
    }

    #[test]
    fn levels_occupancy_false_on_empty_window_or_degenerate_range() {
        let levels = [level(0.0, 1.0, -20.0, false)];
        assert!(!b_has_energy_from_levels(&levels, 5.0, 6.0));
        assert!(!b_has_energy_from_levels(&levels, 1.0, 1.0));
        assert!(!b_has_energy_from_levels(&[], 0.0, 1.0));
    }

    #[test]
    fn b_range_fully_scanned_requires_complete_coverage() {
        assert!(b_range_fully_scanned(1.0, 5.0, Some(5.0)));
        assert!(!b_range_fully_scanned(1.0, 5.1, Some(5.0)));
        assert!(!b_range_fully_scanned(-0.1, 4.0, Some(5.0)));
        assert!(!b_range_fully_scanned(1.0, 4.0, None));
        assert!(!b_range_fully_scanned(2.0, 2.0, Some(5.0)));
    }

    #[test]
    fn mutual_silence_intervals_use_shared_silence_not_fillability() {
        use crate::domain::gap_equivalence::GapEquivalenceVerdict;

        let gaps = [
            Gap {
                video_a_start_secs: 10.0,
                video_a_end_secs: 20.0,
                video_b_start_secs: Some(3.0),
                video_b_end_secs: Some(13.0),
                // Absolute occupancy wrong (false) — still excluded via RepairableDropout.
                b_has_energy: false,
            },
            Gap {
                video_a_start_secs: 30.0,
                video_a_end_secs: 40.0,
                video_b_start_secs: Some(23.0),
                video_b_end_secs: Some(33.0),
                b_has_energy: true, // fillability would exclude; SharedSilence includes
            },
            Gap {
                video_a_start_secs: 50.0,
                video_a_end_secs: 60.0,
                video_b_start_secs: None,
                video_b_end_secs: None,
                b_has_energy: false,
            },
            Gap {
                video_a_start_secs: 70.0,
                video_a_end_secs: 80.0,
                video_b_start_secs: Some(63.0),
                video_b_end_secs: Some(73.0),
                b_has_energy: false, // NotEvaluated fallback includes
            },
        ];
        let equivalence = [
            GapEquivalenceVerdict {
                class: GapEquivalenceClass::RepairableDropout,
                drop: false,
                a_gap_rms_db: Some(-100.0),
                noise_floor_db: Some(-50.0),
                a_below_noise_db: Some(-50.0),
                donor_silence_fraction: Some(0.0),
            },
            GapEquivalenceVerdict {
                class: GapEquivalenceClass::SharedSilence,
                drop: true,
                a_gap_rms_db: Some(-80.0),
                noise_floor_db: Some(-70.0),
                a_below_noise_db: Some(-10.0),
                donor_silence_fraction: Some(1.0),
            },
            GapEquivalenceVerdict {
                class: GapEquivalenceClass::NotEvaluated,
                drop: false,
                a_gap_rms_db: None,
                noise_floor_db: None,
                a_below_noise_db: None,
                donor_silence_fraction: None,
            },
            GapEquivalenceVerdict {
                class: GapEquivalenceClass::NotEvaluated,
                drop: false,
                a_gap_rms_db: None,
                noise_floor_db: None,
                a_below_noise_db: None,
                donor_silence_fraction: None,
            },
        ];
        let intervals = mutual_silence_intervals_from_gaps(&gaps, &equivalence);
        assert_eq!(intervals.len(), 2);
        assert!((intervals[0].start_secs - 30.0).abs() < f64::EPSILON);
        assert!((intervals[1].start_secs - 70.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cross_check_ignores_a_only_dropouts_when_spurious_shift_would_win() {
        // Dropout on A would spuriously align with unrelated B quiet at Δ≈341; mutual quiet agrees at −7.
        let a_with_dropout = [interval(10.0, 30.0), interval(100.0, 110.0)];
        let a_mutual_only = [interval(100.0, 110.0)];
        let b_spurious = [interval(93.0, 103.0), interval(351.0, 371.0)];
        let b_mutual = [interval(93.0, 103.0)];

        let spurious = silence_based_offset(&a_with_dropout, &b_spurious).expect("offset");
        assert!(
            (spurious - 341.0).abs() < 1.0,
            "unfiltered A dropouts should pick spurious Δ, got {spurious}"
        );

        let filtered = silence_based_offset(&a_mutual_only, &b_mutual).expect("offset");
        assert!(
            (filtered - (-7.0)).abs() < 1e-6,
            "mutual quiet only should recover true Δ, got {filtered}"
        );

        let agreement =
            check_gap_offset_agreement(&a_mutual_only, &b_mutual, -7.0, 0.5).expect("agreement");
        assert!(agreement.agrees);
    }

    #[test]
    fn silence_offset_recovered_from_colocated_gaps() {
        // A has silence at [10, 20]; B (shifted +5 s) has silence at [15, 25].
        let a = [interval(10.0, 20.0)];
        let b = [interval(15.0, 25.0)];
        let delta = silence_based_offset(&a, &b).expect("should find offset");
        assert!((delta - 5.0).abs() < 1e-6, "expected Δ≈5.0, got {delta}");
    }

    #[test]
    fn silence_offset_multiple_gaps() {
        // Two co-located pairs with consistent offset +12.
        let a = [interval(0.0, 5.0), interval(30.0, 40.0)];
        let b = [interval(12.0, 17.0), interval(42.0, 52.0)];
        let delta = silence_based_offset(&a, &b).expect("should find offset");
        assert!((delta - 12.0).abs() < 1e-6, "expected Δ≈12.0, got {delta}");
    }

    #[test]
    fn silence_offset_no_shared_silence_returns_none() {
        let a: Vec<SilenceInterval> = vec![];
        let b = [interval(10.0, 20.0)];
        assert!(silence_based_offset(&a, &b).is_none());

        let a2 = [interval(10.0, 20.0)];
        let b2: Vec<SilenceInterval> = vec![];
        assert!(silence_based_offset(&a2, &b2).is_none());
    }

    #[test]
    fn gap_offset_agreement_agrees_within_tolerance() {
        let a = [interval(10.0, 20.0)];
        let b = [interval(15.0, 25.0)]; // true offset = 5.0
        let result = check_gap_offset_agreement(&a, &b, 5.05, 0.5).expect("should compute");
        assert!(
            result.agrees,
            "delta {} should be within tolerance 0.5",
            result.delta_secs
        );
        assert!((result.delta_secs - 0.05).abs() < 1e-3);
    }

    #[test]
    fn gap_offset_disagreement_flagged() {
        let a = [interval(10.0, 20.0)];
        let b = [interval(15.0, 25.0)]; // silence-based offset = 5.0
                                        // Alignment says offset is 8.0 — well outside tolerance.
        let result = check_gap_offset_agreement(&a, &b, 8.0, 0.5).expect("should compute");
        assert!(
            !result.agrees,
            "delta {} should exceed tolerance 0.5",
            result.delta_secs
        );
        assert!((result.delta_secs - 3.0).abs() < 1e-3);
    }

    #[test]
    fn cross_check_excludes_intervals_outside_overlap() {
        use crate::domain::align::TimelineOverlap;

        // True offset −10.956: A [100,101] ↔ B [89.044, 90.044] on native clocks.
        let overlap = TimelineOverlap {
            video_a_start_secs: 10.956,
            video_a_end_secs: 900.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 889.044,
            shared_length_secs: 889.044,
        };
        let a = [
            interval(0.0, 16.0),      // pre-roll — outside overlap
            interval(100.0, 101.0),   // inside overlap
            interval(5979.0, 6180.0), // tail — outside overlap
        ];
        let b = [
            interval(0.0, 16.0),
            interval(89.044, 90.044),
            interval(5968.0, 6169.0),
        ];

        let (a_in, b_in) = filter_intervals_for_cross_check(&a, &b, &overlap);
        assert_eq!(a_in.len(), 1, "A pre-roll and tail should be excluded");
        assert_eq!(
            b_in.len(),
            2,
            "B leading silence is still inside B overlap [0, 889]"
        );
        assert!((a_in[0].start_secs - 100.0).abs() < 1e-6);

        let result = check_gap_offset_agreement_in_overlap(&a, &b, Some(&overlap), -10.956, 0.5)
            .expect("overlap-contained pair should agree");
        assert!(
            result.agrees,
            "expected agreement, got delta {}",
            result.delta_secs
        );
    }

    #[test]
    fn cross_check_returns_none_when_overlap_filters_all_intervals_on_one_side() {
        use crate::domain::align::TimelineOverlap;

        let overlap = TimelineOverlap {
            video_a_start_secs: 10.0,
            video_a_end_secs: 60.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 50.0,
            shared_length_secs: 50.0,
        };
        let a = [interval(0.0, 5.0)];
        let b = [interval(20.0, 25.0)];

        assert!(check_gap_offset_agreement_in_overlap(&a, &b, Some(&overlap), 0.0, 0.5).is_none());
    }
}
