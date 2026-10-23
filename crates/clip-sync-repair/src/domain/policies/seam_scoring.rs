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

/// Inclusive `[lo, hi]` band for the FFT seam/repeat evaluators.
///
/// One type for both the *start* range (`fill_*_correlations_band`) and the already-offset *base*
/// range (`mono_seam_band`). Width is `hi − lo + 1` when `hi >= lo`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BandRange {
    pub lo: usize,
    pub hi: usize,
}

impl BandRange {
    pub fn width(self) -> Option<usize> {
        self.hi.checked_sub(self.lo).map(|d| d + 1)
    }
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
///
/// Mono / 1-channel media scores the downmix. Multichannel uses the same ~20 dB energy selection as
/// seam Pearson / residual ([`seam_score_channel_indices`]) and **does not** fold in the mono term —
/// empty selection or no scoreable window is `0.0` ("no repeat"), not a mono fallback.
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

    let repeat_pre_mono = if !a_pre.is_empty()
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
    let repeat_post_mono = if !a_post.is_empty()
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
        return (repeat_pre_mono, repeat_post_mono);
    }

    let score_channels = seam_score_channel_indices(a_pre_ch, a_post_ch);
    if score_channels.is_empty() {
        return (0.0, 0.0);
    }

    // Outer gate is phrased against the mono window / `b_mono` (band twin must match).
    let ch_pre = if start + pre_repeat_window <= b_mono.len() {
        let mut best = f64::NEG_INFINITY;
        for &ch in &score_channels {
            if ch >= a_pre_ch.len() || ch >= b_ch.len() {
                continue;
            }
            let a_ch = &a_pre_ch[ch];
            let b_side = &b_ch[ch];
            let border_len = a_ch.len();
            let w = effective_repeat_window_frames(
                repeat_window_frames,
                gap_frames,
                border_len,
                pre_window,
            );
            if w <= border_len && start + w <= b_side.len() {
                best = best.max(seam_pearson(
                    &a_ch[border_len.saturating_sub(w)..],
                    &b_side[start..start + w],
                ));
            }
        }
        if best.is_finite() {
            best
        } else {
            0.0
        }
    } else {
        0.0
    };

    let ch_post = if tail_start + post_repeat_window <= b_mono.len() {
        let mut best = f64::NEG_INFINITY;
        for &ch in &score_channels {
            if ch >= a_post_ch.len() || ch >= b_ch.len() {
                continue;
            }
            let a_ch = &a_post_ch[ch];
            let b_side = &b_ch[ch];
            let border_len = a_ch.len();
            let w = effective_repeat_window_frames(
                repeat_window_frames,
                gap_frames,
                border_len,
                post_window,
            );
            let tail = start + gap_frames.saturating_sub(w);
            if w <= border_len && tail + w <= b_side.len() {
                best = best.max(seam_pearson(&a_ch[..w], &b_side[tail..tail + w]));
            }
        }
        if best.is_finite() {
            best
        } else {
            0.0
        }
    } else {
        0.0
    };

    (ch_pre, ch_post)
}

