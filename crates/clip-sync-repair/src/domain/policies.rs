use clip_sync::{normalized_correlation, MultiChannelPcm};

/// A contiguous silent region on a media timeline (seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilentRun {
    pub start_secs: f64,
    pub end_secs: f64,
}

/// Accumulates silent runs by classifying PCM in fixed-duration analysis blocks.
pub struct SilenceRunScanner {
    block_secs: f64,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
    min_gap_secs: f64,
    /// How many consecutive non-silent blocks to tolerate before closing a run.
    hold_blocks: u32,
    held_count: u32,
    run_start: Option<f64>,
    /// End of the last confirmed-silent block. Reported gap boundaries use this, not
    /// `held_end`, so held non-silent blocks at the tail of a run are never included in
    /// the output interval (avoiding boundary bloat past actual silence).
    silent_tail: Option<f64>,
    runs: Vec<SilentRun>,
}

impl SilenceRunScanner {
    pub fn new(
        block_secs: f64,
        silence_peak_fraction: f32,
        min_gap_secs: f64,
        hold_blocks: u32,
        absolute_rms_floor: f32,
    ) -> Self {
        Self {
            block_secs,
            silence_peak_fraction,
            absolute_rms_floor,
            min_gap_secs,
            hold_blocks,
            held_count: 0,
            run_start: None,
            silent_tail: None,
            runs: Vec::new(),
        }
    }

    /// Classify `pcm` (starting at `timeline_start_secs` on the file timeline) into blocks.
    ///
    /// Silence requires every channel in a block to pass [`is_silent_interleaved`] (ffmpeg
    /// `silencedetect` default: all channels quiet simultaneously).
    pub fn feed(&mut self, pcm: &MultiChannelPcm, timeline_start_secs: f64) {
        if self.block_secs <= 0.0 || pcm.samples.is_empty() {
            return;
        }

        let channels = pcm.channels.max(1) as usize;
        let rate = pcm.sample_rate;
        let block_frames = (self.block_secs * f64::from(rate))
            .round()
            .max(1.0) as usize;
        let total_frames = pcm.frames();

        let mut offset_frames = 0usize;
        while offset_frames < total_frames {
            let end_frames = (offset_frames + block_frames).min(total_frames);
            let block_start_secs =
                timeline_start_secs + offset_frames as f64 / f64::from(rate);
            let block_end_secs = timeline_start_secs + end_frames as f64 / f64::from(rate);
            let block_start = offset_frames * channels;
            let block_end = end_frames * channels;
            let block = &pcm.samples[block_start..block_end];

            if is_silent_interleaved(
                block,
                channels,
                self.silence_peak_fraction,
                self.absolute_rms_floor,
            ) {
                self.held_count = 0;
                if self.run_start.is_none() {
                    self.run_start = Some(block_start_secs);
                }
                // Advance the confirmed-silent boundary past any previously held blocks.
                self.silent_tail = Some(block_end_secs);
            } else if self.run_start.is_some() {
                // In an active run: absorb up to `hold_blocks` consecutive non-silent blocks
                // without updating `silent_tail` — gap boundaries stay tight.
                if self.held_count < self.hold_blocks {
                    self.held_count += 1;
                } else {
                    self.held_count = 0;
                    self.close_open_run();
                }
            }

            offset_frames = end_frames;
        }
    }

    /// Close any open run and return all detected intervals.
    pub fn finish(mut self) -> Vec<SilentRun> {
        self.close_open_run();
        self.runs
    }

    /// Break an open silent run when decoded PCM has a timeline hole (e.g. skipped decode chunk).
    pub fn note_pcm_discontinuity(&mut self) {
        self.held_count = 0;
        self.close_open_run();
    }

    fn close_open_run(&mut self) {
        let (Some(start), Some(end)) = (self.run_start.take(), self.silent_tail.take()) else {
            return;
        };
        if end - start >= self.min_gap_secs {
            self.runs.push(SilentRun {
                start_secs: start,
                end_secs: end,
            });
        }
    }
}

/// Returns true if `samples` represent silence.
///
/// A block is silent when either:
/// - peak amplitude is zero, or
/// - `peak < absolute_rms_floor` — matches ffmpeg's `silencedetect` semantics: silence when the
///   peak is below an absolute amplitude floor (default ≈ −60 dBFS = 33 on the i16 scale), or
/// - `RMS < peak × silence_peak_fraction` — catches codec noise: a block whose RMS is negligible
///   relative to its own peak (i.e. a few isolated transients in a sea of zeros).
///
/// Pass `absolute_rms_floor = 0.0` to disable the peak-floor check.
pub fn is_silent(samples: &[i16], silence_peak_fraction: f32, absolute_rms_floor: f32) -> bool {
    is_silent_interleaved(samples, 1, silence_peak_fraction, absolute_rms_floor)
}

