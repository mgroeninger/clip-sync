//! `probes` slice of the gap_fingerprint measure folder split.

use crate::application::gap_fingerprint::schema::*;

use clip_sync::normalized_correlation;

use crate::domain::dual_fit::DUALFIT_STEP_REAL_MARGIN;
#[cfg(test)]
use crate::domain::dual_fit::{dual_fit_at_nominal, DualFitParams};
use crate::domain::gap_energy::energy_bins;
use crate::domain::gap_signature::energy_envelope_is_flat;
use crate::domain::lag_envelope::EnvelopeCfg;
use crate::domain::pcm::interleaved_to_mono;
use crate::domain::policies::{border_templates_for_gap, RefinedGapFrames};
use crate::domain::seam_locks::{
    nominal_first_seam_locks, wide_a_slicer, EnvelopeSearch, SeamGateFloors, SeamLockTemplates,
};

use super::config::*;
use super::lag::*;
use super::pcm::*;

/// ± lag sweep (ms) used to gauge each dual-fit seam's peak uniqueness (the periodicity/alias guard).
pub(in crate::application::gap_fingerprint::measure) const DUALFIT_SEAM_UNIQ_LAG_MS: f64 = 30.0;
/// Half-width (ms) of the per-shoulder **seam-local** lag search in [`splice_dualfit_at`], anchored on the
/// **nominal `b_mapped`** (not the gross 1 s `lag_decision`). The seam defines its own placement, so the
/// search must cover the full registration range: a gross lag that locked onto distant content (7·g3: 1 s
/// pre lag −319 ms but the seam is at +18 ms) would otherwise clip a live seam. Set to the `lag_decision`
/// range so anything the 1 s sweep could register, the seam sweep can too. `peak_z` on the seam curve is the
/// alias guard against a wide search locking onto a far periodic peak. Calibrate at ledger A5/C6.
pub(in crate::application::gap_fingerprint::measure) const SEAM_LOCAL_SEARCH_MS: f64 = 600.0;
/// Seam-anchor relocation (`docs/dev/TEMP-seam-anchor-relocation-plan.md` §2): outward-walk +
/// three-stage funnel constants for [`anchor_relocated_lag`]. Diagnostic-only (Tier-3), nothing
/// gates on these — deliberately not `infrastructure/config.rs` (see the plan's "Tunable placement").
///
/// Near-edge bound (in [`ANCHOR_BUCKET_MS`] strides) on the outward walk from the seam. The far edge
/// of even the first candidate reaches `ANCHOR_SEARCH_RADIUS_SECS + ANCHOR_WIDE_WINDOW_SECS +
/// ANCHOR_WIDE_LAG_MS` (~15.4 s default) from the seam — see `characterize.rs`'s pad widening.
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_SEARCH_RADIUS_SECS: f64 = 5.0;
/// Stride (ms) of the outward walk in stage 1.
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_BUCKET_MS: f64 = 250.0;
/// Bin width (ms) for stage 1's cheap `energy_bins` flatness screen. A placeholder pending §5 — only
/// needs to be fine enough that a real feature isn't averaged away; need not match `WIDE_ENV_BIN_MS`
/// since stage 1 never feeds a lag estimate anywhere, only a bool.
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_SCREEN_BIN_MS: f64 = 100.0;
/// Stage 2's coarse-envelope correlation window (widened from the nominal-placement confirmer's
/// `WIDE_ENV_WINDOW_SECS`).
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_WIDE_WINDOW_SECS: f64 = 10.0;
/// Stage 2's lag reach (ms). Starting point: reuse `WIDE_ENV_MAX_LAG_MS`'s 400 ms.
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_WIDE_LAG_MS: f64 = 400.0;
/// Stage 3's confirm half-width (ms) — the existing ±30 ms `seam_local_peak` confirm, re-centered on
/// the accepted candidate's stage-2 lag instead of the nominal map lag.
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_CONFIRM_WINDOW_MS: f64 = 30.0;
/// Step-boundary guard floor (ms), evaluated *after* stage 3 against the seam's own nominal lag (§2
/// step 5) — not the coarse stage-2 lag, which is quantized to `WIDE_ENV_BIN_MS` and would dominate
/// a guard applied there. Same value as `clip-sync-repair-harness`'s `CLEAN_STEP_MS` (`report.rs`),
/// re-declared locally: `probes.rs` cannot depend on the harness crate, which depends on
/// `clip-sync-repair`, not the other way around.
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_STEP_BOUNDARY_MS: f64 = 2.0;
/// RMS of the **energy-weighted** downmix over interleaved frames `[lo, hi)`. Each channel is weighted by
/// its own energy, so a loud center isn't diluted by quiet surrounds/LFE the way a straight `1/N` mix is
/// (which buried these 5.1 seams 13–15 dB and over-flagged them "quiet" — §3.6a froze level on this mix).
/// Correlation stays on mono; only the level/SNR uses this. `0.0` for an empty/over-range span.
pub(in crate::application::gap_fingerprint::measure) fn weighted_downmix_rms(
    samples: &[f32],
    channels: usize,
    lo: usize,
    hi: usize,
) -> f64 {
    let ch = channels.max(1);
    let total_frames = samples.len() / ch;
    let hi = hi.min(total_frames);
    if hi <= lo {
        return 0.0;
    }
    let n = (hi - lo) as f64;
    let mut ms = vec![0.0f64; ch];
    for f in lo..hi {
        let base = f * ch;
        for (c, m) in ms.iter_mut().enumerate() {
            let s = samples.get(base + c).copied().unwrap_or(0.0) as f64;
            *m += s * s;
        }
    }
    let total: f64 = ms.iter().sum();
    if total <= f64::EPSILON {
        return 0.0;
    }
    let weights: Vec<f64> = ms.iter().map(|e| e / total).collect();
    let mut acc = 0.0;
    for f in lo..hi {
        let base = f * ch;
        let y: f64 = weights
            .iter()
            .enumerate()
            .map(|(c, w)| samples.get(base + c).copied().unwrap_or(0.0) as f64 * w)
            .sum();
        acc += y * y;
    }
    (acc / n).sqrt()
}
/// One side's [`SeamProbe`]: waveform Pearson at lag 0, recovery over ±`fine_max_lag`, encoding-robust
/// envelope correlation, and level/SNR. Correlation is on the mono `a_win`; the level/SNR is taken from
/// `level_rms` (the energy-weighted-downmix RMS at the raw seam, computed by the caller). `b_ctx` spans
/// `±fine_max_lag` around the seam (lag 0 ⇒ `b_ctx[fine_max_lag .. + a_win.len()]`).
pub(in crate::application::gap_fingerprint::measure) fn seam_probe_side(
    a_win: &[f64],
    b_ctx: &[f64],
    level_rms: f64,
    fine_max_lag: i64,
    sample_rate: u32,
    fine_bin: usize,
    gap_floor_db: f64,
) -> Option<SeamProbe> {
    let w = a_win.len();
    let ml = fine_max_lag.max(0) as usize;
    let rate = f64::from(sample_rate).max(1.0);
    if w < 8 || ml + w > b_ctx.len() {
        return None;
    }
    let curve = lag_correlation_curve(a_win, b_ctx, fine_max_lag);
    if curve.is_empty() {
        return None;
    }
    let waveform_r = curve
        .iter()
        .find(|(l, _)| *l == 0)
        .map(|(_, r)| *r)
        .unwrap_or(f64::NAN);
    let &(peak_lag, recovered_r) = curve
        .iter()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
    let b0 = &b_ctx[ml..ml + w];
    let envelope_r = normalized_correlation(
        &fine_rms_envelope(a_win, fine_bin),
        &fine_rms_envelope(b0, fine_bin),
    );
    let bandlimited_r = crate::domain::seam_robust::bandlimited_pearson(
        a_win,
        b0,
        sample_rate,
        crate::domain::seam_robust::BANDLIMITED_CUTOFF_HZ,
    );
    let spectrum_r = crate::domain::seam_robust::spectrum_correlation(a_win, b0);
    let rms_db = f64::from(to_db(level_rms as f32));
    Some(SeamProbe {
        waveform_r: finite_corr(waveform_r),
        recovered_r: finite_corr(recovered_r),
        recovered_lag_ms: peak_lag as f64 * 1000.0 / rate,
        bandlimited_r: finite_corr(bandlimited_r),
        spectrum_r: finite_corr(spectrum_r),
        envelope_r: finite_corr(envelope_r),
        rms_db,
        snr_db: rms_db - gap_floor_db,
    })
}
// `pub(crate)` so `contract.rs`'s drift guard can pin them (`measure::probes::SEAM_PROBE_*` under
// `cfg(test)`): the `seam_probe` contract quotes both numbers as prose (a `const` table cannot
// `format!`), and a contract that lies about its window is worse than no contract.
pub(crate) const SEAM_PROBE_FINE_LAG_MS: f64 = 25.0;
pub(crate) const SEAM_PROBE_ENV_BIN_MS: f64 = 10.0;
/// Pre/post [`SeamProbe`]s at a placement (mono). Built at **`b_mapped`** registration to diagnose a dead
/// waveform seam: recovery (mis-alignment) vs encoding-robust envelope (cross-encoding) vs level/SNR.
/// `post_shift_frames` is the measured pre-side lag (rounded, from `lag_decision`'s mono pre entry) —
/// the same sequential-registration shift `lag_pair` applies (ledger A2 sequential registration),
/// so the post probe isn't centered on the un-shifted `start_frame + gap_frames` while `lag_decision`'s
/// post search is. The post fine-lag half-width is also raised to `cfg.lag_max_lag_ms` (from the ±25 ms
/// `SEAM_PROBE_FINE_LAG_MS`) since, even after shifting, the residual search still needs to cover the
/// bridge-length mismatch (`D_B - D_A`), not just fine sub-frame jitter. `recovered_lag_ms` is reported
/// gross-relative (shifted back by `post_shift_frames`) to stay comparable with `lag_decision`.
pub(in crate::application::gap_fingerprint::measure) struct SeamProbeAtPlacementInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_samples: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) channels: usize,
    pub(in crate::application::gap_fingerprint::measure) refined: RefinedGapFrames,
    pub(in crate::application::gap_fingerprint::measure) b_mono: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) start_frame: usize,
    pub(in crate::application::gap_fingerprint::measure) post_shift_frames: i64,
    pub(in crate::application::gap_fingerprint::measure) bin_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
    pub(in crate::application::gap_fingerprint::measure) gap_floor_db: f64,
}
pub(in crate::application::gap_fingerprint::measure) fn seam_probe_at_placement(
    input: &SeamProbeAtPlacementInput<'_>,
) -> SeamProbeFingerprint {
    let SeamProbeAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        start_frame,
        post_shift_frames,
        bin_frames,
        cfg,
        sample_rate,
        gap_floor_db,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    let border_spec = probe_border_spec(&refined, bin_frames * 3, cfg);
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let window = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
    let fine_max_lag = ((SEAM_PROBE_FINE_LAG_MS / 1000.0) * rate).round() as i64;
    let fine_bin = ((SEAM_PROBE_ENV_BIN_MS / 1000.0) * rate).round().max(1.0) as usize;
    let ml = fine_max_lag.max(0) as usize;
    let post_max_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * rate).round() as i64;
    let post_ml = post_max_lag.max(0) as usize;

    let pre = (|| {
        let w = window.min(a_pre.len());
        if w < 8 || start_frame < w + ml {
            return None;
        }
        let lo = start_frame - w - ml;
        let hi = (start_frame + ml).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        // Level over the raw seam span (the w frames ending at the gap), on the energy-weighted downmix.
        let level_rms = weighted_downmix_rms(a_samples, ch, start_frame - w, start_frame);
        seam_probe_side(
            &a_pre[a_pre.len() - w..],
            &b_mono[lo..hi],
            level_rms,
            fine_max_lag,
            sample_rate,
            fine_bin,
            gap_floor_db,
        )
    })();
    let post = (|| {
        let w = window.min(a_post.len());
        let post_base =
            (start_frame as i64 + gap_frames as i64 + post_shift_frames).max(0) as usize;
        if w < 8 || post_base < post_ml {
            return None;
        }
        let lo = post_base - post_ml;
        let hi = (post_base + w + post_ml).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        // Level over the raw seam span (the w frames starting at the gap end), energy-weighted downmix.
        let level_rms = weighted_downmix_rms(a_samples, ch, post_base, post_base + w);
        seam_probe_side(
            &a_post[..w],
            &b_mono[lo..hi],
            level_rms,
            post_max_lag,
            sample_rate,
            fine_bin,
            gap_floor_db,
        )
    })();
    let post_shift_ms = post_shift_frames as f64 * 1000.0 / rate;
    let post = post.map(|mut sp| {
        sp.recovered_lag_ms += post_shift_ms;
        sp
    });
    SeamProbeFingerprint { pre, post }
}
/// Inputs for [`splice_dualfit_at`] — the A/B PCM plus the **nominal** `b_mapped` gap-start frame. The seam
/// search re-anchors on nominal (not the gross `lag_decision`), so it needs only the geometry anchor; the
/// pre shoulder butts at `b_mapped_start`, the post at `b_mapped_start + gap_frames`.
pub(in crate::application::gap_fingerprint::measure) struct SpliceDualfitInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_samples: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) channels: usize,
    pub(in crate::application::gap_fingerprint::measure) refined: RefinedGapFrames,
    pub(in crate::application::gap_fingerprint::measure) b_mono: &'a [f64],
    /// Nominal geometry `b_mapped` gap-start frame (no lag adjustment).
    pub(in crate::application::gap_fingerprint::measure) b_mapped_start: usize,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
    /// Seam-anchor relocation (`TEMP-seam-anchor-relocation-plan.md` §4 risk 5): the nearest
    /// neighboring gap's near boundary, in seconds from this gap's own seam, in the pre/post walk
    /// direction respectively. `None` when there is no neighbor on that side (falls back to
    /// `ANCHOR_SEARCH_RADIUS_SECS` alone).
    pub(in crate::application::gap_fingerprint::measure) pre_anchor_max_walk_secs: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) post_anchor_max_walk_secs: Option<f64>,
}

