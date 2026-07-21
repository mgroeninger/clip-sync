//! **A3 dual-fit repair** algorithm — self-contained so it is unit-testable without the production decode
//! machinery. Detect (`dualfit_target`) → fit each shoulder at its seam-local lag → extract the B bridge →
//! interior-trim to the gap length. Reuses the shared primitives (`seam_local_peak`, `donor_interior_at`,
//! `trim_at_lowest_energy_interior`) so it can't drift from the diagnostic `splice_dualfit`.
//!
//! The caller builds the inputs from the decoded window and, on `Some`, re-validates the returned fill with
//! the unchanged production seam gate before splicing (§5.2 step 4).

use clip_sync::normalized_correlation;

use crate::domain::donor::{donor_interior_at, program_quiet_at_nominal, DonorInterior};
use crate::domain::gap_fill_fit::trim_at_lowest_energy_interior;
use crate::domain::seam_local::seam_local_peak;

/// The **step is real** margin: the post seam at its own lag must beat the post seam forced to the pre lag
/// (step = 0) by at least this, else a single constant shift suffices and the reported step is a registration
/// artifact, not a genuine splice. Canonical home (was triplicated: this module's `DualFitParams` field
/// default + the harness `gap_fingerprint_corpus.rs`). The `DualFitParams.step_real_margin` field carries
/// this value into `try_dual_fit`; the cell classifier reads the const directly.
pub const DUALFIT_STEP_REAL_MARGIN: f64 = 0.15;

/// The `dualfit_target()` thresholds + geometry the repair needs (all frame counts / correlations, no config
/// object — mirrors the analyzer's constants: `min_fill_correlation`/`fill_absolute_floor` from the gate,
/// `DUALFIT_STEP_REAL_MARGIN` 0.15, `SEAM_LOCAL_SEARCH_MS` 600 ms). Program-quiet uses
/// [`crate::domain::donor::program_quiet_at_nominal`] directly, so its threshold isn't a field here.
#[derive(Debug, Clone, Copy)]
pub struct DualFitParams {
    pub channels: usize,
    pub sample_rate: u32,
    pub gap_frames: usize,
    /// Seam window (frames) = `fill_seam_search_secs · sr` (the 250 ms border the gate scores).
    /// Also the interior-trim edge guard: the re-validation gate re-scores exactly this many frames
    /// from each end of the assembled fill (`pre_gate_frames`/`post_gate_frames`, capped at this same
    /// `fill_seam_search_secs · sr`), independent of crossfade length — so the interior length-edit
    /// cut must stay clear of a `seam_window_frames`-wide border, not a shorter crossfade-wide one.
    pub seam_window_frames: usize,
    /// Per-shoulder seam search half-width (frames) = `SEAM_LOCAL_SEARCH_MS · sr` (600 ms).
    pub max_lag_frames: usize,
    pub min_fill_correlation: f64,
    pub fill_absolute_floor: f64,
    pub step_real_margin: f64,
    pub a_gap_floor_db: f64,
}

/// A successful dual-fit fill (interleaved, exactly `gap_frames`) + the seam scores at the seam-local
/// placement and the interior trim applied (`bridge − gap`, in frames).
#[derive(Debug, Clone)]
pub struct DualFitResult {
    pub fill: Vec<f32>,
    pub pre_seam_r: f64,
    pub post_seam_r: f64,
    /// Post seam scored at the PRE lag (step forced to 0) — the step-real denominator. Surfaced so the
    /// characterize spec's `SeamLocalTags.post_seam_global_r` is a single-source copy of the value dual-fit
    /// already measured (§2.5.2a; Fingerprint-unification 8a), not a re-measurement.
    pub post_seam_global_r: f64,
    pub trim_frames: i64,
    /// Seam-local lag (frames) each shoulder matched at, relative to the nominal `b_mapped_start` /
    /// `b_mapped_start + gap_frames` search center — lets a caller reconstruct the actual matched B
    /// placement per shoulder (`b_pre_seam`/`b_post_seam`) for residual measurement at the same
    /// per-shoulder mapping dual-fit itself used, instead of one shared lag.
    pub pre_lag: i64,
    pub post_lag: i64,
    /// The aligned donor bridge (`b_pre_seam .. b_post_seam`) measured during the continuity gate. Surfaced
    /// for the characterize spec's `donor_aligned` tag (single-source; §2.5.2a, 8a).
    pub aligned_donor: DonorInterior,
}

