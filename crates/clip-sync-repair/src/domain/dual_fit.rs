//! **A3 dual-fit repair** algorithm — self-contained so it is unit-testable without the production decode
//! machinery. Detect (`dualfit_target`) → fit each shoulder at its seam-local lag → extract the B bridge →
//! interior-edit to the gap length. Reuses the shared primitives (`seam_local_peak`, `donor_interior_at`,
//! `jump_edit_to_gap`) so it can't drift from the diagnostic `splice_dualfit`.
//!
//! The caller builds the inputs from the decoded window and, on `Some`, re-validates the returned fill with
//! the unchanged production seam gate before splicing (§5.2 step 4).

use clip_sync::normalized_correlation;

use crate::domain::donor::{donor_interior_at, program_quiet_at_nominal, DonorInterior};
use crate::domain::gap_fill_fit::{jump_edit_to_gap, JumpEdit};
use crate::domain::seam_locks::{
    nominal_first_seam_locks, EnvelopeSearch, GrownPostLock, SeamGateFloors, SeamLockTemplates,
    SeamLocks,
};

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
    /// Crossfade (frames) of the one jump that reconciles a real-step bridge to the gap length
    /// ([`jump_edit_to_gap`]); production passes `crossfade_secs · sr`.
    pub edit_crossfade_frames: usize,
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
    /// `post_seam_r >= post_seam_global_r + step_real_margin`: a real length edit (interior-trim
    /// bridge extract) vs a rigid single-lag map (trim-0 extract off the pre lock,
    /// docs/dev/archive/TEMP-seam-lock-rigid-fill-plan.md §3.1). No longer gates admission — surfaced so a
    /// caller can distinguish the two `Some` shapes without re-deriving the comparison from
    /// `post_seam_r`/`post_seam_global_r` itself.
    pub step_real: bool,
    /// Seam-local lag (frames) each shoulder matched at, relative to the nominal `b_mapped_start` /
    /// `b_mapped_start + gap_frames` search center — lets a caller reconstruct the actual matched B
    /// placement per shoulder (`b_pre_seam`/`b_post_seam`) for residual measurement at the same
    /// per-shoulder mapping dual-fit itself used, instead of one shared lag.
    pub pre_lag: i64,
    pub post_lag: i64,
    /// The aligned donor bridge (`b_pre_seam .. b_post_seam`) measured during the continuity gate. Surfaced
    /// for the characterize spec's `donor_aligned` tag (single-source; §2.5.2a, 8a).
    pub aligned_donor: DonorInterior,
    /// Where the crossfaded jump edit landed; `None` on a `!step_real` extract or a bridge already
    /// gap-length.
    pub edit: Option<JumpEdit>,
}

/// Measure this gap's two seam locks with the same params the fit will use.
///
/// The floors and the rescue-triage line both come from `p`: triage is `min_fill_correlation`, so a
/// shoulder the gate would accept at its nominal lag is never a rescue candidate and can never be
/// relocated (see `seam_locks::nominal_first_seam_locks`). `envelope`/`wide_a` are `None` here — the
/// production patch path measures at nominal only, exactly as it did before the locks were hoisted;
/// turning the envelope on is a separate, behavior-changing step.
pub fn nominal_seam_locks(
    templates: &SeamLockTemplates,
    b_mono: &[f64],
    p: &DualFitParams,
) -> Option<SeamLocks> {
    nominal_first_seam_locks(
        templates,
        b_mono,
        p.max_lag_frames,
        EnvelopeSearch::off(),
        p.sample_rate,
        SeamGateFloors {
            min_fill_correlation: p.min_fill_correlation,
            fill_absolute_floor: p.fill_absolute_floor,
        },
        p.min_fill_correlation,
    )
}

/// Measure the seam locks at nominal and, if they justify it, attempt the fit.
///
/// The whole per-gap sequence in one call: locks → the two lock-level rejections the caller owns →
/// [`try_dual_fit`]. This is the *only* place those two rejections are spelled out, so the callers
/// cannot drift apart on them.
pub fn dual_fit_at_nominal(
    templates: &SeamLockTemplates,
    b_mono: &[f64],
    b_samples: &[f32],
    p: &DualFitParams,
) -> Option<DualFitResult> {
    let locks = nominal_seam_locks(templates, b_mono, p)?;
    dual_fit_from_locks(templates, &locks, b_mono, b_samples, p)
}

