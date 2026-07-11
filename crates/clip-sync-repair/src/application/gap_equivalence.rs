//! Application-side cheap gap equivalence (Phase 0): compose the four measurements (E1–E4) at the
//! **nominal map, lag 0** from decoded A/B, then apply the domain policy
//! ([`crate::domain::gap_equivalence::equivalence_verdict`]). Plan: `docs/TEMP-gap-equivalence-plan.md` §5.3.
//!
//! **Reuses existing primitives only** (plan §5.0): `border_templates_for_gap` + `fill_seam_correlations`
//! (E1), `seam_chosen_and_floor` (E2), `donor_interior_at` (E3), a mono RMS (E4). No new seam math.
//!
//! **Phase 0** runs on already-decoded buffers (inside the fingerprint path) — the bounded reader extract is a
//! v1 concern. Accordingly this operates on slices; the coordinate contract is stated on [`GapEquivalenceInput`].

use crate::domain::donor::donor_interior_at;
use crate::domain::gap_equivalence::{
    equivalence_verdict, CheapEquivalenceMetrics, EquivalenceVerdict, GapEquivalenceParams,
};
use crate::domain::policies::{
    self, border_templates_for_gap, seam_chosen_and_floor, GapBorderSpec, RefinedGapFrames,
    SeamFloorParams, SeamResidualVerdict, SeamSide,
};

/// Inputs for one gap's cheap equivalence read.
///
/// **Coordinate contract (E2):** `a_samples` is full A (interleaved) on the gap clock; `refined` is the A gap
/// span in that clock. `b_mono` is the per-gap B extract downmixed to mono, whose element `0` is B full-frame
/// `b_extract_start_frame`. The nominal B gap start (B full-frame `b_nominal_start_frame`) therefore sits at
/// `b_mapped_start = b_nominal_start_frame - b_extract_start_frame` inside `b_mono`, and the nominal A→B mapping
/// is `a_to_b_delta = b_mapped_start - refined.start_frame` (identical to the production residual's
/// `nominal_delta`). We place the residual's *chosen* at that same nominal delta, so the residual signal that
/// matters is whether the nominal **floor** cancels (`informative`) — same-source at the nominal map.
pub struct GapEquivalenceInput<'a> {
    pub a_samples: &'a [f32],
    pub channels: usize,
    pub sample_rate: u32,
    pub b_mono: &'a [f64],
    pub b_extract_start_frame: usize,
    pub b_nominal_start_frame: usize,
    pub refined: RefinedGapFrames,
    /// A gap floor (dB) for the donor silence read (reuse the fingerprint `levels.gap_floor_db`).
    pub gap_floor_db: f64,
    /// Seam correlation / border window (frames) — reuse `fill_seam_search_secs`.
    pub seam_window_frames: usize,
    /// Frames skipped immediately adjacent to the gap (reuse `border_standoff_frames`).
    pub standoff_frames: usize,
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
    /// Residual integer-lag radius (reuse `residual_max_lag_frames`).
    pub max_lag_frames: i64,
    /// Residual floor-ok threshold (reuse `residual_floor_ok_db`).
    pub residual_floor_ok_db: f64,
    pub params: GapEquivalenceParams,
}

/// Mono RMS of an interleaved A span, in dB (finite; a silent span floors near −180 dB, not −∞).
fn gap_rms_db(samples: &[f32], channels: usize, start_frame: usize, end_frame: usize) -> Option<f64> {
    let ch = channels.max(1);
    let end = end_frame.min(samples.len() / ch);
    if start_frame >= end {
        return None;
    }
    let mono = policies::interleaved_to_mono(&samples[start_frame * ch..end * ch], ch);
    if mono.is_empty() {
        return None;
    }
    let rms = (mono.iter().map(|v| v * v).sum::<f64>() / mono.len() as f64).sqrt();
    Some(20.0 * rms.max(1e-9).log10())
}