/// One side (pre or post) of [`fill_repeat_correlations_band`], mirroring the corresponding half of
/// [`fill_repeat_correlations`] exactly. `tail = true` is the PRE side (template is `a`'s tail, B base is
/// `start`); `tail = false` is the POST side (template is `a`'s head, B base is `start + gap_frames − w`).
/// The base offset is what makes one helper serve both, and it is **per-window**: the mono side offsets by the
/// mono `w`, each channel by its own.
///
/// Returns `None` when a start-dependent bound is not uniform across `[band.lo, band.hi]` (the caller then
/// scores the band naively), matching [`fill_seam_correlations_band`]'s decline contract.
fn repeat_side_band(
    templates: &SeamTemplates<'_>,
    repeat_window_frames: usize,
    gap_frames: usize,
    seam_window: usize,
    score_channels: &[usize],
    tail: bool,
    band: BandRange,
) -> Option<Vec<f64>> {
    let (a_mono, a_ch) = if tail {
        (templates.a_pre, templates.a_pre_ch)
    } else {
        (templates.a_post, templates.a_post_ch)
    };
    let b_mono = templates.b_mono;
    let b_ch = templates.b_ch;
    let mono_window =
        effective_repeat_window_frames(repeat_window_frames, gap_frames, a_mono.len(), seam_window);
    let width = band.width()?;
    let start_lo = band.lo;
    let start_hi = band.hi;
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

    // Outer / mono gate: phrased against the MONO window and `b_mono`. Monotonic in `start`, so uniform
    // across the band iff it agrees at both ends.
    let fit_lo = start_lo + mono_offset + mono_window <= b_mono.len();
    let fit_hi = start_hi + mono_offset + mono_window <= b_mono.len();
    if fit_lo != fit_hi {
        return None;
    }
    let fit = fit_lo;

    // Mono / 1-channel: return the mono pair outright (naive early-return).
    if b_ch.len() <= 1 {
        let score_mono = !a_mono.is_empty() && fit && mono_window <= a_mono.len();
        if !score_mono {
            return Some(vec![0.0; width]);
        }
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
        return Some(band);
    }

    // Multichannel: energy-selected channels only; mono is not a participant. Empty selection or a
    // failed outer gate → `0.0` ("no repeat"), matching the naive path.
    if score_channels.is_empty() || !fit {
        return Some(vec![0.0; width]);
    }

    let mut bands: Vec<Vec<f64>> = Vec::new();
    for &ch in score_channels {
        if ch >= a_ch.len() || ch >= b_ch.len() {
            continue;
        }
        let a_c = &a_ch[ch];
        let b_c = &b_ch[ch];
        let border_len = a_c.len();
        let w = effective_repeat_window_frames(
            repeat_window_frames,
            gap_frames,
            border_len,
            seam_window,
        );
        if w > border_len {
            continue; // start-independent exclusion — matches the naive path
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
    Some(
        (0..width)
            .map(|i| {
                let v = bands.iter().map(|b| b[i]).fold(f64::NEG_INFINITY, f64::max);
                if v.is_finite() {
                    v
                } else {
                    0.0
                }
            })
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
/// would get wrong (see `TEMP-repeat-band-plan.md` §2.1): multichannel uses [`seam_score_channel_indices`]
/// (not all channels, and not mono-in-the-max), per-channel window lengths that differ within one call, an
/// outer channel gate phrased against the mono window, and `0.0` for unscoreable cases.
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
    band: BandRange,
) -> Option<Vec<(f64, f64)>> {
    let width = band.width()?;
    let score_channels = seam_score_channel_indices(templates.a_pre_ch, templates.a_post_ch);

    let pre = repeat_side_band(
        templates,
        repeat_window_frames,
        gap_frames,
        pre_window,
        &score_channels,
        true,
        band,
    )?;
    let post = repeat_side_band(
        templates,
        repeat_window_frames,
        gap_frames,
        post_window,
        &score_channels,
        false,
        band,
    )?;
    debug_assert_eq!(pre.len(), width);
    debug_assert_eq!(post.len(), width);
    Some(pre.into_iter().zip(post).collect())
}

/// The layout the splice will write at the seams — what the splice-aware scorer must score.
#[derive(Clone, Copy)]
pub enum SpliceSeamGeometry<'a> {
    /// A **dual-fit** fill, whose pre/post shoulders were independently matched at their own
    /// seam-local lags (that's the entire point of dual-fit: no single lag satisfies both seams) —
    /// comparing the fill's head/tail against raw A at lag 0 there is a category error, so scoring
    /// uses the border-window template dual-fit's own seam-local search already validated against.
    BorderTemplate,
    /// An ordinary (single rigid-lag) splice through
    /// [`apply_seam_crossfade`](super::apply_seam_crossfade): the fill head bleeds over A's pre tail
    /// and the fill tail is rewound over A's post head, so those are the crossfade windows scored.
    PlainCrossfade,
    /// A single-lag splice through
    /// [`apply_seam_crossfade_extended`](super::apply_seam_crossfade_extended): `cf` frames of B
    /// either side of the fill are blended over A instead. The margins are cut from `b_full`
    /// (interleaved, [`SpliceSeamContext::channels`]) by [`seam_margin_slices`](super::seam_margin_slices)
    /// — the splice's own cutter — at `fill_start_frame`. Where it cannot cut, the splice falls back
    /// to the plain layout, and so does the score.
    ExtendedCrossfade {
        b_full: &'a [f32],
        fill_start_frame: usize,
    },
}

/// A-side gap bounds for splice-aware seam scoring on decoded PCM.
#[derive(Clone, Copy)]
pub struct SpliceSeamContext<'a> {
    pub seam_cf: usize,
    pub gap_start_frame: usize,
    pub gap_end_frame: usize,
    pub a_samples: &'a [f32],
    pub channels: usize,
    pub geometry: SpliceSeamGeometry<'a>,
}

impl<'a> SpliceSeamGeometry<'a> {
    /// The geometry of a single-lag fill: extended when `--seam-extend-fill` is on and the fill's
    /// full-B start is known, plain otherwise. `fill_start_frame: None` is how a caller says the
    /// splice will not get margins (e.g. a zero-padded fill), so it scores the plain fallback.
    pub fn single_lag(
        seam_extend_fill: bool,
        b_full: &'a [f32],
        fill_start_frame: Option<usize>,
    ) -> Self {
        match fill_start_frame {
            Some(fill_start_frame) if seam_extend_fill => Self::ExtendedCrossfade {
                b_full,
                fill_start_frame,
            },
            _ => Self::PlainCrossfade,
        }
    }
}

impl<'a> SpliceSeamContext<'a> {
    /// The same context for a fill that starts `frames` later in B — the trim-head candidate of
    /// [`pick_fill_length_anchor`](crate::domain::gap_fill_fit::pick_fill_length_anchor), whose
    /// extended margins sit `frames` later too. A no-op for the other geometries.
    pub fn fill_advanced_by(self, frames: usize) -> Self {
        let geometry = match self.geometry {
            SpliceSeamGeometry::ExtendedCrossfade {
                b_full,
                fill_start_frame,
            } => SpliceSeamGeometry::ExtendedCrossfade {
                b_full,
                fill_start_frame: fill_start_frame + frames,
            },
            other => other,
        };
        Self { geometry, ..self }
    }

    /// How many frames the written fill runs ahead of its planned frame, for a fill of
    /// `fill_frames`. The plain splice starts the gap interior at `fill[cf]`, so it is `seam_cf`; the
    /// extended splice starts it at `fill[0]`, so it is 0 — except where its margins cannot be cut and
    /// the splice falls back to plain. Dual-fit plans in the written frame already: 0.
    pub fn written_lead_frames(&self, fill_frames: usize) -> usize {
        match self.geometry {
            SpliceSeamGeometry::BorderTemplate => 0,
            _ if self.extended_margin(fill_frames).is_some() => 0,
            _ => self.seam_cf,
        }
    }

    /// The crossfade windows against raw A are valid only for a single-lag fill.
    fn single_lag(&self) -> bool {
        !matches!(self.geometry, SpliceSeamGeometry::BorderTemplate)
    }

    /// Interleaved `(head, tail)` B margins the extended splice will write, for a fill of
    /// `fill_frames`. `None` for every other geometry, and wherever the splice itself would fall
    /// back to the plain layout.
    fn extended_margin(&self, fill_frames: usize) -> Option<(&'a [f32], &'a [f32])> {
        match self.geometry {
            SpliceSeamGeometry::ExtendedCrossfade {
                b_full,
                fill_start_frame,
            } => super::seam_margin_slices(
                b_full,
                self.channels,
                fill_start_frame,
                fill_frames,
                self.seam_cf,
            ),
            _ => None,
        }
    }
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

/// Seam scores for a fitted fill chunk aligned with the splice `ctx.geometry` names.
///
/// When `ctx.seam_cf > 0` on a single-lag geometry, scores the A PCM windows that are actually
/// crossfaded at splice time, against the B audio the splice blends over them: the fill's own head
/// and tail ([`SpliceSeamGeometry::PlainCrossfade`]) or the B margins either side of it
/// ([`SpliceSeamGeometry::ExtendedCrossfade`]).
pub fn fill_splice_seam_correlations(
    fill_mono: &[f64],
    a_pre: &[f64],
    a_post: &[f64],
    pre_window: usize,
    post_window: usize,
    ctx: SpliceSeamContext<'_>,
) -> (f64, f64) {
    let margin = ctx.extended_margin(fill_mono.len()).map(|(head, tail)| {
        (
            interleaved_to_mono(head, ctx.channels),
            interleaved_to_mono(tail, ctx.channels),
        )
    });
    let (head, tail) = match &margin {
        Some((head, tail)) => (Some(head.as_slice()), Some(tail.as_slice())),
        None => (None, None),
    };
    (
        score_splice_pre_seam(fill_mono, head, a_pre, pre_window, ctx),
        score_splice_post_seam(fill_mono, tail, a_post, post_window, ctx),
    )
}

/// B window blended over A's last `w` pre-gap frames: the extended splice's `head` margin, or the
/// plain splice's fill head.
fn splice_pre_b_window<'b>(
    fill: &'b [f64],
    head: Option<&'b [f64]>,
    cf: usize,
    w: usize,
) -> &'b [f64] {
    match head {
        Some(head) => &head[head.len() - w..],
        None => &fill[cf - w..cf],
    }
}

