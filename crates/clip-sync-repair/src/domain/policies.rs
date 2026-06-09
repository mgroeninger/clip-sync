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

const DISCOVERY_TEMPLATE_POINTS: usize = 4_000;
const DISCOVERY_HAYSTACK_POINTS: usize = 24_000;
const DISCOVERY_COARSE_MIN_CORRELATION: f64 = 0.25;
const DISCOVERY_SCORE_TIE_EPSILON: f64 = 1e-6;
const DISCOVERY_REFINE_RADIUS_FACTOR: usize = 2;

/// Result of sliding a candidate B segment to match A's pre- and post-gap borders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillAlignment {
    /// Frame index into the extended B buffer where the fill should start.
    pub start_frame: usize,
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

    // Peel at most `max_refine_frames` of leading non-silent audio before the reported gap.
    // Cap at `start_frame + max_refine` so a noisy dropout interior cannot push `start` all the
    // way to `end_frame` (block-quantized gaps are typically misaligned by <250 ms).
    let max_start = (start_frame + max_refine_frames).min(end_frame);
    let confirm_frames = (max_refine_frames / 15).max(4).min(4096);
    let mut budget = max_refine_frames;
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

fn gap_border_frame_range(
    samples: &[i16],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    border_frames: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> GapBorderFrameRange {
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
    gap_start_frame: usize,
    gap_end_frame: usize,
    border_frames: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> (Vec<f64>, Vec<f64>) {
    let channels = channels.max(1);
    let range = gap_border_frame_range(
        samples,
        channels,
        gap_start_frame,
        gap_end_frame,
        border_frames,
        silence_peak_fraction,
        absolute_rms_floor,
    );

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
    gap_start_frame: usize,
    gap_end_frame: usize,
    border_frames: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let channels = channels.max(1);
    let range = gap_border_frame_range(
        samples,
        channels,
        gap_start_frame,
        gap_end_frame,
        border_frames,
        silence_peak_fraction,
        absolute_rms_floor,
    );

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

fn median_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

/// Pearson correlation at gap seams; uses per-channel median when `channels > 1`.
fn seam_correlations_at(
    a_pre: &[f64],
    a_post: &[f64],
    a_pre_ch: &[Vec<f64>],
    a_post_ch: &[Vec<f64>],
    b_mono: &[f64],
    b_ch: &[Vec<f64>],
    start: usize,
    gap_frames: usize,
    pre_window: usize,
    post_window: usize,
) -> (f64, f64) {
    let use_channels = b_ch.len() > 1
        && a_pre_ch.len() == b_ch.len()
        && a_post_ch.len() == b_ch.len()
        && a_pre_ch.iter().any(|ch| !ch.is_empty());

    if !use_channels {
        let pre = normalized_correlation(
            &a_pre[a_pre.len().saturating_sub(pre_window)..],
            &b_mono[start.saturating_sub(pre_window)..start],
        );
        let post = normalized_correlation(
            &a_post[..post_window.min(a_post.len())],
            &b_mono[start + gap_frames..start + gap_frames + post_window],
        );
        return (pre, post);
    }

    let mut pre_scores = Vec::with_capacity(b_ch.len());
    let mut post_scores = Vec::with_capacity(b_ch.len());
    for ch in 0..b_ch.len() {
        if a_pre_ch[ch].len() < pre_window || a_post_ch[ch].len() < post_window {
            continue;
        }
        if start < pre_window || start + gap_frames + post_window > b_ch[ch].len() {
            continue;
        }
        pre_scores.push(normalized_correlation(
            &a_pre_ch[ch][a_pre_ch[ch].len() - pre_window..],
            &b_ch[ch][start - pre_window..start],
        ));
        post_scores.push(normalized_correlation(
            &a_post_ch[ch][..post_window],
            &b_ch[ch][start + gap_frames..start + gap_frames + post_window],
        ));
    }

    if pre_scores.is_empty() || post_scores.is_empty() {
        let pre = normalized_correlation(
            &a_pre[a_pre.len().saturating_sub(pre_window)..],
            &b_mono[start.saturating_sub(pre_window)..start],
        );
        let post = normalized_correlation(
            &a_post[..post_window.min(a_post.len())],
            &b_mono[start + gap_frames..start + gap_frames + post_window],
        );
        return (pre, post);
    }

    (median_f64(&pre_scores), median_f64(&post_scores))
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

fn choose_discovery_downsample(template_len: usize, haystack_len: usize) -> usize {
    let mut factor = 1usize;
    while template_len / factor > DISCOVERY_TEMPLATE_POINTS {
        factor += 1;
    }
    while haystack_len / factor > DISCOVERY_HAYSTACK_POINTS {
        factor += 1;
    }
    factor.max(1)
}

fn downsample_f64(samples: &[f64], factor: usize) -> Vec<f64> {
    if factor <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(factor)
        .map(|chunk| chunk.iter().sum::<f64>() / chunk.len() as f64)
        .collect()
}

fn template_correlation_at_lag(
    template_mono: &[f64],
    template_ch: Option<&[Vec<f64>]>,
    haystack_mono: &[f64],
    haystack_ch: Option<&[Vec<f64>]>,
    lag: usize,
) -> f64 {
    if let (Some(t_ch), Some(h_ch)) = (template_ch, haystack_ch) {
        if t_ch.len() > 1 && h_ch.len() == t_ch.len() {
            let template_len = template_mono.len();
            let mut scores = Vec::with_capacity(t_ch.len());
            for (t, h) in t_ch.iter().zip(h_ch.iter()) {
                if t.len() != template_len || lag + template_len > h.len() {
                    continue;
                }
                scores.push(normalized_correlation(t, &h[lag..lag + template_len]));
            }
            if !scores.is_empty() {
                return median_f64(&scores);
            }
        }
    }
    normalized_correlation(template_mono, &haystack_mono[lag..lag + template_mono.len()])
}

/// Slide `template` across `haystack` near `nominal_frame`, searching ±`search_radius_frames`.
pub fn discover_mono_template_near(
    template: &[f64],
    haystack: &[f64],
    nominal_frame: usize,
    search_radius_frames: usize,
) -> Option<(usize, f64)> {
    discover_template_near(template, None, haystack, None, nominal_frame, search_radius_frames)
}

fn discover_template_near(
    template_mono: &[f64],
    template_ch: Option<&[Vec<f64>]>,
    haystack_mono: &[f64],
    haystack_ch: Option<&[Vec<f64>]>,
    nominal_frame: usize,
    search_radius_frames: usize,
) -> Option<(usize, f64)> {
    if template_mono.is_empty() || template_mono.len() > haystack_mono.len() {
        return None;
    }

    let downsample = choose_discovery_downsample(template_mono.len(), haystack_mono.len());
    let template_ds = downsample_f64(template_mono, downsample);
    let haystack_ds = downsample_f64(haystack_mono, downsample);
    if haystack_ds.len() < template_ds.len() || template_ds.is_empty() {
        return None;
    }

    let nominal_ds = nominal_frame / downsample;
    let radius_ds = (search_radius_frames / downsample).max(1);
    let min_lag = nominal_ds.saturating_sub(radius_ds);
    let max_lag = (nominal_ds + radius_ds).min(haystack_ds.len() - template_ds.len());

    let template_ch_ds: Option<Vec<Vec<f64>>> = template_ch.map(|ch| {
        ch.iter()
            .map(|c| downsample_f64(c, downsample))
            .collect()
    });
    let haystack_ch_ds: Option<Vec<Vec<f64>>> = haystack_ch.map(|ch| {
        ch.iter()
            .map(|c| downsample_f64(c, downsample))
            .collect()
    });
    let template_ch_ds_ref = template_ch_ds.as_deref();
    let haystack_ch_ds_ref = haystack_ch_ds.as_deref();

    let mut best_lag = min_lag;
    let mut best_score = f64::NEG_INFINITY;
    for lag in min_lag..=max_lag {
        let score = template_correlation_at_lag(
            &template_ds,
            template_ch_ds_ref,
            &haystack_ds,
            haystack_ch_ds_ref,
            lag,
        );
        let better = score > best_score + DISCOVERY_SCORE_TIE_EPSILON
            || (score >= best_score - DISCOVERY_SCORE_TIE_EPSILON
                && lag.abs_diff(nominal_ds) < best_lag.abs_diff(nominal_ds));
        if better {
            best_score = score;
            best_lag = lag;
        }
    }

    if !best_score.is_finite() {
        return None;
    }

    let refine_radius = downsample.saturating_mul(DISCOVERY_REFINE_RADIUS_FACTOR).max(1);
    let coarse_start = best_lag.saturating_mul(downsample);
    let search_min = nominal_frame.saturating_sub(search_radius_frames);
    let search_max = (nominal_frame + search_radius_frames)
        .min(haystack_mono.len().saturating_sub(template_mono.len()));
    let refine_min = coarse_start.saturating_sub(refine_radius).max(search_min);
    let refine_max = (coarse_start + refine_radius).min(search_max);

    let mut best_start = coarse_start.min(search_max);
    let mut best_full_score = best_score;
    let mut pos = refine_min;
    while pos <= refine_max {
        let score = template_correlation_at_lag(
            template_mono,
            template_ch,
            haystack_mono,
            haystack_ch,
            pos,
        );
        let better = score > best_full_score + DISCOVERY_SCORE_TIE_EPSILON
            || (score >= best_full_score - DISCOVERY_SCORE_TIE_EPSILON
                && pos.abs_diff(nominal_frame) < best_start.abs_diff(nominal_frame));
        if better {
            best_full_score = score;
            best_start = pos;
        }
        pos += 1;
    }

    Some((best_start, best_full_score))
}

/// Locate the B gap start by matching A's pre/post borders in a wide search window.
///
/// When both borders match consistently (fill length ≈ A's gap), that position wins.
/// Otherwise falls back to the best pre-border match, then the offset-mapped nominal.
pub fn discover_fill_start_in_b(
    a_pre_border: &[f64],
    a_post_border: &[f64],
    b_mono: &[f64],
    a_pre_ch: &[Vec<f64>],
    a_post_ch: &[Vec<f64>],
    b_ch: &[Vec<f64>],
    offset_nominal_start: usize,
    gap_start_in_haystack: usize,
    gap_end_in_haystack: usize,
    gap_frames: usize,
    discovery_frames: usize,
    search_radius_frames: usize,
) -> usize {
    if discovery_frames == 0 || a_pre_border.is_empty() || a_post_border.is_empty() {
        return offset_nominal_start;
    }

    let use_channels = b_ch.len() > 1 && a_pre_ch.len() == b_ch.len();
    let pre_ch = if use_channels { Some(a_pre_ch) } else { None };
    let post_ch = if use_channels { Some(a_post_ch) } else { None };
    let b_ch_opt = if use_channels { Some(b_ch) } else { None };

    let pre_len = discovery_frames.min(a_pre_border.len());
    let pre_template = &a_pre_border[a_pre_border.len() - pre_len..];
    let pre_template_ch: Option<Vec<Vec<f64>>> = pre_ch.map(|ch| {
        ch.iter()
            .map(|c| {
                let start = c.len().saturating_sub(pre_len);
                c[start..].to_vec()
            })
            .collect()
    });
    let pre_nominal = gap_start_in_haystack.saturating_sub(pre_len);

    let post_len = discovery_frames.min(a_post_border.len());
    let post_template = &a_post_border[..post_len];
    let post_template_ch: Option<Vec<Vec<f64>>> = post_ch.map(|ch| {
        ch.iter()
            .map(|c| c[..post_len.min(c.len())].to_vec())
            .collect()
    });
    let post_nominal = gap_end_in_haystack;

    let pre_hit = discover_template_near(
        pre_template,
        pre_template_ch.as_deref(),
        b_mono,
        b_ch_opt,
        pre_nominal,
        search_radius_frames,
    )
    .filter(|(_, score)| *score >= DISCOVERY_COARSE_MIN_CORRELATION);

    let post_hit = discover_template_near(
        post_template,
        post_template_ch.as_deref(),
        b_mono,
        b_ch_opt,
        post_nominal,
        search_radius_frames,
    )
    .filter(|(_, score)| *score >= DISCOVERY_COARSE_MIN_CORRELATION);

    if let (Some((pre_start, _)), Some((post_start, _))) = (pre_hit, post_hit) {
        let start = pre_start + pre_len;
        let end = post_start;
        let len = end.saturating_sub(start);
        let len_slack = (gap_frames / 2).max(search_radius_frames.min(gap_frames));
        if end > start && len.abs_diff(gap_frames) <= len_slack {
            return start;
        }
    }

    pre_hit
        .map(|(match_start, _)| match_start + pre_len)
        .unwrap_or(offset_nominal_start)
}

/// Slide a candidate B window to maximize agreement with A's borders at both gap seams.
///
/// `nominal_start_frame` is where the coarse offset maps the fill inside `b_extended`.
/// Search is limited to ±`max_adjustment_frames` around that position.
pub fn align_fill_segment(
    a_pre_border: &[f64],
    a_post_border: &[f64],
    b_extended: &[f64],
    a_pre_ch: &[Vec<f64>],
    a_post_ch: &[Vec<f64>],
    b_ch: &[Vec<f64>],
    gap_frames: usize,
    nominal_start_frame: usize,
    correlate_frames: usize,
    max_adjustment_frames: usize,
) -> Option<FillAlignment> {
    if gap_frames == 0 || correlate_frames == 0 || a_pre_border.is_empty() || a_post_border.is_empty()
    {
        return None;
    }

    let pre_window = correlate_frames.min(a_pre_border.len());
    let post_window = correlate_frames.min(a_post_border.len());
    if pre_window == 0 || post_window == 0 {
        return None;
    }

    // Fast slide search with a shorter template; re-score the winner at full width for the gate.
    let search_frames = correlate_frames.min(12_000).max(1);
    let search_pre = search_frames.min(a_pre_border.len());
    let search_post = search_frames.min(a_post_border.len());
    if search_pre == 0 || search_post == 0 {
        return None;
    }

    let mut best_start = nominal_start_frame;
    let mut best_search_score = f64::NEG_INFINITY;

    for delta in -(max_adjustment_frames as i64)..=(max_adjustment_frames as i64) {
        let start = nominal_start_frame as i64 + delta;
        if start < 0 {
            continue;
        }
        let start = start as usize;
        if start + gap_frames > b_extended.len()
            || start + gap_frames + search_post > b_extended.len()
            || start < search_pre
        {
            continue;
        }

        let (pre_corr, post_corr) = seam_correlations_at(
            a_pre_border,
            a_post_border,
            a_pre_ch,
            a_post_ch,
            b_extended,
            b_ch,
            start,
            gap_frames,
            search_pre,
            search_post,
        );
        let score = pre_corr.min(post_corr);

        let is_better = |candidate_score: f64, candidate_start: usize| -> bool {
            if score > candidate_score + f64::EPSILON {
                return true;
            }
            if (score - candidate_score).abs() > f64::EPSILON {
                return false;
            }
            start.abs_diff(nominal_start_frame) < candidate_start.abs_diff(nominal_start_frame)
        };

        if is_better(best_search_score, best_start) {
            best_search_score = score;
            best_start = start;
        }
    }

    if !best_search_score.is_finite() {
        return None;
    }

    if best_start + gap_frames > b_extended.len()
        || best_start + gap_frames + post_window > b_extended.len()
        || best_start < pre_window
    {
        return None;
    }

    let (pre_correlation, post_correlation) = seam_correlations_at(
        a_pre_border,
        a_post_border,
        a_pre_ch,
        a_post_ch,
        b_extended,
        b_ch,
        best_start,
        gap_frames,
        pre_window,
        post_window,
    );

    Some(FillAlignment {
        start_frame: best_start,
        pre_correlation,
        post_correlation,
    })
}

fn blend_samples(a: f32, b: f32, a_weight: f32, b_weight: f32) -> i16 {
    (a_weight * a + b_weight * b)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// Splice `b_fill` into `a_samples` at the gap, crossfading against A's real border audio.
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

    let pre_available = gap_start_frame;
    let post_available = total_frames.saturating_sub(gap_end_frame);
    let cf = crossfade_frames
        .min(gap_frames / 2)
        .min(pre_available)
        .min(post_available);

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

    // Fade-in: blend A's pre-gap tail with the head of the fill.
    for i in 0..cf {
        let frame = gap_start_frame - cf + i;
        let t = i as f32 / cf as f32;
        let a_w = (t * std::f32::consts::FRAC_PI_2).cos();
        let b_w = (t * std::f32::consts::FRAC_PI_2).sin();
        for ch in 0..channels {
            let a_idx = frame * channels + ch;
            let b_idx = i * channels + ch;
            a_samples[a_idx] = blend_samples(
                a_samples[a_idx] as f32,
                b_fill[b_idx] as f32,
                a_w,
                b_w,
            );
        }
    }

    // Middle: pure fill (offset by `cf` frames consumed in the fade-in).
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

/// Equal-power crossfade: blend `fill` into `into` at both seams.
///
/// The effective crossfade length is `crossfade_frames.min(total_frames / 2)`.
/// - Fade-in (first `cf` frames): a_w = cos(t*π/2), b_w = sin(t*π/2)
/// - Middle: pure fill
/// - Fade-out (last `cf` frames): a_w = sin(t*π/2), b_w = cos(t*π/2)
///
/// Samples are written into `into` — `into` contains A's original samples and is replaced.
pub fn apply_crossfade(into: &mut [i16], fill: &[i16], channels: usize, crossfade_frames: usize) {
    let channels = channels.max(1);
    let total_frames = into.len() / channels;
    let cf = crossfade_frames.min(total_frames / 2);

    for frame in 0..total_frames {
        if cf == 0 || (frame >= cf && frame < total_frames - cf) {
            // Middle: pure fill
            for ch in 0..channels {
                let idx = frame * channels + ch;
                if idx < fill.len() {
                    into[idx] = fill[idx];
                }
            }
        } else if frame < cf {
            // Fade-in: blend from A into fill
            let t = frame as f32 / cf as f32;
            let a_w = (t * std::f32::consts::FRAC_PI_2).cos();
            let b_w = (t * std::f32::consts::FRAC_PI_2).sin();
            for ch in 0..channels {
                let idx = frame * channels + ch;
                let a_val = into[idx] as f32;
                let b_val = if idx < fill.len() { fill[idx] as f32 } else { 0.0 };
                into[idx] = (a_w * a_val + b_w * b_val)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            }
        } else {
            // Fade-out: blend from fill back into A
            let t = (frame - (total_frames - cf)) as f32 / cf as f32;
            let a_w = (t * std::f32::consts::FRAC_PI_2).sin();
            let b_w = (t * std::f32::consts::FRAC_PI_2).cos();
            for ch in 0..channels {
                let idx = frame * channels + ch;
                let a_val = into[idx] as f32;
                let b_val = if idx < fill.len() { fill[idx] as f32 } else { 0.0 };
                into[idx] = (a_w * a_val + b_w * b_val)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            }
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
    fn apply_crossfade_middle_is_pure_fill() {
        // 10 mono frames, fill = all 1000, A = all 0, cf=2
        let fill = vec![1000i16; 10];
        let mut into = vec![0i16; 10];
        apply_crossfade(&mut into, &fill, 1, 2);
        // Middle frames [2..8) should be pure fill = 1000
        for i in 2..8 {
            assert_eq!(into[i], 1000, "frame {i} should be 1000 (pure fill)");
        }
    }

    #[test]
    fn apply_crossfade_is_continuous() {
        // A=0, B=1000, cf=4, n=10 mono frames
        let fill = vec![1000i16; 10];
        let mut into = vec![0i16; 10];
        apply_crossfade(&mut into, &fill, 1, 4);
        for i in 1..into.len() {
            let diff = (into[i] as i32 - into[i - 1] as i32).abs();
            assert!(diff <= 500, "jump of {diff} between frame {} and {} (values {} {})", i-1, i, into[i-1], into[i]);
        }
    }

    #[test]
    fn discover_mono_template_near_finds_offset_match() {
        let rate = 1000.0;
        let template: Vec<f64> = (0..200)
            .map(|i| (std::f32::consts::TAU as f64 * 3.0 * i as f64 / rate).sin())
            .collect();
        let mut haystack = vec![0.0; 50];
        haystack.extend(&template);
        haystack.extend(vec![0.0; 80]);

        let true_start = 50usize;
        let nominal = true_start + 35;
        let found = discover_mono_template_near(&template, &haystack, nominal, 60)
            .expect("discovery");
        assert!(
            found.0.abs_diff(true_start) <= 2,
            "expected start near {true_start}, got {}",
            found.0
        );
        assert!(found.1 > 0.9, "score={}", found.1);
    }

    #[test]
    fn discover_fill_start_in_b_prefers_border_match_over_offset_nominal() {
        let rate = 1000.0;
        let chirp = |offset: usize, len: usize| {
            (0..len)
                .map(|i| {
                    let t = (offset + i) as f64 / rate;
                    (std::f32::consts::TAU as f64 * 80.0 * t * t).sin()
                })
                .collect::<Vec<_>>()
        };
        let pre = chirp(0, 120);
        let post = chirp(200, 120);
        let gap_frames = 40usize;

        let mut haystack = vec![0.0; 20];
        haystack.extend(&pre);
        haystack.extend(vec![0.0; gap_frames]);
        haystack.extend(&post);
        haystack.extend(vec![0.0; 80]);

        let true_gap_start = 20 + pre.len();
        let true_gap_end = true_gap_start + gap_frames;
        let offset_nominal = true_gap_start + 25;
        let discovered = discover_fill_start_in_b(
            &pre,
            &post,
            &haystack,
            &[],
            &[],
            &[],
            offset_nominal,
            offset_nominal,
            true_gap_end + 25,
            gap_frames,
            80,
            40,
        );
        assert!(
            discovered.abs_diff(true_gap_start) <= 2,
            "expected gap start near {true_gap_start}, got {discovered}"
        );
    }

    #[test]
    fn align_fill_segment_finds_shifted_match() {
        let rate = 441.0;
        let chirp = |i: usize| {
            let t = i as f64 / rate;
            (std::f32::consts::TAU as f64 * 120.0 * t * t).sin() * 10_000.0
        };

        let pre: Vec<f64> = (0..120).map(chirp).collect();
        let fill: Vec<f64> = (120..320).map(chirp).collect();
        let post: Vec<f64> = (320..440).map(chirp).collect();

        let mut extended = vec![0.0; 30];
        extended.extend(&pre);
        extended.extend(&fill);
        extended.extend(&post);
        extended.extend([0.0; 80]);

        let gap_frames = fill.len();
        let true_start = 30 + pre.len();
        let nominal = true_start + 15;

        let alignment = align_fill_segment(
            &pre,
            &post,
            &extended,
            &[],
            &[],
            &[],
            gap_frames,
            nominal,
            80,
            20,
        )
        .expect("alignment");

        assert!(
            alignment.start_frame.abs_diff(true_start) <= 2,
            "expected start near {true_start}, got {}",
            alignment.start_frame
        );
        assert!(alignment.pre_correlation > 0.9);
        assert!(alignment.post_correlation > 0.9);
        assert!(
            alignment.start_frame.abs_diff(true_start) < nominal.abs_diff(true_start),
            "alignment should be closer to true start than nominal was"
        );
    }

    #[test]
    fn apply_seam_crossfade_blends_from_border_not_silence() {
        // Layout: [pre-border loud][gap silent][post-border loud]
        let cf = 4usize;
        let gap_start = 10usize;
        let gap_end = 20usize;
        let total = 30usize;
        let gap_frames = gap_end - gap_start;

        let mut a = vec![0i16; total];
        for s in &mut a[0..gap_start] {
            *s = 8_000;
        }
        for s in &mut a[gap_end..total] {
            *s = 8_000;
        }

        let b_fill = vec![4_000i16; gap_frames];
        apply_seam_crossfade(&mut a, &b_fill, 1, gap_start, gap_end, cf);

        assert!(
            a[gap_start] > 1_000,
            "first gap frame should blend from loud pre-border, got {}",
            a[gap_start]
        );
        assert_eq!(a[gap_start + cf], 4_000, "middle should be pure fill");
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

    #[test]
    fn border_templates_trim_quiet_fade_before_dropout() {
        let channels = 1usize;
        let mut samples = Vec::new();
        samples.extend(vec![8_000i16; 8]);
        samples.extend(vec![200i16; 4]);
        samples.extend(vec![0i16; 4]);
        samples.extend(vec![200i16; 4]);
        samples.extend(vec![8_000i16; 8]);

        let (pre, post) = border_templates_for_gap(&samples, channels, 16, 20, 12, 0.01, 0.0);
        assert!(!pre.is_empty());
        assert!(pre.iter().all(|&v| v.abs() > 1_000.0));
        assert!(!post.is_empty());
        assert!(post.iter().all(|&v| v.abs() > 1_000.0));
    }

    #[test]
    fn border_templates_for_gap_skip_adjacent_silence() {
        let channels = 1usize;
        let mut samples = vec![8_000i16; 5];
        samples.extend(vec![0i16; 5]);
        samples.extend(vec![8_000i16; 5]);

        let (pre, post) = border_templates_for_gap(&samples, channels, 5, 10, 5, 0.01, 0.0);
        assert_eq!(pre.len(), 5);
        assert!(pre.iter().all(|&v| (v - 8_000.0).abs() < 1.0));
        assert_eq!(post.len(), 5);
        assert!(post.iter().all(|&v| (v - 8_000.0).abs() < 1.0));
    }
}
