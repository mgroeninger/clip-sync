//! Pearson seam scoring (repeat, splice-aware, FFT band) and channel diagnostics.
//!
//! The correlation half of the seam gate: how well a candidate B fill's shoulders match A's
//! borders, and which channels are energetic enough to be worth scoring. `seam_residual` measures
//! the complementary cancellation half on top of `seam_pearson` from here.
use crate::domain::metrics::normalized_correlation;
use crate::domain::pcm::interleaved_to_mono;
use crate::domain::seam_local::seam_correlation_over_bases;

pub(crate) fn template_mean_square(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64
}

/// A-side channels that carry seam signal — those within ~20 dB of the loudest channel's
/// energy. Lets seam scoring follow the channel(s) that actually hold content (e.g. a
/// center-dominant 5.1 mix where front L/R are near-silent) instead of assuming front L/R.
/// Returns empty when every channel is near-silent, so the caller falls back to the mono mix.
pub(crate) fn seam_score_channel_indices(
    a_pre_ch: &[Vec<f64>],
    a_post_ch: &[Vec<f64>],
) -> Vec<usize> {
    let n = a_pre_ch.len().min(a_post_ch.len());
    if n == 0 {
        return Vec::new();
    }
    let energy: Vec<f64> = (0..n)
        .map(|ch| template_mean_square(&a_pre_ch[ch]).max(template_mean_square(&a_post_ch[ch])))
        .collect();
    let max_energy = energy.iter().copied().fold(0.0, f64::max);
    if max_energy <= f64::EPSILON {
        return Vec::new();
    }
    // Mean-square ratio 0.01 ≈ −20 dB in amplitude: a channel must carry meaningful signal
    // relative to the loudest to be scored, so silent surrounds/LFE never veto a good splice.
    let threshold = max_energy * 0.01;
    (0..n).filter(|&ch| energy[ch] >= threshold).collect()
}

pub(crate) fn seam_pearson(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    // Pearson r is invariant to positive per-vector scaling; `normalized_correlation` mean-centers
    // and divides by RMS — peak normalization before it was redundant (G2).
    normalized_correlation(left, right)
}

fn best_channel_correlation(scores: &[f64]) -> f64 {
    scores.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

/// Borrowed A-side border templates and B-side haystack audio for seam correlation.
pub struct SeamTemplates<'a> {
    pub a_pre: &'a [f64],
    pub a_post: &'a [f64],
    pub a_pre_ch: &'a [Vec<f64>],
    pub a_post_ch: &'a [Vec<f64>],
    pub b_mono: &'a [f64],
    pub b_ch: &'a [Vec<f64>],
}

/// Candidate fill placement evaluated by the seam gate.
#[derive(Clone, Copy)]
pub struct SeamPlacement {
    pub start: usize,
    pub gap_frames: usize,
    pub pre_window: usize,
    pub post_window: usize,
}

/// Cap repeat-correlation window length for short gaps and seam-adjacent scoring.
///
/// Without this, `repeat_window_frames` from border discovery (often 2 s via
/// `min_border_discovery_secs`) exceeds short gap brackets and `a_post.len()`,
/// disabling `repeat_post` entirely, or comparing `a_post` against fill plus
/// haystack past the bracket end.
fn effective_repeat_window_frames(
    repeat_window_frames: usize,
    gap_frames: usize,
    border_len: usize,
    seam_window: usize,
) -> usize {
    let mut w = repeat_window_frames.max(1);
    if gap_frames > 0 {
        w = w.min(gap_frames);
    }
    if border_len > 0 {
        w = w.min(border_len);
    }
    if seam_window > 0 {
        w = w.min(seam_window);
    }
    w.max(1)
}

/// Pearson correlation of A border templates with B fill interior (repeat-at-seam detector).
pub fn fill_repeat_correlations(
    templates: &SeamTemplates<'_>,
    placement: SeamPlacement,
    repeat_window_frames: usize,
) -> (f64, f64) {
    let SeamTemplates {
        a_pre,
        a_post,
        a_pre_ch,
        a_post_ch,
        b_mono,
        b_ch,
    } = *templates;
    let SeamPlacement {
        start,
        gap_frames,
        pre_window,
        post_window,
    } = placement;

    let pre_repeat_window =
        effective_repeat_window_frames(repeat_window_frames, gap_frames, a_pre.len(), pre_window);
    let post_repeat_window =
        effective_repeat_window_frames(repeat_window_frames, gap_frames, a_post.len(), post_window);

    let repeat_pre = if !a_pre.is_empty()
        && start + pre_repeat_window <= b_mono.len()
        && pre_repeat_window <= a_pre.len()
    {
        seam_pearson(
            &a_pre[a_pre.len().saturating_sub(pre_repeat_window)..],
            &b_mono[start..start + pre_repeat_window],
        )
    } else {
        0.0
    };

    let tail_start = start + gap_frames.saturating_sub(post_repeat_window);
    let repeat_post = if !a_post.is_empty()
        && tail_start + post_repeat_window <= b_mono.len()
        && post_repeat_window <= a_post.len()
    {
        seam_pearson(
            &a_post[..post_repeat_window],
            &b_mono[tail_start..tail_start + post_repeat_window],
        )
    } else {
        0.0
    };

    if b_ch.len() <= 1 {
        return (repeat_pre, repeat_post);
    }

    let ch_pre = if start + pre_repeat_window <= b_mono.len() {
        a_pre_ch
            .iter()
            .zip(b_ch.iter())
            .filter_map(|(a_ch, b_ch)| {
                let border_len = a_ch.len();
                let w = effective_repeat_window_frames(
                    repeat_window_frames,
                    gap_frames,
                    border_len,
                    pre_window,
                );
                if w <= border_len && start + w <= b_ch.len() {
                    Some(seam_pearson(
                        &a_ch[border_len.saturating_sub(w)..],
                        &b_ch[start..start + w],
                    ))
                } else {
                    None
                }
            })
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        f64::NEG_INFINITY
    };

    let ch_post = if tail_start + post_repeat_window <= b_mono.len() {
        a_post_ch
            .iter()
            .zip(b_ch.iter())
            .filter_map(|(a_ch, b_ch)| {
                let border_len = a_ch.len();
                let w = effective_repeat_window_frames(
                    repeat_window_frames,
                    gap_frames,
                    border_len,
                    post_window,
                );
                let tail = start + gap_frames.saturating_sub(w);
                if w <= border_len && tail + w <= b_ch.len() {
                    Some(seam_pearson(&a_ch[..w], &b_ch[tail..tail + w]))
                } else {
                    None
                }
            })
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        f64::NEG_INFINITY
    };

    (
        best_channel_correlation(&[repeat_pre, ch_pre]),
        best_channel_correlation(&[repeat_post, ch_post]),
    )
}