/// Attempt a dual-fit fill. `a_pre_mono`/`a_post_mono` are A's mono seam borders (the `seam_window` frames
/// **ending at** the gap start / **starting at** the gap end). `b_mono`/`b_samples` are the decoded B window
/// (mono + interleaved); `b_mapped_start` is the nominal geometry `b_mapped` gap-start frame within it.
///
/// Returns `None` unless the gap is a `dualfit_target`: seams pass the gate (`min ≥ floors`) at their
/// seam-local lags, the step is real (`post_own − post@pre ≥ margin`), the aligned donor bridges (continuous),
/// and it is not program-quiet (nominal silence `< frac`). On `Some`, the caller re-validates `fill` with the
/// production gate before splicing.
pub fn try_dual_fit(
    a_pre_mono: &[f64],
    a_post_mono: &[f64],
    b_mono: &[f64],
    b_samples: &[f32],
    b_mapped_start: usize,
    p: &DualFitParams,
) -> Option<DualFitResult> {
    let ch = p.channels.max(1);
    let w = p.seam_window_frames;
    if a_pre_mono.len() < w || a_post_mono.len() < w || p.gap_frames == 0 {
        tracing::debug!(
            a_pre_len = a_pre_mono.len(),
            a_post_len = a_post_mono.len(),
            w,
            gap_frames = p.gap_frames,
            "dual_fit: declined — shoulder shorter than seam window or empty gap"
        );
        return None;
    }
    let a_pre = &a_pre_mono[a_pre_mono.len() - w..];
    let a_post = &a_post_mono[..w];

    // Fit each shoulder at its seam-local lag around the NOMINAL b_mapped.
    let Some(pre_start) = b_mapped_start.checked_sub(w) else {
        tracing::debug!(b_mapped_start, w, "dual_fit: declined — pre seam window runs before B start");
        return None;
    };
    let Some((pre_r, pre_lag, pre_z)) = seam_local_peak(a_pre, b_mono, pre_start, p.max_lag_frames) else {
        tracing::debug!(
            pre_start,
            max_lag_frames = p.max_lag_frames,
            b_mono_len = b_mono.len(),
            "dual_fit: declined — pre seam-local search out of range"
        );
        return None;
    };
    let b_post_nominal = b_mapped_start + p.gap_frames;
    let Some((post_r, post_lag, post_z)) = seam_local_peak(a_post, b_mono, b_post_nominal, p.max_lag_frames) else {
        tracing::debug!(
            b_post_nominal,
            max_lag_frames = p.max_lag_frames,
            b_mono_len = b_mono.len(),
            "dual_fit: declined — post seam-local search out of range"
        );
        return None;
    };
    tracing::debug!(
        pre_r,
        pre_lag,
        ?pre_z,
        post_r,
        post_lag,
        ?post_z,
        "dual_fit: seam-local peaks"
    );

    // gate_pass.
    let smin = pre_r.min(post_r);
    if smin < p.min_fill_correlation || smin < p.fill_absolute_floor {
        tracing::debug!(
            smin,
            pre_r,
            post_r,
            min_fill_correlation = p.min_fill_correlation,
            fill_absolute_floor = p.fill_absolute_floor,
            "dual_fit: declined — gate_pass (seam-local correlation below floor)"
        );
        return None;
    }

    // Seam-local shoulder placements → bridge span.
    let b_pre_seam = (b_mapped_start as i64 + pre_lag).max(0) as usize;
    let b_post_seam = (b_post_nominal as i64 + post_lag).max(0) as usize;
    if b_post_seam <= b_pre_seam {
        tracing::debug!(b_pre_seam, b_post_seam, "dual_fit: declined — seam-local shoulders cross/collapse");
        return None;
    }

    // step-real: post seam at the PRE lag (step forced 0) must be materially worse than at its own lag.
    let b_post_global = b_pre_seam + p.gap_frames;
    let post_global = if b_post_global + w <= b_mono.len() { normalized_correlation(a_post, &b_mono[b_post_global..b_post_global + w]) } else { f64::NAN };
    if post_r
        .partial_cmp(&(post_global + p.step_real_margin))
        .is_none_or(|ord| ord == std::cmp::Ordering::Less)
    {
        tracing::debug!(
            post_r,
            post_global,
            step_real_margin = p.step_real_margin,
            "dual_fit: declined — step_real (rigid single-lag map already explains the post seam)"
        );
        return None;
    }

    // Donor: aligned bridge continuous (something to fill) ∧ nominal not program-quiet (content, not silence).
    let Some(aligned) = donor_interior_at(b_mono, b_pre_seam, b_post_seam, p.a_gap_floor_db, p.sample_rate) else {
        tracing::debug!(
            b_pre_seam,
            b_post_seam,
            "dual_fit: declined — donor_interior_at out of range"
        );
        return None;
    };
    if !aligned.continuous {
        tracing::debug!(
            silence_fraction = aligned.silence_fraction,
            longest_silence_ms = aligned.longest_silence_ms,
            "dual_fit: declined — aligned donor bridge is not continuous"
        );
        return None;
    }
    // Readability note (archive/TEMP-production-repair-perf-plan.md §2.5 lever 1c #3): this content-existence
    // gate — "is the nominal donor span even non-silent, i.e. is there anything to fill?" — depends only on
    // `b_mono` + nominal geometry, none of the seam peaks above, so it could be hoisted ahead of the two
    // `seam_local_peak` searches (cheap gate first). Left here deliberately: dual-fit is a per-gap rescue path
    // (not a hot loop), so the reorder is a readability nicety, not a measured win — do it only when next
    // touching this function, and keep the accept/reject decision identical.
    if program_quiet_at_nominal(b_mono, b_mapped_start, p.gap_frames, p.a_gap_floor_db, p.sample_rate) {
        tracing::debug!("dual_fit: declined — nominal donor span is program-quiet");
        return None;
    }

    // Assemble: B bridge (interleaved) between the seam-local shoulders → interior-trim to the gap length.
    let b_total = b_samples.len() / ch;
    if b_post_seam > b_total {
        tracing::debug!(b_post_seam, b_total, "dual_fit: declined — post seam runs past decoded B window");
        return None;
    }
    let bridge = &b_samples[b_pre_seam * ch..b_post_seam * ch];
    let fill = trim_at_lowest_energy_interior(bridge, ch, p.gap_frames, p.seam_window_frames);
    let trim_frames = (b_post_seam - b_pre_seam) as i64 - p.gap_frames as i64;

    Some(DualFitResult {
        fill,
        pre_seam_r: pre_r,
        post_seam_r: post_r,
        post_seam_global_r: post_global,
        trim_frames,
        pre_lag,
        post_lag,
        aligned_donor: aligned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_to_interleaved(m: &[f64], ch: usize) -> Vec<f32> {
        m.iter().flat_map(|&x| std::iter::repeat_n(x as f32, ch)).collect()
    }

    #[test]
    fn recovers_a_stepped_silence_splice() {
        // Build a same-master gap: A has a silent hole; B carries the content, its post shoulder offset from
        // the pre by a STEP (so a single rigid shift can't satisfy both seams — the dual-fit case).
        let sr = 48_000u32;
        let ch = 1;
        let w = 1200usize; // 25 ms seam window
        let gap = 4000usize;
        let step = 200i64; // post shoulder is 200 frames further along in B than a rigid map

        // Broadband source so shifts genuinely decorrelate.
        let mut seed = 0xDEAD_BEEF_1234_5678u64;
        let mut rng = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        // Long B content the fill draws from.
        let bn = 40_000usize;
        let b_mono: Vec<f64> = (0..bn).map(|_| rng()).collect();
        let b_samples = mono_to_interleaved(&b_mono, ch);

        // Nominal b_mapped gap-start; pre seam aligns at lag 0, post seam is +step.
        let b_mapped_start = 10_000usize;
        // A pre border = B just before the gap start (lag 0); A post border = B just after (b_mapped + gap + step).
        let a_pre_mono: Vec<f64> = b_mono[b_mapped_start - w..b_mapped_start].to_vec();
        let post_src = b_mapped_start + gap + step as usize;
        let a_post_mono: Vec<f64> = b_mono[post_src..post_src + w].to_vec();

        let p = DualFitParams {
            channels: ch,
            sample_rate: sr,
            gap_frames: gap,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sr as f64) as usize,
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
            step_real_margin: 0.15,
            a_gap_floor_db: -60.0,
        };

        let r = try_dual_fit(&a_pre_mono, &a_post_mono, &b_mono, &b_samples, b_mapped_start, &p)
            .expect("dual-fit target");
        assert!(r.pre_seam_r > 0.9 && r.post_seam_r > 0.9, "both seams recover: {r:?}");
        assert_eq!(r.fill.len(), gap * ch, "fill is exactly the gap length");
        assert_eq!(r.trim_frames, step, "trim = the step");
    }

    /// Regression for the 2026-07-05 production false-skip (the "previously fixed gaps now
    /// rejected" bug). `try_dual_fit`'s returned `pre_seam_r`/`post_seam_r` ARE the assembled
    /// seams' scores at the correct placement — A's near-gap border vs the B window just OUTSIDE
    /// the bridge that each shoulder matched (`b_mono[b_pre_seam-w..b_pre_seam]` for pre,
    /// `[b_post_seam..b_post_seam+w]` for post). The interior trim only edits the bridge interior,
    /// so those scores survive assembly and are what `skip_or_dual_fit` must classify.
    ///
    /// This pins the fix and documents the trap the earlier "revalidation gate" fixes fell into:
    /// routing the re-validation through `fill_splice_seam_correlations_interleaved` compares A's
    /// border against the fill's OWN head/tail (`fill[..w]`/`fill[len-w..]`) — the B window on the
    /// INSIDE of the shoulder, immediately adjacent to (not overlapping) the window the seam
    /// actually matched. For broadband audio those adjacent windows correlate at ~0, so a perfect
    /// fill scored a false ~0 and was skipped regardless of which A-border source or crossfade
    /// branch was used. The test asserts BOTH: the returned seam scores are strong, and the
    /// border-scorer path collapses on A's real near-gap audio — so it must not be used.
    #[test]
    fn dual_fit_result_passes_the_production_revalidation_gate() {
        use crate::domain::policies::{
            fill_splice_seam_correlations_interleaved, BorderSeamTemplates, SpliceSeamContext,
        };

        let sr = 48_000u32;
        let ch = 1;
        let w = 1200usize;
        let gap = 4000usize;
        let step = 200i64;

        // Broadband source, as in `recovers_a_stepped_silence_splice`, so seam-local lags are
        // genuinely distinguishable (a smooth/periodic signal would let `try_dual_fit`'s own
        // step-real gate reject the fixture, since every nearby lag would score about as well).
        let mut seed = 0xDEAD_BEEF_1234_5678u64;
        let mut rng = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        let bn = 40_000usize;
        let b_mono: Vec<f64> = (0..bn).map(|_| rng()).collect();
        let b_samples = mono_to_interleaved(&b_mono, ch);

        // A's real near-gap borders: the same-master content immediately before the gap start
        // (lag 0 → b_pre_seam = b_mapped_start) and immediately after the gap end (offset by the
        // step → b_post_seam = b_mapped_start + gap + step). These are the actual windows
        // `seam_local_peak` matches — NOT the fill's own head/tail.
        let b_mapped_start = 10_000usize;
        let a_pre_mono: Vec<f64> = b_mono[b_mapped_start - w..b_mapped_start].to_vec();
        let post_src = b_mapped_start + gap + step as usize;
        let a_post_mono: Vec<f64> = b_mono[post_src..post_src + w].to_vec();

        let p = DualFitParams {
            channels: ch,
            sample_rate: sr,
            gap_frames: gap,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sr as f64) as usize,
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
            step_real_margin: 0.15,
            a_gap_floor_db: -60.0,
        };
        let r = try_dual_fit(&a_pre_mono, &a_post_mono, &b_mono, &b_samples, b_mapped_start, &p)
            .expect("dual-fit target");

        // The fix: production classifies confidence from these — the correct assembled-seam scores.
        assert!(
            r.pre_seam_r > 0.9 && r.post_seam_r > 0.9,
            "returned seam scores (what production now re-validates on) recover: {r:?}"
        );

        // The trap: the border-splice scorer, given A's REAL near-gap borders, collapses because it
        // correlates them against the fill's own head/tail — the adjacent-but-non-overlapping B
        // window. This reproduces the production symptom (pre≈-0.02, post≈0.02) and is exactly why
        // `skip_or_dual_fit` must NOT re-validate through this path.
        let a_start_frame = 500_000usize;
        let a_end_frame = a_start_frame + gap;
        let a_samples = vec![0.0f32; a_end_frame + w + 10];
        let borders = BorderSeamTemplates {
            a_pre: &a_pre_mono,
            a_post: &a_post_mono,
            a_pre_ch: &[],
            a_post_ch: &[],
            pre_window: w,
            post_window: w,
        };
        let border_ctx = SpliceSeamContext {
            seam_cf: 480,
            gap_start_frame: a_start_frame,
            gap_end_frame: a_end_frame,
            a_samples: &a_samples,
            channels: ch,
            single_lag_alignment: false,
        };
        let (pre_bug, post_bug) =
            fill_splice_seam_correlations_interleaved(&r.fill, ch, &borders, border_ctx);
        assert!(
            pre_bug.min(post_bug) < 0.5,
            "sanity check: the border-splice scorer must collapse on A's real near-gap borders \
             (pre={pre_bug} post={post_bug}) — that false ~0 is the production false-skip we route \
             around by classifying r.pre_seam_r/r.post_seam_r instead"
        );
    }

    #[test]
    fn declines_donor_broken_bridge() {
        // Real-corpus class (14/62 gaps in the re-anchor-dual-fit-on-nominal golden set, e.g. pair
        // 1·g4/g7/g9…/g21 and pair 6·g1/g3/g6…g11): both shoulders re-fit at their seam-local lag with
        // high correlation (~0.9-1.0) and the step is genuinely real (a rigid single-lag map decorrelates
        // the post seam), yet the ALIGNED bridge between the seam-local placements has its own internal
        // silent run ≥ DONOR_CONTINUITY_MS — B doesn't actually carry content across the whole span, so
        // there's nothing coherent to splice in. Distinct from `declines_program_quiet_donor` (fully
        // silent nominal donor): here the seams and most of the bridge are loud, only the middle is dead.
        let sr = 48_000u32;
        let ch = 1;
        let w = 1200usize; // 25 ms seam window
        let gap = 4000usize;
        let step = 24_000i64; // 500 ms step -> a long aligned bridge with room for an internal hole

        let mut seed = 0x0BAD_C0DE_1234_5678u64;
        let mut rng = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        let bn = 80_000usize;
        let mut b_mono: Vec<f64> = (0..bn).map(|_| rng()).collect();

        let b_mapped_start = 20_000usize;
        // Punch a 200 ms hole well inside the aligned bridge (b_mapped_start .. b_mapped_start+gap+step),
        // clear of both seam-local search windows so pre/post correlation is untouched.
        let hole_start = b_mapped_start + 10_000;
        let hole_len = (0.2 * sr as f64) as usize; // 200 ms > DONOR_CONTINUITY_MS (150 ms)
        for v in b_mono[hole_start..hole_start + hole_len].iter_mut() {
            *v = 0.0;
        }
        let b_samples = mono_to_interleaved(&b_mono, ch);

        let a_pre_mono: Vec<f64> = b_mono[b_mapped_start - w..b_mapped_start].to_vec();
        let post_src = b_mapped_start + gap + step as usize;
        let a_post_mono: Vec<f64> = b_mono[post_src..post_src + w].to_vec();

        let p = DualFitParams {
            channels: ch,
            sample_rate: sr,
            gap_frames: gap,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sr as f64) as usize,
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
            step_real_margin: 0.15,
            a_gap_floor_db: -60.0,
        };

        assert!(
            try_dual_fit(&a_pre_mono, &a_post_mono, &b_mono, &b_samples, b_mapped_start, &p).is_none(),
            "internally-broken donor bridge is declined despite high seam correlation and a real step"
        );
    }

    #[test]
    fn declines_program_quiet_donor() {
        // Same geometry but B is SILENT in the gap interior (program-quiet) → nothing to fill → decline.
        let sr = 48_000u32;
        let ch = 1;
        let w = 1200usize;
        let gap = 4000usize;
        let mut seed = 1u64;
        let mut rng = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        let bn = 40_000usize;
        let mut b_mono: Vec<f64> = (0..bn).map(|_| rng()).collect();
        let b_mapped_start = 10_000usize;
        let a_pre_mono: Vec<f64> = b_mono[b_mapped_start - w..b_mapped_start].to_vec();
        let a_post_mono: Vec<f64> = b_mono[b_mapped_start + gap..b_mapped_start + gap + w].to_vec();
        // Silence B's interior (the whole nominal gap span).
        for v in b_mono[b_mapped_start..b_mapped_start + gap].iter_mut() {
            *v = 0.0;
        }
        let b_samples = mono_to_interleaved(&b_mono, ch);
        let p = DualFitParams {
            channels: ch,
            sample_rate: sr,
            gap_frames: gap,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sr as f64) as usize,
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
            step_real_margin: 0.15,
            a_gap_floor_db: -60.0,
        };
        assert!(
            try_dual_fit(&a_pre_mono, &a_post_mono, &b_mono, &b_samples, b_mapped_start, &p).is_none(),
            "program-quiet donor is declined"
        );
    }
}
