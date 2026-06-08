use crate::domain::gap::GapOffsetAgreement;

/// A silence interval on a single file's native timeline.
pub struct SilenceInterval {
    pub start_secs: f64,
    pub end_secs: f64,
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

    let mut candidates: Vec<f64> =
        Vec::with_capacity(2 * a_intervals.len() * b_intervals.len());
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
        SilenceInterval { start_secs: start, end_secs: end }
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
        // A and B each have silence but they never overlap regardless of shift.
        // A: [0, 1]. B: [1000, 1001]. No candidate aligns them with nonzero overlap
        // unless the shift is exactly 999 or 1000, which would.
        // Use disjoint intervals where all candidate shifts produce zero overlap:
        // A: [0, 1]. B: [1000, 1001].
        // Candidate shifts: 1000-0=1000, 1001-1=1000. Only one candidate: delta=1000.
        // Overlap at delta=1000: bi mapped to A-clock = [1000-1000, 1001-1000] = [0, 1].
        // That overlaps A [0,1] perfectly! So this test case would find an offset.
        // Let me pick truly disjoint: A has NO silence, B has NO silence.
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
        assert!(result.agrees, "delta {} should be within tolerance 0.5", result.delta_secs);
        assert!((result.delta_secs - 0.05).abs() < 1e-3);
    }

    #[test]
    fn gap_offset_disagreement_flagged() {
        let a = [interval(10.0, 20.0)];
        let b = [interval(15.0, 25.0)]; // silence-based offset = 5.0
        // Alignment says offset is 8.0 — well outside tolerance.
        let result = check_gap_offset_agreement(&a, &b, 8.0, 0.5).expect("should compute");
        assert!(!result.agrees, "delta {} should exceed tolerance 0.5", result.delta_secs);
        assert!((result.delta_secs - 3.0).abs() < 1e-3);
    }
}
