//! Sequence pass for the injected-time operator warning (A4).
//!
//! Detects a **bounded** extra/missing-program island from a run of consecutive planned gaps
//! whose ungated pre/post shoulder lags disagree. Construction of those lags is
//! [`crate::domain::lag_decision::shoulder_lags_at_b_mapped`]; this module only walks the
//! sequence. See `docs/dev/TEMP-injected-time-detector-plan.md`.

use serde::Serialize;

/// `|post − pre|` floor (ms) for a gap to count as loud. Conservative: prefer a miss over a
/// false warning. Pinned from the 2026-09-08 corpus read.
pub const INJECTED_TIME_DELTA_MS: f64 = 150.0;
/// Minimum consecutive loud gaps that constitute a run. Pair 31's positive is exactly two.
pub const INJECTED_TIME_MIN_RUN: usize = 2;

/// One consecutive-T stretch of planned gaps.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InjectedTimeRun {
    /// 0-based [`crate::domain::gap_fill::FillRegion::gap_index`].
    pub first_gap_index: usize,
    pub last_gap_index: usize,
    /// First gap's report `video_a_start_secs` (not refined).
    pub a_start_secs: f64,
    /// Last gap's report `video_a_end_secs` (not refined).
    pub a_end_secs: f64,
}

/// File-level injected-time finding. Absent from JSON when the sequence pass does not flag.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InjectedTimeReport {
    pub runs: Vec<InjectedTimeRun>,
}

/// Walk already-sorted measured gaps (`gap_index`, `pre_frac_lag_ms`, `post_frac_lag_ms`).
///
/// Consecutive means sorted `gap_index` among **measured** gaps differs by exactly 1. A missing
/// shoulder (the gap is not in `measured`), an unplanned index, or a dip at or below `t_ms`
/// breaks a run. Isolated spikes do not warn. `a_start_secs` / `a_end_secs` on the returned
/// runs are 0; the production pre-pass stamps report times.
pub fn find_injected_time_runs(
    measured: &[(usize, f64, f64)],
    t_ms: f64,
    n: usize,
) -> Vec<InjectedTimeRun> {
    if n == 0 || measured.is_empty() {
        return Vec::new();
    }

    let mut runs = Vec::new();
    let mut run_first_i: Option<usize> = None;
    let mut run_len = 0usize;

    let flush = |runs: &mut Vec<InjectedTimeRun>, first_i: usize, len: usize| {
        if len >= n {
            let first = measured[first_i].0;
            let last = measured[first_i + len - 1].0;
            runs.push(InjectedTimeRun {
                first_gap_index: first,
                last_gap_index: last,
                a_start_secs: 0.0,
                a_end_secs: 0.0,
            });
        }
    };

    for i in 0..measured.len() {
        let (idx, pre, post) = measured[i];
        let loud = (post - pre).abs() > t_ms;
        let consecutive = run_first_i.is_some_and(|start| {
            let prev = measured[start + run_len - 1].0;
            idx == prev + 1
        });

        if loud && (run_first_i.is_none() || consecutive) {
            if run_first_i.is_none() {
                run_first_i = Some(i);
                run_len = 1;
            } else {
                run_len += 1;
            }
            continue;
        }

        if let Some(start) = run_first_i {
            flush(&mut runs, start, run_len);
        }
        if loud {
            run_first_i = Some(i);
            run_len = 1;
        } else {
            run_first_i = None;
            run_len = 0;
        }
    }
    if let Some(start) = run_first_i {
        flush(&mut runs, start, run_len);
    }
    runs
}