/// Inputs for [`anchor_relocated_lag`] — one call per seam side (post: forward walk, deeper into
/// resumed A; pre: backward walk, further before the gap — §2's one-directional-by-construction
/// note). `seam_frame` is the A-side geometry anchor the walk steps outward from (`refined.end_frame`
/// for post, `refined.start_frame` for pre). `nominal_anchor_frame` is the untouched-geometry B frame
/// that corresponds to `seam_frame` at lag 0 (`b_mapped_start + gap_frames` for post,
/// `b_mapped_start` for pre) — the walk assumes this same offset tracks outward with `d` (§0's
/// negligible-local-drift premise). `a_seam_template` is the seam's own already-built dual-fit
/// template (`a_post`/`a_pre` in [`splice_dualfit_at`]); stage 3 re-locks *this* window, not the
/// candidate's own probe window. `nominal_lag_frames` is the seam's own already-locked nominal lag
/// (`locks.post.lag` / `locks.pre.lag`) — the step-boundary guard's baseline (§2 step 5), never the
/// coarse stage-2 lag.
pub(in crate::application::gap_fingerprint::measure) struct AnchorRelocatedLagInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_samples: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) channels: usize,
    pub(in crate::application::gap_fingerprint::measure) b_mono: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
    pub(in crate::application::gap_fingerprint::measure) seam_frame: usize,
    pub(in crate::application::gap_fingerprint::measure) a_seam_template: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) nominal_anchor_frame: usize,
    pub(in crate::application::gap_fingerprint::measure) nominal_lag_frames: i64,
    pub(in crate::application::gap_fingerprint::measure) forward: bool,
    pub(in crate::application::gap_fingerprint::measure) max_walk_secs: Option<f64>,
}

/// Result of [`anchor_relocated_lag`] — either an accepted candidate's stage-2/stage-3 measurements
/// (`reason: None`), or a named reason why no anchor was accepted (§2 step 6, §3). Field meanings
/// mirror the `*_anchor_*` fields on `SpliceDualfit`/`SeamLocalTags` one-for-one.
#[derive(Debug, Clone, Copy)]
pub(in crate::application::gap_fingerprint::measure) struct AnchorRelocatedLag {
    pub(in crate::application::gap_fingerprint::measure) lag_ms: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) distance_ms: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) r: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) prom: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) screened_count: u32,
    pub(in crate::application::gap_fingerprint::measure) confirm_r: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) confirm_prom: Option<f64>,
    pub(in crate::application::gap_fingerprint::measure) reason: Option<AnchorReason>,
    /// §4 risk 5: true when the walk stopped because it hit the neighboring gap's own dropout
    /// (`max_walk_secs`) short of `ANCHOR_SEARCH_RADIUS_SECS`, rather than genuinely exhausting the
    /// stage `reason` names. Always `false` on an accepted candidate (`reason: None`) — a candidate
    /// was found before the bound was reached.
    pub(in crate::application::gap_fingerprint::measure) neighbor_bounded: bool,
}

impl AnchorRelocatedLag {
    fn no_anchor(reason: AnchorReason, screened_count: u32, neighbor_bounded: bool) -> Self {
        Self {
            lag_ms: None,
            distance_ms: None,
            r: None,
            prom: None,
            screened_count,
            confirm_r: None,
            confirm_prom: None,
            reason: Some(reason),
            neighbor_bounded,
        }
    }
}