/// One side (pre or post) of [`fill_repeat_correlations_band`], mirroring the corresponding half of
/// [`fill_repeat_correlations`] exactly. `tail = true` is the PRE side (template is `a`'s tail, B base is
/// `start`); `tail = false` is the POST side (template is `a`'s head, B base is `start + gap_frames − w`).
/// The base offset is what makes one helper serve both, and it is **per-window**: the mono side offsets by the
/// mono `w`, each channel by its own.
///
/// Returns `None` when a start-dependent bound is not uniform across `[start_lo, start_hi]` (the caller then
/// scores the band naively), matching [`fill_seam_correlations_band`]'s decline contract.
#[allow(clippy::too_many_arguments)]
fn repeat_side_band(
    a_mono: &[f64],
    a_ch: &[Vec<f64>],
    b_mono: &[f64],
    b_ch: &[Vec<f64>],
    mono_window: usize,
    repeat_window_frames: usize,
    gap_frames: usize,
    seam_window: usize,
    tail: bool,
    start_lo: usize,
    start_hi: usize,
    width: usize,
) -> Option<Vec<f64>> {
    // The POST side compares A's head against the fill's TAIL, so its B base trails `start` by
    // `gap_frames − w`; the PRE side reads at `start` itself.
    let base_offset = |w: usize| {
        if tail {
            0
        } else {
            gap_frames.saturating_sub(w)
        }
    };
    let mono_offset = base_offset(mono_window);

    // The one start-dependent term shared by the mono scoring gate (`:127` / `:139`) and the OUTER channel gate
    // (`:155` / `:181`) — both phrased against the MONO window and `b_mono`, even though the outer one gates
    // per-channel work. Monotonic in `start`, so uniform across the band iff it agrees at both ends.
    let fit_lo = start_lo + mono_offset + mono_window <= b_mono.len();
    let fit_hi = start_hi + mono_offset + mono_window <= b_mono.len();
    if fit_lo != fit_hi {
        return None;
    }
    let fit = fit_lo;

    // Mono band. The remaining conjuncts (`!a.is_empty()`, `w <= a.len()`) are start-independent; when they
    // fail the naive path yields a literal `0.0` (NOT `NEG_INFINITY` — that value is the channel set's).
    let score_mono = !a_mono.is_empty() && fit && mono_window <= a_mono.len();
    let mono_band = if score_mono {
        let template: &[f64] = if tail {
            &a_mono[a_mono.len() - mono_window..]
        } else {
            &a_mono[..mono_window]
        };
        let band = seam_correlation_over_bases(
            template,
            b_mono,
            start_lo + mono_offset,
            start_hi + mono_offset,
        );
        if band.len() != width {
            return None;
        }
        band
    } else {
        vec![0.0; width]
    };

    // `fill_repeat_correlations` returns the mono pair outright for mono/1-channel media (`:151-153`), never
    // reaching the channel fold — so the band must not synthesize a `NEG_INFINITY` channel term here either.
    if b_ch.len() <= 1 {
        return Some(mono_band);
    }

    // Outer gate failed ⇒ the whole channel set is `NEG_INFINITY` for every start, regardless of whether an
    // individual channel's (possibly SHORTER) window would have fit. Reproducing that is the point.
    let ch_band_max: Vec<f64> = if !fit {
        vec![f64::NEG_INFINITY; width]
    } else {
        let mut bands: Vec<Vec<f64>> = Vec::new();
        for (a_c, b_c) in a_ch.iter().zip(b_ch.iter()) {
            let border_len = a_c.len();
            let w = effective_repeat_window_frames(
                repeat_window_frames,
                gap_frames,
                border_len,
                seam_window,
            );
            if w > border_len {
                continue; // start-independent exclusion — matches the naive `filter_map`
            }
            let off = base_offset(w);
            let lo_ok = start_lo + off + w <= b_c.len();
            let hi_ok = start_hi + off + w <= b_c.len();
            if lo_ok != hi_ok {
                return None; // channel would be scored for some starts, skipped for others
            }
            if !hi_ok {
                continue;
            }
            let template: &[f64] = if tail {
                &a_c[border_len - w..]
            } else {
                &a_c[..w]
            };
            let band = seam_correlation_over_bases(template, b_c, start_lo + off, start_hi + off);
            if band.len() != width {
                return None;
            }
            bands.push(band);
        }
        (0..width)
            .map(|i| bands.iter().map(|b| b[i]).fold(f64::NEG_INFINITY, f64::max))
            .collect()
    };

    // Mono is a PARTICIPANT in the max here, not a fallback used only when no channel scored — that is the
    // seam band's `combine_seam_band` rule, and it is the wrong one for repeat.
    Some(
        (0..width)
            .map(|i| best_channel_correlation(&[mono_band[i], ch_band_max[i]]))
            .collect(),
    )
}

/// Lever 1b(b) (`TEMP-repeat-band-plan.md` §2): precompute `(repeat_pre, repeat_post)` for **every** `start` in
/// `[start_lo, start_hi]` in one FFT band pass per channel per side, mirroring [`fill_repeat_correlations`]
/// exactly. Entry `i` corresponds to `start_lo + i` and equals the per-start call within FFT ε (≤ 1e-8;
/// naive-exact below the FFT crossover).
///
/// This is the repeat-window twin of [`fill_seam_correlations_band`], which banded the *seam* window only. It
/// is a separate window, so it needs its own pass — and it differs from the seam band in ways a copy-paste
/// would get wrong (see `TEMP-repeat-band-plan.md` §2.1): no `score_channels` filter (repeat scores **all**
/// channels), per-channel window lengths that differ within one call, an outer channel gate phrased against
/// the mono window, and `0.0`-vs-`NEG_INFINITY` failure values that are not interchangeable.
///
/// Returns `None` on the same contract as the seam band: any start-dependent bound that is not uniform across
/// the band, or a band that does not fit. Correctness is further guaranteed downstream by the exact re-score
/// of the winning placement.
// Wired into the production start-search refine by `gap_fill_fit::build_repeat_band` (plan §3).
pub(crate) fn fill_repeat_correlations_band(
    templates: &SeamTemplates<'_>,
    gap_frames: usize,
    pre_window: usize,
    post_window: usize,
    repeat_window_frames: usize,
    start_lo: usize,
    start_hi: usize,
) -> Option<Vec<(f64, f64)>> {
    if start_hi < start_lo {
        return None;
    }
    let width = start_hi - start_lo + 1;
    let SeamTemplates {
        a_pre,
        a_post,
        a_pre_ch,
        a_post_ch,
        b_mono,
        b_ch,
    } = *templates;

    // Start-independent (this is the precondition that makes banding possible at all): the effective windows
    // derive only from the gap/border/seam geometry, none of which moves with `start`.
    let pre_repeat_window =
        effective_repeat_window_frames(repeat_window_frames, gap_frames, a_pre.len(), pre_window);
    let post_repeat_window =
        effective_repeat_window_frames(repeat_window_frames, gap_frames, a_post.len(), post_window);

    let pre = repeat_side_band(
        a_pre,
        a_pre_ch,
        b_mono,
        b_ch,
        pre_repeat_window,
        repeat_window_frames,
        gap_frames,
        pre_window,
        true,
        start_lo,
        start_hi,
        width,
    )?;
    let post = repeat_side_band(
        a_post,
        a_post_ch,
        b_mono,
        b_ch,
        post_repeat_window,
        repeat_window_frames,
        gap_frames,
        post_window,
        false,
        start_lo,
        start_hi,
        width,
    )?;
    Some(pre.into_iter().zip(post).collect())
}