/// Returns true when every channel in the interleaved block passes [`is_silent`].
///
/// Matches ffmpeg `silencedetect` with `mono=0` (default): all channels must be quiet.
pub fn is_silent_interleaved(
    samples: &[i16],
    channels: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> bool {
    let channels = channels.max(1);
    if samples.is_empty() {
        return true;
    }
    let frames = samples.len() / channels;
    if frames == 0 {
        return true;
    }
    (0..channels).all(|channel| {
        is_silent_channel(
            samples,
            channel,
            channels,
            frames,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    })
}

fn is_silent_channel(
    samples: &[i16],
    channel: usize,
    channels: usize,
    frames: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> bool {
    let mut peak = 0u32;
    let mut sum_sq = 0f64;
    for frame in 0..frames {
        let sample = samples[frame * channels + channel];
        peak = peak.max(u32::from(sample.unsigned_abs()));
        let v = f64::from(sample);
        sum_sq += v * v;
    }

    if peak == 0 {
        return true;
    }

    let peak_f = peak as f32;
    if absolute_rms_floor > 0.0 && peak_f < absolute_rms_floor {
        return true;
    }

    let rms = (sum_sq / frames as f64).sqrt() as f32;
    rms < peak_f * silence_peak_fraction
}

fn rms_i16(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|s| {
            let v = f64::from(*s);
            v * v
        })
        .sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

/// RMS of interleaved (multi-channel) i16 samples.
pub fn rms_interleaved(samples: &[i16]) -> f32 {
    rms_i16(samples)
}

/// Compute a gain factor to match `b_segment_rms` to `a_border_rms`.
///
/// The gain is clamped to ±`max_gain_db` dB. Returns `1.0` when either RMS is zero.
pub fn compute_fill_gain(a_border_rms: f32, b_segment_rms: f32, max_gain_db: f64) -> f32 {
    if a_border_rms == 0.0 || b_segment_rms == 0.0 {
        return 1.0;
    }
    let gain = a_border_rms / b_segment_rms;
    let max_gain = 10f32.powf((max_gain_db / 20.0) as f32);
    let min_gain = 1.0 / max_gain;
    gain.clamp(min_gain, max_gain)
}

/// Result of bracketing a B fill between matched pre/post borders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillAlignment {
    /// Frame index into the extended B buffer where the fill should start.
    pub start_frame: usize,
    /// B-derived fill length in frames (`post` border starts at `start_frame + fill_frames`).
    pub fill_frames: usize,
    pub pre_correlation: f64,
    pub post_correlation: f64,
}

/// Downmix interleaved i16 PCM to mono `f64` (channel average).
pub fn interleaved_to_mono(samples: &[i16], channels: usize) -> Vec<f64> {
    let channels = channels.max(1);
    samples
        .chunks(channels)
        .map(|frame| frame.iter().map(|&s| f64::from(s)).sum::<f64>() / channels as f64)
        .collect()
}

/// Refined gap boundaries on A's PCM timeline (frame indices, `[start, end)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefinedGapFrames {
    pub start_frame: usize,
    pub end_frame: usize,
}

fn silent_run(
    samples: &[i16],
    channels: usize,
    start_frame: usize,
    run_frames: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> bool {
    (start_frame..start_frame + run_frames).all(|frame| {
        is_silent_frame(
            samples,
            channels,
            frame,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    })
}

/// Returns `true` when a single interleaved frame passes [`is_silent_interleaved`].
pub fn is_silent_frame(
    samples: &[i16],
    channels: usize,
    frame: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> bool {
    let channels = channels.max(1);
    let start = frame * channels;
    let end = start + channels;
    if end > samples.len() {
        return true;
    }
    is_silent_interleaved(
        &samples[start..end],
        channels,
        silence_peak_fraction,
        absolute_rms_floor,
    )
}

/// Tighten a reported gap against A's decoded PCM.
///
/// - Retracts `start` through leading silence before the reported boundary (scanner block
///   quantization can start the run slightly late).
/// - Advances `start` past leading non-silent frames (scanner started the run too early).
/// - Extends `end` through trailing silence (scanner closed the run too early).
pub fn refine_gap_frames(
    samples: &[i16],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
    max_refine_frames: usize,
) -> RefinedGapFrames {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let mut start = start_frame.min(total_frames);
    let mut end = end_frame.min(total_frames);
    if start >= end {
        return RefinedGapFrames {
            start_frame: start,
            end_frame: end,
        };
    }

    let mut budget = max_refine_frames;
    while start > 0
        && budget > 0
        && is_silent_frame(
            samples,
            channels,
            start - 1,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        start -= 1;
        budget -= 1;
    }

    // Peel at most `max_refine_frames` of leading non-silent audio before the reported gap.
    // Cap at `start_frame + max_refine` so a noisy dropout interior cannot push `start` all the
    // way to `end_frame` (block-quantized gaps are typically misaligned by <250 ms).
    let max_start = (start_frame + max_refine_frames).min(end_frame);
    let confirm_frames = (max_refine_frames / 15).clamp(4, 4096);
    while start + confirm_frames <= max_start && budget > 0 {
        if silent_run(
            samples,
            channels,
            start,
            confirm_frames,
            silence_peak_fraction,
            absolute_rms_floor,
        ) {
            break;
        }
        start += 1;
        budget -= 1;
    }

    budget = max_refine_frames;
    while end < total_frames
        && budget > 0
        && is_silent_frame(
            samples,
            channels,
            end,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        end += 1;
        budget -= 1;
    }

    RefinedGapFrames {
        start_frame: start,
        end_frame: end.max(start),
    }
}

struct GapBorderFrameRange {
    pre_start: usize,
    pre_end: usize,
    post_start: usize,
    post_end: usize,
}

/// Gap bounds, template sizing, and silence thresholds shared by the border-template builders.
#[derive(Clone, Copy)]
pub struct GapBorderSpec {
    pub gap_start_frame: usize,
    pub gap_end_frame: usize,
    pub border_frames: usize,
    pub border_standoff_frames: usize,
    pub silence_peak_fraction: f32,
    pub absolute_rms_floor: f32,
}

fn gap_border_frame_range(
    samples: &[i16],
    channels: usize,
    spec: &GapBorderSpec,
) -> GapBorderFrameRange {
    let GapBorderSpec {
        gap_start_frame,
        gap_end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_rms_floor,
    } = *spec;
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;

    let pre_start = gap_start_frame.saturating_sub(border_frames);
    let mut pre_end = gap_start_frame.min(total_frames);
    while pre_end > pre_start
        && is_silent_frame(
            samples,
            channels,
            pre_end - 1,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        pre_end -= 1;
    }

    let post_end = (gap_end_frame + border_frames).min(total_frames);
    let mut post_start = gap_end_frame.min(total_frames);
    while post_start < post_end
        && is_silent_frame(
            samples,
            channels,
            post_start,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        post_start += 1;
    }

    // Skip audio immediately adjacent to the dropout (A-side templates only).
    if border_standoff_frames > 0 {
        pre_end = pre_end.saturating_sub(border_standoff_frames).max(pre_start);
        post_start = (post_start + border_standoff_frames).min(post_end);
    }

    GapBorderFrameRange {
        pre_start,
        pre_end,
        post_start,
        post_end,
    }
}

/// Downmix interleaved PCM to one `f64` vector per channel.
pub fn interleaved_to_channels(samples: &[i16], channels: usize) -> Vec<Vec<f64>> {
    let channels = channels.max(1);
    (0..channels)
        .map(|ch| {
            samples
                .chunks(channels)
                .map(|frame| f64::from(frame[ch]))
                .collect()
        })
        .collect()
}

/// Build mono border templates for seam correlation, skipping silence adjacent to the gap.
pub fn border_templates_for_gap(
    samples: &[i16],
    channels: usize,
    spec: &GapBorderSpec,
) -> (Vec<f64>, Vec<f64>) {
    let channels = channels.max(1);
    let range = gap_border_frame_range(samples, channels, spec);

    let mut pre_mono = if range.pre_end > range.pre_start {
        interleaved_to_mono(
            &samples[range.pre_start * channels..range.pre_end * channels],
            channels,
        )
    } else {
        Vec::new()
    };
    let mut post_mono = if range.post_end > range.post_start {
        interleaved_to_mono(
            &samples[range.post_start * channels..range.post_end * channels],
            channels,
        )
    } else {
        Vec::new()
    };

    pre_mono = trim_low_energy_suffix(&pre_mono);
    post_mono = trim_low_energy_prefix(&post_mono);

    (pre_mono, post_mono)
}

/// Per-channel border templates (same frame ranges as [`border_templates_for_gap`]).
pub fn border_templates_per_channel_for_gap(
    samples: &[i16],
    channels: usize,
    spec: &GapBorderSpec,
) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let channels = channels.max(1);
    let range = gap_border_frame_range(samples, channels, spec);

    let mut pre_ch = if range.pre_end > range.pre_start {
        interleaved_to_channels(
            &samples[range.pre_start * channels..range.pre_end * channels],
            channels,
        )
    } else {
        vec![Vec::new(); channels]
    };
    let mut post_ch = if range.post_end > range.post_start {
        interleaved_to_channels(
            &samples[range.post_start * channels..range.post_end * channels],
            channels,
        )
    } else {
        vec![Vec::new(); channels]
    };

    for ch in &mut pre_ch {
        *ch = trim_low_energy_suffix(ch);
    }
    for ch in &mut post_ch {
        *ch = trim_low_energy_prefix(ch);
    }

    (pre_ch, post_ch)
}

/// Channel indices used for seam scoring on multichannel audio (front L/R).
fn seam_score_channel_indices(channel_count: usize) -> Vec<usize> {
    if channel_count >= 2 {
        vec![0, 1]
    } else {
        vec![0]
    }
}

fn peak_normalize_f64(samples: &[f64]) -> Vec<f64> {
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return samples.to_vec();
    }
    samples.iter().map(|s| s / peak).collect()
}

/// Peak-normalized Pearson correlation (reduces level mismatch between encodes).
fn seam_pearson(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    normalized_correlation(&peak_normalize_f64(left), &peak_normalize_f64(right))
}

fn best_channel_correlation(scores: &[f64]) -> f64 {
    scores
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
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

/// Pearson correlation of A border templates with B fill interior (repeat-at-seam detector).
pub fn fill_repeat_correlations(
    templates: &SeamTemplates<'_>,
    placement: SeamPlacement,
    repeat_window_frames: usize,
) -> (f64, f64) {
    let SeamTemplates { a_pre, a_post, a_pre_ch, a_post_ch, b_mono, b_ch } = *templates;
    let SeamPlacement { start, gap_frames, .. } = placement;
    let repeat_window = repeat_window_frames.max(1);

    let repeat_pre = if !a_pre.is_empty()
        && start + repeat_window <= b_mono.len()
        && repeat_window <= a_pre.len()
    {
        seam_pearson(
            &a_pre[a_pre.len().saturating_sub(repeat_window)..],
            &b_mono[start..start + repeat_window],
        )
    } else {
        0.0
    };

    let tail_start = start + gap_frames.saturating_sub(repeat_window);
    let repeat_post = if !a_post.is_empty()
        && tail_start + repeat_window <= b_mono.len()
        && repeat_window <= a_post.len()
    {
        seam_pearson(
            &a_post[..repeat_window.min(a_post.len())],
            &b_mono[tail_start..tail_start + repeat_window],
        )
    } else {
        0.0
    };

    if b_ch.len() <= 1 {
        return (repeat_pre, repeat_post);
    }

    let ch_pre = if start + repeat_window <= b_mono.len() {
        a_pre_ch
            .iter()
            .zip(b_ch.iter())
            .filter_map(|(a_ch, b_ch)| {
                if repeat_window <= a_ch.len() && start + repeat_window <= b_ch.len() {
                    Some(seam_pearson(
                        &a_ch[a_ch.len().saturating_sub(repeat_window)..],
                        &b_ch[start..start + repeat_window],
                    ))
                } else {
                    None
                }
            })
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        f64::NEG_INFINITY
    };

    let ch_post = if tail_start + repeat_window <= b_mono.len() {
        a_post_ch
            .iter()
            .zip(b_ch.iter())
            .filter_map(|(a_ch, b_ch)| {
                if repeat_window <= a_ch.len() && tail_start + repeat_window <= b_ch.len() {
                    Some(seam_pearson(
                        &a_ch[..repeat_window.min(a_ch.len())],
                        &b_ch[tail_start..tail_start + repeat_window],
                    ))
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

/// Effective crossfade length at gap seams (shared by splice and splice-aware scoring).
pub fn effective_seam_crossfade_frames(
    crossfade_frames: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    total_a_frames: usize,
) -> usize {
    let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
    let pre_available = gap_start_frame;
    let post_available = total_a_frames.saturating_sub(gap_end_frame);
    crossfade_frames
        .min(gap_frames / 2)
        .min(pre_available)
        .min(post_available)
}

/// A-side gap bounds for splice-aware seam scoring on decoded PCM.
#[derive(Clone, Copy)]
pub struct SpliceSeamContext<'a> {
    pub seam_cf: usize,
    pub gap_start_frame: usize,
    pub gap_end_frame: usize,
    pub a_samples: &'a [i16],
    pub channels: usize,
}

fn mono_timeline_frames_f64(
    samples: &[i16],
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
        .map(|&s| f64::from(s))
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
    if ctx.seam_cf > 0 && ctx.gap_start_frame > 0 {
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
    seam_pearson(
        &a_pre[a_pre.len() - w..],
        &fill_mono[..w],
    )
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
    if ctx.seam_cf > 0 && ctx.gap_end_frame < total_frames {
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
    fill_interleaved: &[i16],
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
    let mut pre_scores = Vec::with_capacity(channels);
    let mut post_scores = Vec::with_capacity(channels);
    for ch in 0..channels {
        let ch_fill: Vec<f64> = fill_interleaved
            .iter()
            .skip(ch)
            .step_by(channels)
            .map(|&s| f64::from(s))
            .collect();
        if ch_fill.len() != gap_frames {
            continue;
        }
        let pre = score_splice_pre_seam_channel(
            &ch_fill,
            &a_pre_ch[ch],
            pre_window,
            ctx,
            ch,
        );
        if pre.is_finite() && pre > f64::NEG_INFINITY {
            pre_scores.push(pre);
        }
        let post = score_splice_post_seam_channel(
            &ch_fill,
            &a_post_ch[ch],
            post_window,
            ctx,
            ch,
        );
        if post.is_finite() && post > f64::NEG_INFINITY {
            post_scores.push(post);
        }
    }

    let mono = fill_splice_seam_correlations(
        &fill_mono,
        a_pre,
        a_post,
        pre_window,
        post_window,
        ctx,
    );
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

fn interleaved_channel_timeline_f64(
    samples: &[i16],
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
        .map(|frame| f64::from(samples[frame * channels + channel]))
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
    if ctx.seam_cf > 0 && ctx.gap_start_frame > 0 {
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
    if ctx.seam_cf > 0 && ctx.gap_end_frame < total_frames {
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
    let SeamTemplates { a_pre, a_post, a_pre_ch, a_post_ch, b_mono, b_ch } = *templates;
    let SeamPlacement { start, gap_frames, pre_window, post_window } = placement;
    let use_channels = b_ch.len() > 1
        && a_pre_ch.len() == b_ch.len()
        && a_post_ch.len() == b_ch.len()
        && a_pre_ch.iter().any(|ch| !ch.is_empty());

    let score_pre = pre_window > 0
        && !a_pre.is_empty()
        && start >= pre_window
        && start <= b_mono.len();
    let score_post = post_window > 0
        && !a_post.is_empty()
        && start + gap_frames + post_window <= b_mono.len();

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

    let score_channels = seam_score_channel_indices(b_ch.len());
    let mut pre_scores = Vec::with_capacity(score_channels.len());
    let mut post_scores = Vec::with_capacity(score_channels.len());
    for &ch in &score_channels {
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

/// Drop quiet tail samples (e.g. fade into a dropout) so seam templates use full-level audio.
fn trim_low_energy_suffix(samples: &[f64]) -> Vec<f64> {
    if samples.is_empty() {
        return Vec::new();
    }
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return Vec::new();
    }
    let threshold = peak * 0.12;
    let mut end = samples.len();
    while end > 0 && samples[end - 1].abs() < threshold {
        end -= 1;
    }
    samples[..end].to_vec()
}

/// Drop quiet head samples (e.g. fade out of a dropout) so seam templates use full-level audio.
fn trim_low_energy_prefix(samples: &[f64]) -> Vec<f64> {
    if samples.is_empty() {
        return Vec::new();
    }
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return Vec::new();
    }
    let threshold = peak * 0.12;
    let mut start = 0usize;
    while start < samples.len() && samples[start].abs() < threshold {
        start += 1;
    }
    samples[start..].to_vec()
}

fn blend_samples(a: f32, b: f32, a_weight: f32, b_weight: f32) -> i16 {
    (a_weight * a + b_weight * b)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// Splice `b_fill` into `a_samples` at the gap, crossfading against A's real border audio.
///
/// Pre-seam: equal-power crossfade bleeds the fill head into the last `cf` pre-gap frames only;
/// the gap interior starts at full `b_fill[cf]` so there is no silence ramp inside the dropout.
/// Post-seam: blends fill tail with post-gap head across the boundary (same value on both sides).
///
/// `gap_start_frame` / `gap_end_frame` are frame indices (not interleaved sample indices).
pub fn apply_seam_crossfade(
    a_samples: &mut [i16],
    b_fill: &[i16],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    crossfade_frames: usize,
) {
    let channels = channels.max(1);
    let total_frames = a_samples.len() / channels;
    let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
    if gap_frames == 0 || b_fill.len() < gap_frames * channels {
        return;
    }

    let cf = effective_seam_crossfade_frames(
        crossfade_frames,
        gap_start_frame,
        gap_end_frame,
        total_frames,
    );

    if cf == 0 {
        for frame in gap_start_frame..gap_end_frame {
            for ch in 0..channels {
                let idx = frame * channels + ch;
                let b_idx = (frame - gap_start_frame) * channels + ch;
                a_samples[idx] = b_fill[b_idx];
            }
        }
        return;
    }

    // Pre-seam: crossfade fill head into pre-gap tail only (gap starts at full b_fill[cf]).
    for i in 0..cf {
        let t = i as f32 / cf as f32;
        let a_w = (t * std::f32::consts::FRAC_PI_2).cos();
        let b_w = (t * std::f32::consts::FRAC_PI_2).sin();
        let pre_frame = gap_start_frame - cf + i;
        for ch in 0..channels {
            let pre_idx = pre_frame * channels + ch;
            let a_val = a_samples[pre_idx] as f32;
            let b_val = b_fill[i * channels + ch] as f32;
            a_samples[pre_idx] = blend_samples(a_val, b_val, a_w, b_w);
        }
    }

    // Gap interior: pure fill (first `cf` B frames were consumed in the pre-seam bleed).
    for frame in gap_start_frame..(gap_end_frame - cf) {
        for ch in 0..channels {
            let a_idx = frame * channels + ch;
            let b_idx = (frame - gap_start_frame + cf) * channels + ch;
            a_samples[a_idx] = b_fill[b_idx];
        }
    }

    // Fade-out: blend fill tail with A's post-gap head across the seam.
    for i in 0..cf {
        let t = i as f32 / cf as f32;
        let b_w = (t * std::f32::consts::FRAC_PI_2).cos();
        let a_w = (t * std::f32::consts::FRAC_PI_2).sin();
        let b_frame = gap_frames - cf + i;
        for ch in 0..channels {
            let b_val = b_fill[b_frame * channels + ch] as f32;
            let post_idx = (gap_end_frame + i) * channels + ch;
            let a_val = a_samples[post_idx] as f32;
            let blended = blend_samples(a_val, b_val, a_w, b_w);
            let gap_idx = (gap_end_frame - cf + i) * channels + ch;
            a_samples[gap_idx] = blended;
            a_samples[post_idx] = blended;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clip_sync::MultiChannelPcm;

    fn mono_pcm(rate: u32, samples: Vec<i16>) -> MultiChannelPcm {
        MultiChannelPcm {
            sample_rate: rate,
            channels: 1,
            samples,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
        }
    }

    #[test]
    fn empty_clip_is_silent() {
        assert!(is_silent(&[], 0.01, 0.0));
    }

    #[test]
    fn all_zeros_is_silent() {
        assert!(is_silent(&vec![0i16; 1000], 0.01, 0.0));
    }

    #[test]
    fn loud_sine_is_not_silent() {
        let samples: Vec<i16> = (0..1000)
            .map(|i| (f32::sin(i as f32 * 0.1) * 10_000.0) as i16)
            .collect();
        assert!(!is_silent(&samples, 0.01, 0.0));
    }

    #[test]
    fn single_spike_in_sea_of_zeros_is_silent() {
        // Peak = 100, threshold = 100 * 0.01 = 1.0.
        // 1 spike in 11025 zeros: RMS = sqrt(10000/11025) ≈ 0.95 < 1.0 → silent.
        let mut samples = vec![0i16; 11_025];
        samples[0] = 100;
        assert!(is_silent(&samples, 0.01, 0.0));
    }

    #[test]
    fn absolute_floor_catches_low_level_codec_noise() {
        // All samples at ±1: peak = 1, RMS ≈ 1.
        // Relative check alone (RMS ≈ peak → fraction ≈ 1.0 >> 0.01) would NOT flag as silent.
        // Peak-floor check: peak(1) < floor(2) → silent.
        let samples: Vec<i16> = (0..11_025).map(|i| if i % 2 == 0 { 1 } else { -1 }).collect();
        assert!(!is_silent(&samples, 0.01, 0.0), "no floor: should not be silent");
        assert!(is_silent(&samples, 0.01, 2.0), "floor=2: peak=1 < 2 → silent");
        // A signal with peak above the floor is NOT classified silent by the floor check alone.
        let loud_samples: Vec<i16> = (0..11_025).map(|i| if i % 2 == 0 { 5 } else { -5 }).collect();
        assert!(!is_silent(&loud_samples, 0.01, 2.0), "floor=2: peak=5 > 2 → not silent by floor");
    }

    fn sine_samples(rate: u32, secs: f64) -> Vec<i16> {
        let count = (rate as f64 * secs).round() as usize;
        (0..count)
            .map(|i| (f32::sin(i as f32 * 0.3) * 8_000.0) as i16)
            .collect()
    }

    #[test]
    fn silence_run_scanner_detects_three_second_gap() {
        let rate = 11_025u32;
        let block_secs = 0.25;
        let mut samples = sine_samples(rate, 5.0);
        samples.extend(std::iter::repeat_n(0i16, (rate as f64 * 3.0).round() as usize));
        samples.extend(sine_samples(rate, 5.0));

        let pcm = mono_pcm(rate, samples);

        let mut scanner = SilenceRunScanner::new(block_secs, 0.01, 1.0, 0, 0.0);
        scanner.feed(&pcm, 0.0);
        let runs = scanner.finish();

        assert_eq!(runs.len(), 1);
        assert!((runs[0].start_secs - 5.0).abs() < block_secs);
        assert!((runs[0].end_secs - 8.0).abs() < block_secs);
    }

    #[test]
    fn silence_run_scanner_merges_runs_across_feed_calls() {
        let rate = 11_025u32;
        let block_secs = 0.25;
        let mut scanner = SilenceRunScanner::new(block_secs, 0.01, 1.0, 0, 0.0);

        let first = mono_pcm(rate, vec![0i16; (rate as f64 * 2.0).round() as usize]);
        let second = mono_pcm(rate, vec![0i16; (rate as f64 * 2.0).round() as usize]);

        scanner.feed(&first, 0.0);
        scanner.feed(&second, 2.0);
        let runs = scanner.finish();

        assert_eq!(runs.len(), 1);
        assert!((runs[0].start_secs - 0.0).abs() < 0.001);
        assert!((runs[0].end_secs - 4.0).abs() < block_secs);
    }

    #[test]
    fn silence_run_scanner_ignores_gaps_shorter_than_min() {
        let rate = 11_025u32;
        let block_secs = 0.25;
        let mut samples = sine_samples(rate, 2.0);
        samples.extend(vec![0i16; (rate as f64 * 0.5).round() as usize]);
        samples.extend(sine_samples(rate, 2.0));

        let pcm = mono_pcm(rate, samples);

        let mut scanner = SilenceRunScanner::new(block_secs, 0.01, 1.0, 0, 0.0);
        scanner.feed(&pcm, 0.0);
        assert!(scanner.finish().is_empty());
    }

    #[test]
    fn silence_run_scanner_hold_bridges_single_noisy_block() {
        // Build the signal aligned to block boundaries so the noisy region occupies
        // exactly one block and doesn't bleed into adjacent blocks.
        let rate = 11_025u32;
        let block_secs = 0.25;
        let block_samples = (block_secs * rate as f64).round() as usize;

        // 8 silent blocks + 1 noisy block + 8 silent blocks
        let mut samples = vec![0i16; block_samples * 8];
        samples.extend(sine_samples(rate, block_secs));
        samples.extend(vec![0i16; block_samples * 8]);

        let pcm = mono_pcm(rate, samples);

        let mut scanner = SilenceRunScanner::new(block_secs, 0.01, 1.0, 1, 0.0);
        scanner.feed(&pcm, 0.0);
        let runs = scanner.finish();

        assert_eq!(runs.len(), 1, "hold=1 should merge across the single noisy block");
        // Total span is 17 blocks = 4.25s; duration should be close to that.
        assert!(runs[0].end_secs - runs[0].start_secs >= 4.0,
            "merged run should span most of the 4.25s signal, got {}s", runs[0].end_secs - runs[0].start_secs);
    }

    #[test]
    fn silence_run_scanner_hold_zero_splits_on_any_noise() {
        // Same block-aligned signal but hold=0 → two separate runs.
        let rate = 11_025u32;
        let block_secs = 0.25;
        let block_samples = (block_secs * rate as f64).round() as usize;

        let mut samples = vec![0i16; block_samples * 8];
        samples.extend(sine_samples(rate, block_secs));
        samples.extend(vec![0i16; block_samples * 8]);

        let pcm = mono_pcm(rate, samples);

        let mut scanner = SilenceRunScanner::new(block_secs, 0.01, 1.0, 0, 0.0);
        scanner.feed(&pcm, 0.0);
        assert_eq!(scanner.finish().len(), 2, "hold=0 should split at the noisy block");
    }

    #[test]
    fn stereo_left_only_is_not_silent_when_right_is_quiet() {
        let rate = 11_025u32;
        let frames = rate as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let tone = (f32::sin(i as f32 * 0.3) * 8_000.0) as i16;
            samples.push(tone);
            samples.push(0);
        }
        assert!(
            !is_silent_interleaved(&samples, 2, 0.01, 0.0),
            "one hot channel should prevent silence"
        );
    }

    #[test]
    fn stereo_both_channels_quiet_is_silent() {
        let samples = vec![0i16; 11_025 * 2];
        assert!(is_silent_interleaved(&samples, 2, 0.01, 0.0));
    }

    #[test]
    fn rms_interleaved_of_constant() {
        let samples = vec![1000i16; 100];
        let result = rms_interleaved(&samples);
        assert!((result - 1000.0).abs() < 1.0, "rms of constant 1000 should be ~1000, got {result}");
    }

    #[test]
    fn compute_fill_gain_clamps_to_max_db() {
        // a=1000, b=1 => raw gain=1000; max_db=12 => max_gain=10^(12/20)≈3.981
        let gain = compute_fill_gain(1000.0, 1.0, 12.0);
        let expected_max = 10f32.powf(12.0 / 20.0);
        assert!((gain - expected_max).abs() < 0.001, "gain should be clamped to {expected_max}, got {gain}");
    }

    #[test]
    fn compute_fill_gain_unity_when_rms_zero() {
        assert_eq!(compute_fill_gain(0.0, 500.0, 12.0), 1.0);
        assert_eq!(compute_fill_gain(500.0, 0.0, 12.0), 1.0);
    }

    #[test]
    fn fill_splice_seam_correlations_uses_crossfade_offset() {
        let pre_window = 2usize;
        let post_window = 2usize;
        let cf = 2usize;
        let gap_start = 4usize;
        let gap_end = 6usize;
        // A: ramp into gap; B fill matches the pre/post windows at splice time.
        let a_samples: Vec<i16> = vec![100, 200, 300, 400, 0, 0, 500, 600, 0, 0];
        let fill = vec![300.0, 400.0, 0.0, 0.0, 500.0, 600.0];
        let a_pre = vec![1.0, 0.5];
        let a_post = vec![0.8, -0.6];
        let ctx = SpliceSeamContext {
            seam_cf: cf,
            gap_start_frame: gap_start,
            gap_end_frame: gap_end,
            a_samples: &a_samples,
            channels: 1,
        };

        let (pre_cf, post_cf) = fill_splice_seam_correlations(
            &fill,
            &a_pre,
            &a_post,
            pre_window,
            post_window,
            ctx,
        );
        let (pre_no_cf, post_no_cf) = fill_splice_seam_correlations(
            &fill,
            &a_pre,
            &a_post,
            pre_window,
            post_window,
            SpliceSeamContext {
                seam_cf: 0,
                gap_start_frame: gap_start,
                gap_end_frame: gap_end,
                a_samples: &a_samples,
                channels: 1,
            },
        );

        assert!(pre_cf > pre_no_cf + 0.5, "pre should score bleed tail on A timeline");
        assert!(post_cf > post_no_cf + 0.5, "post should score fade head on A timeline");
        assert!(pre_cf > 0.9 && post_cf > 0.9);
    }

    #[test]
    fn apply_seam_crossfade_bleeds_fill_head_into_pre_gap_tail() {
        // Layout: [pre-border loud][gap silent][post-border loud]
        let cf = 4usize;
        let gap_start = 10usize;
        let gap_end = 20usize;
        let gap_frames = gap_end - gap_start;

        let mut a = vec![0i16; 30];
        for s in &mut a[0..gap_start] {
            *s = 8_000;
        }
        for s in &mut a[gap_end..] {
            *s = 8_000;
        }

        let b_fill = vec![4_000i16; gap_frames];
        apply_seam_crossfade(&mut a, &b_fill, 1, gap_start, gap_end, cf);

        assert_eq!(
            a[gap_start - cf - 1],
            8_000,
            "pre-gap audio before the crossfade window should be untouched"
        );
        assert_eq!(
            a[gap_start - cf],
            8_000,
            "crossfade should start from pure pre-gap audio"
        );
        assert_eq!(
            a[gap_start],
            4_000,
            "gap should start at full fill level, not a silence ramp"
        );
        assert_eq!(a[gap_start + 1], 4_000, "gap interior should be pure fill");
        assert!(
            a[gap_start - 1] > 3_000,
            "pre-gap tail should bleed into fill before the gap boundary"
        );
    }

    #[test]
    fn apply_seam_crossfade_is_continuous_at_pre_seam() {
        let cf = 4usize;
        let gap_start = 10usize;
        let gap_end = 20usize;
        let gap_frames = gap_end - gap_start;

        let mut a = vec![0i16; 30];
        for s in &mut a[0..gap_start] {
            *s = 8_000;
        }
        for s in &mut a[gap_end..] {
            *s = 8_000;
        }

        let b_fill = vec![4_000i16; gap_frames];
        apply_seam_crossfade(&mut a, &b_fill, 1, gap_start, gap_end, cf);

        let diff = (a[gap_start] as i32 - a[gap_start - 1] as i32).abs();
        assert!(
            diff <= 4_500,
            "jump of {diff} across pre seam ({} -> {})",
            a[gap_start - 1],
            a[gap_start]
        );
    }

    #[test]
    fn refine_gap_frames_retracts_through_leading_silence_before_reported_start() {
        let channels = 2usize;
        // [loud 5][silent 10][loud 5] — reported gap starts two frames late.
        let mut samples = Vec::new();
        for _ in 0..5 {
            samples.extend([8_000i16, 8_000i16]);
        }
        for _ in 0..10 {
            samples.extend([0i16, 0i16]);
        }
        for _ in 0..5 {
            samples.extend([8_000i16, 8_000i16]);
        }

        let refined = refine_gap_frames(&samples, channels, 7, 14, 0.01, 0.0, 10);
        assert_eq!(refined.start_frame, 5);
        assert_eq!(refined.end_frame, 15);
    }

    #[test]
    fn refine_gap_frames_advances_past_leading_audio_and_extends_trailing_silence() {
        let channels = 2usize;
        // [loud 5][silent 10][loud 5] — reported gap starts one frame too early and ends one frame too early.
        let mut samples = Vec::new();
        for _ in 0..5 {
            samples.extend([8_000i16, 8_000i16]);
        }
        for _ in 0..10 {
            samples.extend([0i16, 0i16]);
        }
        for _ in 0..5 {
            samples.extend([8_000i16, 8_000i16]);
        }

        let refined = refine_gap_frames(&samples, channels, 4, 14, 0.01, 0.0, 10);
        assert_eq!(refined.start_frame, 5);
        assert_eq!(refined.end_frame, 15);
    }

    #[test]
    fn refine_gap_frames_caps_start_advance_before_reported_end() {
        let channels = 1usize;
        let mut samples = Vec::new();
        // Leading audio (2 frames), low-level dropout (8 frames), trailing audio.
        samples.extend([8_000i16; 2]);
        samples.extend([3i16; 8]);
        samples.extend([8_000i16; 2]);

        let refined = refine_gap_frames(
            &samples,
            channels,
            2,
            10,
            0.01,
            0.0,
            20,
        );
        assert!(
            refined.start_frame < 10,
            "start should not advance all the way to reported end"
        );
        assert_eq!(refined.end_frame, 10);
    }

    fn test_border_spec(
        gap_start_frame: usize,
        gap_end_frame: usize,
        border_frames: usize,
        border_standoff_frames: usize,
    ) -> GapBorderSpec {
        GapBorderSpec {
            gap_start_frame,
            gap_end_frame,
            border_frames,
            border_standoff_frames,
            silence_peak_fraction: 0.01,
            absolute_rms_floor: 0.0,
        }
    }

    #[test]
    fn border_templates_trim_quiet_fade_before_dropout() {
        let channels = 1usize;
        let mut samples = Vec::new();
        samples.extend(vec![8_000i16; 8]);
        samples.extend(vec![200i16; 4]);
        samples.extend(vec![0i16; 4]);
        samples.extend(vec![200i16; 4]);
        samples.extend(vec![8_000i16; 8]);

        let (pre, post) = border_templates_for_gap(&samples, channels, &test_border_spec(16, 20, 12, 0));
        assert!(!pre.is_empty());
        assert!(pre.iter().all(|&v| v.abs() > 1_000.0));
        assert!(!post.is_empty());
        assert!(post.iter().all(|&v| v.abs() > 1_000.0));
    }

    #[test]
    fn border_standoff_excludes_audio_adjacent_to_dropout() {
        let channels = 1usize;
        let mut samples = vec![8_000i16; 20];
        samples.extend(vec![0i16; 5]);
        samples.extend(vec![8_000i16; 20]);

        let (pre_no, _) =
            border_templates_for_gap(&samples, channels, &test_border_spec(20, 25, 15, 0));
        let (pre_standoff, _) =
            border_templates_for_gap(&samples, channels, &test_border_spec(20, 25, 15, 5));
        assert_eq!(pre_no.len(), 15);
        assert_eq!(pre_standoff.len(), 10);
    }

    #[test]
    fn border_templates_for_gap_skip_adjacent_silence() {
        let channels = 1usize;
        let mut samples = vec![8_000i16; 5];
        samples.extend(vec![0i16; 5]);
        samples.extend(vec![8_000i16; 5]);

        let (pre, post) = border_templates_for_gap(&samples, channels, &test_border_spec(5, 10, 5, 0));
        assert_eq!(pre.len(), 5);
        assert!(pre.iter().all(|&v| (v - 8_000.0).abs() < 1.0));
        assert_eq!(post.len(), 5);
        assert!(post.iter().all(|&v| (v - 8_000.0).abs() < 1.0));
    }
}