/// Seam-anchor relocation (`docs/dev/TEMP-seam-anchor-relocation-plan.md` §2): walk outward from the
/// seam in the one direction that is ever valid for this side, and run each candidate through the
/// three-stage funnel — a cheap A-only flatness screen (stage 1), a coarse wide-envelope correlation
/// on stage-1-qualifying candidates only (stage 2), and the existing ±[`ANCHOR_CONFIRM_WINDOW_MS`]
/// seam-local confirm re-centered on the accepted candidate's stage-2 lag (stage 3), followed by a
/// post-stage-3 step-boundary guard.
///
/// **Only a step-boundary rejection resumes the walk** toward a farther candidate (§2 step 5) — a
/// `confirm_failed_to_lock` on an otherwise-clean candidate is terminal for this seam; a farther
/// candidate would only weaken the negligible-drift premise this mechanism is built on, not
/// strengthen it.
pub(in crate::application::gap_fingerprint::measure) fn anchor_relocated_lag(
    input: &AnchorRelocatedLagInput<'_>,
) -> AnchorRelocatedLag {
    let AnchorRelocatedLagInput {
        a_samples,
        channels,
        b_mono,
        sample_rate,
        cfg,
        seam_frame,
        a_seam_template,
        nominal_anchor_frame,
        nominal_lag_frames,
        forward,
        max_walk_secs,
    } = *input;
    let ch = channels.max(1);
    let rate = f64::from(sample_rate).max(1.0);
    let a_frames = a_samples.len() / ch;

    let bucket_frames = (((ANCHOR_BUCKET_MS / 1000.0) * rate).round().max(1.0)) as usize;
    let radius_frames = ((ANCHOR_SEARCH_RADIUS_SECS * rate).round().max(1.0)) as usize;
    let window_frames = ((ANCHOR_WIDE_WINDOW_SECS * rate).round().max(1.0)) as usize;
    let wide_lag_frames = (((ANCHOR_WIDE_LAG_MS / 1000.0) * rate).round().max(1.0)) as usize;
    let screen_bin_frames = (((ANCHOR_SCREEN_BIN_MS / 1000.0) * rate).round().max(1.0)) as usize;
    let env_bin = ((WIDE_ENV_BIN_MS / 1000.0) * rate).round().max(1.0) as usize;
    let confirm_max_lag = (((ANCHOR_CONFIRM_WINDOW_MS / 1000.0) * rate)
        .round()
        .max(1.0)) as usize;
    let ml = ((DUALFIT_SEAM_UNIQ_LAG_MS / 1000.0) * rate).round() as i64;
    let mlu = ml.max(0) as usize;

    // §4 risk 5: clamp the far edge of any candidate window short of the neighboring gap's own
    // dropout, when that neighbor is closer than the fixed search radius would otherwise reach.
    let max_reach_frames = max_walk_secs
        .map(|s| (s.max(0.0) * rate).round() as usize)
        .unwrap_or(usize::MAX);

    let mut screened_count: u32 = 0;
    let mut any_stage1_pass = false;
    let mut any_stage2_pass = false;
    let mut neighbor_bounded = false;

    let mut d = bucket_frames;
    while d <= radius_frames {
        let (window_start, window_end) = if forward {
            let start = seam_frame + d;
            (start, start + window_frames)
        } else {
            let Some(end) = seam_frame.checked_sub(d) else {
                break;
            };
            let Some(start) = end.checked_sub(window_frames) else {
                break;
            };
            (start, end)
        };
        if window_end > a_frames {
            break;
        }
        if d + window_frames > max_reach_frames {
            neighbor_bounded = true;
            break;
        }

        // Stage 1 — cheap A-only flatness screen, on the same window stage 2 would use if this
        // candidate qualifies (one segment, not two window concepts).
        let bins = energy_bins(
            a_samples,
            ch,
            window_start,
            window_end,
            screen_bin_frames,
            cfg.silence_peak_fraction,
            cfg.absolute_silence_rms,
        );
        if energy_envelope_is_flat(&bins) {
            screened_count += 1;
            d += bucket_frames;
            continue;
        }
        any_stage1_pass = true;

        // Stage 2 — coarse wide-envelope correlation, reusing `wide_envelope_side` verbatim against
        // the B slice the same one-directional offset assumption (§0) projects for this candidate.
        let a_win = interleaved_to_mono(&a_samples[window_start * ch..window_end * ch], ch);
        let (b_lo, b_hi) = if forward {
            let b_start = nominal_anchor_frame + d;
            let lo = b_start.saturating_sub(wide_lag_frames);
            let hi = (b_start + window_frames + wide_lag_frames).min(b_mono.len());
            (lo, hi)
        } else {
            let Some(b_end) = nominal_anchor_frame.checked_sub(d) else {
                break;
            };
            let lo = b_end.saturating_sub(window_frames + wide_lag_frames);
            let hi = (b_end + wide_lag_frames).min(b_mono.len());
            (lo, hi)
        };
        if b_hi <= b_lo {
            d += bucket_frames;
            continue;
        }
        let Some(env_peak) = wide_envelope_side(
            &a_win,
            &b_mono[b_lo..b_hi],
            sample_rate,
            env_bin,
            wide_lag_frames,
        ) else {
            d += bucket_frames;
            continue;
        };
        if env_peak.peak_r < f64::from(cfg.min_fill_correlation) {
            d += bucket_frames;
            continue;
        }
        any_stage2_pass = true;

        // Stage 3 — the existing ±`ANCHOR_CONFIRM_WINDOW_MS` seam-local confirm, at the seam's own
        // template, re-centered on this candidate's stage-2 lag instead of the nominal map lag.
        let stage2_lag_frames = (env_peak.peak_lag_ms / 1000.0 * rate).round() as i64;
        // The seam template's own naive (zero-lag) B start: `nominal_anchor_frame` for post (the
        // template starts exactly at the seam), `nominal_anchor_frame - a_seam_template.len()` for
        // pre (the template ends at the seam).
        let confirm_anchor_nominal = if forward {
            nominal_anchor_frame
        } else {
            nominal_anchor_frame.saturating_sub(a_seam_template.len())
        };
        let confirm_anchor = confirm_anchor_nominal as i64 + stage2_lag_frames;
        if confirm_anchor < 0 {
            return AnchorRelocatedLag::no_anchor(
                AnchorReason::ConfirmFailedToLock,
                screened_count,
                false,
            );
        }
        let confirm_anchor = confirm_anchor as usize;
        let Some(peak) = seam_local_peak(a_seam_template, b_mono, confirm_anchor, confirm_max_lag)
        else {
            // Terminal for this seam (§2 step 5) — a farther candidate only weakens the drift
            // premise further, not less.
            return AnchorRelocatedLag::no_anchor(
                AnchorReason::ConfirmFailedToLock,
                screened_count,
                false,
            );
        };

        // Step-boundary guard (§2 step 5), evaluated after stage 3 against the seam's own nominal
        // lag — not the coarse stage-2 lag, which is quantized to `WIDE_ENV_BIN_MS`.
        let confirm_lag_frames = stage2_lag_frames + peak.lag;
        let step_ms = (confirm_lag_frames - nominal_lag_frames) as f64 * 1000.0 / rate;
        if step_ms.abs() > ANCHOR_STEP_BOUNDARY_MS {
            // The only outcome that resumes the walk — this candidate was in the wrong regime, the
            // next one is a fresh, independent chance to be in the right one.
            d += bucket_frames;
            continue;
        }
        if peak.r < f64::from(cfg.min_fill_correlation) {
            return AnchorRelocatedLag::no_anchor(
                AnchorReason::ConfirmFailedToLock,
                screened_count,
                false,
            );
        }

        let seam_at = confirm_anchor as i64 + peak.lag;
        let confirm_prom = (seam_at >= mlu as i64
            && (seam_at as usize) + a_seam_template.len() + mlu <= b_mono.len())
        .then(|| {
            let seam_at = seam_at as usize;
            seam_prominence(
                a_seam_template,
                &b_mono[seam_at - mlu..seam_at + a_seam_template.len() + mlu],
                ml,
                sample_rate,
            )
        })
        .flatten();

        return AnchorRelocatedLag {
            lag_ms: Some(env_peak.peak_lag_ms),
            distance_ms: Some(d as f64 * 1000.0 / rate),
            r: Some(env_peak.peak_r),
            prom: Some(env_peak.prominence),
            screened_count,
            confirm_r: Some(peak.r),
            confirm_prom,
            reason: None,
            neighbor_bounded: false,
        };
    }

    let reason = if any_stage2_pass {
        AnchorReason::StepBoundaryRejected
    } else if any_stage1_pass {
        AnchorReason::Stage2NoCorrelation
    } else {
        AnchorReason::Stage1NoContour
    };
    AnchorRelocatedLag::no_anchor(reason, screened_count, neighbor_bounded)
}
/// Dual-fit viability: score the pre/post seams at each shoulder's own seam-local lag, over the gate's
/// `fill_seam_search_secs` window, and gate on `min(pre, post)`. Mirrors production `try_dual_fit`'s
/// shoulder search so [`dual_fit_rescue_flag`] can predict rescue from dump measurements (F14).
///
/// **The locks come from [`nominal_first_seam_locks`], the same call production makes** — templates,
/// range guard, nominal centers, envelope rescue, gate, bridge and trim are all its arithmetic rather
/// than a parallel copy here. Before 2026-08-28 this ran its own ±600 ms search with **no envelope**,
/// so a shoulder production rescued (and therefore gated, and possibly skipped as `shoulders_abut`)
/// still read ungated in the dump — the fingerprint contradicted the repair it exists to predict.
/// `triage_r` is `min_fill_correlation` for the same reason, not the 1 s census's
/// `LAG_DECISION_RECOVERABLE_R`: production's triage line is its gate floor.
///
/// **A borders are raw `mono(refined ± w)`** — the same construction `build_dual_fit_input` /
/// `try_dual_fit` use, now via `SeamLockTemplates::build`. Do **not** route through
/// [`border_templates_for_gap`]: silence-skip + energy trim moves the pre template, which moves
/// `pre_lag` and therefore `post_seam_global_r`, flipping the step-real term on gaps production rescues.
///
/// Fingerprint-only, and staying here: `post_seam_global_r` (is the step necessary?) and the two
/// `*_seam_prom` uniqueness measurements. Production computes neither.
///
/// `None` when either seam window falls out of range.
pub(in crate::application::gap_fingerprint::measure) fn splice_dualfit_at(
    input: &SpliceDualfitInput<'_>,
) -> Option<SpliceDualfit> {
    let SpliceDualfitInput {
        a_samples,
        channels,
        refined,
        b_mono,
        b_mapped_start,
        cfg,
        sample_rate,
        pre_anchor_max_walk_secs,
        post_anchor_max_walk_secs,
    } = *input;
    let ch = channels.max(1);
    let rate = f64::from(sample_rate).max(1.0);
    let window = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
    // Carries the range guard this function and `build_dual_fit_input` each used to spell out.
    let templates = SeamLockTemplates::build(
        a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        b_mapped_start,
        window,
    )?;
    let gap_frames = templates.gap_frames();
    // The two validators below correlate the borders against slices of `b_mono` at their own offsets,
    // which the templates cannot hand out by reference while `nominal_first_seam_locks` borrows them.
    let a_pre = templates.a_pre().to_vec();
    let a_post = templates.a_post().to_vec();

    let max_lag = (((SEAM_LOCAL_SEARCH_MS / 1000.0) * rate).round() as usize).max(1);

    // Seam-local viability, **re-anchored on nominal `b_mapped`**: the fill places each shoulder at the lag
    // that maximizes ITS own seam, so search ±`max_lag` around the nominal shoulder (pre butts at
    // `b_mapped_start`, post at `b_mapped_start + gap_frames`) and take the peak. Anchoring on the gross 1 s
    // `lag_decision` (the prior behavior) clipped seams whose lag diverges far from the 1 s peak — e.g. 7·g3,
    // pre 1 s lag −319 ms but the seam is at +18 ms, outside any narrow window around the gross placement.
    // Production `try_dual_fit` uses the same nominal centers (`dual_fit.rs`) — and now the same code.
    let env_cfg = EnvelopeCfg::from_knobs(
        cfg.lag_envelope_max_lag_ms,
        cfg.lag_envelope_window_secs,
        sample_rate,
    );
    // A is decoded whole here, exactly as on the patch path, so the envelope's wider border is the
    // same slice production takes — same helper, so the two cannot widen differently.
    let wide_a = wide_a_slicer(
        a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        env_cfg.map(|c| c.window).unwrap_or(0),
    );
    let locks = nominal_first_seam_locks(
        &templates,
        b_mono,
        max_lag,
        EnvelopeSearch {
            cfg: env_cfg,
            wide_a: Some(&wide_a),
        },
        sample_rate,
        SeamGateFloors {
            min_fill_correlation: f64::from(cfg.min_fill_correlation),
            fill_absolute_floor: f64::from(cfg.fill_absolute_floor),
        },
        f64::from(cfg.min_fill_correlation),
    )?;
    let (pre_seam_r, pre_seam_z) = (locks.pre.r, locks.pre.z);
    let (post_seam_r, post_seam_z) = (locks.post.r, locks.post.z);

    // Seam correlations arrive already finite: `seam_local_peak` returns `finite(peak_r)`, so a
    // zero-variance (digital-silence) shoulder reads **0.0, not NaN** — and production reads the same
    // 0.0, because it calls the same function. (Corrected 2026-08-28: this comment used to claim raw
    // NaN was preserved here and that mapping it would invert `gate_pass`. It was never preserved.
    // The NaN caution below on `post_seam_global_r` IS real — that one comes straight from
    // `normalized_correlation`, which does not clamp.) F14.
    // The seam-local shoulder placements (nominal ± the per-seam search) define the bridge + step, and
    // the gate is `SeamGateFloors::passes` — literally production's, not a restatement of it.
    let (b_pre_seam, b_post_seam) = (locks.b_pre_seam, locks.b_post_seam);
    let bridge_frames = locks.bridge_frames;
    let gate_pass = locks.gate_pass;

    // Validator 1 — is the step necessary? Post seam at the PRE shoulder's seam-local lag (step forced 0):
    // if the post seam also clears there, one constant shift fixes both ⇒ registration artifact, not a splice.
    let b_post_global = b_pre_seam + gap_frames;
    let post_seam_global_r = if window >= 8 && b_post_global + window <= b_mono.len() {
        normalized_correlation(&a_post, &b_mono[b_post_global..b_post_global + window])
    } else {
        f64::NAN
    };

    // Validator 2 — is each seam a unique (non-periodic) match? Prominence of the placement peak over its
    // best rival within ±30 ms.
    let ml = ((DUALFIT_SEAM_UNIQ_LAG_MS / 1000.0) * rate).round() as i64;
    let mlu = ml.max(0) as usize;
    // Same window as the PHAT probes below, over the Pearson curve — kept as a full `LagSummary`
    // (not routed through `seam_prominence`, which only returns `.prominence`) so `frac_lag_ms` is
    // available for the same-lag-agreement comparison against PHAT (`pre_seam_lag_ms` et al.).
    let pre_seam_pearson = (window >= 8
        && b_pre_seam >= window + mlu
        && b_pre_seam + mlu <= b_mono.len())
    .then(|| {
        let curve = lag_correlation_curve(
            &a_pre,
            &b_mono[b_pre_seam - window - mlu..b_pre_seam + mlu],
            ml,
        );
        summarize_lag_curve(&curve, sample_rate, 0, 0, LagChannel::Mono)
    });
    let post_seam_pearson = (window >= 8
        && b_post_seam >= mlu
        && b_post_seam + window + mlu <= b_mono.len())
    .then(|| {
        let curve = lag_correlation_curve(
            &a_post,
            &b_mono[b_post_seam - mlu..b_post_seam + window + mlu],
            ml,
        );
        summarize_lag_curve(&curve, sample_rate, 0, 0, LagChannel::Mono)
    });
    let pre_seam_pearson = pre_seam_pearson.flatten();
    let post_seam_pearson = post_seam_pearson.flatten();
    let pre_seam_prom = pre_seam_pearson.as_ref().and_then(|s| s.prominence);
    let post_seam_prom = post_seam_pearson.as_ref().and_then(|s| s.prominence);

    // GCC-PHAT experiment (`TEMP-phat-seam-scoring-plan.md`): the phase-transform equivalent of the
    // prominence probes above, over the exact same windows — same call sites, same `Option` gating, only
    // the curve function differs. Diagnostic-only; `seam_locks.rs` (the production gate) never calls this.
    let pre_seam_phat = (window >= 8
        && b_pre_seam >= window + mlu
        && b_pre_seam + mlu <= b_mono.len())
    .then(|| {
        let curve = phat_lag_correlation_curve(
            &a_pre,
            &b_mono[b_pre_seam - window - mlu..b_pre_seam + mlu],
            ml,
        );
        summarize_lag_curve(&curve, sample_rate, 0, 0, LagChannel::Mono)
    });
    let post_seam_phat = (window >= 8
        && b_post_seam >= mlu
        && b_post_seam + window + mlu <= b_mono.len())
    .then(|| {
        let curve = phat_lag_correlation_curve(
            &a_post,
            &b_mono[b_post_seam - mlu..b_post_seam + window + mlu],
            ml,
        );
        summarize_lag_curve(&curve, sample_rate, 0, 0, LagChannel::Mono)
    });
    let pre_seam_phat = pre_seam_phat.flatten();
    let post_seam_phat = post_seam_phat.flatten();

    // Seam-anchor relocation (`TEMP-seam-anchor-relocation-plan.md` §2), diagnostic-only: one
    // outward walk per side, post strictly forward, pre strictly backward (§2's one-directional
    // note). `nominal_anchor_frame` is each side's own untouched-geometry B correspondent —
    // `b_post_global` for post (== `b_pre_seam + gap_frames`, the nominal post anchor at zero
    // relative lag), `b_mapped_start` for pre.
    let post_anchor = anchor_relocated_lag(&AnchorRelocatedLagInput {
        a_samples,
        channels: ch,
        b_mono,
        sample_rate,
        cfg,
        seam_frame: refined.end_frame,
        a_seam_template: &a_post,
        nominal_anchor_frame: b_mapped_start + gap_frames,
        nominal_lag_frames: locks.post.lag,
        forward: true,
        max_walk_secs: post_anchor_max_walk_secs,
    });
    let pre_anchor = anchor_relocated_lag(&AnchorRelocatedLagInput {
        a_samples,
        channels: ch,
        b_mono,
        sample_rate,
        cfg,
        seam_frame: refined.start_frame,
        a_seam_template: &a_pre,
        nominal_anchor_frame: b_mapped_start,
        nominal_lag_frames: locks.pre.lag,
        forward: false,
        max_walk_secs: pre_anchor_max_walk_secs,
    });

    Some(SpliceDualfit {
        pre_seam_r,
        post_seam_r,
        gap_frames,
        bridge_frames,
        trim_frames: bridge_frames - gap_frames as i64,
        gate_pass,
        post_seam_global_r,
        pre_seam_prom,
        post_seam_prom,
        pre_seam_z,
        post_seam_z,
        pre_seam_phat_r: pre_seam_phat.as_ref().map(|s| s.peak_r),
        post_seam_phat_r: post_seam_phat.as_ref().map(|s| s.peak_r),
        pre_seam_phat_prom: pre_seam_phat.as_ref().and_then(|s| s.prominence),
        post_seam_phat_prom: post_seam_phat.as_ref().and_then(|s| s.prominence),
        pre_seam_phat_z: pre_seam_phat.as_ref().and_then(|s| s.peak_z),
        post_seam_phat_z: post_seam_phat.as_ref().and_then(|s| s.peak_z),
        pre_seam_lag_ms: pre_seam_pearson.as_ref().map(|s| s.frac_lag_ms),
        post_seam_lag_ms: post_seam_pearson.as_ref().map(|s| s.frac_lag_ms),
        pre_seam_phat_lag_ms: pre_seam_phat.as_ref().map(|s| s.frac_lag_ms),
        post_seam_phat_lag_ms: post_seam_phat.as_ref().map(|s| s.frac_lag_ms),
        pre_seam_rescued: Some(locks.pre.rescued),
        post_seam_rescued: Some(locks.post.rescued),
        pre_seam_edge_pinned: Some(locks.pre.edge_pinned),
        post_seam_edge_pinned: Some(locks.post.edge_pinned),
        seams_clamped: Some(locks.clamped()),
        post_seam_anchor_lag_ms: post_anchor.lag_ms,
        post_seam_anchor_distance_ms: post_anchor.distance_ms,
        post_seam_anchor_r: post_anchor.r,
        post_seam_anchor_prom: post_anchor.prom,
        post_seam_anchor_screened_count: Some(post_anchor.screened_count),
        post_seam_anchor_confirm_r: post_anchor.confirm_r,
        post_seam_anchor_confirm_prom: post_anchor.confirm_prom,
        post_seam_anchor_reason: post_anchor.reason,
        post_seam_anchor_neighbor_bounded: post_anchor.reason.map(|_| post_anchor.neighbor_bounded),
        pre_seam_anchor_lag_ms: pre_anchor.lag_ms,
        pre_seam_anchor_distance_ms: pre_anchor.distance_ms,
        pre_seam_anchor_r: pre_anchor.r,
        pre_seam_anchor_prom: pre_anchor.prom,
        pre_seam_anchor_screened_count: Some(pre_anchor.screened_count),
        pre_seam_anchor_confirm_r: pre_anchor.confirm_r,
        pre_seam_anchor_confirm_prom: pre_anchor.confirm_prom,
        pre_seam_anchor_reason: pre_anchor.reason,
        pre_seam_anchor_neighbor_bounded: pre_anchor.reason.map(|_| pre_anchor.neighbor_bounded),
    })
}