/// A-side gap bounds for splice-aware seam scoring on decoded PCM.
#[derive(Clone, Copy)]
pub struct SpliceSeamContext<'a> {
    pub seam_cf: usize,
    pub gap_start_frame: usize,
    pub gap_end_frame: usize,
    pub a_samples: &'a [f32],
    pub channels: usize,
    /// True for an ordinary (single rigid-lag) splice, where the fill genuinely sits at
    /// `gap_start_frame`/`gap_end_frame` with no lag correction — so the crossfade-window scoring
    /// below (checking the literal blended overlap against A's raw neighboring samples) is valid.
    /// False for a **dual-fit** fill, whose pre/post shoulders were independently matched at their
    /// own seam-local lags (that's the entire point of dual-fit: no single lag satisfies both
    /// seams) — comparing the fill's head/tail against raw A at lag 0 there is a category error,
    /// so scoring falls back to the border-window template dual-fit's own seam-local search
    /// already validated against.
    pub single_lag_alignment: bool,
}

fn mono_timeline_frames_f64(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> Vec<f64> {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let start = start_frame.min(total_frames);
    let end = end_frame.min(total_frames);
    if start >= end {
        return Vec::new();
    }
    samples[start * channels..end * channels]
        .iter()
        .step_by(channels)
        .map(|&s| s as f64)
        .collect()
}

/// Seam scores for a fitted fill chunk aligned with [`apply_seam_crossfade`] placement.
///
/// When `ctx.seam_cf > 0`, scores the A PCM windows that are actually crossfaded at splice time.
pub fn fill_splice_seam_correlations(
    fill_mono: &[f64],
    a_pre: &[f64],
    a_post: &[f64],
    pre_window: usize,
    post_window: usize,
    ctx: SpliceSeamContext<'_>,
) -> (f64, f64) {
    (
        score_splice_pre_seam(fill_mono, a_pre, pre_window, ctx),
        score_splice_post_seam(fill_mono, a_post, post_window, ctx),
    )
}

fn score_splice_pre_seam(
    fill_mono: &[f64],
    a_pre: &[f64],
    pre_window: usize,
    ctx: SpliceSeamContext<'_>,
) -> f64 {
    if pre_window == 0 || fill_mono.is_empty() {
        return 0.0;
    }
    if ctx.single_lag_alignment && ctx.seam_cf > 0 && ctx.gap_start_frame > 0 {
        let w = pre_window
            .min(ctx.seam_cf)
            .min(ctx.gap_start_frame)
            .min(fill_mono.len());
        if w > 0 {
            let b = &fill_mono[ctx.seam_cf - w..ctx.seam_cf];
            let a = mono_timeline_frames_f64(
                ctx.a_samples,
                ctx.channels,
                ctx.gap_start_frame - w,
                ctx.gap_start_frame,
            );
            if a.len() == b.len() {
                return seam_pearson(&a, b);
            }
        }
    }
    score_splice_pre_seam_border(fill_mono, a_pre, pre_window)
}

fn score_splice_pre_seam_border(fill_mono: &[f64], a_pre: &[f64], pre_window: usize) -> f64 {
    if a_pre.is_empty() {
        return 0.0;
    }
    let w = pre_window.min(fill_mono.len()).min(a_pre.len());
    if w == 0 {
        return 0.0;
    }
    seam_pearson(&a_pre[a_pre.len() - w..], &fill_mono[..w])
}

fn score_splice_post_seam(
    fill_mono: &[f64],
    a_post: &[f64],
    post_window: usize,
    ctx: SpliceSeamContext<'_>,
) -> f64 {
    if post_window == 0 || fill_mono.is_empty() {
        return 0.0;
    }
    let len = fill_mono.len();
    let total_frames = ctx.a_samples.len() / ctx.channels.max(1);
    if ctx.single_lag_alignment && ctx.seam_cf > 0 && ctx.gap_end_frame < total_frames {
        let w = post_window
            .min(ctx.seam_cf)
            .min(total_frames.saturating_sub(ctx.gap_end_frame))
            .min(len.saturating_sub(len.saturating_sub(ctx.seam_cf)));
        if w > 0 {
            let b_start = len.saturating_sub(ctx.seam_cf);
            let b_end = (b_start + w).min(len);
            let b = &fill_mono[b_start..b_end];
            let a = mono_timeline_frames_f64(
                ctx.a_samples,
                ctx.channels,
                ctx.gap_end_frame,
                ctx.gap_end_frame + b.len(),
            );
            if a.len() == b.len() {
                return seam_pearson(&a, b);
            }
        }
    }
    score_splice_post_seam_border(fill_mono, a_post, post_window)
}

fn score_splice_post_seam_border(fill_mono: &[f64], a_post: &[f64], post_window: usize) -> f64 {
    if a_post.is_empty() {
        return 0.0;
    }
    let len = fill_mono.len();
    let w = post_window.min(len).min(a_post.len());
    if w == 0 {
        return 0.0;
    }
    seam_pearson(&a_post[..w], &fill_mono[len - w..])
}

/// A-side border templates and seam window sizes for splice scoring on decoded fill PCM.
pub struct BorderSeamTemplates<'a> {
    pub a_pre: &'a [f64],
    pub a_post: &'a [f64],
    pub a_pre_ch: &'a [Vec<f64>],
    pub a_post_ch: &'a [Vec<f64>],
    pub pre_window: usize,
    pub post_window: usize,
}

/// Like [`fill_splice_seam_correlations`] but scores each channel when stereo borders are present.
pub fn fill_splice_seam_correlations_interleaved(
    fill_interleaved: &[f32],
    channels: usize,
    borders: &BorderSeamTemplates<'_>,
    ctx: SpliceSeamContext<'_>,
) -> (f64, f64) {
    let BorderSeamTemplates {
        a_pre,
        a_post,
        a_pre_ch,
        a_post_ch,
        pre_window,
        post_window,
    } = *borders;
    let channels = channels.max(1);
    let fill_mono = interleaved_to_mono(fill_interleaved, channels);
    let use_channels = channels > 1
        && a_pre_ch.len() == channels
        && a_post_ch.len() == channels
        && a_pre_ch.iter().any(|ch| !ch.is_empty());
    if !use_channels {
        return fill_splice_seam_correlations(
            &fill_mono,
            a_pre,
            a_post,
            pre_window,
            post_window,
            ctx,
        );
    }

    let gap_frames = fill_mono.len();
    let score_channels = seam_score_channel_indices(a_pre_ch, a_post_ch);
    let mut pre_scores = Vec::with_capacity(score_channels.len());
    let mut post_scores = Vec::with_capacity(score_channels.len());
    for &ch in &score_channels {
        let ch_fill: Vec<f64> = fill_interleaved
            .iter()
            .skip(ch)
            .step_by(channels)
            .map(|&s| s as f64)
            .collect();
        if ch_fill.len() != gap_frames {
            continue;
        }
        let pre = score_splice_pre_seam_channel(&ch_fill, &a_pre_ch[ch], pre_window, ctx, ch);
        if pre.is_finite() && pre > f64::NEG_INFINITY {
            pre_scores.push(pre);
        }
        let post = score_splice_post_seam_channel(&ch_fill, &a_post_ch[ch], post_window, ctx, ch);
        if post.is_finite() && post > f64::NEG_INFINITY {
            post_scores.push(post);
        }
    }

    let mono =
        fill_splice_seam_correlations(&fill_mono, a_pre, a_post, pre_window, post_window, ctx);
    let pre = if pre_scores.is_empty() {
        mono.0
    } else {
        best_channel_correlation(&pre_scores)
    };
    let post = if post_scores.is_empty() {
        mono.1
    } else {
        best_channel_correlation(&post_scores)
    };
    (pre, post)
}