/// Stamp report A-times onto sequence-pass runs (first gap's start, last gap's end).
pub fn stamp_injected_time_report_times(
    mut runs: Vec<InjectedTimeRun>,
    times: &[(usize, f64, f64)],
) -> Option<InjectedTimeReport> {
    if runs.is_empty() {
        return None;
    }
    for run in &mut runs {
        if let Some((_, start, _)) = times.iter().find(|(i, _, _)| *i == run.first_gap_index) {
            run.a_start_secs = *start;
        }
        if let Some((_, _, end)) = times.iter().find(|(i, _, _)| *i == run.last_gap_index) {
            run.a_end_secs = *end;
        }
    }
    Some(InjectedTimeReport { runs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagnostics::format_injected_time_warning;
    use crate::domain::lag_decision::{shoulder_lags_at_b_mapped, ShoulderLagKnobs};
    use crate::domain::pcm::interleaved_to_mono;
    use crate::domain::policies::{refine_gap_frames, GAP_EDGE_REFINE_SECS};

    fn runs_at_pin(measured: &[(usize, f64, f64)]) -> Vec<(usize, usize)> {
        find_injected_time_runs(measured, INJECTED_TIME_DELTA_MS, INJECTED_TIME_MIN_RUN)
            .into_iter()
            .map(|r| (r.first_gap_index, r.last_gap_index))
            .collect()
    }

    /// Pair 31 g7–g8: two-gap run at T=150. Neighbors small.
    #[test]
    fn pair_31_two_gap_run_warns() {
        let measured = [
            (6, 0.0, 36.0),
            (7, 0.0, 1122.2),
            (8, 0.0, 1688.4),
            (9, -1150.0, -1148.0),
        ];
        assert_eq!(runs_at_pin(&measured), vec![(7, 8)]);
    }

    /// Pair 50: g14 dip splits two runs. Do not glue across the dip.
    #[test]
    fn pair_50_dip_is_two_runs_not_one() {
        let measured = [
            (8, 0.0, 19.9),
            (9, 0.0, 512.0),
            (10, 0.0, 943.0),
            (11, 0.0, 1040.0),
            (12, 0.0, 433.0),
            (13, 0.0, 932.0),
            (14, 0.0, 38.5),
            (15, 0.0, 985.0),
            (16, 0.0, 274.0),
            (17, 0.0, 799.0),
            (18, 0.0, 160.0),
        ];
        assert_eq!(runs_at_pin(&measured), vec![(9, 13), (15, 18)]);
    }

    /// 19/g2 and 25/g2 isolates stay under N=2.
    #[test]
    fn corpus_isolates_do_not_warn() {
        let measured = [(1, 0.0, 5.0), (2, 0.0, 63.9), (3, 0.0, 4.0)];
        assert!(runs_at_pin(&measured).is_empty());
        let measured = [(1, 0.0, 2.0), (2, 0.0, 62.6), (3, 0.0, 1.0)];
        assert!(runs_at_pin(&measured).is_empty());
    }

    /// A skipped / unmeasured index in a loud stretch is a hole, not glue.
    #[test]
    fn skipped_index_splits_a_loud_stretch() {
        let measured = [
            (9, 0.0, 512.0),
            (10, 0.0, 943.0),
            // 11 missing
            (12, 0.0, 433.0),
            (13, 0.0, 932.0),
        ];
        assert_eq!(runs_at_pin(&measured), vec![(9, 10), (12, 13)]);
    }

    /// Case 3 sequence-pass floor: p98.7 ≈ 42 ms everywhere plus the 63 ms isolates.
    #[test]
    fn case_3_noise_floor_vectors_are_silent() {
        let floor: Vec<(usize, f64, f64)> = (0..20).map(|i| (i, 0.0, 42.4)).collect();
        assert!(runs_at_pin(&floor).is_empty());
        let mut with_isolates = floor;
        with_isolates[2] = (2, 0.0, 63.9);
        with_isolates[5] = (5, 0.0, 62.6);
        assert!(runs_at_pin(&with_isolates).is_empty());
    }

    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    fn unique_program(n: usize, seed: u64) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let env = 0.25
                    + 0.75
                        * ((i as f64 / n.max(1) as f64) * std::f64::consts::TAU * 5.0)
                            .sin()
                            .abs();
                (noise(seed, i) * env) as f32 * 0.6
            })
            .collect()
    }

    fn measure_gaps(
        a: &[f32],
        b: &[f32],
        gaps: &[(f64, f64)],
        sample_rate: u32,
    ) -> Vec<(usize, f64, f64)> {
        let knobs = ShoulderLagKnobs::production(sample_rate, 0.01, 0.0, 2000, 2.0);
        let b_mono = interleaved_to_mono(b, 1);
        let rate = f64::from(sample_rate);
        let max_refine = (GAP_EDGE_REFINE_SECS * rate).round() as usize;
        let mut out = Vec::new();
        for (i, &(start_secs, end_secs)) in gaps.iter().enumerate() {
            let reported_start = (start_secs * rate) as usize;
            let reported_end = (end_secs * rate) as usize;
            let refined =
                refine_gap_frames(a, 1, reported_start, reported_end, 0.01, 0.0, max_refine);
            let start_frame = crate::domain::lag_decision::b_mapped_frame_in_haystack(
                refined.start_frame,
                rate,
                0.0,
                0.0,
            );
            let (pre, post) =
                shoulder_lags_at_b_mapped(a, 1, &refined, &b_mono, start_frame, &knobs);
            let (Some(pre), Some(post)) = (pre, post) else {
                continue;
            };
            out.push((i, pre.frac_lag_ms, post.frac_lag_ms));
        }
        out
    }

    /// Case 1: ≥5 gaps, ~3 s extra B in a bounded middle span (outside ±600 ms and ±2000 ms).
    #[test]
    fn case_1_injected_region_returns_one_run() {
        const SR: u32 = 48_000;
        let total = (12.0 * f64::from(SR)) as usize;
        let extra = (3.0 * f64::from(SR)) as usize;
        let insert_at = (4.0 * f64::from(SR)) as usize;
        let realign_at = (9.0 * f64::from(SR)) as usize;

        let program = unique_program(total, 11);
        // Bounded extra on B: A[t<4] and A[t≥9] stay registered at offset 0; the middle is 3 s
        // off (outside ±600 ms and ±2000 ms). Compensating drop keeps durations equal.
        let mut b = Vec::with_capacity(total);
        b.extend_from_slice(&program[..insert_at]);
        b.extend(unique_program(extra, 99));
        b.extend_from_slice(&program[insert_at..realign_at - extra]);
        b.extend_from_slice(&program[realign_at..]);
        assert_eq!(b.len(), program.len());
        let mut a = program;

        let gaps = [
            (1.5, 1.65),
            (3.0, 3.15),
            (5.5, 5.65),
            (7.0, 7.15),
            (8.5, 8.65),
            (10.5, 10.65),
        ];
        // Punch silence on A at each gap so refine has a hole (B keeps program — donor energy).
        for &(s, e) in &gaps {
            let lo = (s * f64::from(SR)) as usize;
            let hi = (e * f64::from(SR)) as usize;
            for slot in a.iter_mut().take(hi).skip(lo) {
                *slot = 0.0;
            }
        }

        let measured = measure_gaps(&a, &b, &gaps, SR);
        assert!(
            measured.len() >= 5,
            "need both shoulders on most gaps, got {measured:?}"
        );
        let island: Vec<_> = measured
            .iter()
            .filter(|(i, pre, post)| {
                *i >= 2 && *i <= 4 && (*post - *pre).abs() > INJECTED_TIME_DELTA_MS
            })
            .copied()
            .collect();
        assert!(
            island.len() >= 2,
            "island deltas should stay elevated, measured={measured:?}"
        );
        let runs = runs_at_pin(&measured);
        assert_eq!(
            runs.len(),
            1,
            "one bounded island, got {runs:?} from {measured:?}"
        );
        assert!(
            runs[0].0 >= 2 && runs[0].1 <= 4,
            "run should sit in the island gaps 2–4, got {:?}",
            runs[0]
        );
    }

    /// Case 2: one gap's B window replaced; neighbors identity.
    #[test]
    fn case_2_isolated_seam_does_not_warn() {
        const SR: u32 = 48_000;
        let total = (8.0 * f64::from(SR)) as usize;
        let mut a = unique_program(total, 3);
        let gaps = [(1.5, 1.65), (3.5, 3.65), (5.5, 5.65)];
        for &(s, e) in &gaps {
            let lo = (s * f64::from(SR)) as usize;
            let hi = (e * f64::from(SR)) as usize;
            for slot in a.iter_mut().take(hi).skip(lo) {
                *slot = 0.0;
            }
        }
        let mut b = a.clone();
        // Unrelated 3 s of B around the middle gap — well outside both lag windows.
        let lo = (2.5 * f64::from(SR)) as usize;
        let hi = (5.5 * f64::from(SR)) as usize;
        let junk = unique_program(hi - lo, 77);
        b[lo..hi].copy_from_slice(&junk);

        let measured = measure_gaps(&a, &b, &gaps, SR);
        assert!(
            runs_at_pin(&measured).is_empty(),
            "isolated defect must not warn: {measured:?}"
        );
    }

    /// Case 3 identity PCM: deltas sit at ~0, not a synthetic "corpus noise" sine.
    #[test]
    fn case_3_identity_pcm_is_near_zero() {
        const SR: u32 = 48_000;
        let total = (8.0 * f64::from(SR)) as usize;
        let mut a = unique_program(total, 5);
        let gaps = [(2.5, 2.65), (4.0, 4.15), (5.5, 5.65)];
        for &(s, e) in &gaps {
            let lo = (s * f64::from(SR)) as usize;
            let hi = (e * f64::from(SR)) as usize;
            for slot in a.iter_mut().take(hi).skip(lo) {
                *slot = 0.0;
            }
        }
        let b = a.clone();
        let measured = measure_gaps(&a, &b, &gaps, SR);
        assert_eq!(measured.len(), 3, "{measured:?}");
        for (i, pre, post) in &measured {
            let d = (post - pre).abs();
            assert!(
                d < 10.0,
                "identity gap {i} delta {d} ms (pre={pre} post={post})"
            );
        }
        assert!(runs_at_pin(&measured).is_empty());
    }

    #[test]
    fn warning_copy_names_pair_31_span() {
        let report = InjectedTimeReport {
            runs: vec![InjectedTimeRun {
                first_gap_index: 7,
                last_gap_index: 8,
                a_start_secs: 1234.5,
                a_end_secs: 1238.0,
            }],
        };
        let line = format_injected_time_warning(&report, false);
        assert_eq!(
            line,
            "Warning: this pair looks like it has injected time (extra or missing program in a bounded span) around gap #8–#9 — filling those gaps can fail or sound wrong; the run continues (Ctrl-C to stop before the long search)"
        );
        let verbose = format_injected_time_warning(&report, true);
        assert!(verbose.contains("1234.5s–1238.0s"), "{verbose}");
        assert!(!line.contains("1234.5"), "mid-run copy has no A-times");
    }

    #[test]
    fn warning_copy_joins_two_runs_with_and_and_does_not_glue() {
        let report = InjectedTimeReport {
            runs: vec![
                InjectedTimeRun {
                    first_gap_index: 9,
                    last_gap_index: 13,
                    a_start_secs: 10.0,
                    a_end_secs: 14.0,
                },
                InjectedTimeRun {
                    first_gap_index: 15,
                    last_gap_index: 18,
                    a_start_secs: 16.0,
                    a_end_secs: 19.0,
                },
            ],
        };
        let line = format_injected_time_warning(&report, false);
        assert!(
            line.contains("around gap #10–#14 and gap #16–#19"),
            "{line}"
        );
        assert!(!line.contains("#10–#19"), "{line}");
    }

    #[test]
    fn patch_summary_omits_injected_time_key_when_none() {
        let summary = crate::domain::PatchSummary::from_outcomes(vec![]);
        let value = serde_json::to_value(&summary).expect("json");
        assert!(
            value.get("injected_time").is_none(),
            "absent when the pass does not flag: {value}"
        );
    }
}