/// Fixed ladder of post-seam window widths (ms) probed by [`post_window_ladder_at`]
/// (`docs/dev/archive/TEMP-dual-fit-post-window-ladder-plan.md` §3.1). Frozen — do not add, remove, or
/// reorder without updating the plan and any consumer that assumes this exact set/order.
pub(in crate::application::gap_fingerprint::measure) const POST_WINDOW_LADDER_MS: [u32; 6] =
    [250, 375, 500, 625, 750, 1000];

/// Inputs for [`post_window_ladder_at`] — same anchor geometry as [`SpliceDualfitInput`].
pub(in crate::application::gap_fingerprint::measure) struct PostWindowLadderInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_samples: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) channels: usize,
    pub(in crate::application::gap_fingerprint::measure) refined: RefinedGapFrames,
    pub(in crate::application::gap_fingerprint::measure) b_mono: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) b_mapped_start: usize,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
}

/// Re-lock the **post** seam at a fixed ladder of window widths, all starting at the refined gap end,
/// envelope off, ±[`SEAM_LOCAL_SEARCH_MS`] (`docs/dev/archive/TEMP-dual-fit-post-window-ladder-plan.md`).
///
/// The pre collar is built from `cfg.fill_seam_search_secs` (production's actual configured seam
/// window) via [`nominal_first_seam_locks`] with `EnvelopeSearch::off()`. Every rung, including the
/// literal-250ms one, is instead computed independently from its entry in [`POST_WINDOW_LADDER_MS`]
/// via a raw A slice + a direct [`seam_local_peak`] call — the same primitive `nominal_first_seam_locks`
/// bottoms out to with the envelope off, so the two paths agree bit-for-bit **only when
/// `cfg.fill_seam_search_secs` is at its `0.25` (250 ms) default** (§3.3). `fill_seam_search_secs` has
/// no CLI flag today (TOML-only override, `infrastructure/config.rs`), so this holds for every run in
/// practice, but is not a structural guarantee — a non-default seam window would make the "250 ms"
/// rung's window differ from production's real post-lock window while still being labeled 250. Every
/// non-250 rung cannot reuse [`SeamLockTemplates`] regardless (its `a_pre`/`a_post` are locked to equal
/// length by `build`), so it re-derives the raw A slice the same way.
///
/// Rungs are omitted, not fake-zeroed, when their window runs off A or the B search is out of range.
/// Returns `None` only when even the 250 ms collar cannot be measured (mirrors [`splice_dualfit_at`]).
pub(in crate::application::gap_fingerprint::measure) fn post_window_ladder_at(
    input: &PostWindowLadderInput<'_>,
) -> Option<Vec<PostWindowLadderRung>> {
    let PostWindowLadderInput {
        a_samples,
        channels,
        refined,
        b_mono,
        b_mapped_start,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let rate = f64::from(sample_rate).max(1.0);
    let a_frames = a_samples.len() / ch;

    // The pre collar stays at the existing production 250 ms lock — never grown. `build` also carries
    // the range guard this function would otherwise have to spell out for the pre side.
    let pre_window = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
    let templates = SeamLockTemplates::build(
        a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        b_mapped_start,
        pre_window,
    )?;
    let gap_frames = templates.gap_frames();
    let max_lag = (((SEAM_LOCAL_SEARCH_MS / 1000.0) * rate).round() as usize).max(1);
    let locks_250 = nominal_first_seam_locks(
        &templates,
        b_mono,
        max_lag,
        EnvelopeSearch::off(),
        sample_rate,
        SeamGateFloors {
            min_fill_correlation: f64::from(cfg.min_fill_correlation),
            fill_absolute_floor: f64::from(cfg.fill_absolute_floor),
        },
        f64::from(cfg.min_fill_correlation),
    )?;
    let pre_lag = locks_250.pre.lag;
    let anchor_start = b_mapped_start + gap_frames;
    let ml = ((DUALFIT_SEAM_UNIQ_LAG_MS / 1000.0) * rate).round() as i64;
    let mlu = ml.max(0) as usize;

    let mut lag_w250_ms: Option<f64> = None;
    let mut rungs = Vec::with_capacity(POST_WINDOW_LADDER_MS.len());
    for &window_ms in POST_WINDOW_LADDER_MS.iter() {
        let w = ((f64::from(window_ms) / 1000.0) * rate).round() as usize;
        if w < 8 || refined.end_frame + w > a_frames {
            continue;
        }
        let a_post_w = interleaved_to_mono(
            &a_samples[refined.end_frame * ch..(refined.end_frame + w) * ch],
            ch,
        );
        let Some(peak) = seam_local_peak(&a_post_w, b_mono, anchor_start, max_lag) else {
            continue;
        };
        let post_global_r = if anchor_start + w <= b_mono.len() {
            normalized_correlation(&a_post_w, &b_mono[anchor_start..anchor_start + w])
        } else {
            f64::NAN
        };
        let seam_at = (anchor_start as i64 + peak.lag) as usize;
        let post_prom = (seam_at >= mlu && seam_at + w + mlu <= b_mono.len())
            .then(|| {
                seam_prominence(
                    &a_post_w,
                    &b_mono[seam_at - mlu..seam_at + w + mlu],
                    ml,
                    sample_rate,
                )
            })
            .flatten();
        let step_real = peak
            .r
            .partial_cmp(&(post_global_r + DUALFIT_STEP_REAL_MARGIN))
            .map(|ord| ord != std::cmp::Ordering::Less);
        let lag_ms = peak.lag as f64 * 1000.0 / rate;
        let lag_minus_w250_ms = (window_ms != 250).then_some(lag_w250_ms).flatten();
        if window_ms == 250 {
            lag_w250_ms = Some(lag_ms);
        }
        let lag_minus_pre_ms = (peak.lag - pre_lag) as f64 * 1000.0 / rate;
        let a_post_rms_db = Some(to_db(mono_rms(
            a_samples,
            ch,
            refined.end_frame,
            refined.end_frame + w,
        )));
        rungs.push(PostWindowLadderRung {
            window_ms,
            post_r: peak.r,
            post_lag_ms: lag_ms,
            post_z: peak.z,
            post_prom,
            post_global_r,
            step_real,
            lag_minus_pre_ms,
            lag_minus_w250_ms: lag_minus_w250_ms.map(|w250| lag_ms - w250),
            a_post_rms_db,
        });
    }
    Some(rungs)
}
/// Fixed ladder of pre-seam window widths (ms) probed by [`pre_window_ladder_at`]
/// (`docs/dev/archive/TEMP-dual-fit-pre-window-ladder-plan.md` §3.1). Frozen — do not add, remove, or
/// reorder without updating the plan and any consumer that assumes this exact set/order.
pub(in crate::application::gap_fingerprint::measure) const PRE_WINDOW_LADDER_MS: [u32; 6] =
    [250, 375, 500, 625, 750, 1000];

/// Inputs for [`pre_window_ladder_at`] — same anchor geometry as [`SpliceDualfitInput`].
pub(in crate::application::gap_fingerprint::measure) struct PreWindowLadderInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_samples: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) channels: usize,
    pub(in crate::application::gap_fingerprint::measure) refined: RefinedGapFrames,
    pub(in crate::application::gap_fingerprint::measure) b_mono: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) b_mapped_start: usize,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
}