pub(crate) fn interleaved_channel_timeline_f64(
    samples: &[f32],
    channels: usize,
    channel: usize,
    start_frame: usize,
    end_frame: usize,
) -> Vec<f64> {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let start = start_frame.min(total_frames);
    let end = end_frame.min(total_frames);
    if start >= end {
        return Vec::new();
    }
    (start..end)
        .map(|frame| samples[frame * channels + channel] as f64)
        .collect()
}

fn score_splice_pre_seam_channel(
    fill_mono: &[f64],
    a_pre: &[f64],
    pre_window: usize,
    ctx: SpliceSeamContext<'_>,
    channel: usize,
) -> f64 {
    if pre_window == 0 || fill_mono.is_empty() {
        return 0.0;
    }
    if ctx.single_lag_alignment && ctx.seam_cf > 0 && ctx.gap_start_frame > 0 {
        let w = pre_window
            .min(ctx.seam_cf)
            .min(ctx.gap_start_frame)
            .min(fill_mono.len());
        if w > 0 {
            let b = &fill_mono[ctx.seam_cf - w..ctx.seam_cf];
            let a = interleaved_channel_timeline_f64(
                ctx.a_samples,
                ctx.channels,
                channel,
                ctx.gap_start_frame - w,
                ctx.gap_start_frame,
            );
            if a.len() == b.len() {
                return seam_pearson(&a, b);
            }
        }
    }
    score_splice_pre_seam_border(fill_mono, a_pre, pre_window)
}

fn score_splice_post_seam_channel(
    fill_mono: &[f64],
    a_post: &[f64],
    post_window: usize,
    ctx: SpliceSeamContext<'_>,
    channel: usize,
) -> f64 {
    if post_window == 0 || fill_mono.is_empty() {
        return 0.0;
    }
    let len = fill_mono.len();
    let total_frames = ctx.a_samples.len() / ctx.channels.max(1);
    if ctx.single_lag_alignment && ctx.seam_cf > 0 && ctx.gap_end_frame < total_frames {
        let w = post_window
            .min(ctx.seam_cf)
            .min(total_frames.saturating_sub(ctx.gap_end_frame))
            .min(len.saturating_sub(len.saturating_sub(ctx.seam_cf)));
        if w > 0 {
            let b_start = len.saturating_sub(ctx.seam_cf);
            let b_end = (b_start + w).min(len);
            let b = &fill_mono[b_start..b_end];
            let a = interleaved_channel_timeline_f64(
                ctx.a_samples,
                ctx.channels,
                channel,
                ctx.gap_end_frame,
                ctx.gap_end_frame + b.len(),
            );
            if a.len() == b.len() {
                return seam_pearson(&a, b);
            }
        }
    }
    score_splice_post_seam_border(fill_mono, a_post, post_window)
}

/// Pearson correlation at gap seams; uses best front L/R channel when `channels > 1`.
pub fn fill_seam_correlations(
    templates: &SeamTemplates<'_>,
    placement: SeamPlacement,
) -> (f64, f64) {
    let score_channels = seam_score_channel_indices(templates.a_pre_ch, templates.a_post_ch);
    fill_seam_correlations_with_channels(templates, placement, &score_channels)
}

/// Placement-invariant seam channel selection for a template set. Precompute once per search and pass to
/// [`fill_seam_correlations_with_channels`] so the per-candidate loop does not recompute it (perf lever 2,
/// TEMP-production-repair-perf-plan.md §2.3).
pub(crate) fn seam_score_channels(templates: &SeamTemplates<'_>) -> Vec<usize> {
    seam_score_channel_indices(templates.a_pre_ch, templates.a_post_ch)
}

/// [`fill_seam_correlations`] with the channel selection already computed. The selection depends only on
/// the A-side templates (not the placement), so the unified search hoists it out of the candidate loop.
/// Byte-identical to `fill_seam_correlations` when `score_channels == seam_score_channels(templates)`.
pub(crate) fn fill_seam_correlations_with_channels(
    templates: &SeamTemplates<'_>,
    placement: SeamPlacement,
    score_channels: &[usize],
) -> (f64, f64) {
    let SeamTemplates {
        a_pre,
        a_post,
        a_pre_ch,
        a_post_ch,
        b_mono,
        b_ch,
    } = *templates;
    let SeamPlacement {
        start,
        gap_frames,
        pre_window,
        post_window,
    } = placement;
    let use_channels = b_ch.len() > 1
        && a_pre_ch.len() == b_ch.len()
        && a_post_ch.len() == b_ch.len()
        && a_pre_ch.iter().any(|ch| !ch.is_empty());

    let score_pre =
        pre_window > 0 && !a_pre.is_empty() && start >= pre_window && start <= b_mono.len();
    let score_post =
        post_window > 0 && !a_post.is_empty() && start + gap_frames + post_window <= b_mono.len();

    if !use_channels {
        let pre = if score_pre {
            seam_pearson(
                &a_pre[a_pre.len().saturating_sub(pre_window)..],
                &b_mono[start - pre_window..start],
            )
        } else {
            0.0
        };
        let post = if score_post {
            seam_pearson(
                &a_post[..post_window.min(a_post.len())],
                &b_mono[start + gap_frames..start + gap_frames + post_window],
            )
        } else {
            0.0
        };
        return (pre, post);
    }

    let mut pre_scores = Vec::with_capacity(score_channels.len());
    let mut post_scores = Vec::with_capacity(score_channels.len());
    for &ch in score_channels {
        if score_pre && a_pre_ch[ch].len() >= pre_window && start <= b_ch[ch].len() {
            pre_scores.push(seam_pearson(
                &a_pre_ch[ch][a_pre_ch[ch].len() - pre_window..],
                &b_ch[ch][start - pre_window..start],
            ));
        }
        if score_post
            && a_post_ch[ch].len() >= post_window
            && start + gap_frames + post_window <= b_ch[ch].len()
        {
            post_scores.push(seam_pearson(
                &a_post_ch[ch][..post_window],
                &b_ch[ch][start + gap_frames..start + gap_frames + post_window],
            ));
        }
    }

    let pre = if pre_scores.is_empty() {
        if score_pre {
            seam_pearson(
                &a_pre[a_pre.len().saturating_sub(pre_window)..],
                &b_mono[start - pre_window..start],
            )
        } else {
            0.0
        }
    } else {
        best_channel_correlation(&pre_scores)
    };
    let post = if post_scores.is_empty() {
        if score_post {
            seam_pearson(
                &a_post[..post_window.min(a_post.len())],
                &b_mono[start + gap_frames..start + gap_frames + post_window],
            )
        } else {
            0.0
        }
    } else {
        best_channel_correlation(&post_scores)
    };

    (pre, post)
}

