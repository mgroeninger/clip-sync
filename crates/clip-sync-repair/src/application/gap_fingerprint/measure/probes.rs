//! `probes` slice of the gap_fingerprint measure folder split.

use crate::application::gap_fingerprint::schema::*;

use clip_sync::normalized_correlation;

use crate::domain::lag_envelope::EnvelopeCfg;
use crate::domain::policies::{border_templates_for_gap, RefinedGapFrames};
use crate::domain::seam_locks::{
    nominal_first_seam_locks, wide_a_slicer, SeamGateFloors, SeamLockTemplates,
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
/// `None` when either seam window falls out of range. On success, [`SpliceDualfitMeasure::dualfit`]
/// is the wire row; `pre_lag_frames` is the 250 ms pre lock the keep-path shift probe reads and is
/// **not** serialized on `SpliceDualfit`.
pub(in crate::application::gap_fingerprint::measure) struct SpliceDualfitMeasure {
    pub(in crate::application::gap_fingerprint::measure) dualfit: SpliceDualfit,
    pub(in crate::application::gap_fingerprint::measure) pre_lag_frames: i64,
}

impl std::ops::Deref for SpliceDualfitMeasure {
    type Target = SpliceDualfit;
    fn deref(&self) -> &Self::Target {
        &self.dualfit
    }
}

pub(in crate::application::gap_fingerprint::measure) fn splice_dualfit_at(
    input: &SpliceDualfitInput<'_>,
) -> Option<SpliceDualfitMeasure> {
    let SpliceDualfitInput {
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
        env_cfg,
        Some(&wide_a),
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
    let pre_seam_prom =
        (window >= 8 && b_pre_seam >= window + mlu && b_pre_seam + mlu <= b_mono.len())
            .then(|| {
                seam_prominence(
                    &a_pre,
                    &b_mono[b_pre_seam - window - mlu..b_pre_seam + mlu],
                    ml,
                    sample_rate,
                )
            })
            .flatten();
    let post_seam_prom =
        (window >= 8 && b_post_seam >= mlu && b_post_seam + window + mlu <= b_mono.len())
            .then(|| {
                seam_prominence(
                    &a_post,
                    &b_mono[b_post_seam - mlu..b_post_seam + window + mlu],
                    ml,
                    sample_rate,
                )
            })
            .flatten();

    Some(SpliceDualfitMeasure {
        dualfit: SpliceDualfit {
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
            pre_seam_rescued: Some(locks.pre.rescued),
            post_seam_rescued: Some(locks.post.rescued),
            pre_seam_edge_pinned: Some(locks.pre.edge_pinned),
            post_seam_edge_pinned: Some(locks.post.edge_pinned),
            seams_clamped: Some(locks.clamped()),
        },
        pre_lag_frames: locks.pre.lag,
    })
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