/// The two lock-level rejections, then the fit — from locks the caller **already measured**.
///
/// This is the body [`dual_fit_at_nominal`] delegates to, split out for the one caller that has to
/// measure the locks itself: `characterize_region` needs them *before* the gate search so it can
/// skip on `abut()`, and re-measuring them here would be both a duplicate ±600 ms search per gap
/// and a chance for the skip decision and the fit to disagree about a lag. The rejections stay
/// spelled out in exactly one place either way.
pub fn dual_fit_from_locks(
    templates: &SeamLockTemplates,
    locks: &SeamLocks,
    b_mono: &[f64],
    b_samples: &[f32],
    p: &DualFitParams,
) -> Option<DualFitResult> {
    dual_fit_from_locks_with_grown_post(templates, locks, b_mono, b_samples, p, None)
}

/// [`dual_fit_from_locks`] with an optional grown post lock
/// (`docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §0 lock 2) — the caller passes the whole
/// [`GrownPostLock`] `grow_post_lock` produced, and this function applies the post-shoulder swap
/// (`SeamLocks::post`/`b_post_seam`/`bridge_frames`/`gate_pass`, recomputed against `p`'s floors) and
/// threads its `a_post`/`window_frames` into [`try_dual_fit`]'s `post@pre` / `step_real` computation
/// itself. A caller can no longer pair a grown `post.r` with a stale (or a different gap's) `a_post`
/// template — there is exactly one `GrownPostLock` to derive both from. `None` behaves exactly like
/// [`dual_fit_from_locks`].
pub fn dual_fit_from_locks_with_grown_post(
    templates: &SeamLockTemplates,
    locks: &SeamLocks,
    b_mono: &[f64],
    b_samples: &[f32],
    p: &DualFitParams,
    grown_post: Option<&GrownPostLock>,
) -> Option<DualFitResult> {
    let (locks, a_post_override) = match grown_post {
        Some(grown) => {
            let b_post_seam_unclamped = templates.b_post_nominal() as i64 + grown.lock.lag;
            let b_post_seam = b_post_seam_unclamped.max(0) as usize;
            let mut grown_locks = *locks;
            grown_locks.post = grown.lock;
            grown_locks.b_post_seam = b_post_seam;
            grown_locks.b_post_seam_unclamped = b_post_seam_unclamped;
            grown_locks.bridge_frames = b_post_seam as i64 - grown_locks.b_pre_seam as i64;
            grown_locks.gate_pass = SeamGateFloors {
                min_fill_correlation: p.min_fill_correlation,
                fill_absolute_floor: p.fill_absolute_floor,
            }
            .passes(grown_locks.pre.r.min(grown_locks.post.r));
            (
                grown_locks,
                Some((grown.a_post.as_slice(), grown.window_frames)),
            )
        }
        None => (*locks, None),
    };
    if !locks.gate_pass {
        tracing::debug!(
            pre_r = locks.pre.r,
            post_r = locks.post.r,
            min_fill_correlation = p.min_fill_correlation,
            fill_absolute_floor = p.fill_absolute_floor,
            "dual_fit: declined — gate_pass (seam-local correlation below floor)"
        );
        return None;
    }
    if locks.bridge_frames <= 0 {
        tracing::debug!(
            b_pre_seam = locks.b_pre_seam,
            b_post_seam = locks.b_post_seam,
            "dual_fit: declined — seam-local shoulders cross/collapse"
        );
        return None;
    }
    try_dual_fit_with_grown_post(templates, &locks, b_mono, b_samples, p, a_post_override)
}

/// Attempt a dual-fit fill from **already-measured** seam locks.
///
/// `templates` carries A's two raw seam borders and the nominal B geometry; `locks` carries where
/// each of them matched. Both come from [`crate::domain::seam_locks`] — this function no longer
/// measures them, so the diagnostic `splice_dualfit` and the production repair cannot disagree about
/// a lag, a `gate_pass`, or a bridge length.
///
/// **The caller owns the two lock-level rejections** (`locks.gate_pass` and the collapse test
/// `b_post_seam > b_pre_seam`), because they are exactly the conditions that let it skip the
/// expensive work ahead of this call. They are re-asserted here in debug builds so a caller that
/// forgets cannot silently splice a crossed bridge.
///
/// Returns `None` unless the remaining `dualfit_target` conditions hold: `post_global` is
/// measurable, the aligned donor bridges (continuous), and it is not program-quiet. Whether the
/// step is real (`post_own − post@pre ≥ margin`) no longer gates admission — it selects the
/// extract: a real step gets the interior-trim search over the bridge (a length edit); a rigid
/// single-lag map gets a trim-0 fill read straight off the pre lock (docs/dev/archive/TEMP-seam-lock-rigid-fill-plan.md
/// §3.1). On `Some`, the caller re-validates `fill` with the production gate before splicing.
pub fn try_dual_fit(
    templates: &SeamLockTemplates,
    locks: &SeamLocks,
    b_mono: &[f64],
    b_samples: &[f32],
    p: &DualFitParams,
) -> Option<DualFitResult> {
    try_dual_fit_with_grown_post(templates, locks, b_mono, b_samples, p, None)
}