/// Re-lock the **pre** seam at a fixed ladder of window widths, all ending at the refined gap
/// start, envelope off, ±[`SEAM_LOCAL_SEARCH_MS`] (`docs/dev/archive/TEMP-dual-fit-pre-window-ladder-plan.md`).
/// Mirror of [`post_window_ladder_at`] with the shoulder flipped — see that function's doc for the
/// shared caveats (the collar-length-vs-`fill_seam_search_secs` note, the omit-not-fake-zero rule).
///
/// The post collar stays at production's actual `cfg.fill_seam_search_secs` lock (envelope off),
/// via [`nominal_first_seam_locks`] — never grown; this probe only widens pre.
///
/// **Anchor differs from the post ladder on purpose (§0 anchor note of the plan).** `pre_global_r`
/// is scored at the true post-lock conjunct — `locks_250.b_post_seam − gap_frames − w` — not the
/// nominal lag-0 anchor `post_window_ladder_at`'s `post_global_r` took as a shortcut. This probe's
/// whole purpose is to test a rescued rung against post's *actual measured* lock (25/g11's
/// `Δpre`), and `step_real` (production's own-vs-forced refusal) only means anything computed
/// against a real anchor.
///
/// Returns `None` only when even the 250 ms collar cannot be measured (mirrors
/// [`splice_dualfit_at`] / [`post_window_ladder_at`]).
pub(in crate::application::gap_fingerprint::measure) fn pre_window_ladder_at(
    input: &PreWindowLadderInput<'_>,
) -> Option<Vec<PreWindowLadderRung>> {
    let PreWindowLadderInput {
        a_samples,
        channels,
        refined,
        b_mono,
        b_mapped_start,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let rate = f64::from(sample_rate).max(1.0);

    // The post collar stays at the existing production 250 ms lock — never grown. `build` also
    // carries the range guard this function would otherwise have to spell out for the post side.
    let post_window = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
    let templates = SeamLockTemplates::build(
        a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        b_mapped_start,
        post_window,
    )?;
    let gap_frames = templates.gap_frames();
    let max_lag = (((SEAM_LOCAL_SEARCH_MS / 1000.0) * rate).round() as usize).max(1);
    let locks_250 = nominal_first_seam_locks(
        &templates,
        b_mono,
        max_lag,
        EnvelopeSearch::off(),
        sample_rate,
        SeamGateFloors {
            min_fill_correlation: f64::from(cfg.min_fill_correlation),
            fill_absolute_floor: f64::from(cfg.fill_absolute_floor),
        },
        f64::from(cfg.min_fill_correlation),
    )?;
    let post_lag_ms = locks_250.post.lag as f64 * 1000.0 / rate;
    // True conjunct: the B end-position the post lock's actual measured seam implies for the pre
    // template, gap-length back — see the anchor note above.
    let conj_end = locks_250.b_post_seam as i64 - gap_frames as i64;
    let ml = ((DUALFIT_SEAM_UNIQ_LAG_MS / 1000.0) * rate).round() as i64;
    let mlu = ml.max(0) as usize;

    let mut lag_w250_ms: Option<f64> = None;
    let mut rungs = Vec::with_capacity(PRE_WINDOW_LADDER_MS.len());
    for &window_ms in PRE_WINDOW_LADDER_MS.iter() {
        let w = ((f64::from(window_ms) / 1000.0) * rate).round() as usize;
        if w < 8 || refined.start_frame < w {
            continue;
        }
        let Some(anchor_start) = b_mapped_start.checked_sub(w) else {
            continue;
        };
        let a_pre_w = interleaved_to_mono(
            &a_samples[(refined.start_frame - w) * ch..refined.start_frame * ch],
            ch,
        );
        let Some(peak) = seam_local_peak(&a_pre_w, b_mono, anchor_start, max_lag) else {
            continue;
        };
        let pre_global_r = if conj_end >= w as i64 && (conj_end as usize) <= b_mono.len() {
            let hi = conj_end as usize;
            normalized_correlation(&a_pre_w, &b_mono[hi - w..hi])
        } else {
            f64::NAN
        };
        let seam_at_start = (anchor_start as i64 + peak.lag) as usize;
        let pre_prom = (seam_at_start >= mlu && seam_at_start + w + mlu <= b_mono.len())
            .then(|| {
                seam_prominence(
                    &a_pre_w,
                    &b_mono[seam_at_start - mlu..seam_at_start + w + mlu],
                    ml,
                    sample_rate,
                )
            })
            .flatten();
        let step_real = peak
            .r
            .partial_cmp(&(pre_global_r + DUALFIT_STEP_REAL_MARGIN))
            .map(|ord| ord != std::cmp::Ordering::Less);
        let lag_ms = peak.lag as f64 * 1000.0 / rate;
        let lag_minus_w250_ms = (window_ms != 250).then_some(lag_w250_ms).flatten();
        if window_ms == 250 {
            lag_w250_ms = Some(lag_ms);
        }
        let lag_minus_post_ms = lag_ms - post_lag_ms;
        let a_pre_rms_db = Some(to_db(mono_rms(
            a_samples,
            ch,
            refined.start_frame - w,
            refined.start_frame,
        )));
        rungs.push(PreWindowLadderRung {
            window_ms,
            pre_r: peak.r,
            pre_lag_ms: lag_ms,
            pre_z: peak.z,
            pre_prom,
            pre_global_r,
            step_real,
            lag_minus_post_ms,
            lag_minus_w250_ms: lag_minus_w250_ms.map(|w250| lag_ms - w250),
            a_pre_rms_db,
        });
    }
    Some(rungs)
}
/// Mono [`LagSummary`] for one side of a [`LagFingerprint`].
pub(in crate::application::gap_fingerprint::measure) fn mono_lag_side(
    lag: &LagFingerprint,
    pre: bool,
) -> Option<&LagSummary> {
    let entries = if pre {
        &lag.pre_anchor
    } else {
        &lag.post_anchor
    };
    entries.iter().find(|s| s.channel == LagChannel::Mono)
}
/// First-class splice summary from decision-seam `lag_decision` (mono): step + per-side peaks / `peak_z`.
pub fn splice_summary_from_lag(lag: &LagFingerprint) -> Option<SpliceSummary> {
    let pre = mono_lag_side(lag, true)?;
    let post = mono_lag_side(lag, false)?;
    // `step` spans both shoulders, so either shoulder being search-exhausted taints it. `None` only when
    // neither side recorded the flag (pre-edge-pin fingerprints).
    let edge_pinned = match (pre.edge_pinned, post.edge_pinned) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(false) || b.unwrap_or(false)),
    };
    Some(SpliceSummary {
        step_ms: post.frac_lag_ms - pre.frac_lag_ms,
        pre_peak_r: pre.peak_r,
        post_peak_r: post.peak_r,
        pre_peak_z: pre.peak_z,
        post_peak_z: post.peak_z,
        edge_pinned,
    })
}
/// One side of the wide-envelope segment-identity confirmer: bucketed RMS envelope lag peak + prominence.
pub(in crate::application::gap_fingerprint::measure) fn wide_envelope_side(
    a_win: &[f64],
    b_wave_ctx: &[f64],
    sample_rate: u32,
    env_bin: usize,
    wide_lag_samples: usize,
) -> Option<EnvPeak> {
    if a_win.len() < env_bin.max(8) {
        return None;
    }
    let rate = f64::from(sample_rate).max(1.0);
    let ea = fine_rms_envelope(a_win, env_bin);
    let eb = fine_rms_envelope(b_wave_ctx, env_bin);
    if ea.len() < 3 {
        return None;
    }
    let env_max_lag = (wide_lag_samples / env_bin.max(1)) as i64;
    if env_max_lag < 1 {
        return None;
    }
    let curve = lag_correlation_curve(&ea, &eb, env_max_lag);
    if curve.is_empty() {
        return None;
    }
    let (pi, &(peak_lag_bin, peak_r)) = curve
        .iter()
        .enumerate()
        .max_by(|(_, (_, x)), (_, (_, y))| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))?;
    let prominence = secondary_peak(&curve, pi)
        .map(|(_, r)| peak_r - r)
        .unwrap_or(0.0);
    let peak_lag_ms = peak_lag_bin as f64 * env_bin as f64 * 1000.0 / rate;
    Some(EnvPeak {
        peak_r: finite_corr(peak_r),
        peak_lag_ms,
        prominence: finite_corr(prominence),
    })
}
/// Pre/post wide-envelope confirmers at **`b_mapped`** registration — cross-scale check vs `lag_decision`.
/// `post_shift_frames` mirrors `lag_pair`'s sequential centering (ledger A2): the post window is centered on `start_frame + gap_frames + post_shift_frames`, and its
/// search half-width is raised to `cfg.lag_max_lag_ms` (aligned with `lag_decision`, not the frozen
/// ±400 ms `WIDE_ENV_MAX_LAG_MS`) so it can still resolve the bridge-length mismatch after shifting.
/// `peak_lag_ms` is reported gross-relative for comparability with `lag_decision`.
pub(in crate::application::gap_fingerprint::measure) struct WideEnvelopeAtPlacementInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_samples: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) channels: usize,
    pub(in crate::application::gap_fingerprint::measure) refined: RefinedGapFrames,
    pub(in crate::application::gap_fingerprint::measure) b_mono: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) start_frame: usize,
    pub(in crate::application::gap_fingerprint::measure) post_shift_frames: i64,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
}
pub(in crate::application::gap_fingerprint::measure) fn wide_envelope_at_placement(
    input: &WideEnvelopeAtPlacementInput<'_>,
) -> WideEnvelopeFingerprint {
    let WideEnvelopeAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        start_frame,
        post_shift_frames,
        cfg,
        sample_rate,
    } = *input;
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    let window = ((WIDE_ENV_WINDOW_SECS * rate).round() as usize).max(8);
    let wide_lag = ((WIDE_ENV_MAX_LAG_MS / 1000.0) * rate).round() as usize;
    let post_wide_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * rate).round() as usize;
    let env_bin = ((WIDE_ENV_BIN_MS / 1000.0) * rate).round().max(1.0) as usize;
    let border_spec = probe_border_spec(
        &refined,
        (window + wide_lag).max(window + post_wide_lag),
        cfg,
    );
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);

    let pre = (|| {
        let w = window.min(a_pre.len());
        if w < env_bin || start_frame < w + wide_lag {
            return None;
        }
        let lo = start_frame.saturating_sub(w + wide_lag);
        let hi = (start_frame + wide_lag).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        wide_envelope_side(
            &a_pre[a_pre.len() - w..],
            &b_mono[lo..hi],
            sample_rate,
            env_bin,
            wide_lag,
        )
    })();
    let post = (|| {
        let w = window.min(a_post.len());
        let post_base =
            (start_frame as i64 + gap_frames as i64 + post_shift_frames).max(0) as usize;
        if w < env_bin || post_base < post_wide_lag {
            return None;
        }
        let lo = post_base.saturating_sub(post_wide_lag);
        let hi = (post_base + w + post_wide_lag).min(b_mono.len());
        if hi <= lo {
            return None;
        }
        wide_envelope_side(
            &a_post[..w],
            &b_mono[lo..hi],
            sample_rate,
            env_bin,
            post_wide_lag,
        )
    })();
    let post_shift_ms = post_shift_frames as f64 * 1000.0 / rate;
    let post = post.map(|mut ep| {
        ep.peak_lag_ms += post_shift_ms;
        ep
    });
    WideEnvelopeFingerprint { pre, post }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    fn base_noise(seed: u64, len: usize) -> Vec<f64> {
        (0..len).map(|i| noise(seed, i)).collect()
    }

    /// Noise under a linear amplitude ramp. Flat noise has a flat RMS envelope, which the coarse
    /// envelope search cannot register against — the ramp is what gives it something to lock onto,
    /// and it is the same shaping `lag.rs`'s envelope-rescue fixtures use.
    fn write_shaped_noise(buf: &mut [f32], start: usize, end: usize, seed: u64, rising: bool) {
        let span = (end - start).max(1) as f64;
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            let t = (f - start) as f64 / span;
            let env = if rising { t } else { 1.0 - t };
            *slot = (noise(seed, f) * 0.5 * (0.2 + 0.8 * env)) as f32;
        }
    }

    fn frames_at_ms(rate: u32, ms: u64) -> usize {
        (ms as f64 / 1000.0 * f64::from(rate)).round() as usize
    }

    fn lerp(base: &[f64], x: f64) -> f64 {
        if x <= 0.0 {
            return base[0];
        }
        let i = x.floor() as usize;
        if i + 1 >= base.len() {
            return base[base.len() - 1];
        }
        let f = x - i as f64;
        base[i] * (1.0 - f) + base[i + 1] * f
    }

    /// Build (a, b_ctx) where B is the shared broadband signal delayed by `offset` samples (possibly
    /// fractional), so the lag curve should peak at lag ≈ `offset`.
    fn shifted_pair(offset: f64, n: usize, max_lag: i64) -> (Vec<f64>, Vec<f64>) {
        let ml = max_lag as f64;
        let a0 = (ml + offset.abs()).ceil() as usize + 4;
        let base = base_noise(1, a0 + n + max_lag as usize + 8);
        let a: Vec<f64> = (0..n).map(|i| base[a0 + i]).collect();
        let ctx_len = n + 2 * max_lag as usize;
        let b_ctx: Vec<f64> = (0..ctx_len)
            .map(|k| lerp(&base, a0 as f64 + (k as f64 - ml - offset)))
            .collect();
        (a, b_ctx)
    }

    #[test]
    fn weighted_downmix_recovers_center_dominant_level() {
        // 6ch center-dominant 5.1 seam: loud center (idx 2), quiet L/R, silent surrounds/LFE.
        let ch = 6;
        let n = 480;
        let mut s = vec![0.0f32; n * ch];
        for f in 0..n {
            s[f * ch + 2] =
                (std::f64::consts::TAU * 200.0 * f as f64 / 48_000.0).sin() as f32 * 0.5;
            s[f * ch] = 0.005;
            s[f * ch + 1] = -0.005;
        }
        let weighted = weighted_downmix_rms(&s, ch, 0, n);
        let mono: Vec<f64> = s
            .chunks(ch)
            .map(|fr| fr.iter().map(|&x| x as f64).sum::<f64>() / ch as f64)
            .collect();
        let mono_rms = (mono.iter().map(|v| v * v).sum::<f64>() / mono.len() as f64).sqrt();
        // The straight 1/6 mix buries the center; the energy-weighted mix keeps it (~0.5/√2 ≈ 0.35).
        assert!(
            weighted > 0.2,
            "weighted preserves center level: {weighted}"
        );
        assert!(
            weighted > mono_rms * 3.0,
            "weighted {weighted} ≫ straight mono {mono_rms}"
        );
        // Over-range / empty spans are guarded.
        assert_eq!(weighted_downmix_rms(&s, ch, 10, 10), 0.0);
        assert_eq!(
            weighted_downmix_rms(&s, ch, n - 5, n + 100),
            weighted_downmix_rms(&s, ch, n - 5, n)
        );
    }

    /// **Step 6's whole point** (31/g7 class): a post shoulder whose true lag is −1200 ms, i.e. twice
    /// outside the ±600 ms seam sweep. Production reaches it through the coarse envelope and then skips
    /// the gap as `shoulders_abut`; before 2026-08-28 this probe ran its own sweep with **no envelope**,
    /// read the shoulder dead, and wrote `gate_pass: false` — the dump flatly contradicted the repair it
    /// exists to predict. Both arms are asserted, so the old reading is pinned as the *envelope-off*
    /// reading rather than left as a thing that might come back.
    ///
    /// The fixture is introduced silence: B runs continuously, A has 1.2 s of digital silence spliced
    /// into it, so the post border matches B exactly one gap-length early and the two locks land on the
    /// same B frame — `bridge_frames == 0`, no donor material between them.
    #[test]
    fn splice_dualfit_rescues_a_far_post_shoulder_and_reads_the_abut() {
        const RATE: u32 = 48_000;
        let pre_len = frames_at_ms(RATE, 4000);
        let gap = frames_at_ms(RATE, 1200);
        let post_len = frames_at_ms(RATE, 3000);
        let gap_start = pre_len;
        let gap_end = pre_len + gap;
        let total = gap_end + post_len + frames_at_ms(RATE, 500);

        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, 0, gap_start, 11, true);
        write_shaped_noise(&mut a, gap_end, gap_end + post_len, 23, false);

        // B: the same two runs of content, butted together with nothing between them.
        let mut b_mono = vec![0.0f64; gap_start + post_len + frames_at_ms(RATE, 500)];
        for (i, &v) in a.iter().take(gap_start).enumerate() {
            b_mono[i] = f64::from(v);
        }
        for j in 0..post_len {
            b_mono[gap_start + j] = f64::from(a[gap_end + j]);
        }

        let refined = RefinedGapFrames {
            start_frame: gap_start,
            end_frame: gap_end,
        };
        let measure = |cfg: &FingerprintConfig| {
            splice_dualfit_at(&SpliceDualfitInput {
                a_samples: &a,
                channels: 1,
                refined,
                b_mono: &b_mono,
                b_mapped_start: gap_start,
                cfg,
                sample_rate: RATE,
                pre_anchor_max_walk_secs: None,
                post_anchor_max_walk_secs: None,
            })
            .expect("seam windows in range")
        };

        let on = measure(&FingerprintConfig::default());
        assert!(
            on.pre_seam_r > 0.8 && on.post_seam_r > 0.8,
            "both shoulders should lock: pre={} post={}",
            on.pre_seam_r,
            on.post_seam_r
        );
        assert_eq!(on.pre_seam_rescued, Some(false), "pre is true at nominal");
        assert_eq!(
            on.post_seam_rescued,
            Some(true),
            "post is only reachable through the envelope"
        );
        assert!(on.gate_pass, "gate_pass must agree with production");
        assert!(
            on.bridge_frames.abs() <= frames_at_ms(RATE, 25) as i64,
            "the shoulders should land on the same B frame, got bridge={}",
            on.bridge_frames
        );
        assert!(
            on.bridge_frames <= 0 && on.gate_pass,
            "this is the `SeamLocks::abut` shape production skips on"
        );
        assert_eq!(
            on.seams_clamped,
            Some(false),
            "a real abut, not two seams collapsing onto B frame 0"
        );

        // Envelope off: the pre-2026-08-28 reading, and the reason the fingerprint disagreed.
        let off = measure(&FingerprintConfig {
            lag_envelope_max_lag_ms: 0,
            ..FingerprintConfig::default()
        });
        assert!(
            !off.gate_pass && off.post_seam_r < 0.35,
            "without the envelope the post shoulder is out of reach: r={} gate_pass={}",
            off.post_seam_r,
            off.gate_pass
        );
        assert_eq!(
            off.pre_seam_r, on.pre_seam_r,
            "the envelope must not touch a shoulder that already reads well"
        );
    }

    /// A shaped-noise world of `len` frames, amplitude-ramped end to end so a 10 s stage-2 window
    /// anywhere in it reads as non-flat (§4a's stage-2/stage-3 fixtures need real structure, not the
    /// stage-1 fixture's silence).
    fn shaped_signal(len: usize, seed: u64) -> Vec<f64> {
        let span = len.max(1) as f64;
        (0..len)
            .map(|i| noise(seed, i) * 0.5 * (0.2 + 0.8 * (i as f64 / span)))
            .collect()
    }

    /// Same shape as `shaped_signal` but with the envelope trend inverted (loud-to-quiet instead of
    /// quiet-to-loud) and an unrelated seed — used to build an A/B pair whose wide-envelope
    /// cross-correlation (stage 2) is deliberately weak despite neither side being flat (stage 1
    /// passes on both).
    fn inverse_shaped_signal(len: usize, seed: u64) -> Vec<f64> {
        let span = len.max(1) as f64;
        (0..len)
            .map(|i| noise(seed, i) * 0.5 * (1.0 - 0.8 * (i as f64 / span)))
            .collect()
    }

    /// Builds an anchor-relocation A/B pair: A is `shaped_signal`, B is the same world uniformly
    /// delayed by `shift_frames` (`b[m] = world[m - shift_frames]`), matching `anchor_relocated_lag`'s
    /// "true B position = predicted position + lag" convention verified against `post_window_ladder_at`.
    /// `total_a_len` must cover the seam plus the full `ANCHOR_SEARCH_RADIUS_SECS` walk plus one
    /// `ANCHOR_WIDE_WINDOW_SECS` stage-2 window.
    fn build_anchor_shift_world(
        total_a_len: usize,
        shift_frames: i64,
        seed: u64,
    ) -> (Vec<f32>, Vec<f64>) {
        let world = shaped_signal(total_a_len + 20_000, seed);
        let a: Vec<f32> = world[..total_a_len].iter().map(|&v| v as f32).collect();
        let b_len = world.len() + shift_frames.max(0) as usize + 20_000;
        let mut b = vec![0.0f64; b_len];
        for (m, slot) in b.iter_mut().enumerate() {
            let src = m as i64 - shift_frames;
            if src >= 0 && (src as usize) < world.len() {
                *slot = world[src as usize];
            }
        }
        (a, b)
    }

    const ANCHOR_TEST_SEAM_FRAME: usize = 96_000; // 2s @ 48kHz
    const ANCHOR_TEST_TOTAL_A_LEN: usize = ANCHOR_TEST_SEAM_FRAME + 240_000 + 480_000 + 48_000;
    const ANCHOR_TEST_TMPL_MS: u64 = 100;

    /// §4a assertion 1: a flat/silent A region rejected outright by stage 1. `screened_count` covers
    /// every bucket the walk ever visited (`ANCHOR_SEARCH_RADIUS_SECS` / `ANCHOR_BUCKET_MS`), which is
    /// only possible if stage 2 (`wide_envelope_side`) never ran on any candidate — a stage-2 pass
    /// (even one that later failed) would have produced a different final reason.
    #[test]
    fn stage1_screen_rejects_a_flat_region_without_ever_reaching_stage2() {
        const RATE: u32 = 48_000;
        let a = vec![0.0f32; ANCHOR_TEST_TOTAL_A_LEN];
        let tmpl = vec![0.0f64; frames_at_ms(RATE, ANCHOR_TEST_TMPL_MS)];
        let cfg = FingerprintConfig::default();

        let result = anchor_relocated_lag(&AnchorRelocatedLagInput {
            a_samples: &a,
            channels: 1,
            b_mono: &[],
            sample_rate: RATE,
            cfg: &cfg,
            seam_frame: ANCHOR_TEST_SEAM_FRAME,
            a_seam_template: &tmpl,
            nominal_anchor_frame: ANCHOR_TEST_SEAM_FRAME,
            nominal_lag_frames: 0,
            forward: true,
            max_walk_secs: None,
        });

        assert_eq!(
            result.reason,
            Some(AnchorReason::Stage1NoContour),
            "an all-silent region must be screened out at stage 1"
        );
        assert!(result.lag_ms.is_none());
        let expected_buckets =
            (ANCHOR_SEARCH_RADIUS_SECS * 1000.0 / ANCHOR_BUCKET_MS).round() as u32;
        assert_eq!(
            result.screened_count, expected_buckets,
            "every candidate in the walk must have been screened at stage 1, none reaching stage 2"
        );
        assert!(
            !result.neighbor_bounded,
            "the walk ran the full radius unbounded — this is genuine exhaustion, not a neighbor cutoff"
        );
    }

    /// §4a assertion 2: a region with real structure at a known injected lag clears stage 2 and the
    /// reported lag matches the injection.
    #[test]
    fn stage2_clears_and_reports_the_injected_lag() {
        const RATE: u32 = 48_000;
        let tmpl_len = frames_at_ms(RATE, ANCHOR_TEST_TMPL_MS);
        let (a, b) = build_anchor_shift_world(ANCHOR_TEST_TOTAL_A_LEN, 0, 7);
        let tmpl: Vec<f64> = a[ANCHOR_TEST_SEAM_FRAME..ANCHOR_TEST_SEAM_FRAME + tmpl_len]
            .iter()
            .map(|&v| v as f64)
            .collect();
        let cfg = FingerprintConfig::default();

        let result = anchor_relocated_lag(&AnchorRelocatedLagInput {
            a_samples: &a,
            channels: 1,
            b_mono: &b,
            sample_rate: RATE,
            cfg: &cfg,
            seam_frame: ANCHOR_TEST_SEAM_FRAME,
            a_seam_template: &tmpl,
            nominal_anchor_frame: ANCHOR_TEST_SEAM_FRAME,
            nominal_lag_frames: 0,
            forward: true,
            max_walk_secs: None,
        });

        assert!(
            result.reason.is_none(),
            "a clean zero-lag match must be accepted, got {:?}",
            result.reason
        );
        assert_eq!(
            result.lag_ms,
            Some(0.0),
            "the injected lag was zero and must be reported as such"
        );
        assert!(
            result.confirm_r.unwrap_or(0.0) > 0.9,
            "the confirm stage should lock cleanly on identical content: {:?}",
            result.confirm_r
        );
        assert!(
            !result.neighbor_bounded,
            "an accepted candidate was found well within the unbounded walk"
        );
    }

    /// §4a assertion 3: nominal lag and stage-2 lag differ by far more than ±`ANCHOR_CONFIRM_WINDOW_MS`
    /// — stage 3 must re-center its ±30 ms confirm search on the stage-2 lag, not on the nominal
    /// (zero-lag) anchor, since a confirm centered on nominal could never reach a 300 ms-away peak.
    #[test]
    fn stage3_recenters_the_confirm_on_the_stage2_lag_not_nominal() {
        const RATE: u32 = 48_000;
        const SHIFT_MS: i64 = 300; // exact ANCHOR_WIDE_ENV_BIN_MS(100ms) multiple; >> the ±30ms confirm window
        let shift_frames = frames_at_ms(RATE, SHIFT_MS as u64) as i64;
        let tmpl_len = frames_at_ms(RATE, ANCHOR_TEST_TMPL_MS);
        let (a, b) = build_anchor_shift_world(ANCHOR_TEST_TOTAL_A_LEN, shift_frames, 11);
        let tmpl: Vec<f64> = a[ANCHOR_TEST_SEAM_FRAME..ANCHOR_TEST_SEAM_FRAME + tmpl_len]
            .iter()
            .map(|&v| v as f64)
            .collect();
        let cfg = FingerprintConfig::default();

        // The seam's own lock already tracks this same global drift, so the step-boundary guard sees
        // no discontinuity — only the recentering itself is under test here.
        let result = anchor_relocated_lag(&AnchorRelocatedLagInput {
            a_samples: &a,
            channels: 1,
            b_mono: &b,
            sample_rate: RATE,
            cfg: &cfg,
            seam_frame: ANCHOR_TEST_SEAM_FRAME,
            a_seam_template: &tmpl,
            nominal_anchor_frame: ANCHOR_TEST_SEAM_FRAME,
            nominal_lag_frames: shift_frames,
            forward: true,
            max_walk_secs: None,
        });

        assert!(
            result.reason.is_none(),
            "a consistent 300ms drift must be accepted, got {:?}",
            result.reason
        );
        assert_eq!(result.lag_ms, Some(SHIFT_MS as f64));
        assert!(
            result.confirm_r.unwrap_or(0.0) > 0.9,
            "confirm must lock once centered on the stage-2 lag: {:?}",
            result.confirm_r
        );

        // Demonstrate the recentering was load-bearing: a confirm search centered on the nominal
        // (zero-lag) anchor, with the same ±confirm-window reach, cannot find this peak at all.
        let confirm_max_lag = ((ANCHOR_CONFIRM_WINDOW_MS / 1000.0 * f64::from(RATE))
            .round()
            .max(1.0)) as usize;
        let naive = seam_local_peak(&tmpl, &b, ANCHOR_TEST_SEAM_FRAME, confirm_max_lag);
        assert!(
            naive.map(|p| p.r).unwrap_or(0.0) < 0.5,
            "a confirm centered on nominal (no recentering) must not reach a 300ms-away peak"
        );
        assert!(
            !result.neighbor_bounded,
            "an accepted candidate was found well within the unbounded walk"
        );
    }

    /// §4a assertion 4: a planted step discontinuity — every stage-1/stage-2-qualifying candidate
    /// confirms a lag that disagrees with the seam's own nominal lag by far more than
    /// `ANCHOR_STEP_BOUNDARY_MS` — must yield exactly `StepBoundaryRejected`, not an accepted anchor.
    #[test]
    fn planted_step_discontinuity_yields_step_boundary_rejected() {
        const RATE: u32 = 48_000;
        const SHIFT_MS: i64 = 300;
        let shift_frames = frames_at_ms(RATE, SHIFT_MS as u64) as i64;
        let tmpl_len = frames_at_ms(RATE, ANCHOR_TEST_TMPL_MS);
        let (a, b) = build_anchor_shift_world(ANCHOR_TEST_TOTAL_A_LEN, shift_frames, 13);
        let tmpl: Vec<f64> = a[ANCHOR_TEST_SEAM_FRAME..ANCHOR_TEST_SEAM_FRAME + tmpl_len]
            .iter()
            .map(|&v| v as f64)
            .collect();
        let cfg = FingerprintConfig::default();

        // Nominal lag stays at 0 — every candidate's real 300ms drift is a step relative to it.
        let result = anchor_relocated_lag(&AnchorRelocatedLagInput {
            a_samples: &a,
            channels: 1,
            b_mono: &b,
            sample_rate: RATE,
            cfg: &cfg,
            seam_frame: ANCHOR_TEST_SEAM_FRAME,
            a_seam_template: &tmpl,
            nominal_anchor_frame: ANCHOR_TEST_SEAM_FRAME,
            nominal_lag_frames: 0,
            forward: true,
            max_walk_secs: None,
        });

        assert_eq!(
            result.reason,
            Some(AnchorReason::StepBoundaryRejected),
            "a uniform 300ms drift against a 0ms nominal lock must be rejected as a step boundary, got {:?}",
            result
        );
        assert!(result.lag_ms.is_none());
        assert!(
            !result.neighbor_bounded,
            "every candidate ran to completion (confirm + step-boundary check) inside the unbounded walk"
        );
    }

    /// §4 risk 5: `max_walk_secs` set well short of even the first candidate's stage-2 window means the
    /// walk breaks before stage 1 ever runs on any candidate. This must report `neighbor_bounded: true`
    /// alongside `Stage1NoContour` (zero candidates attempted) — not silently look identical to genuine
    /// full-radius exhaustion (the flat-region case above, which asserts `neighbor_bounded: false`).
    #[test]
    fn neighbor_bound_stops_the_walk_before_any_stage_runs() {
        const RATE: u32 = 48_000;
        let a = vec![0.0f32; ANCHOR_TEST_TOTAL_A_LEN];
        let tmpl = vec![0.0f64; frames_at_ms(RATE, ANCHOR_TEST_TMPL_MS)];
        let cfg = FingerprintConfig::default();

        // A single stage-2 window is 10s wide; 1s of walk room can't fit even the first candidate's
        // window, so the neighbor-bound break fires on iteration 1, before stage 1 runs at all.
        let result = anchor_relocated_lag(&AnchorRelocatedLagInput {
            a_samples: &a,
            channels: 1,
            b_mono: &[],
            sample_rate: RATE,
            cfg: &cfg,
            seam_frame: ANCHOR_TEST_SEAM_FRAME,
            a_seam_template: &tmpl,
            nominal_anchor_frame: ANCHOR_TEST_SEAM_FRAME,
            nominal_lag_frames: 0,
            forward: true,
            max_walk_secs: Some(1.0),
        });

        assert!(
            result.neighbor_bounded,
            "the walk must report it was cut short by the neighbor bound, not genuine exhaustion"
        );
        assert_eq!(
            result.reason,
            Some(AnchorReason::Stage1NoContour),
            "no candidate ever got far enough to run stage 1: {:?}",
            result.reason
        );
        assert_eq!(
            result.screened_count, 0,
            "the bound must trigger before any stage-1 screen runs"
        );
    }

    /// §4 risk 5, continued: the neighbor bound is not exclusive to `Stage1NoContour` — it can equally
    /// cut the walk short after a candidate has already passed stage 1 (`any_stage1_pass`) but failed
    /// stage 2, yielding `Stage2NoCorrelation` with `neighbor_bounded: true`. This is the case the
    /// `AnchorReason::Stage1NoContour` doc comment's "(or the neighbor-gap bound, if closer)" aside does
    /// not mention — a dump reader must not assume only `Stage1NoContour` can be neighbor-bounded.
    #[test]
    fn neighbor_bound_can_also_mark_a_stage2_no_correlation_reason() {
        const RATE: u32 = 48_000;
        // A has real (non-flat) structure so stage 1 passes on the first candidate; B is an
        // independent, inverted-envelope-trend signal so stage 2's wide-envelope correlation stays
        // weak — the first candidate fails stage 2, then the walk's second candidate breaks on the
        // neighbor bound before stage 1 even runs on it.
        let a: Vec<f32> = shaped_signal(ANCHOR_TEST_TOTAL_A_LEN, 21)
            .iter()
            .map(|&v| v as f32)
            .collect();
        let b = inverse_shaped_signal(a.len() + 20_000, 99);
        let tmpl_len = frames_at_ms(RATE, ANCHOR_TEST_TMPL_MS);
        let tmpl: Vec<f64> = a[ANCHOR_TEST_SEAM_FRAME..ANCHOR_TEST_SEAM_FRAME + tmpl_len]
            .iter()
            .map(|&v| v as f64)
            .collect();
        let cfg = FingerprintConfig::default();

        // 10.3s fits the first candidate's 10s stage-2 window (d=250ms + 10s = 10.25s) but not the
        // second's (d=500ms + 10s = 10.5s).
        let result = anchor_relocated_lag(&AnchorRelocatedLagInput {
            a_samples: &a,
            channels: 1,
            b_mono: &b,
            sample_rate: RATE,
            cfg: &cfg,
            seam_frame: ANCHOR_TEST_SEAM_FRAME,
            a_seam_template: &tmpl,
            nominal_anchor_frame: ANCHOR_TEST_SEAM_FRAME,
            nominal_lag_frames: 0,
            forward: true,
            max_walk_secs: Some(10.3),
        });

        assert!(
            result.neighbor_bounded,
            "the second candidate's window no longer fits inside the 10.3s bound"
        );
        assert_eq!(
            result.reason,
            Some(AnchorReason::Stage2NoCorrelation),
            "the first candidate passed stage 1 but not stage 2, so this must not read as Stage1NoContour: {:?}",
            result.reason
        );
    }

    /// §3.3 of `docs/dev/archive/TEMP-dual-fit-post-window-ladder-plan.md`: the ladder's 250 ms rung must
    /// reproduce production's post lock exactly, not approximately — both call
    /// `nominal_first_seam_locks`/`seam_local_peak` with the envelope off, so on identical A/B content
    /// (trivial gate-pass, trim 0) the two readings must agree to floating-point precision.
    #[test]
    fn post_window_ladder_250ms_rung_matches_dual_fit_at_nominal() {
        const RATE: u32 = 48_000;
        let total = frames_at_ms(RATE, 6000);
        let signal: Vec<f32> = (0..total).map(|i| (noise(3, i) * 0.5) as f32).collect();
        let b_mono: Vec<f64> = signal.iter().map(|&v| f64::from(v)).collect();

        let gap_start = frames_at_ms(RATE, 2000);
        let gap_frames = frames_at_ms(RATE, 800);
        let gap_end = gap_start + gap_frames;
        let refined = RefinedGapFrames {
            start_frame: gap_start,
            end_frame: gap_end,
        };

        let cfg = FingerprintConfig::default();
        let ladder = post_window_ladder_at(&PostWindowLadderInput {
            a_samples: &signal,
            channels: 1,
            refined,
            b_mono: &b_mono,
            b_mapped_start: gap_start,
            cfg: &cfg,
            sample_rate: RATE,
        })
        .expect("ladder measured");
        let rung_250 = ladder
            .iter()
            .find(|r| r.window_ms == 250)
            .expect("250ms rung present");

        let rate = f64::from(RATE);
        let seam_window_frames = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
        let max_lag_frames = ((SEAM_LOCAL_SEARCH_MS / 1000.0 * rate).round() as usize).max(1);
        let templates = SeamLockTemplates::build(
            &signal,
            1,
            gap_start,
            gap_end,
            gap_start,
            seam_window_frames,
        )
        .expect("templates build");
        let params = DualFitParams {
            channels: 1,
            sample_rate: RATE,
            gap_frames: templates.gap_frames(),
            seam_window_frames,
            max_lag_frames,
            min_fill_correlation: f64::from(cfg.min_fill_correlation),
            fill_absolute_floor: f64::from(cfg.fill_absolute_floor),
            step_real_margin: DUALFIT_STEP_REAL_MARGIN,
            a_gap_floor_db: -60.0,
        };
        let dual = dual_fit_at_nominal(&templates, &b_mono, &signal, &params)
            .expect("production dual-fit succeeds on identical A/B content");

        assert!(
            (rung_250.post_r - dual.post_seam_r).abs() < 1e-9,
            "250ms rung post_r must equal production: {} vs {}",
            rung_250.post_r,
            dual.post_seam_r
        );
        assert!(
            (rung_250.post_lag_ms - dual.post_lag as f64 * 1000.0 / rate).abs() < 1e-9,
            "250ms rung lag must equal production's post_lag"
        );
    }

    /// Quiet-then-speech post collar: the true content starts partway into the 250 ms collar, so a
    /// short window's peak can lock onto whatever quiet noise floor precedes it while a wider window
    /// spans enough real content to snap to the correct lag. Exercises the ladder's actual purpose —
    /// this is the shape `docs/dev/archive/TEMP-dual-fit-post-window-ladder-plan.md` exists to characterize.
    #[test]
    fn post_window_ladder_wide_rung_snaps_where_the_250ms_rung_reads_quiet_noise() {
        const RATE: u32 = 48_000;
        let quiet_ms = 300u64;
        let speech_ms = 900u64;
        let quiet_frames = frames_at_ms(RATE, quiet_ms);
        let speech_frames = frames_at_ms(RATE, speech_ms);
        let post_len = quiet_frames + speech_frames;

        let gap_start = frames_at_ms(RATE, 2000);
        let gap_frames = frames_at_ms(RATE, 800);
        let gap_end = gap_start + gap_frames;
        let total = gap_end + post_len + frames_at_ms(RATE, 200);

        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, 0, gap_start, 5, true);
        // Post shoulder: near-silent noise floor, then real (louder, distinctly-seeded) content.
        for (f, slot) in a
            .iter_mut()
            .enumerate()
            .take(gap_end + quiet_frames)
            .skip(gap_end)
        {
            *slot = (noise(9, f) * 0.002) as f32;
        }
        for (f, slot) in a
            .iter_mut()
            .enumerate()
            .take(gap_end + post_len)
            .skip(gap_end + quiet_frames)
        {
            *slot = (noise(31, f) * 0.5) as f32;
        }

        // B: matches A's pre shoulder at nominal; everywhere else is independent background noise
        // (same tiny scale as A's quiet prefix, but uncorrelated with it) except for the real speech,
        // which is placed only at its true +40 ms shift. A 250 ms post window — entirely inside the
        // 300 ms quiet prefix — has nothing real to lock onto; a wider window that reaches the speech
        // does.
        let shift = frames_at_ms(RATE, 40) as i64;
        let mut b_mono = vec![0.0f64; total + shift.unsigned_abs() as usize + 8];
        for (i, slot) in b_mono.iter_mut().enumerate() {
            *slot = noise(77, i) * 0.002;
        }
        for (i, &v) in a.iter().take(gap_start).enumerate() {
            b_mono[i] = f64::from(v);
        }
        for (f, &v) in a
            .iter()
            .enumerate()
            .take(gap_end + post_len)
            .skip(gap_end + quiet_frames)
        {
            let bi = (f as i64 + shift) as usize;
            b_mono[bi] = f64::from(v);
        }

        let refined = RefinedGapFrames {
            start_frame: gap_start,
            end_frame: gap_end,
        };
        let cfg = FingerprintConfig::default();
        let ladder = post_window_ladder_at(&PostWindowLadderInput {
            a_samples: &a,
            channels: 1,
            refined,
            b_mono: &b_mono,
            b_mapped_start: gap_start,
            cfg: &cfg,
            sample_rate: RATE,
        })
        .expect("ladder measured");

        let rate = f64::from(RATE);
        let shift_ms = shift as f64 * 1000.0 / rate;
        let rung = |ms: u32| {
            ladder
                .iter()
                .find(|r| r.window_ms == ms)
                .unwrap_or_else(|| panic!("{ms}ms rung present"))
        };
        let r250 = rung(250);
        let r750 = rung(750);
        assert!(
            (r250.post_lag_ms - shift_ms).abs() > 5.0,
            "250ms rung (all quiet) should NOT land on the true +{shift_ms}ms shift, got {}",
            r250.post_lag_ms
        );
        assert!(
            (r750.post_lag_ms - shift_ms).abs() < 1.0,
            "750ms rung (reaches the speech) should snap to the true +{shift_ms}ms shift, got {}",
            r750.post_lag_ms
        );
    }

    /// §3.3 of `docs/dev/archive/TEMP-dual-fit-pre-window-ladder-plan.md`: the ladder's 250 ms rung must
    /// reproduce production's pre lock exactly, not approximately — both call
    /// `nominal_first_seam_locks`/`seam_local_peak` with the envelope off, so on identical A/B content
    /// (trivial gate-pass, trim 0) the two readings must agree to floating-point precision.
    #[test]
    fn pre_window_ladder_250ms_rung_matches_dual_fit_at_nominal() {
        const RATE: u32 = 48_000;
        let total = frames_at_ms(RATE, 6000);
        let signal: Vec<f32> = (0..total).map(|i| (noise(3, i) * 0.5) as f32).collect();
        let b_mono: Vec<f64> = signal.iter().map(|&v| f64::from(v)).collect();

        let gap_start = frames_at_ms(RATE, 2000);
        let gap_frames = frames_at_ms(RATE, 800);
        let gap_end = gap_start + gap_frames;
        let refined = RefinedGapFrames {
            start_frame: gap_start,
            end_frame: gap_end,
        };

        let cfg = FingerprintConfig::default();
        let ladder = pre_window_ladder_at(&PreWindowLadderInput {
            a_samples: &signal,
            channels: 1,
            refined,
            b_mono: &b_mono,
            b_mapped_start: gap_start,
            cfg: &cfg,
            sample_rate: RATE,
        })
        .expect("ladder measured");
        let rung_250 = ladder
            .iter()
            .find(|r| r.window_ms == 250)
            .expect("250ms rung present");

        let rate = f64::from(RATE);
        let seam_window_frames = ((cfg.fill_seam_search_secs * rate).round() as usize).max(8);
        let max_lag_frames = ((SEAM_LOCAL_SEARCH_MS / 1000.0 * rate).round() as usize).max(1);
        let templates = SeamLockTemplates::build(
            &signal,
            1,
            gap_start,
            gap_end,
            gap_start,
            seam_window_frames,
        )
        .expect("templates build");
        let params = DualFitParams {
            channels: 1,
            sample_rate: RATE,
            gap_frames: templates.gap_frames(),
            seam_window_frames,
            max_lag_frames,
            min_fill_correlation: f64::from(cfg.min_fill_correlation),
            fill_absolute_floor: f64::from(cfg.fill_absolute_floor),
            step_real_margin: DUALFIT_STEP_REAL_MARGIN,
            a_gap_floor_db: -60.0,
        };
        let dual = dual_fit_at_nominal(&templates, &b_mono, &signal, &params)
            .expect("production dual-fit succeeds on identical A/B content");

        assert!(
            (rung_250.pre_r - dual.pre_seam_r).abs() < 1e-9,
            "250ms rung pre_r must equal production: {} vs {}",
            rung_250.pre_r,
            dual.pre_seam_r
        );
        assert!(
            (rung_250.pre_lag_ms - dual.pre_lag as f64 * 1000.0 / rate).abs() < 1e-9,
            "250ms rung lag must equal production's pre_lag"
        );
    }

    /// Quiet-then-speech pre collar: the true content ends right before the gap only after a quiet
    /// prefix nearer the gap, so a short window's peak can lock onto whatever quiet noise floor
    /// immediately precedes the gap while a wider window spans enough real content (further back)
    /// to snap to the correct lag. Exercises the ladder's actual purpose — this is the shape
    /// `docs/dev/archive/TEMP-dual-fit-pre-window-ladder-plan.md` exists to characterize (25/g11's class).
    #[test]
    fn pre_window_ladder_wide_rung_snaps_where_the_250ms_rung_reads_quiet_noise() {
        const RATE: u32 = 48_000;
        let speech_ms = 900u64;
        let quiet_ms = 300u64;
        let speech_frames = frames_at_ms(RATE, speech_ms);
        let quiet_frames = frames_at_ms(RATE, quiet_ms);
        let pre_len = speech_frames + quiet_frames;

        let gap_start = pre_len;
        let gap_frames = frames_at_ms(RATE, 800);
        let gap_end = gap_start + gap_frames;
        let post_len = frames_at_ms(RATE, 2000);
        let total = gap_end + post_len;

        let mut a = vec![0.0f32; total];
        // Pre shoulder: real (louder, distinctly-seeded) content, then a near-silent noise floor
        // immediately before the gap.
        for (f, slot) in a.iter_mut().enumerate().take(speech_frames) {
            *slot = (noise(31, f) * 0.5) as f32;
        }
        for (f, slot) in a.iter_mut().enumerate().take(pre_len).skip(speech_frames) {
            *slot = (noise(9, f) * 0.002) as f32;
        }
        write_shaped_noise(&mut a, gap_end, total, 23, false);

        // B: matches A's post shoulder at nominal; everywhere else is independent background noise
        // (same tiny scale as A's quiet prefix, but uncorrelated with it) except for the real speech,
        // which is placed only at its true +40 ms shift. A 250 ms pre window — entirely inside the
        // 300 ms quiet suffix — has nothing real to lock onto; a wider window that reaches the speech
        // does.
        let shift = frames_at_ms(RATE, 40) as i64;
        let mut b_mono = vec![0.0f64; total + shift.unsigned_abs() as usize + 8];
        for (i, slot) in b_mono.iter_mut().enumerate() {
            *slot = noise(77, i) * 0.002;
        }
        for (i, &v) in a.iter().enumerate().take(total).skip(gap_end) {
            b_mono[i] = f64::from(v);
        }
        for (f, &v) in a.iter().enumerate().take(speech_frames) {
            let bi = (f as i64 + shift) as usize;
            b_mono[bi] = f64::from(v);
        }

        let refined = RefinedGapFrames {
            start_frame: gap_start,
            end_frame: gap_end,
        };
        let cfg = FingerprintConfig::default();
        let ladder = pre_window_ladder_at(&PreWindowLadderInput {
            a_samples: &a,
            channels: 1,
            refined,
            b_mono: &b_mono,
            b_mapped_start: gap_start,
            cfg: &cfg,
            sample_rate: RATE,
        })
        .expect("ladder measured");

        let rate = f64::from(RATE);
        let shift_ms = shift as f64 * 1000.0 / rate;
        let rung = |ms: u32| {
            ladder
                .iter()
                .find(|r| r.window_ms == ms)
                .unwrap_or_else(|| panic!("{ms}ms rung present"))
        };
        let r250 = rung(250);
        let r1000 = rung(1000);
        assert!(
            (r250.pre_lag_ms - shift_ms).abs() > 5.0,
            "250ms rung (all quiet) should NOT land on the true +{shift_ms}ms shift, got {}",
            r250.pre_lag_ms
        );
        assert!(
            (r1000.pre_lag_ms - shift_ms).abs() < 1.0,
            "1000ms rung (reaches the speech) should snap to the true +{shift_ms}ms shift, got {}",
            r1000.pre_lag_ms
        );
    }

    /// Acceptance criterion (§5): `pre_global_r` must be the true post-lock conjunct, not lag 0 —
    /// pin a fixture where a real step makes the two answers differ. B carries A's pre content only
    /// at a `+step_ms` shift beyond the post lock's own (zero) offset, so scoring the pre template at
    /// the post-lock conjunct (this rung's actual job) reads near the true correlation while a lag-0
    /// score at the untouched nominal position would read near zero.
    #[test]
    fn pre_window_ladder_global_r_uses_the_post_lock_conjunct_not_lag_zero() {
        const RATE: u32 = 48_000;
        let pre_len = frames_at_ms(RATE, 1000);
        let gap_frames = frames_at_ms(RATE, 800);
        let post_len = frames_at_ms(RATE, 1000);
        let gap_start = pre_len;
        let gap_end = gap_start + gap_frames;
        let post_lag = frames_at_ms(RATE, 60) as i64;
        let total = gap_end + post_len;

        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, 0, gap_start, 5, true);
        write_shaped_noise(&mut a, gap_end, total, 23, false);

        // B: post shoulder's true match sits at `post_lag`, not nominal, so the post lock reports a
        // nonzero lag and the true post-lock conjunct (b_post_seam − gap_frames = gap_start + post_lag)
        // differs from the nominal/lag-zero position (gap_start). A's pre content is placed in B only
        // at that shifted conjunct, so a lag-zero read of B's pre region finds nothing but noise.
        let mut b_mono = vec![0.0f64; total + post_lag as usize + 8];
        for (i, slot) in b_mono.iter_mut().enumerate() {
            *slot = noise(88, i) * 0.001;
        }
        for (f, &v) in a.iter().enumerate().take(total).skip(gap_end) {
            let bi = (f as i64 + post_lag) as usize;
            b_mono[bi] = f64::from(v);
        }
        for (f, &v) in a.iter().enumerate().take(gap_start) {
            let bi = (f as i64 + post_lag) as usize;
            b_mono[bi] = f64::from(v);
        }

        let refined = RefinedGapFrames {
            start_frame: gap_start,
            end_frame: gap_end,
        };
        let cfg = FingerprintConfig::default();
        let ladder = pre_window_ladder_at(&PreWindowLadderInput {
            a_samples: &a,
            channels: 1,
            refined,
            b_mono: &b_mono,
            b_mapped_start: gap_start,
            cfg: &cfg,
            sample_rate: RATE,
        })
        .expect("ladder measured");
        let r250 = ladder
            .iter()
            .find(|r| r.window_ms == 250)
            .expect("250ms rung present");

        assert!(
            r250.pre_global_r > 0.8,
            "pre_global_r must score at the true post-lock conjunct, not lag 0: got {}",
            r250.pre_global_r
        );
    }

    #[test]
    fn splice_summary_from_lag_decision_mono() {
        let lag = LagFingerprint {
            pre_anchor: vec![LagSummary {
                window_ms: 1000,
                max_lag_ms: 200,
                channel: LagChannel::Mono,
                lag0_r: Some(0.1),
                peak_r: 0.99,
                second_peak_r: Some(0.2),
                peak_z: Some(16.0),
                prominence: Some(0.79),
                top2_spacing_ms: Some(40.0),
                peak_lag_samples: -500,
                frac_lag_samples: -500.0,
                frac_lag_ms: -10.5,
                edge_pinned: Some(false),
                border_displacement_ms: None,
                envelope_window_ms: None,
                envelope_lag_ms: None,
                envelope_peak_r: None,
                envelope_edge_pinned: None,
                envelope_rescue: None,
                verdict: LagVerdict::TimingOffset,
            }],
            post_anchor: vec![LagSummary {
                window_ms: 1000,
                max_lag_ms: 200,
                channel: LagChannel::Mono,
                lag0_r: Some(0.2),
                peak_r: 0.96,
                second_peak_r: Some(0.3),
                peak_z: Some(14.0),
                prominence: Some(0.66),
                top2_spacing_ms: Some(50.0),
                peak_lag_samples: -300,
                frac_lag_samples: -300.0,
                frac_lag_ms: -6.2,
                edge_pinned: Some(false),
                border_displacement_ms: None,
                envelope_window_ms: None,
                envelope_lag_ms: None,
                envelope_peak_r: None,
                envelope_edge_pinned: None,
                envelope_rescue: None,
                verdict: LagVerdict::TimingOffset,
            }],
        };
        let s = splice_summary_from_lag(&lag).expect("splice");
        assert!((s.step_ms - 4.3).abs() < 0.01, "step {}", s.step_ms);
        assert!((s.pre_peak_r - 0.99).abs() < 1e-6);
        assert!((s.post_peak_r - 0.96).abs() < 1e-6);
        assert_eq!(s.pre_peak_z, Some(16.0));
        assert_eq!(s.post_peak_z, Some(14.0));
        // Neither shoulder search-exhausted → step is trustworthy.
        assert_eq!(s.edge_pinned, Some(false));

        // One edge-pinned shoulder taints the step (either side ⇒ true).
        let mut pinned = lag.clone();
        pinned.post_anchor[0].edge_pinned = Some(true);
        assert_eq!(
            splice_summary_from_lag(&pinned)
                .expect("splice")
                .edge_pinned,
            Some(true),
        );

        // No shoulder carries the flag (old fingerprint) → unknown, not a false negative.
        let mut legacy = lag.clone();
        legacy.pre_anchor[0].edge_pinned = None;
        legacy.post_anchor[0].edge_pinned = None;
        assert_eq!(
            splice_summary_from_lag(&legacy)
                .expect("splice")
                .edge_pinned,
            None,
        );
    }

    #[test]
    fn edge_pinned_flags_boundary_peak() {
        // A curve whose maximum sits at the top boundary lag is search-exhausted (true optimum ≥ edge).
        let sr = 48_000u32;
        let curve: Vec<(i64, f64)> = (-4800..=4800).map(|l| (l, l as f64 / 4800.0)).collect();
        let s = summarize_lag_curve(&curve, sr, 1000, 100, LagChannel::Mono).expect("summary");
        assert_eq!(s.peak_lag_samples, 4800, "peak at the boundary");
        assert_eq!(s.edge_pinned, Some(true), "boundary peak is edge-pinned");

        // A curve peaking in the interior is not edge-pinned.
        let interior: Vec<(i64, f64)> = (-4800..=4800)
            .map(|l| (l, -((l as f64) / 4800.0).powi(2)))
            .collect();
        let s2 = summarize_lag_curve(&interior, sr, 1000, 100, LagChannel::Mono).expect("summary");
        assert_eq!(s2.peak_lag_samples, 0, "peak in the interior");
        assert_eq!(s2.edge_pinned, Some(false), "interior peak not edge-pinned");
    }

    #[test]
    fn wide_envelope_finds_shifted_segment_peak() {
        let rate = 48_000u32;
        let r = f64::from(rate);
        let max_lag = ((WIDE_ENV_MAX_LAG_MS / 1000.0) * r).round() as i64;
        let window = (WIDE_ENV_WINDOW_SECS * r).round() as usize;
        let env_bin = ((WIDE_ENV_BIN_MS / 1000.0) * r).round() as usize;
        // Offset must be visible at 100 ms-bin resolution (~1 bin = 100 ms).
        let offset_samples = env_bin as f64;
        let (a, b_ctx) = shifted_pair(offset_samples, window, max_lag);
        let wide_lag = max_lag as usize;
        let peak = wide_envelope_side(&a, &b_ctx, rate, env_bin, wide_lag).expect("env peak");
        assert!(peak.peak_r > 0.9, "segment match {}", peak.peak_r);
        let expected_ms = offset_samples * 1000.0 / r;
        assert!(
            (peak.peak_lag_ms - expected_ms).abs() < 50.0,
            "env peak lag {} should land near {expected_ms} ms",
            peak.peak_lag_ms
        );
        // Non-vacuous: `finite_corr` guarantees finite outputs even on a flat/degenerate envelope.
        assert!(
            peak.prominence.is_finite() && peak.peak_r.is_finite(),
            "finite: {peak:?}"
        );
    }
}