/// Lever 1 (TEMP-production-repair-perf-plan.md §2.5): precompute `(pre, post)` seam correlations for **every**
/// `start` in `[start_lo, start_hi]` (inclusive) in one FFT band pass per channel, mirroring
/// [`fill_seam_correlations_with_channels`] exactly — `use_channels` / bounds / per-channel selection /
/// `best_channel_correlation` / mono fallback are all reproduced. Entry `i` corresponds to `start_lo + i` and
/// equals the per-start call within FFT ε (≤ 1e-8; naive-exact below the FFT crossover).
///
/// Returns `None` when any start-dependent bound is **not uniform** across the band (a band-edge case where the
/// naive channel set would vary per start) or a band does not fit; the caller then scores that band the naive
/// per-candidate way. This keeps the FFT path to the interior where it provably matches naive — correctness is
/// further guaranteed downstream by the exact re-score of the winning placement.
// Wired into the unified start-search refine (`gap_fill_fit::build_wave_min_band`), §2.5 Part B.
pub(crate) fn fill_seam_correlations_band(
    templates: &SeamTemplates<'_>,
    gap_frames: usize,
    pre_window: usize,
    post_window: usize,
    score_channels: &[usize],
    start_lo: usize,
    start_hi: usize,
) -> Option<Vec<(f64, f64)>> {
    if start_hi < start_lo {
        return None;
    }
    let width = start_hi - start_lo + 1;
    let SeamTemplates {
        a_pre,
        a_post,
        a_pre_ch,
        a_post_ch,
        b_mono,
        b_ch,
    } = *templates;

    let use_channels = b_ch.len() > 1
        && a_pre_ch.len() == b_ch.len()
        && a_post_ch.len() == b_ch.len()
        && a_pre_ch.iter().any(|ch| !ch.is_empty());

    // --- PRE side. `score_pre`'s start-dependent parts (start >= pre_window, start <= b_mono.len()) are
    // monotonic in `start`, so they are uniform across the band iff they agree at both ends. ---
    let pre_ok_lo = start_lo >= pre_window && start_lo <= b_mono.len();
    let pre_ok_hi = start_hi >= pre_window && start_hi <= b_mono.len();
    if pre_ok_lo != pre_ok_hi {
        return None;
    }
    let score_pre = pre_window > 0 && !a_pre.is_empty() && pre_ok_lo;
    let mono_pre = mono_seam_band(
        score_pre,
        a_pre,
        b_mono,
        pre_window,
        start_lo.saturating_sub(pre_window),
        start_hi.saturating_sub(pre_window),
        true,
        width,
    )?;
    let mut pre_ch_bands: Vec<Vec<f64>> = Vec::new();
    if use_channels && score_pre {
        for &ch in score_channels {
            if a_pre_ch[ch].len() < pre_window {
                continue; // permanently excluded (start-independent) — matches naive
            }
            let lo_ok = start_lo <= b_ch[ch].len();
            let hi_ok = start_hi <= b_ch[ch].len();
            if lo_ok != hi_ok {
                return None; // channel would be scored for some starts, skipped for others
            }
            if !hi_ok {
                continue;
            }
            let band = seam_correlation_over_bases(
                &a_pre_ch[ch][a_pre_ch[ch].len() - pre_window..],
                &b_ch[ch],
                start_lo - pre_window,
                start_hi - pre_window,
            );
            if band.len() != width {
                return None;
            }
            pre_ch_bands.push(band);
        }
    }

    // --- POST side. ---
    let post_ok_lo = start_lo + gap_frames + post_window <= b_mono.len();
    let post_ok_hi = start_hi + gap_frames + post_window <= b_mono.len();
    if post_ok_lo != post_ok_hi {
        return None;
    }
    let score_post = post_window > 0 && !a_post.is_empty() && post_ok_lo;
    let mono_post = mono_seam_band(
        score_post,
        a_post,
        b_mono,
        post_window,
        start_lo + gap_frames,
        start_hi + gap_frames,
        false,
        width,
    )?;
    let mut post_ch_bands: Vec<Vec<f64>> = Vec::new();
    if use_channels && score_post {
        for &ch in score_channels {
            if a_post_ch[ch].len() < post_window {
                continue;
            }
            let lo_ok = start_lo + gap_frames + post_window <= b_ch[ch].len();
            let hi_ok = start_hi + gap_frames + post_window <= b_ch[ch].len();
            if lo_ok != hi_ok {
                return None;
            }
            if !hi_ok {
                continue;
            }
            let band = seam_correlation_over_bases(
                &a_post_ch[ch][..post_window],
                &b_ch[ch],
                start_lo + gap_frames,
                start_hi + gap_frames,
            );
            if band.len() != width {
                return None;
            }
            post_ch_bands.push(band);
        }
    }

    let pre = combine_seam_band(width, score_pre, &mono_pre, &pre_ch_bands);
    let post = combine_seam_band(width, score_post, &mono_post, &post_ch_bands);
    Some(pre.into_iter().zip(post).collect())
}

/// The mono seam correlation over a band, matching the mono branch / fallback of
/// [`fill_seam_correlations_with_channels`]: `0.0` for all starts when the seam is not scored or the template
/// is shorter than the window (naive `seam_pearson` returns 0.0 on unequal lengths); else the FFT band. `tail`
/// selects the pre-side tail vs the post-side head of `a`.
#[allow(clippy::too_many_arguments)]
fn mono_seam_band(
    score: bool,
    a: &[f64],
    b_mono: &[f64],
    window: usize,
    base_lo: usize,
    base_hi: usize,
    tail: bool,
    width: usize,
) -> Option<Vec<f64>> {
    if !score || a.len() < window {
        return Some(vec![0.0; width]);
    }
    let template: &[f64] = if tail {
        &a[a.len() - window..]
    } else {
        &a[..window]
    };
    let band = seam_correlation_over_bases(template, b_mono, base_lo, base_hi);
    (band.len() == width).then_some(band)
}

/// Per-start combine matching `fill_seam_correlations_with_channels`: with selected channels, take the best
/// (max) channel score; with none, fall back to the mono band when the seam is scored, else `0.0`.
fn combine_seam_band(width: usize, score: bool, mono: &[f64], ch_bands: &[Vec<f64>]) -> Vec<f64> {
    (0..width)
        .map(|i| {
            if ch_bands.is_empty() {
                if score {
                    mono[i]
                } else {
                    0.0
                }
            } else {
                ch_bands
                    .iter()
                    .map(|b| b[i])
                    .fold(f64::NEG_INFINITY, f64::max)
            }
        })
        .collect()
}