/// Compute the cheap equivalence verdict for one gap (E1–E4 + §5.4 policy). Returns `NotEvaluated` only when
/// the nominal span doesn't fit the extract (a setup failure, treated conservatively — no skip).
pub fn measure_gap_equivalence(input: &GapEquivalenceInput<'_>) -> EquivalenceVerdict {
    let refined = input.refined;
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let Some(b_mapped_start) = input.b_nominal_start_frame.checked_sub(input.b_extract_start_frame) else {
        return EquivalenceVerdict::not_evaluated();
    };
    if gap_frames == 0 || b_mapped_start + gap_frames > input.b_mono.len() {
        return EquivalenceVerdict::not_evaluated();
    }
    let sw = input.seam_window_frames;

    // E1 — nominal seam Pearson (lag 0). Borders from the same primitive production uses.
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: sw,
        border_standoff_frames: input.standoff_frames,
        silence_peak_fraction: input.silence_peak_fraction,
        absolute_rms_floor: input.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(input.a_samples, input.channels, &border_spec);
    let (nominal_pre, nominal_post) = crate::domain::gap_equivalence::nominal_seams(
        &a_pre, &a_post, input.b_mono, b_mapped_start, gap_frames, sw, sw,
    );

    // E3 — donor interior at the nominal span (must be occupied, not program-quiet).
    let donor = donor_interior_at(
        input.b_mono,
        b_mapped_start,
        b_mapped_start + gap_frames,
        input.gap_floor_db,
        input.sample_rate,
    );

    // E4 — A gap RMS (is A a real dropout?).
    let a_gap_rms_db = gap_rms_db(input.a_samples, input.channels, refined.start_frame, refined.end_frame);

    // E2 — same-source residual at the nominal throat (chosen == nominal ⇒ the signal is floor cancellation).
    let a_to_b_delta = b_mapped_start as i64 - refined.start_frame as i64;
    let floor_params = SeamFloorParams {
        a_samples: input.a_samples,
        channels: input.channels,
        b_mono: input.b_mono,
        window: sw,
        standoff_frames: input.standoff_frames,
        a_to_b_delta,
        step_frames: sw.max(1),
        max_walk_frames: input.sample_rate as usize * 3,
        absolute_silence_rms: input.absolute_silence_rms,
        max_lag_frames: input.max_lag_frames,
    };
    let (chosen_pre, floor_pre) = seam_chosen_and_floor(
        &floor_params, SeamSide::Pre, refined.start_frame, refined.end_frame, a_to_b_delta,
    );
    let (chosen_post, floor_post) = seam_chosen_and_floor(
        &floor_params, SeamSide::Post, refined.start_frame, refined.end_frame, a_to_b_delta,
    );
    let residual = Some(SeamResidualVerdict::from_parts_with_placement(
        &chosen_pre, &chosen_post, &floor_pre, &floor_post,
        input.residual_floor_ok_db, 0, input.max_lag_frames,
    ));

    let metrics = CheapEquivalenceMetrics { nominal_pre, nominal_post, residual, donor, a_gap_rms_db };
    equivalence_verdict(&metrics, &input.params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap_equivalence::EquivalenceDisposition;

    const RATE: u32 = 48_000;

    /// Deterministic broadband content in [-amp, amp].
    fn fill(buf: &mut [f32], start: usize, end: usize, seed: u64, amp: f32) {
        for (i, s) in buf.iter_mut().enumerate().take(end).skip(start) {
            let mut z = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(seed);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            let u = ((z ^ (z >> 27)) >> 40) as f64 / (1u64 << 24) as f64;
            *s = ((u * 2.0 - 1.0) as f32) * amp;
        }
    }

    fn input_for<'a>(a: &'a [f32], b_mono: &'a [f64], start: usize, end: usize) -> GapEquivalenceInput<'a> {
        GapEquivalenceInput {
            a_samples: a,
            channels: 1,
            sample_rate: RATE,
            b_mono,
            b_extract_start_frame: 0,
            b_nominal_start_frame: start, // nominal offset 0 (A/B share the clock in the test)
            refined: RefinedGapFrames { start_frame: start, end_frame: end },
            gap_floor_db: -50.0,
            seam_window_frames: 4_800, // 0.1 s
            standoff_frames: 0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            max_lag_frames: 480,
            residual_floor_ok_db: crate::domain::policies::DEFAULT_RESIDUAL_FLOOR_OK_DB,
            params: GapEquivalenceParams::default(),
        }
    }

    /// **Same master** — B is identical to A except A's gap interior is zeroed (a dropout). At nominal the
    /// borders correlate ~1, the floor cancels (same source), the donor is occupied, and A's gap is silent →
    /// **Skip**. Exercises the full E1–E4 chain + coordinate contract with the real primitives.
    #[test]
    fn same_master_end_to_end_skip() {
        let (start, end, total) = (48_000usize, 57_600usize, 96_000usize);
        let mut a = vec![0f32; total];
        fill(&mut a, 0, total, 1, 0.3);
        let b = a.clone();
        // A has a dropout in the gap; B still carries the program audio there.
        for s in &mut a[start..end] {
            *s = 0.0;
        }
        let b_mono: Vec<f64> = b.iter().map(|&x| x as f64).collect();
        let v = measure_gap_equivalence(&input_for(&a, &b_mono, start, end));
        assert_eq!(v.disposition, EquivalenceDisposition::Skip, "verdict: {v:?}");
        assert!(v.residual_informative, "floor should cancel for same-source: {v:?}");
        assert!(v.nominal_pre.unwrap() > 0.9 && v.nominal_post.unwrap() > 0.9, "seams: {v:?}");
    }

    /// **Different content** — B is unrelated noise. Seams are weak and the floor does not cancel →
    /// **AttemptPatch** (never skip a real, non-matching gap).
    #[test]
    fn decorrelated_end_to_end_attempt_patch() {
        let (start, end, total) = (48_000usize, 57_600usize, 96_000usize);
        let mut a = vec![0f32; total];
        fill(&mut a, 0, total, 1, 0.3);
        for s in &mut a[start..end] {
            *s = 0.0;
        }
        let mut b = vec![0f32; total];
        fill(&mut b, 0, total, 999, 0.3); // unrelated
        let b_mono: Vec<f64> = b.iter().map(|&x| x as f64).collect();
        let v = measure_gap_equivalence(&input_for(&a, &b_mono, start, end));
        assert_eq!(v.disposition, EquivalenceDisposition::AttemptPatch, "verdict: {v:?}");
    }

    /// Nominal span outside the extract → `NotEvaluated` (conservative setup failure, no skip).
    #[test]
    fn out_of_range_not_evaluated() {
        let a = vec![0.1f32; 96_000];
        let b_mono = vec![0.1f64; 10_000];
        let mut inp = input_for(&a, &b_mono, 48_000, 57_600);
        inp.b_nominal_start_frame = 48_000; // but b_mono only 10k long ⇒ span doesn't fit
        assert_eq!(measure_gap_equivalence(&inp).disposition, EquivalenceDisposition::NotEvaluated);
    }
}