/// [`try_dual_fit`] with an optional grown post template
/// (`docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §0 lock 2): `grown_post` is `(a_post, window_frames)`
/// from [`crate::domain::seam_locks::grow_post_lock`], used ONLY for the `post_global` (`post@pre`,
/// the step-real denominator) computation below — the one place this function reads `a_post`/`w`
/// before extraction. Everything else (the interior-trim edge guard, the `debug_assert_eq!` against
/// `templates.seam_window_frames()`) stays keyed to the untouched 250 ms `templates`/`p`, because the
/// keep-path gate window (`fill_seam_search_secs`) does not change. `None` behaves exactly like
/// [`try_dual_fit`].
pub fn try_dual_fit_with_grown_post(
    templates: &SeamLockTemplates,
    locks: &SeamLocks,
    b_mono: &[f64],
    b_samples: &[f32],
    p: &DualFitParams,
    grown_post: Option<(&[f64], usize)>,
) -> Option<DualFitResult> {
    debug_assert!(
        locks.gate_pass,
        "try_dual_fit called on locks the fill gate rejects — the caller must check gate_pass"
    );
    debug_assert!(
        locks.b_post_seam > locks.b_pre_seam,
        "try_dual_fit called on collapsed/crossed seam locks — the caller must check the bridge"
    );
    debug_assert_eq!(
        (templates.seam_window_frames(), templates.gap_frames()),
        (p.seam_window_frames, p.gap_frames),
        "templates were cut against different geometry than the params being fit with"
    );
    let ch = p.channels.max(1);
    // Geometry comes from the templates, not `p`: they are the windows the locks were actually
    // measured with, and every length below (step-real window, bridge, trim guard) must match them.
    // A grown post (§0 lock 2) overrides `w`/`a_post` for the `post_global` computation only — the
    // interior-trim edge guard below still uses `p.seam_window_frames` (the keep-path gate window).
    let (w, a_post) = match grown_post {
        Some((a_post, window_frames)) => (window_frames, a_post),
        None => (templates.seam_window_frames(), templates.a_post()),
    };
    let b_mapped_start = templates.b_mapped_start();
    let (pre_r, pre_lag) = (locks.pre.r, locks.pre.lag);
    let (post_r, post_lag) = (locks.post.r, locks.post.lag);
    let (b_pre_seam, b_post_seam) = (locks.b_pre_seam, locks.b_post_seam);
    tracing::debug!(
        pre_r,
        pre_lag,
        post_r,
        post_lag,
        b_pre_seam,
        b_post_seam,
        "dual_fit: seam locks (measured by the caller)"
    );

    // step-real: post seam at the PRE lag (step forced 0) must be materially worse than at its own lag.
    let b_post_global = b_pre_seam + p.gap_frames;
    let post_global = if b_post_global + w <= b_mono.len() {
        normalized_correlation(a_post, &b_mono[b_post_global..b_post_global + w])
    } else {
        f64::NAN
    };
    // `step_real` is no longer an admission gate — it selects which extract runs, below. A NaN
    // `post_global` (OOB window or zero-variance silence) is still a hard decline: unmeasured is
    // not "a constant shift suffices."
    let Some(step_real_ord) = post_r.partial_cmp(&(post_global + p.step_real_margin)) else {
        tracing::debug!(
            post_r,
            post_global,
            step_real_margin = p.step_real_margin,
            "dual_fit: declined — post_global unmeasurable (NaN)"
        );
        return None;
    };
    let step_real = step_real_ord != std::cmp::Ordering::Less;
    tracing::debug!(
        post_r,
        post_global,
        step_real,
        step_real_margin = p.step_real_margin,
        "dual_fit: step_real measured (report axis, not an admission gate)"
    );

    // Donor: aligned bridge continuous (something to fill) ∧ nominal not program-quiet (content, not silence).
    let Some(aligned) = donor_interior_at(
        b_mono,
        b_pre_seam,
        b_post_seam,
        p.a_gap_floor_db,
        p.sample_rate,
    ) else {
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
    if program_quiet_at_nominal(
        b_mono,
        b_mapped_start,
        p.gap_frames,
        p.a_gap_floor_db,
        p.sample_rate,
    ) {
        tracing::debug!("dual_fit: declined — nominal donor span is program-quiet");
        return None;
    }

    // Assemble. `step_real`: B bridge (interleaved) between the seam-local shoulders, interior-trimmed
    // to the gap length — a real length edit. `!step_real`: the locks already sit ~G apart (a rigid
    // single-lag map, not a spacing change), so extract exactly the gap window from the pre lock —
    // no interior-trim search; there is no length edit to find (§3.1).
    let b_total = b_samples.len() / ch;
    let extract_end = if step_real {
        b_post_seam
    } else {
        b_pre_seam + p.gap_frames
    };
    if extract_end > b_total {
        tracing::debug!(
            extract_end,
            b_total,
            "dual_fit: declined — extract runs past decoded B window"
        );
        return None;
    }
    let (fill, edit) = if step_real {
        let bridge = &b_samples[b_pre_seam * ch..extract_end * ch];
        jump_edit_to_gap(
            bridge,
            ch,
            p.gap_frames,
            p.seam_window_frames,
            p.edit_crossfade_frames,
        )
    } else {
        (b_samples[b_pre_seam * ch..extract_end * ch].to_vec(), None)
    };
    let trim_frames = (b_post_seam - b_pre_seam) as i64 - p.gap_frames as i64;
    if let Some(e) = edit {
        tracing::debug!(
            edit_frame = e.frame,
            edit_kind = if e.jump_frames > 0 { "skip" } else { "repeat" },
            jump_frames = e.jump_frames,
            crossfade_frames = e.crossfade_frames,
            edit_span_db = 10.0 * e.span_mean_square.max(1e-20).log10(),
            "dual_fit: crossfaded interior edit"
        );
    }

    Some(DualFitResult {
        fill,
        pre_seam_r: pre_r,
        post_seam_r: post_r,
        post_seam_global_r: post_global,
        trim_frames,
        step_real,
        pre_lag,
        post_lag,
        aligned_donor: aligned,
        edit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The raw `mono(refined ± w)` seam borders these fixtures already build by hand, packaged as
    /// the templates the production path cuts with `SeamLockTemplates::build`.
    fn templates(
        a_pre_mono: &[f64],
        a_post_mono: &[f64],
        b_mapped_start: usize,
        p: &DualFitParams,
    ) -> SeamLockTemplates {
        SeamLockTemplates::from_mono(
            a_pre_mono.to_vec(),
            a_post_mono.to_vec(),
            p.gap_frames,
            b_mapped_start,
            p.seam_window_frames,
        )
        .expect("fixture borders are exactly one seam window")
    }

    fn mono_to_interleaved(m: &[f64], ch: usize) -> Vec<f32> {
        m.iter()
            .flat_map(|&x| std::iter::repeat_n(x as f32, ch))
            .collect()
    }

    /// The crossfaded interior edit (docs/dev/archive/TEMP-dualfit-interior-edit-plan.md): for either
    /// sign of the step, both seam windows of the fill are B exactly as each shoulder matched it, so
    /// the re-validation gate scores the seams the locks measured, and only the interior is edited.
    #[test]
    fn crossfade_edit_changes_only_the_interior_for_both_signs() {
        let (sr, ch, w, gap) = (48_000u32, 2usize, 1200usize, 6000usize);
        let mut seed = 0x5EED_0FED_1700_0000u64;
        let mut rng = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        let b_mono: Vec<f64> = (0..40_000).map(|_| rng()).collect();
        let b_samples = mono_to_interleaved(&b_mono, ch);
        let b_mapped_start = 10_000usize;
        for step in [300i64, -300] {
            let a_pre_mono = b_mono[b_mapped_start - w..b_mapped_start].to_vec();
            let post_src = (b_mapped_start + gap) as i64 + step;
            let a_post_mono = b_mono[post_src as usize..post_src as usize + w].to_vec();
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
                edit_crossfade_frames: 480,
            };
            let t = templates(&a_pre_mono, &a_post_mono, b_mapped_start, &p);
            let r = dual_fit_at_nominal(&t, &b_mono, &b_samples, &p).expect("dual-fit");
            assert_eq!(r.trim_frames, step);
            assert_eq!(r.fill.len(), gap * ch, "step {step}: gap length");
            let e = r.edit.expect("jump edit");
            assert_eq!(e.jump_frames, step);
            assert_eq!(e.crossfade_frames, 480);
            let bridge_end = post_src as usize;
            assert_eq!(
                r.fill[..w * ch],
                b_samples[b_mapped_start * ch..(b_mapped_start + w) * ch],
                "pre seam window"
            );
            assert_eq!(
                r.fill[(gap - w) * ch..],
                b_samples[(bridge_end - w) * ch..bridge_end * ch],
                "post seam window"
            );
        }
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
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
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
            edit_crossfade_frames: 480,
        };

        let r = dual_fit_at_nominal(
            &templates(&a_pre_mono, &a_post_mono, b_mapped_start, &p),
            &b_mono,
            &b_samples,
            &p,
        )
        .expect("dual-fit target");
        assert!(
            r.pre_seam_r > 0.9 && r.post_seam_r > 0.9,
            "both seams recover: {r:?}"
        );
        assert_eq!(r.fill.len(), gap * ch, "fill is exactly the gap length");
        assert_eq!(r.trim_frames, step, "trim = the step");

        // The locks behind that fill are interior reads, not exhausted searches: both peaks sit far from
        // the ±600 ms boundary, so `edge_pinned` is false and the skip payload would carry no such tag.
        let l = nominal_seam_locks(
            &templates(&a_pre_mono, &a_post_mono, b_mapped_start, &p),
            &b_mono,
            &p,
        )
        .expect("locks");
        assert!(l.gate_pass, "both shoulders clear the gate: {l:?}");
        assert!(
            !l.pre.edge_pinned && !l.post.edge_pinned,
            "interior peaks: {:?} / {:?}",
            l.pre,
            l.post
        );
        assert!(!l.clamped(), "seams are well inside B: {l:?}");
    }

    /// Seam-lock rigid fill (docs/dev/archive/TEMP-seam-lock-rigid-fill-plan.md §3.1, §4 Phase 1). A's post
    /// border sits at the NOMINAL gap end (no step): a single rigid map already explains both seams,
    /// so `step_real` is false. Before this Change `try_dual_fit` declined here; now it assembles a
    /// trim-0 fill read straight off the pre lock instead of running the interior-trim search a real
    /// length edit would need.
    #[test]
    fn assembles_a_rigid_trim0_fill_when_step_is_not_real() {
        let sr = 48_000u32;
        let ch = 1;
        let w = 1200usize;
        let gap = 4000usize;

        let mut seed = 0xC0FF_EE00_1234_5678u64;
        let mut rng = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        let bn = 40_000usize;
        let b_mono: Vec<f64> = (0..bn).map(|_| rng()).collect();
        let b_samples = mono_to_interleaved(&b_mono, ch);

        // No step: A's post border is B's content immediately at the nominal gap end.
        let b_mapped_start = 10_000usize;
        let a_pre_mono: Vec<f64> = b_mono[b_mapped_start - w..b_mapped_start].to_vec();
        let a_post_mono: Vec<f64> = b_mono[b_mapped_start + gap..b_mapped_start + gap + w].to_vec();

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
            edit_crossfade_frames: 480,
        };

        let r = dual_fit_at_nominal(
            &templates(&a_pre_mono, &a_post_mono, b_mapped_start, &p),
            &b_mono,
            &b_samples,
            &p,
        )
        .expect("rigid map still assembles a fill — step_real is a report axis, not a gate");

        assert!(
            r.pre_seam_r > 0.9 && r.post_seam_r > 0.9,
            "both seams recover at the nominal (rigid) lag: {r:?}"
        );
        assert!(!r.step_real, "rigid map: step_real reports false: {r:?}");
        assert!(
            r.post_seam_r < r.post_seam_global_r + p.step_real_margin,
            "step is NOT real — the post seam at its own lag does not beat post@pre by the margin: {r:?}"
        );
        assert_eq!(r.fill.len(), gap * ch, "fill is exactly the gap length");
        assert_eq!(
            r.fill,
            mono_to_interleaved(&b_mono[b_mapped_start..b_mapped_start + gap], ch),
            "trim-0: the fill is read straight off the pre lock, not an interior-trimmed search"
        );
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
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
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
            edit_crossfade_frames: 480,
        };
        let r = dual_fit_at_nominal(
            &templates(&a_pre_mono, &a_post_mono, b_mapped_start, &p),
            &b_mono,
            &b_samples,
            &p,
        )
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
            geometry: crate::domain::policies::SpliceSeamGeometry::BorderTemplate,
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
        // Real-corpus class (14/62 gaps observed on the re-anchor corpus, e.g. pair
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
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
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
            edit_crossfade_frames: 480,
        };

        assert!(
            dual_fit_at_nominal(&templates(&a_pre_mono, &a_post_mono, b_mapped_start, &p), &b_mono, &b_samples, &p).is_none(),
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
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
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
            edit_crossfade_frames: 480,
        };
        assert!(
            dual_fit_at_nominal(
                &templates(&a_pre_mono, &a_post_mono, b_mapped_start, &p),
                &b_mono,
                &b_samples,
                &p
            )
            .is_none(),
            "program-quiet donor is declined"
        );
    }
}