/// Per-channel seam diagnostics at a placement, for debug logging of multichannel scoring.
#[derive(Debug, Clone)]
pub struct SeamChannelDiagnostics {
    /// Energy-selected channels actually scored (see [`seam_score_channel_indices`]).
    pub selected: Vec<usize>,
    /// `(pre, post)` Pearson per channel index; `NaN` where the seam window did not fit.
    pub per_channel: Vec<(f64, f64)>,
    /// Mono downmix fallback `(pre, post)`.
    pub mono: (f64, f64),
}

/// Recompute per-channel seam correlations at `placement` without the best-channel reduction,
/// for debug diagnostics. Mirrors the windows used by [`fill_seam_correlations`].
pub fn seam_channel_diagnostics(
    templates: &SeamTemplates<'_>,
    placement: SeamPlacement,
) -> SeamChannelDiagnostics {
    let SeamTemplates {
        a_pre,
        a_post,
        a_pre_ch,
        a_post_ch,
        b_mono,
        b_ch,
    } = *templates;
    let SeamPlacement {
        start,
        gap_frames,
        pre_window,
        post_window,
    } = placement;

    let pre_fits = |len: usize| pre_window > 0 && start >= pre_window && start <= len;
    let post_fits = |len: usize| post_window > 0 && start + gap_frames + post_window <= len;

    let mut per_channel = Vec::with_capacity(b_ch.len());
    for ch in 0..b_ch.len() {
        let pre = if ch < a_pre_ch.len()
            && a_pre_ch[ch].len() >= pre_window
            && pre_fits(b_ch[ch].len())
        {
            seam_pearson(
                &a_pre_ch[ch][a_pre_ch[ch].len() - pre_window..],
                &b_ch[ch][start - pre_window..start],
            )
        } else {
            f64::NAN
        };
        let post = if ch < a_post_ch.len()
            && a_post_ch[ch].len() >= post_window
            && post_fits(b_ch[ch].len())
        {
            seam_pearson(
                &a_post_ch[ch][..post_window],
                &b_ch[ch][start + gap_frames..start + gap_frames + post_window],
            )
        } else {
            f64::NAN
        };
        per_channel.push((pre, post));
    }

    let mono_pre = if !a_pre.is_empty() && pre_fits(b_mono.len()) {
        seam_pearson(
            &a_pre[a_pre.len().saturating_sub(pre_window)..],
            &b_mono[start - pre_window..start],
        )
    } else {
        f64::NAN
    };
    let mono_post = if !a_post.is_empty() && post_fits(b_mono.len()) {
        seam_pearson(
            &a_post[..post_window.min(a_post.len())],
            &b_mono[start + gap_frames..start + gap_frames + post_window],
        )
    } else {
        f64::NAN
    };

    SeamChannelDiagnostics {
        selected: seam_score_channel_indices(a_pre_ch, a_post_ch),
        per_channel,
        mono: (mono_pre, mono_post),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det_noise(seed: u64, n: usize) -> Vec<f64> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 33) as f64 / (1u64 << 30) as f64) - 1.0
            })
            .collect()
    }

    /// The FFT band evaluator must reproduce the per-start naive `fill_seam_correlations_with_channels` at
    /// every start in the band, within FFT ε — for both the multichannel (best-of-selected) and mono paths.
    /// Sized so the pre band crosses the FFT crossover (exercises the accelerated branch end-to-end).
    #[test]
    fn fill_seam_correlations_band_matches_per_start() {
        let (pre_window, post_window, gap_frames) = (200usize, 220usize, 120usize);
        let total_b = 8000usize;
        let (start_lo, start_hi) = (1000usize, 6000usize); // interior; pre band width 5001 ⇒ FFT branch

        // --- multichannel (use_channels = true, score a subset) ---
        let nch = 3usize;
        let a_pre_ch: Vec<Vec<f64>> = (0..nch).map(|c| det_noise(100 + c as u64, 256)).collect();
        let a_post_ch: Vec<Vec<f64>> = (0..nch).map(|c| det_noise(200 + c as u64, 256)).collect();
        let b_ch: Vec<Vec<f64>> = (0..nch)
            .map(|c| det_noise(300 + c as u64, total_b))
            .collect();
        let a_pre = det_noise(1, 256);
        let a_post = det_noise(2, 256);
        let b_mono = det_noise(3, total_b);
        let score_channels = vec![0usize, 2usize];
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let band = fill_seam_correlations_band(
            &templates,
            gap_frames,
            pre_window,
            post_window,
            &score_channels,
            start_lo,
            start_hi,
        )
        .expect("band applies for interior starts");
        assert_eq!(band.len(), start_hi - start_lo + 1);
        for (i, &(pre, post)) in band.iter().enumerate() {
            let (npre, npost) = fill_seam_correlations_with_channels(
                &templates,
                SeamPlacement {
                    start: start_lo + i,
                    gap_frames,
                    pre_window,
                    post_window,
                },
                &score_channels,
            );
            assert!(
                (pre - npre).abs() < 1e-8 && (post - npost).abs() < 1e-8,
                "mc start {}: pre {pre} vs {npre}, post {post} vs {npost}",
                start_lo + i
            );
        }

        // --- mono (use_channels = false: no per-channel templates) ---
        let empty: Vec<Vec<f64>> = Vec::new();
        let mono_templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &empty,
            a_post_ch: &empty,
            b_mono: &b_mono,
            b_ch: &empty,
        };
        let mband = fill_seam_correlations_band(
            &mono_templates,
            gap_frames,
            pre_window,
            post_window,
            &[],
            start_lo,
            start_hi,
        )
        .expect("mono band applies");
        for (i, &(pre, post)) in mband.iter().enumerate() {
            let (npre, npost) = fill_seam_correlations_with_channels(
                &mono_templates,
                SeamPlacement {
                    start: start_lo + i,
                    gap_frames,
                    pre_window,
                    post_window,
                },
                &[],
            );
            assert!(
                (pre - npre).abs() < 1e-8 && (post - npost).abs() < 1e-8,
                "mono start {}: pre {pre} vs {npre}, post {post} vs {npost}",
                start_lo + i
            );
        }
    }

    /// `NEG_INFINITY` (the empty-channel-set sentinel) must match exactly — `inf − inf` is `NaN`, so the ε
    /// comparison would silently pass for any pair of infinities and, worse, for `NaN` vs `NaN`.
    fn repeat_eq(a: f64, b: f64) -> bool {
        if a.is_infinite() || b.is_infinite() {
            a == b
        } else {
            (a - b).abs() < 1e-8
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn assert_repeat_band_matches(
        templates: &SeamTemplates<'_>,
        gap_frames: usize,
        pre_window: usize,
        post_window: usize,
        repeat_window_frames: usize,
        start_lo: usize,
        start_hi: usize,
        label: &str,
    ) {
        let band = fill_repeat_correlations_band(
            templates,
            gap_frames,
            pre_window,
            post_window,
            repeat_window_frames,
            start_lo,
            start_hi,
        )
        .unwrap_or_else(|| panic!("{label}: band declined but the bounds are uniform"));
        assert_eq!(band.len(), start_hi - start_lo + 1, "{label}: band width");
        for (i, &(pre, post)) in band.iter().enumerate() {
            let start = start_lo + i;
            let (npre, npost) = fill_repeat_correlations(
                templates,
                SeamPlacement {
                    start,
                    gap_frames,
                    pre_window,
                    post_window,
                },
                repeat_window_frames,
            );
            assert!(
                repeat_eq(pre, npre) && repeat_eq(post, npost),
                "{label} start {start}: pre {pre} vs {npre}, post {post} vs {npost}"
            );
        }
    }

    /// Lever 1b(b): the repeat band must reproduce the per-start naive `fill_repeat_correlations` at every
    /// start in the band. Covers both `lag_correlation_curve_auto` branches (**asserted**, not assumed — an
    /// equivalence test that drifts onto the naive branch proves nothing about the FFT), per-channel windows
    /// that differ within a single call, and each gate/decline path in `TEMP-repeat-band-plan.md` §2.1.
    #[test]
    fn fill_repeat_correlations_band_matches_per_start() {
        use crate::domain::seam_local::FFT_CROSSOVER_OPS;

        let (gap_frames, pre_window, post_window, repeat_window_frames) = (500, 400, 400, 400);
        let total_b = 8000usize;

        // Unequal per-channel border lengths ⇒ unequal effective repeat windows within one call
        // (`effective_repeat_window_frames` caps at `border_len`): 120 / 200 / 260, vs the mono side's 300.
        let border_lens = [120usize, 200, 260];
        let a_pre_ch: Vec<Vec<f64>> = border_lens
            .iter()
            .enumerate()
            .map(|(c, &n)| det_noise(100 + c as u64, n))
            .collect();
        let a_post_ch: Vec<Vec<f64>> = border_lens
            .iter()
            .enumerate()
            .map(|(c, &n)| det_noise(200 + c as u64, n))
            .collect();
        let b_ch: Vec<Vec<f64>> = (0..border_lens.len())
            .map(|c| det_noise(300 + c as u64, total_b))
            .collect();
        let a_pre = det_noise(1, 300);
        let a_post = det_noise(2, 300);
        let b_mono = det_noise(3, total_b);

        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };

        // --- A. FFT branch. Smallest template (120) over a 5001-wide band still clears the crossover. ---
        let (lo, hi) = (1000usize, 6000usize);
        assert!(
            120u64 * (2 * (hi - lo) as u64 + 1) > FFT_CROSSOVER_OPS,
            "case A must exercise the FFT branch"
        );
        assert_repeat_band_matches(
            &templates,
            gap_frames,
            pre_window,
            post_window,
            repeat_window_frames,
            lo,
            hi,
            "fft",
        );

        // --- B. Naive branch. Same data, narrow band: even the largest template (mono 300) stays under. ---
        let (nlo, nhi) = (1000usize, 1100usize);
        assert!(
            300u64 * (2 * (nhi - nlo) as u64 + 1) <= FFT_CROSSOVER_OPS,
            "case B must exercise the naive branch"
        );
        assert_repeat_band_matches(
            &templates,
            gap_frames,
            pre_window,
            post_window,
            repeat_window_frames,
            nlo,
            nhi,
            "naive",
        );

        // --- C. §2.1 #3: the OUTER channel gate is phrased against the MONO window and `b_mono`. With a short
        // mono buffer it fails for the whole band, so every channel scores `NEG_INFINITY` **even though each
        // channel's own (shorter) window fits comfortably inside its own full-length `b_ch`**. A band that
        // only ported the per-channel `start + w <= b_ch.len()` checks returns real correlations here. ---
        let b_mono_short = det_noise(4, 6100);
        let short_mono = SeamTemplates {
            b_mono: &b_mono_short,
            ..copy_templates(&templates)
        };
        assert!(
            5900 + 300 > b_mono_short.len() && 6000 + 300 > b_mono_short.len(),
            "case C: the mono gate must fail uniformly across the band"
        );
        assert!(
            6000 + 260 <= b_ch[2].len(),
            "case C is only meaningful if the per-channel windows would have fit"
        );
        assert_repeat_band_matches(
            &short_mono,
            gap_frames,
            pre_window,
            post_window,
            repeat_window_frames,
            5900,
            6000,
            "outer-gate-fails",
        );

        // --- D. §2.1 #5: mono's start-independent conjunct fails (empty `a_pre`) ⇒ mono contributes a literal
        // `0.0`, NOT `NEG_INFINITY`, while the channel set still scores normally. ---
        let no_pre = SeamTemplates {
            a_pre: &[],
            ..copy_templates(&templates)
        };
        assert_repeat_band_matches(
            &no_pre,
            gap_frames,
            pre_window,
            post_window,
            repeat_window_frames,
            lo,
            hi,
            "empty-a-pre",
        );

        // --- E. §2.1 #1/#6: mono/1-channel media returns the mono pair outright, never reaching the channel
        // fold — so no `NEG_INFINITY` channel term is synthesized. ---
        let one_ch: Vec<Vec<f64>> = vec![b_ch[0].clone()];
        let a_pre_one: Vec<Vec<f64>> = vec![a_pre_ch[0].clone()];
        let a_post_one: Vec<Vec<f64>> = vec![a_post_ch[0].clone()];
        let mono_media = SeamTemplates {
            a_pre_ch: &a_pre_one,
            a_post_ch: &a_post_one,
            b_ch: &one_ch,
            ..copy_templates(&templates)
        };
        assert_repeat_band_matches(
            &mono_media,
            gap_frames,
            pre_window,
            post_window,
            repeat_window_frames,
            lo,
            hi,
            "single-channel",
        );

        // --- F. A per-channel bound that is NOT uniform across the band must decline, not guess: channel 1
        // (w = 200) fits at `lo` but runs off its short `b_ch` before `hi`. ---
        let mut ragged = b_ch.clone();
        ragged[1] = det_noise(5, 3000);
        assert!(
            lo + 200 <= ragged[1].len() && hi + 200 > ragged[1].len(),
            "case F: channel 1 must straddle its bound"
        );
        let ragged_templates = SeamTemplates {
            b_ch: &ragged,
            ..copy_templates(&templates)
        };
        assert!(
            fill_repeat_correlations_band(
                &ragged_templates,
                gap_frames,
                pre_window,
                post_window,
                repeat_window_frames,
                lo,
                hi,
            )
            .is_none(),
            "case F: non-uniform per-channel bound must decline"
        );
    }

    /// `SeamTemplates` holds shared refs but is not `Copy` (it has no derive), so `..` struct-update in the
    /// tests above needs an explicit reborrow.
    fn copy_templates<'a>(t: &SeamTemplates<'a>) -> SeamTemplates<'a> {
        SeamTemplates {
            a_pre: t.a_pre,
            a_post: t.a_post,
            a_pre_ch: t.a_pre_ch,
            a_post_ch: t.a_post_ch,
            b_mono: t.b_mono,
            b_ch: t.b_ch,
        }
    }

    #[test]
    fn effective_repeat_window_caps_to_gap_and_seam_on_short_brackets() {
        assert_eq!(
            effective_repeat_window_frames(96_000, 48_000, 79_200, 12_000),
            12_000
        );
        assert_eq!(
            effective_repeat_window_frames(96_000, 48_000, 79_200, 0),
            48_000
        );
        // Previously `repeat_window > a_post.len()` disabled repeat_post entirely.
        assert_eq!(
            effective_repeat_window_frames(96_000, 48_000, 12_000, 12_000),
            12_000
        );
    }

    #[test]
    fn fill_repeat_post_detects_speech_onset_in_fill_tail_on_one_second_gap() {
        let gap_frames = 48_000usize;
        let seam_window = 12_000usize;
        let border_frames = 96_000usize;
        let a_post: Vec<f64> = (0..seam_window).map(|i| (i as f64 * 0.12).sin()).collect();
        let mut fill = vec![0.02f64; gap_frames];
        fill[gap_frames - seam_window..].copy_from_slice(&a_post);
        let templates = SeamTemplates {
            a_pre: &[],
            a_post: &a_post,
            a_pre_ch: &[],
            a_post_ch: &[],
            b_mono: &fill,
            b_ch: &[fill.clone()],
        };
        let placement = SeamPlacement {
            start: 0,
            gap_frames,
            pre_window: 0,
            post_window: seam_window,
        };
        let (_, repeat_speech) = fill_repeat_correlations(&templates, placement, border_frames);
        assert!(
            repeat_speech > 0.55,
            "speech copied into fill tail should register repeat_post, got {repeat_speech}"
        );

        let music_fill = vec![0.02f64; gap_frames];
        let templates_music = SeamTemplates {
            b_mono: &music_fill,
            b_ch: std::slice::from_ref(&music_fill),
            ..templates
        };
        let (_, repeat_music) =
            fill_repeat_correlations(&templates_music, placement, border_frames);
        assert!(
            repeat_speech > repeat_music + 0.4,
            "music-only fill should score lower repeat_post (speech={repeat_speech}, music={repeat_music})"
        );
    }

    #[test]
    fn fill_repeat_post_stays_inside_fill_not_haystack_past_bracket() {
        let gap_frames = 48_000usize;
        let seam_window = 12_000usize;
        let border_frames = 96_000usize;
        let a_post: Vec<f64> = (0..seam_window).map(|i| (i as f64 * 0.15).cos()).collect();
        let mut haystack = vec![0.02f64; gap_frames + seam_window];
        haystack[gap_frames..].copy_from_slice(&a_post);
        let templates = SeamTemplates {
            a_pre: &[],
            a_post: &a_post,
            a_pre_ch: &[],
            a_post_ch: &[],
            b_mono: &haystack,
            b_ch: &[haystack.clone()],
        };
        let placement = SeamPlacement {
            start: 0,
            gap_frames,
            pre_window: 0,
            post_window: seam_window,
        };
        let (_, repeat_interior) = fill_repeat_correlations(&templates, placement, border_frames);
        assert!(
            repeat_interior < 0.3,
            "haystack past fill end must not inflate repeat_post, got {repeat_interior}"
        );
    }

    #[test]
    fn seam_score_channel_indices_picks_signal_channels() {
        let quiet = vec![1.0, -1.0, 1.0, -1.0];
        let loud = vec![1000.0, -1000.0, 1000.0, -1000.0];

        // Center-dominant 5.1 (FL, FR, FC, LFE, SL, SR): only the center carries signal.
        let pre = vec![
            quiet.clone(),
            quiet.clone(),
            loud.clone(),
            vec![],
            quiet.clone(),
            quiet.clone(),
        ];
        assert_eq!(seam_score_channel_indices(&pre, &pre), vec![2]);

        // Stereo with equal energy: both front channels are scored (prior behavior).
        let stereo = vec![loud.clone(), loud.clone()];
        assert_eq!(seam_score_channel_indices(&stereo, &stereo), vec![0, 1]);

        // Every channel near-silent → empty, so the caller falls back to the mono mix.
        let silent = vec![vec![0.0; 4], vec![0.0; 4]];
        assert!(seam_score_channel_indices(&silent, &silent).is_empty());
    }

    #[test]
    fn fill_seam_correlations_follows_center_channel_when_front_is_silent() {
        // 5.1-style: front L/R carry only low noise; the center channel holds the signal that
        // matches B. Seam scoring must follow the center, not the hardcoded front L/R (which
        // would have scored noise and returned ~0).
        let pre_window = 8usize;
        let post_window = 8usize;
        let gap_frames = 4usize;
        let start = 8usize;

        let front: Vec<f64> = vec![5.0, -5.0, 5.0, -5.0, 5.0, -5.0, 5.0, -5.0];
        let center_pre: Vec<f64> = (1..=8).map(|i| i as f64 * 100.0).collect(); // 100..800
        let center_post: Vec<f64> = (1..=8).rev().map(|i| i as f64 * 100.0).collect(); // 800..100

        let a_pre_ch = vec![front.clone(), front.clone(), center_pre.clone()];
        let a_post_ch = vec![front.clone(), front.clone(), center_post.clone()];

        let mut b_center = vec![0.0f64; 20];
        b_center[0..8].copy_from_slice(&center_pre); // pre window [start-8 .. start]
        b_center[12..20].copy_from_slice(&center_post); // post window [start+gap .. +8]
                                                        // Front B channels: anti-correlated noise — would drag the score down if scored.
        let front_b: Vec<f64> = (0..20)
            .map(|i| if i % 2 == 0 { -5.0 } else { 5.0 })
            .collect();
        let b_ch = vec![front_b.clone(), front_b.clone(), b_center.clone()];

        let templates = SeamTemplates {
            a_pre: &center_pre,
            a_post: &center_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_center,
            b_ch: &b_ch,
        };
        let placement = SeamPlacement {
            start,
            gap_frames,
            pre_window,
            post_window,
        };
        let (pre, post) = fill_seam_correlations(&templates, placement);
        assert!(
            pre > 0.9,
            "pre seam should track the center channel, got {pre}"
        );
        assert!(
            post > 0.9,
            "post seam should track the center channel, got {post}"
        );
    }

    #[test]
    fn seam_pearson_invariant_under_positive_scale() {
        let left: Vec<f64> = (0..64).map(|i| (i as f64 * 0.11).sin()).collect();
        let right: Vec<f64> = (0..64)
            .map(|i| (i as f64 * 0.11).sin() * 0.8 + 0.05)
            .collect();
        let base = seam_pearson(&left, &right);
        let scaled_left: Vec<f64> = left.iter().map(|s| s * 3.0).collect();
        let scaled_right: Vec<f64> = right.iter().map(|s| s * 7.0).collect();
        assert!(
            (base - seam_pearson(&scaled_left, &scaled_right)).abs() < 1e-12,
            "Pearson r should be scale-invariant: {base} vs scaled"
        );
    }
}