/// B window blended over A's first `w` post-gap frames: the extended splice's `tail` margin, or
/// the plain splice's rewound fill tail.
fn splice_post_b_window<'b>(
    fill: &'b [f64],
    tail: Option<&'b [f64]>,
    cf: usize,
    w: usize,
) -> &'b [f64] {
    match tail {
        Some(tail) => &tail[..w.min(tail.len())],
        None => {
            let b_start = fill.len().saturating_sub(cf);
            &fill[b_start..(b_start + w).min(fill.len())]
        }
    }
}

fn score_splice_pre_seam(
    fill_mono: &[f64],
    head: Option<&[f64]>,
    a_pre: &[f64],
    pre_window: usize,
    ctx: SpliceSeamContext<'_>,
) -> f64 {
    if pre_window == 0 || fill_mono.is_empty() {
        return 0.0;
    }
    if ctx.single_lag() && ctx.seam_cf > 0 && ctx.gap_start_frame > 0 {
        let w = pre_window
            .min(ctx.seam_cf)
            .min(ctx.gap_start_frame)
            .min(fill_mono.len());
        if w > 0 {
            let b = splice_pre_b_window(fill_mono, head, ctx.seam_cf, w);
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
    tail: Option<&[f64]>,
    a_post: &[f64],
    post_window: usize,
    ctx: SpliceSeamContext<'_>,
) -> f64 {
    if post_window == 0 || fill_mono.is_empty() {
        return 0.0;
    }
    let len = fill_mono.len();
    let total_frames = ctx.a_samples.len() / ctx.channels.max(1);
    if ctx.single_lag() && ctx.seam_cf > 0 && ctx.gap_end_frame < total_frames {
        let w = post_window
            .min(ctx.seam_cf)
            .min(total_frames.saturating_sub(ctx.gap_end_frame))
            .min(len.saturating_sub(len.saturating_sub(ctx.seam_cf)));
        if w > 0 {
            let b = splice_post_b_window(fill_mono, tail, ctx.seam_cf, w);
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
    let margin = ctx.extended_margin(gap_frames);
    let channel_of = |samples: &[f32], ch: usize| -> Vec<f64> {
        samples
            .iter()
            .skip(ch)
            .step_by(channels)
            .map(|&s| s as f64)
            .collect()
    };
    let score_channels = seam_score_channel_indices(a_pre_ch, a_post_ch);
    let mut pre_scores = Vec::with_capacity(score_channels.len());
    let mut post_scores = Vec::with_capacity(score_channels.len());
    for &ch in &score_channels {
        let ch_fill = channel_of(fill_interleaved, ch);
        if ch_fill.len() != gap_frames {
            continue;
        }
        let ch_margin = margin.map(|(head, tail)| (channel_of(head, ch), channel_of(tail, ch)));
        let (ch_head, ch_tail) = match &ch_margin {
            Some((head, tail)) => (Some(head.as_slice()), Some(tail.as_slice())),
            None => (None, None),
        };
        let pre =
            score_splice_pre_seam_channel(&ch_fill, ch_head, &a_pre_ch[ch], pre_window, ctx, ch);
        if pre.is_finite() && pre > f64::NEG_INFINITY {
            pre_scores.push(pre);
        }
        let post =
            score_splice_post_seam_channel(&ch_fill, ch_tail, &a_post_ch[ch], post_window, ctx, ch);
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
    head: Option<&[f64]>,
    a_pre: &[f64],
    pre_window: usize,
    ctx: SpliceSeamContext<'_>,
    channel: usize,
) -> f64 {
    if pre_window == 0 || fill_mono.is_empty() {
        return 0.0;
    }
    if ctx.single_lag() && ctx.seam_cf > 0 && ctx.gap_start_frame > 0 {
        let w = pre_window
            .min(ctx.seam_cf)
            .min(ctx.gap_start_frame)
            .min(fill_mono.len());
        if w > 0 {
            let b = splice_pre_b_window(fill_mono, head, ctx.seam_cf, w);
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
    tail: Option<&[f64]>,
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
    if ctx.single_lag() && ctx.seam_cf > 0 && ctx.gap_end_frame < total_frames {
        let w = post_window
            .min(ctx.seam_cf)
            .min(total_frames.saturating_sub(ctx.gap_end_frame))
            .min(len.saturating_sub(len.saturating_sub(ctx.seam_cf)));
        if w > 0 {
            let b = splice_post_b_window(fill_mono, tail, ctx.seam_cf, w);
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
    band: BandRange,
) -> Option<Vec<(f64, f64)>> {
    let width = band.width()?;
    let start_lo = band.lo;
    let start_hi = band.hi;
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
        BandRange {
            lo: start_lo.saturating_sub(pre_window),
            hi: start_hi.saturating_sub(pre_window),
        },
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
        BandRange {
            lo: start_lo + gap_frames,
            hi: start_hi + gap_frames,
        },
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
fn mono_seam_band(
    score: bool,
    a: &[f64],
    b_mono: &[f64],
    window: usize,
    bases: BandRange,
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
    let band = seam_correlation_over_bases(template, b_mono, bases.lo, bases.hi);
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
            BandRange {
                lo: start_lo,
                hi: start_hi,
            },
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
            BandRange {
                lo: start_lo,
                hi: start_hi,
            },
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

    fn assert_repeat_band_matches(
        templates: &SeamTemplates<'_>,
        gap_frames: usize,
        pre_window: usize,
        post_window: usize,
        repeat_window_frames: usize,
        band: BandRange,
        label: &str,
    ) {
        let filled = fill_repeat_correlations_band(
            templates,
            gap_frames,
            pre_window,
            post_window,
            repeat_window_frames,
            band,
        )
        .unwrap_or_else(|| panic!("{label}: band declined but the bounds are uniform"));
        assert_eq!(filled.len(), band.width().unwrap(), "{label}: band width");
        for (i, &(pre, post)) in filled.iter().enumerate() {
            let start = band.lo + i;
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
            BandRange { lo, hi },
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
            BandRange { lo: nlo, hi: nhi },
            "naive",
        );

        // --- C. Outer channel gate is phrased against the MONO window and `b_mono`. With a short mono
        // buffer it fails for the whole band, so every start scores `0.0` **even though each channel's own
        // (shorter) window fits inside its full-length `b_ch`**. A band that only ported the per-channel
        // `start + w <= b_ch.len()` checks would return real correlations here. ---
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
            BandRange { lo: 5900, hi: 6000 },
            "outer-gate-fails",
        );

        // --- D. Empty `a_pre` only affects the mono early-return path; multichannel scores selected
        // channels and ignores mono, so the band must still match the naive channel fold. ---
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
            BandRange { lo, hi },
            "empty-a-pre",
        );

        // --- E. Mono/1-channel media returns the mono pair outright, never reaching the channel fold. ---
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
            BandRange { lo, hi },
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
                BandRange { lo, hi },
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
    fn fill_repeat_empty_channel_selection_is_zero_not_mono() {
        // All-silent multichannel borders → empty energy selection → (0, 0), even if mono would correlate.
        let w = 32usize;
        let gap = 16usize;
        let signal: Vec<f64> = (0..w).map(|i| (i as f64 * 0.3).sin()).collect();
        let mut b_mono = vec![0.0f64; 64];
        b_mono[8..8 + w.min(gap)].copy_from_slice(&signal[..w.min(gap)]);
        // Put the same signal in mono-only haystack; per-channel B is silent.
        let silent = vec![0.0f64; 64];
        let a_silent = vec![0.0f64; w];
        let a_pre_ch = vec![a_silent.clone(), a_silent.clone()];
        let a_post_ch = vec![a_silent.clone(), a_silent.clone()];
        let b_ch = vec![silent.clone(), silent];
        let templates = SeamTemplates {
            a_pre: &signal,
            a_post: &signal,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let placement = SeamPlacement {
            start: 8,
            gap_frames: gap,
            pre_window: w,
            post_window: w,
        };
        assert!(seam_score_channel_indices(&a_pre_ch, &a_post_ch).is_empty());
        let (pre, post) = fill_repeat_correlations(&templates, placement, w);
        assert_eq!((pre, post), (0.0, 0.0));
        assert_repeat_band_matches(
            &templates,
            gap,
            w,
            w,
            w,
            BandRange { lo: 8, hi: 8 },
            "empty-selection",
        );
    }

    /// Center-dominant repeat: silent fronts must not participate; selected center still flags repeat.
    #[test]
    fn fill_repeat_follows_energy_selected_channels() {
        let w = 64usize;
        let gap = 64usize;
        let start = 64usize;
        let quiet = vec![0.01f64; w];
        let loud: Vec<f64> = (0..w).map(|i| (i as f64 * 0.17).sin() * 10.0).collect();
        // 3ch: FL/FR quiet, FC loud — same layout as seam center-dominant test.
        let a_pre_ch = vec![quiet.clone(), quiet.clone(), loud.clone()];
        let a_post_ch = vec![quiet.clone(), quiet.clone(), loud.clone()];
        let mut b_center = vec![0.0f64; start + gap + w];
        // Pre-repeat reads B at `start..start+w` against A's pre tail.
        b_center[start..start + w].copy_from_slice(&loud);
        // Post-repeat reads fill tail.
        b_center[start + gap - w..start + gap].copy_from_slice(&loud);
        let front_b = vec![0.01f64; b_center.len()];
        let b_ch = vec![front_b.clone(), front_b, b_center.clone()];
        let templates = SeamTemplates {
            a_pre: &loud,
            a_post: &loud,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_center,
            b_ch: &b_ch,
        };
        assert_eq!(seam_score_channel_indices(&a_pre_ch, &a_post_ch), vec![2]);
        let placement = SeamPlacement {
            start,
            gap_frames: gap,
            pre_window: w,
            post_window: w,
        };
        let (pre, post) = fill_repeat_correlations(&templates, placement, w);
        assert!(pre > 0.9, "pre repeat should follow center, got {pre}");
        assert!(post > 0.9, "post repeat should follow center, got {post}");
        assert_repeat_band_matches(
            &templates,
            gap,
            w,
            w,
            w,
            BandRange {
                lo: start,
                hi: start,
            },
            "center-repeat",
        );
    }

    /// Smeared multichannel repeat: same template on several equal-energy channels (no mono term).
    /// Confirms dropping mono does not blind the detector when the repeat is per-channel-visible.
    #[test]
    fn fill_repeat_smeared_multichannel_still_detects_without_mono() {
        let w = 48usize;
        let gap = 48usize;
        let start = 48usize;
        let tmpl: Vec<f64> = (0..w).map(|i| (i as f64 * 0.21).sin()).collect();
        let a_pre_ch = vec![tmpl.clone(), tmpl.clone(), tmpl.clone()];
        let a_post_ch = vec![tmpl.clone(), tmpl.clone(), tmpl.clone()];
        let mut b_side = vec![0.0f64; start + gap + w];
        b_side[start..start + w].copy_from_slice(&tmpl);
        b_side[start + gap - w..start + gap].copy_from_slice(&tmpl);
        let b_ch = vec![b_side.clone(), b_side.clone(), b_side.clone()];
        // Mono downmix ≈ one channel here; we assert the selected per-channel path alone is enough.
        let templates = SeamTemplates {
            a_pre: &tmpl,
            a_post: &tmpl,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_side,
            b_ch: &b_ch,
        };
        let selected = seam_score_channel_indices(&a_pre_ch, &a_post_ch);
        assert_eq!(selected, vec![0, 1, 2]);
        let placement = SeamPlacement {
            start,
            gap_frames: gap,
            pre_window: w,
            post_window: w,
        };
        let (pre, post) = fill_repeat_correlations(&templates, placement, w);
        assert!(pre > 0.9, "smeared pre repeat got {pre}");
        assert!(post > 0.9, "smeared post repeat got {post}");
        assert_repeat_band_matches(
            &templates,
            gap,
            w,
            w,
            w,
            BandRange {
                lo: start,
                hi: start,
            },
            "smeared-repeat",
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
    /// A tone under deterministic noise. Pearson needs a signal that is not self-similar under a
    /// shift — a ramp scores the same at every lag, so it cannot tell a displaced window apart.
    fn splice_probe_signal(seed: u64, frames: usize) -> Vec<f32> {
        let noise = det_noise(seed, frames);
        (0..frames)
            .map(|i| {
                let tone = (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 48_000.0).sin();
                (0.3 * tone + 0.3 * noise[i]) as f32
            })
            .collect()
    }

    /// `(cf, gap_start, gap_end)` of the splice-geometry fixtures: A is B, so the true fill is
    /// `B[gap_start..gap_end]` at registration zero.
    const SPLICE_PROBE: (usize, usize, usize) = (480, 8_000, 8_960);

    fn splice_probe_ctx<'a>(
        a: &'a [f32],
        channels: usize,
        geometry: SpliceSeamGeometry<'a>,
    ) -> SpliceSeamContext<'a> {
        let (cf, gs, ge) = SPLICE_PROBE;
        SpliceSeamContext {
            seam_cf: cf,
            gap_start_frame: gs,
            gap_end_frame: ge,
            a_samples: a,
            channels,
            geometry,
        }
    }

    /// The extended geometry scores the frames `apply_seam_crossfade_extended` writes over A — B
    /// either side of the fill — so a fill at exact registration scores a perfect seam on both
    /// sides. The plain geometry scores the fill's own head and rewound tail there, which a
    /// correctly placed fill does not match: that mismatch is the bug the geometry exists to fix.
    #[test]
    fn extended_splice_geometry_scores_the_written_margins() {
        let (cf, gs, ge) = SPLICE_PROBE;
        let b = splice_probe_signal(11, 20_000);
        let a = b.clone();
        let fill: Vec<f64> = b[gs..ge].iter().map(|&s| s as f64).collect();
        let a_pre: Vec<f64> = a[gs - cf..gs].iter().map(|&s| s as f64).collect();
        let a_post: Vec<f64> = a[ge..ge + cf].iter().map(|&s| s as f64).collect();
        let score = |geometry| {
            fill_splice_seam_correlations(
                &fill,
                &a_pre,
                &a_post,
                cf,
                cf,
                splice_probe_ctx(&a, 1, geometry),
            )
        };

        let (pre, post) = score(SpliceSeamGeometry::ExtendedCrossfade {
            b_full: &b,
            fill_start_frame: gs,
        });
        assert!(
            pre >= 0.99 && post >= 0.99,
            "extended: pre={pre} post={post}"
        );
        let (plain_pre, plain_post) = score(SpliceSeamGeometry::PlainCrossfade);
        assert!(
            plain_pre.min(plain_post) < 0.5,
            "fixture must separate the geometries: plain pre={plain_pre} post={plain_post}"
        );
    }

    /// As above through the per-channel path: the interleaved entry point de-interleaves the
    /// margins the same way it de-interleaves the fill.
    #[test]
    fn extended_splice_geometry_scores_the_written_margins_per_channel() {
        let (cf, gs, ge) = SPLICE_PROBE;
        let channels = 2usize;
        let frames = 20_000usize;
        let left = splice_probe_signal(11, frames);
        let right = splice_probe_signal(29, frames);
        let b: Vec<f32> = (0..frames).flat_map(|i| [left[i], right[i]]).collect();
        let a = b.clone();
        let fill = b[gs * channels..ge * channels].to_vec();
        let window = |ch: &[f32], lo: usize, hi: usize| -> Vec<f64> {
            ch[lo..hi].iter().map(|&s| s as f64).collect()
        };
        let a_pre_ch = vec![window(&left, gs - cf, gs), window(&right, gs - cf, gs)];
        let a_post_ch = vec![window(&left, ge, ge + cf), window(&right, ge, ge + cf)];
        let a_pre = interleaved_to_mono(&a[(gs - cf) * channels..gs * channels], channels);
        let a_post = interleaved_to_mono(&a[ge * channels..(ge + cf) * channels], channels);
        let borders = BorderSeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            pre_window: cf,
            post_window: cf,
        };
        let score = |geometry| {
            fill_splice_seam_correlations_interleaved(
                &fill,
                channels,
                &borders,
                splice_probe_ctx(&a, channels, geometry),
            )
        };

        let (pre, post) = score(SpliceSeamGeometry::ExtendedCrossfade {
            b_full: &b,
            fill_start_frame: gs,
        });
        assert!(
            pre >= 0.99 && post >= 0.99,
            "extended: pre={pre} post={post}"
        );
        let (plain_pre, plain_post) = score(SpliceSeamGeometry::PlainCrossfade);
        assert!(
            plain_pre.min(plain_post) < 0.5,
            "plain pre={plain_pre} post={plain_post}"
        );
    }

    /// Registration routing converts the planned frame to the written one by this lead: one
    /// crossfade on the plain splice (and on the extended splice's plain fallback), none on the
    /// extended splice or on dual-fit.
    #[test]
    fn written_lead_is_a_crossfade_on_the_plain_splice_only() {
        let (cf, gs, ge) = SPLICE_PROBE;
        let b = splice_probe_signal(11, 20_000);
        let lead = |geometry: SpliceSeamGeometry<'_>| {
            SpliceSeamContext {
                seam_cf: cf,
                gap_start_frame: gs,
                gap_end_frame: ge,
                a_samples: &b,
                channels: 1,
                geometry,
            }
            .written_lead_frames(ge - gs)
        };
        let extended = |b_full| SpliceSeamGeometry::ExtendedCrossfade {
            b_full,
            fill_start_frame: gs,
        };
        assert_eq!(lead(SpliceSeamGeometry::PlainCrossfade), cf);
        assert_eq!(lead(extended(&b)), 0);
        assert_eq!(lead(extended(&b[..ge + cf - 1])), cf, "plain fallback");
        assert_eq!(lead(SpliceSeamGeometry::BorderTemplate), 0);
    }

    /// Where the splice cannot cut a margin it falls back to the plain crossfade, so the scorer
    /// must score exactly that: here B ends one frame short of the tail margin.
    #[test]
    fn extended_splice_geometry_falls_back_to_plain_where_the_splice_does() {
        let (cf, gs, ge) = SPLICE_PROBE;
        let b = splice_probe_signal(11, 20_000);
        let a = b.clone();
        let b_short = &b[..ge + cf - 1];
        assert!(super::super::seam_margin_slices(b_short, 1, gs, ge - gs, cf).is_none());
        let fill: Vec<f64> = b[gs..ge].iter().map(|&s| s as f64).collect();
        let a_pre: Vec<f64> = a[gs - cf..gs].iter().map(|&s| s as f64).collect();
        let a_post: Vec<f64> = a[ge..ge + cf].iter().map(|&s| s as f64).collect();
        let score = |geometry| {
            fill_splice_seam_correlations(
                &fill,
                &a_pre,
                &a_post,
                cf,
                cf,
                splice_probe_ctx(&a, 1, geometry),
            )
        };

        let extended = score(SpliceSeamGeometry::ExtendedCrossfade {
            b_full: b_short,
            fill_start_frame: gs,
        });
        assert_eq!(extended, score(SpliceSeamGeometry::PlainCrossfade));
    }
}
