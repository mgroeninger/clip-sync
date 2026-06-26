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
///   peak is below an absolute amplitude floor (in the f32 `[-1.0, 1.0]` domain; default ≈
///   −60 dBFS ≈ 0.001007), or
/// - `RMS < peak × silence_peak_fraction` — catches codec noise: a block whose RMS is negligible
///   relative to its own peak (i.e. a few isolated transients in a sea of zeros).
///
/// Pass `absolute_rms_floor = 0.0` to disable the peak-floor check.
pub fn is_silent(samples: &[f32], silence_peak_fraction: f32, absolute_rms_floor: f32) -> bool {
    is_silent_interleaved(samples, 1, silence_peak_fraction, absolute_rms_floor)
}

/// Returns true when every channel in the interleaved block passes [`is_silent`].
///
/// Matches ffmpeg `silencedetect` with `mono=0` (default): all channels must be quiet.
pub fn is_silent_interleaved(
    samples: &[f32],
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
    samples: &[f32],
    channel: usize,
    channels: usize,
    frames: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> bool {
    let mut peak = 0.0f32;
    let mut sum_sq = 0f64;
    for frame in 0..frames {
        let sample = samples[frame * channels + channel];
        peak = peak.max(sample.abs());
        let v = sample as f64;
        sum_sq += v * v;
    }

    if peak == 0.0 {
        return true;
    }

    if absolute_rms_floor > 0.0 && peak < absolute_rms_floor {
        return true;
    }

    let rms = (sum_sq / frames as f64).sqrt() as f32;
    rms < peak * silence_peak_fraction
}

fn rms_f32(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|s| {
            let v = *s as f64;
            v * v
        })
        .sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

/// RMS of interleaved (multi-channel) f32 samples.
pub fn rms_interleaved(samples: &[f32]) -> f32 {
    rms_f32(samples)
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

/// Downmix interleaved f32 PCM to mono `f64` (channel average).
pub fn interleaved_to_mono(samples: &[f32], channels: usize) -> Vec<f64> {
    let channels = channels.max(1);
    samples
        .chunks(channels)
        .map(|frame| frame.iter().map(|&s| s as f64).sum::<f64>() / channels as f64)
        .collect()
}

/// Refined gap boundaries on A's PCM timeline (frame indices, `[start, end)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefinedGapFrames {
    pub start_frame: usize,
    pub end_frame: usize,
}

fn silent_run(
    samples: &[f32],
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
    samples: &[f32],
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
    samples: &[f32],
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
    samples: &[f32],
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
pub fn interleaved_to_channels(samples: &[f32], channels: usize) -> Vec<Vec<f64>> {
    let channels = channels.max(1);
    (0..channels)
        .map(|ch| {
            samples
                .chunks(channels)
                .map(|frame| frame[ch] as f64)
                .collect()
        })
        .collect()
}

/// Build mono border templates for seam correlation, skipping silence adjacent to the gap.
pub fn border_templates_for_gap(
    samples: &[f32],
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

/// Count of samples above 12% of border peak (matches trim_low_energy threshold).
pub(crate) fn border_active_extent_frames(border: &[f64]) -> usize {
    if border.is_empty() {
        return 0;
    }
    let peak = border.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return 0;
    }
    let floor = peak * 0.12;
    border.iter().filter(|s| s.abs() >= floor).count()
}

/// Adaptive seam window: cap at active extent, bounded by min/max seam frames.
pub(crate) fn adaptive_seam_window_frames(
    border_len: usize,
    min_frames: usize,
    max_frames: usize,
    active_extent_frames: usize,
) -> usize {
    if border_len == 0 {
        return 0;
    }
    let min_frames = min_frames.max(1);
    let max_frames = max_frames.max(min_frames);
    let target = active_extent_frames.max(min_frames).min(max_frames);
    target.min(border_len).max(1)
}

/// Per-channel border templates (same frame ranges as [`border_templates_for_gap`]).
pub fn border_templates_per_channel_for_gap(
    samples: &[f32],
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

/// Mean-square energy of a (peak-domain) seam template.
fn template_mean_square(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64
}

/// A-side channels that carry seam signal — those within ~20 dB of the loudest channel's
/// energy. Lets seam scoring follow the channel(s) that actually hold content (e.g. a
/// center-dominant 5.1 mix where front L/R are near-silent) instead of assuming front L/R.
/// Returns empty when every channel is near-silent, so the caller falls back to the mono mix.
fn seam_score_channel_indices(a_pre_ch: &[Vec<f64>], a_post_ch: &[Vec<f64>]) -> Vec<usize> {
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

/// Energy-selected seam channels for a gap, computed from the same per-channel border templates
/// Pearson scores — the single entry point so residual/floor measurement follows the *same*
/// channels as seam scoring (see `seam-scoring.md`). Returns empty when every channel is
/// near-silent, so the caller falls back to the mono downmix.
pub(crate) fn selected_seam_channels(
    a_samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> Vec<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    seam_score_channel_indices(&a_pre_ch, &a_post_ch)
}

fn peak_normalize_f64(samples: &[f64]) -> Vec<f64> {
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return samples.to_vec();
    }
    samples.iter().map(|s| s / peak).collect()
}

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

/// Cap repeat-correlation window length for short gaps and seam-adjacent scoring.
///
/// Without this, `repeat_window_frames` from border discovery (often 2 s via
/// `min_border_discovery_secs`) exceeds short gap brackets and `a_post.len()`,
/// disabling `repeat_post` entirely, or comparing `a_post` against fill plus
/// haystack past the bracket end.
pub(crate) fn effective_repeat_window_frames(
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
    let SeamTemplates { a_pre, a_post, a_pre_ch, a_post_ch, b_mono, b_ch } = *templates;
    let SeamPlacement {
        start,
        gap_frames,
        pre_window,
        post_window,
    } = placement;

    let pre_repeat_window = effective_repeat_window_frames(
        repeat_window_frames,
        gap_frames,
        a_pre.len(),
        pre_window,
    );
    let post_repeat_window = effective_repeat_window_frames(
        repeat_window_frames,
        gap_frames,
        a_post.len(),
        post_window,
    );

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
                    Some(seam_pearson(
                        &a_ch[..w],
                        &b_ch[tail..tail + w],
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
    pub a_samples: &'a [f32],
    pub channels: usize,
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

    let score_channels = seam_score_channel_indices(a_pre_ch, a_post_ch);
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
    let SeamTemplates { a_pre, a_post, a_pre_ch, a_post_ch, b_mono, b_ch } = *templates;
    let SeamPlacement { start, gap_frames, pre_window, post_window } = placement;

    let pre_fits = |len: usize| pre_window > 0 && start >= pre_window && start <= len;
    let post_fits = |len: usize| post_window > 0 && start + gap_frames + post_window <= len;

    let mut per_channel = Vec::with_capacity(b_ch.len());
    for ch in 0..b_ch.len() {
        let pre = if ch < a_pre_ch.len() && a_pre_ch[ch].len() >= pre_window && pre_fits(b_ch[ch].len()) {
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

/// Integer lag search result for one seam side (`seam_residual_for_side`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct LagFitResult {
    gain: f64,
    best_lag: i64,
    residual_db: f64,
}

/// B-window energy below this is unmeasurable — abstain the lag (L13).
const LSQ_B_ENERGY_FLOOR: f64 = 1.0;

/// Least-squares scalar fit `a ≈ g·b`; returns `(gain, ||a - g·b||² / ||a||²)`.
fn lsq_residual_ratio(a: &[f64], b: &[f64]) -> Option<(f64, f64)> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let aa: f64 = a.iter().map(|x| x * x).sum();
    if aa <= f64::EPSILON {
        return None;
    }
    let bb: f64 = b.iter().map(|y| y * y).sum();
    if bb < LSQ_B_ENERGY_FLOOR {
        return None;
    }
    let ab: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let g = ab / bb;
    let res: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            let d = x - g * y;
            d * d
        })
        .sum();
    Some((g, (res / aa).max(0.0)))
}

/// Search integer lags for the minimum normalized residual of `a_win` against B windows in
/// `b_haystack` at offsets returned by `b_window_bounds`.
///
/// `lag_center` shifts the search window (e.g. `floor.best_lag + nominal_delta − chosen_delta`
/// when placement slide is within reach).
fn seam_residual_for_side<F>(
    a_win: &[f64],
    b_haystack: &[f64],
    b_window_bounds: F,
    max_lag: i64,
    lag_center: i64,
) -> Option<LagFitResult>
where
    F: Fn(i64) -> Option<(usize, usize)>,
{
    let w = a_win.len();
    if w == 0 {
        return None;
    }
    let aa: f64 = a_win.iter().map(|x| x * x).sum();
    if aa <= f64::EPSILON {
        return None;
    }

    let max_lag = max_lag.max(0);
    let mut best_ratio = f64::INFINITY;
    let mut best_lag = 0i64;
    let mut best_gain = 0.0;
    let mut found = false;
    for lag in (lag_center - max_lag)..=(lag_center + max_lag) {
        let Some((lo, hi)) = b_window_bounds(lag) else {
            continue;
        };
        if hi - lo != w {
            continue;
        }
        let b_win = &b_haystack[lo..hi];
        let Some((g, ratio)) = lsq_residual_ratio(a_win, b_win) else {
            continue;
        };
        if ratio < best_ratio {
            best_ratio = ratio;
            best_lag = lag;
            best_gain = g;
            found = true;
        }
    }

    if !found {
        return None;
    }

    Some(LagFitResult {
        gain: best_gain,
        best_lag,
        residual_db: 10.0 * best_ratio.max(1e-12).log10(),
    })
}

/// Reference window must peak at least this multiple of the silence floor to anchor a floor probe.
const SEAM_FLOOR_ENERGY_MARGIN: f64 = 4.0;

/// Which side of the gap a probe was taken from / where its reference window came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamFloorSource {
    /// Immediate border window (just past the standoff) was energetic and usable.
    Border,
    /// Border was empty/quiet; reference came from an energetic window walked further out.
    Walked,
    /// No energetic, in-coverage reference window found within the horizon.
    None,
}

/// Per-gap measured noise-floor probe (report-only): best A-vs-B residual of a clean, energetic
/// reference window at the *nominal* offset, used to interpret a seam residual as headroom.
#[derive(Debug, Clone, Copy)]
pub struct SeamFloorProbe {
    pub source: SeamFloorSource,
    /// Normalized residual in dB at the reference window (`NaN` when `source == None`).
    pub residual_db: f64,
    pub gain: f64,
    pub best_lag: i64,
}

impl SeamFloorProbe {
    fn none() -> Self {
        Self {
            source: SeamFloorSource::None,
            residual_db: f64::NAN,
            gain: f64::NAN,
            best_lag: 0,
        }
    }

    pub fn source_label(&self) -> &'static str {
        match self.source {
            SeamFloorSource::Border => "border",
            SeamFloorSource::Walked => "walked",
            SeamFloorSource::None => "none",
        }
    }
}

/// Which gap edge a floor probe walks out from.
#[derive(Debug, Clone, Copy)]
pub enum SeamSide {
    Pre,
    Post,
}

/// Inputs to [`seam_floor_probe`] (report-only diagnostic).
pub struct SeamFloorParams<'a> {
    /// Full A audio (interleaved) on the same clock as the gap frames.
    pub a_samples: &'a [f32],
    pub channels: usize,
    /// B haystack mono (same buffer the seam scores against).
    pub b_mono: &'a [f64],
    /// Reference window length in frames (use the matching seam window).
    pub window: usize,
    /// Frames skipped immediately adjacent to the gap (reuse the seam standoff).
    pub standoff_frames: usize,
    /// `b_haystack_frame = a_frame + a_to_b_delta` (nominal alignment mapping).
    pub a_to_b_delta: i64,
    /// Outward walk step in frames when the immediate border is quiet.
    pub step_frames: usize,
    /// How far from the gap edge the outward walk may reach.
    pub max_walk_frames: usize,
    pub absolute_silence_rms: f32,
    /// Integer sample lag searched on each side when cancelling A against B.
    pub max_lag_frames: i64,
}

fn mono_window(a_samples: &[f32], channels: usize, lo: usize, hi: usize) -> Vec<f64> {
    let channels = channels.max(1);
    (lo..hi)
        .map(|frame| {
            let base = frame * channels;
            let sum: f64 = (0..channels)
                .map(|c| a_samples.get(base + c).copied().unwrap_or(0.0) as f64)
                .sum();
            sum / channels as f64
        })
        .collect()
}

/// A clean, energetic raw A reference window selected for residual/floor measurement.
struct ReferenceWindow {
    /// Raw mono A samples (no standoff/low-energy trim — see `mono_window`).
    a_win: Vec<f64>,
    /// First A frame of the window (maps to B via a delta).
    a_lo: usize,
    /// Whether the window is the immediate border or one walked outward.
    source: SeamFloorSource,
}

/// Frame range and source of a reference window, decoupled from channel extraction so the same
/// raw range can feed either the mono downmix or per-channel residual measurement (§ channel
/// alignment in `seam-scoring.md`).
struct ReferenceFrames {
    a_lo: usize,
    a_hi: usize,
    source: SeamFloorSource,
}

/// Walk outward from the gap edge to the first reference window that passes `energetic` (the energy
/// gate — B-side availability is checked at measurement time so the same window can be measured at
/// multiple deltas). `energetic(a_lo, a_hi)` decides whether a candidate range carries usable audio.
fn walk_reference_frames(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
    energetic: impl Fn(usize, usize) -> bool,
) -> Option<ReferenceFrames> {
    let channels = params.channels.max(1);
    let w = params.window;
    if w == 0 {
        return None;
    }
    let a_total = params.a_samples.len() / channels;
    let step = params.step_frames.max(1);

    let mut k = 0usize;
    loop {
        let walked = k * step;
        if walked > params.max_walk_frames {
            return None;
        }
        let window = match side {
            SeamSide::Pre => match gap_start_frame.checked_sub(params.standoff_frames + walked) {
                Some(hi) if hi >= w => Some((hi - w, hi)),
                _ => None,
            },
            SeamSide::Post => {
                let lo = gap_end_frame + params.standoff_frames + walked;
                let hi = lo + w;
                (hi <= a_total).then_some((lo, hi))
            }
        };
        let (a_lo, a_hi) = window?;
        if energetic(a_lo, a_hi) {
            let source = if k == 0 {
                SeamFloorSource::Border
            } else {
                SeamFloorSource::Walked
            };
            return Some(ReferenceFrames { a_lo, a_hi, source });
        }
        k += 1;
    }
}

/// Energy gate for the mono downmix path: the downmixed window peak must clear the silence floor.
fn mono_window_energetic<'a>(
    a_samples: &'a [f32],
    channels: usize,
    energy_floor: f64,
) -> impl Fn(usize, usize) -> bool + 'a {
    move |a_lo, a_hi| {
        let win = mono_window(a_samples, channels, a_lo, a_hi);
        win.iter().map(|s| s.abs()).fold(0.0f64, f64::max) >= energy_floor
    }
}

/// Walk outward from the gap edge to the first energetic raw A window (mono downmix energy gate).
fn select_reference_window(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
) -> Option<ReferenceWindow> {
    let channels = params.channels.max(1);
    let energy_floor = f64::from(params.absolute_silence_rms) * SEAM_FLOOR_ENERGY_MARGIN;
    let frames = walk_reference_frames(
        params,
        side,
        gap_start_frame,
        gap_end_frame,
        mono_window_energetic(params.a_samples, channels, energy_floor),
    )?;
    Some(ReferenceWindow {
        a_win: mono_window(params.a_samples, channels, frames.a_lo, frames.a_hi),
        a_lo: frames.a_lo,
        source: frames.source,
    })
}

/// Cancel a raw A window against B at `delta` (`b_frame = a_frame + delta`) with `max_lag` around
/// `lag_center`. `source` is stamped on the resulting probe.
fn measure_a_win_at_delta(
    a_win: &[f64],
    a_lo: usize,
    source: SeamFloorSource,
    b: &[f64],
    delta: i64,
    max_lag: i64,
    lag_center: i64,
) -> SeamFloorProbe {
    let w = a_win.len() as i64;
    let b_len = b.len() as i64;
    let b_start0 = a_lo as i64 + delta;
    let probe = seam_residual_for_side(
        a_win,
        b,
        |lag| {
            let lo = b_start0 + lag;
            let hi = lo + w;
            if lo < 0 || hi > b_len {
                return None;
            }
            Some((lo as usize, hi as usize))
        },
        max_lag,
        lag_center,
    );
    match probe {
        Some(r) => SeamFloorProbe {
            source,
            residual_db: r.residual_db,
            gain: r.gain,
            best_lag: r.best_lag,
        },
        None => SeamFloorProbe {
            source,
            residual_db: f64::NAN,
            gain: f64::NAN,
            best_lag: 0,
        },
    }
}

fn measure_window_at_delta(
    window: &ReferenceWindow,
    b_mono: &[f64],
    delta: i64,
    max_lag: i64,
    lag_center: i64,
) -> SeamFloorProbe {
    measure_a_win_at_delta(&window.a_win, window.a_lo, window.source, b_mono, delta, max_lag, lag_center)
}

/// Measure `(chosen, floor)` for one raw window against one B timeline: floor at the nominal mapping
/// (wide lag), chosen at `chosen_delta` with its lag search centered on the floor's best lag when the
/// placement slide is within reach (so headroom is a pure where-on-B difference). Shared by the mono
/// and per-channel paths.
fn chosen_and_floor_on_window(
    a_win: &[f64],
    a_lo: usize,
    source: SeamFloorSource,
    b: &[f64],
    nominal_delta: i64,
    chosen_delta: i64,
    max_lag: i64,
) -> (SeamFloorProbe, SeamFloorProbe) {
    let floor = measure_a_win_at_delta(a_win, a_lo, source, b, nominal_delta, max_lag, 0);
    let lag_center = chosen_lag_center(&floor, nominal_delta, chosen_delta, max_lag);
    let chosen = measure_a_win_at_delta(a_win, a_lo, source, b, chosen_delta, max_lag, lag_center);
    (chosen, floor)
}

/// Lag the chosen-placement probe should center on: the floor's best lag shifted by the placement
/// slide, so chosen and floor compare the same B content. Falls back to `0` when the slide exceeds
/// the lag radius or the floor did not cancel.
fn chosen_lag_center(
    floor: &SeamFloorProbe,
    nominal_delta: i64,
    chosen_delta: i64,
    max_lag: i64,
) -> i64 {
    let placement_error = chosen_delta - nominal_delta;
    if placement_error.abs() <= max_lag && floor.residual_db.is_finite() {
        floor.best_lag + nominal_delta - chosen_delta
    } else {
        0
    }
}

/// Measure the per-gap noise floor: slide a clean, energetic A reference window against B at the
/// nominal offset (wide lag search). Starts at the immediate border, walks outward if it is quiet.
pub fn seam_floor_probe(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
) -> SeamFloorProbe {
    match select_reference_window(params, side, gap_start_frame, gap_end_frame) {
        Some(window) => {
            measure_window_at_delta(
                &window,
                params.b_mono,
                params.a_to_b_delta,
                params.max_lag_frames,
                0,
            )
        }
        None => SeamFloorProbe::none(),
    }
}

/// Measure one gap side's residual at the **chosen** placement and the **nominal** floor on the
/// *same* raw reference window (so headroom is a pure difference of where-on-B, not of reference
/// audio or lag radius). `params.a_to_b_delta` is the nominal mapping; `chosen_delta` is the
/// chosen-placement mapping. Returns `(chosen, floor)`.
pub fn seam_chosen_and_floor(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
    chosen_delta: i64,
) -> (SeamFloorProbe, SeamFloorProbe) {
    match select_reference_window(params, side, gap_start_frame, gap_end_frame) {
        Some(window) => chosen_and_floor_on_window(
            &window.a_win,
            window.a_lo,
            window.source,
            params.b_mono,
            params.a_to_b_delta,
            chosen_delta,
            params.max_lag_frames,
        ),
        None => (SeamFloorProbe::none(), SeamFloorProbe::none()),
    }
}

/// One selected channel's chosen-placement residual and nominal floor on the same raw reference
/// window (per-channel analog of [`seam_chosen_and_floor`]; see § residual channel policy).
#[derive(Debug, Clone, Copy)]
pub struct SeamChannelResidual {
    pub channel: usize,
    pub chosen: SeamFloorProbe,
    pub floor: SeamFloorProbe,
}

/// Per-channel chosen/floor for one gap side, measured on the **same** energy-selected channels as
/// Pearson seam scoring. Each selected channel is cancelled against its own `b_ch[ch]` timeline on a
/// shared raw reference window (frame range chosen by a per-channel energy gate). Returns one entry
/// per usable selected channel; when `score_channels` is empty or no channel is usable (e.g. B has
/// fewer channels), falls back to a single mono-downmix entry so callers get uniform handling.
///
/// **Alignment is shared, depth is per-channel.** The integer lag is found once across *all* selected
/// channels by summing their per-channel correlation (see [`shared_alignment_lag`]), with no
/// dependence on which channel happens to carry the gap, then each selected channel fits its own
/// scalar gain and residual at that fixed lag. This keeps the time alignment robust on surround/center
/// mixes while still measuring cancellation depth on the right channel.
///
/// `score_channels` must already be filtered to the channels of interest; indices `>= b_ch.len()`
/// are skipped (A/B channel-count mismatch, Risk §8).
pub fn seam_chosen_and_floor_multichannel(
    params: &SeamFloorParams<'_>,
    b_ch: &[Vec<f64>],
    score_channels: &[usize],
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
    chosen_delta: i64,
) -> Vec<SeamChannelResidual> {
    let channels = params.channels.max(1);
    let usable: Vec<usize> = score_channels
        .iter()
        .copied()
        .filter(|&ch| ch < b_ch.len() && ch < channels)
        .collect();

    if usable.is_empty() {
        // Mono downmix fallback — one entry, identical signal path to `seam_chosen_and_floor`.
        let (chosen, floor) =
            seam_chosen_and_floor(params, side, gap_start_frame, gap_end_frame, chosen_delta);
        return vec![SeamChannelResidual { channel: 0, chosen, floor }];
    }

    // Shared frame range: walk outward until *any* selected channel carries usable audio, so quiet
    // surrounds can't push the window past energetic center content (§4c energy gate).
    let energy_floor = f64::from(params.absolute_silence_rms) * SEAM_FLOOR_ENERGY_MARGIN;
    let energetic = |a_lo: usize, a_hi: usize| {
        usable.iter().any(|&ch| {
            interleaved_channel_timeline_f64(params.a_samples, channels, ch, a_lo, a_hi)
                .iter()
                .map(|s| s.abs())
                .fold(0.0f64, f64::max)
                >= energy_floor
        })
    };

    let Some(frames) =
        walk_reference_frames(params, side, gap_start_frame, gap_end_frame, energetic)
    else {
        // No energetic window for any selected channel within the horizon.
        return usable
            .into_iter()
            .map(|ch| SeamChannelResidual {
                channel: ch,
                chosen: SeamFloorProbe::none(),
                floor: SeamFloorProbe::none(),
            })
            .collect();
    };

    // Find the shared integer lag once by summing the per-channel correlation across selected
    // channels, then measure each channel's depth at that fixed lag. Summing *correlations* (not a
    // downmixed waveform) is what makes this robust: a loud channel whose B content differs (or is
    // noise) correlates ~0 at every lag and so never pulls the alignment, while the channel(s) that
    // actually match contribute a sharp peak at the true lag — no dependence on which channel that is.
    let a_wins: Vec<Vec<f64>> = usable
        .iter()
        .map(|&ch| {
            interleaved_channel_timeline_f64(params.a_samples, channels, ch, frames.a_lo, frames.a_hi)
        })
        .collect();
    let shared_floor_lag = shared_alignment_lag(
        &a_wins,
        &usable,
        b_ch,
        frames.a_lo,
        params.a_to_b_delta,
        params.max_lag_frames,
    );
    let placement_error = chosen_delta - params.a_to_b_delta;
    let shared_chosen_lag = if placement_error.abs() <= params.max_lag_frames {
        shared_floor_lag + params.a_to_b_delta - chosen_delta
    } else {
        0
    };

    // Measure each selected channel's depth at the shared lag (max_lag = 0 → fixed lag); only the
    // per-channel scalar gain adapts.
    usable
        .iter()
        .zip(a_wins.iter())
        .map(|(&ch, a_win)| {
            let floor = measure_a_win_at_delta(
                a_win,
                frames.a_lo,
                frames.source,
                &b_ch[ch],
                params.a_to_b_delta,
                0,
                shared_floor_lag,
            );
            let chosen = measure_a_win_at_delta(
                a_win,
                frames.a_lo,
                frames.source,
                &b_ch[ch],
                chosen_delta,
                0,
                shared_chosen_lag,
            );
            SeamChannelResidual { channel: ch, chosen, floor }
        })
        .collect()
}

/// The integer lag that best aligns A and B across *all* selected channels at once: the lag maximizing
/// the summed peak-normalized correlation. Channels whose B content does not match contribute ~0 at
/// every lag, so they neither pull nor veto the alignment — only matching channels shape the peak.
/// Returns the nominal lag (`0`) when no lag produces positive aggregate correlation.
fn shared_alignment_lag(
    a_wins: &[Vec<f64>],
    channels: &[usize],
    b_ch: &[Vec<f64>],
    a_lo: usize,
    nominal_delta: i64,
    max_lag: i64,
) -> i64 {
    let max_lag = max_lag.max(0);
    let mut best_lag = 0i64;
    let mut best_score = f64::NEG_INFINITY;
    for lag in -max_lag..=max_lag {
        let mut score = 0.0;
        for (a_win, &ch) in a_wins.iter().zip(channels) {
            let b = &b_ch[ch];
            let w = a_win.len() as i64;
            let lo = a_lo as i64 + nominal_delta + lag;
            let hi = lo + w;
            if lo < 0 || hi > b.len() as i64 {
                continue;
            }
            score += seam_pearson(a_win, &b[lo as usize..hi as usize]).max(0.0);
        }
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }
    if best_score > 0.0 {
        best_lag
    } else {
        0
    }
}

/// Default `floor_db` ceiling for an established same-master cancellation floor.
///
/// Floors at or below this value are treated as informative for residual gating; see
/// [`floor_probe_informative`] and [`residual_verdict_informative`]. Calibrated in
/// `tests/seam_residual_corpus.rs`; overridable via config when the gate ships.
pub const DEFAULT_RESIDUAL_FLOOR_OK_DB: f64 = -15.0;

/// True when a floor probe measured nominal cancellation at or below `floor_ok_db`.
pub fn floor_probe_informative(probe: &SeamFloorProbe, floor_ok_db: f64) -> bool {
    probe.source != SeamFloorSource::None
        && probe.residual_db.is_finite()
        && probe.residual_db <= floor_ok_db
}

/// Whether headroom on this gap is regime-informative (same-master + aligned at nominal).
///
/// Every **measured** side (`source != None`) must have `floor_db ≤ floor_ok_db`. Unmeasured
/// sides are ignored. Returns false when no side was measured.
pub fn residual_verdict_informative(
    floor_pre: &SeamFloorProbe,
    floor_post: &SeamFloorProbe,
    floor_ok_db: f64,
) -> bool {
    let pre_measured = floor_pre.source != SeamFloorSource::None;
    let post_measured = floor_post.source != SeamFloorSource::None;
    if !pre_measured && !post_measured {
        return false;
    }
    (!pre_measured || floor_probe_informative(floor_pre, floor_ok_db))
        && (!post_measured || floor_probe_informative(floor_post, floor_ok_db))
}

/// Combined residual/floor verdict for one gap (P1 report-only, uniform schema).
///
/// Both the chosen-placement residual and the nominal floor are measured on the **same raw A
/// reference window** with the same lag radius — they differ only in *where on B* (chosen vs
/// nominal mapping). When placement slide is within `max_lag_frames`, the chosen probe's lag
/// search is centered on `floor.best_lag + nominal_delta − chosen_delta`. `informative` uses
/// the supplied `floor_ok_db`. When `placement_slide_frames > max_lag_frames`, the gate abstains
/// (`beyond_lag_reach`) — headroom is not meaningful outside the unified lag radius.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct SeamResidualVerdict {
    pub chosen_pre_db: f64,
    pub chosen_post_db: f64,
    pub floor_pre_db: f64,
    pub floor_post_db: f64,
    pub floor_source_pre: SeamFloorSource,
    pub floor_source_post: SeamFloorSource,
    /// Nominal floor established cancellation on every measured side (`floor_db ≤ FLOOR_OK`).
    pub informative: bool,
    /// `|chosen_delta − nominal_delta|` in frames (0 when unset / harness default).
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub placement_slide_frames: u64,
    /// Unified lag radius used for this verdict (`0` = reach check disabled).
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub max_lag_frames: i64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

impl SeamResidualVerdict {
    /// Assemble from the per-side chosen and floor probes (see [`seam_chosen_and_floor`]).
    pub fn from_parts(
        chosen_pre: &SeamFloorProbe,
        chosen_post: &SeamFloorProbe,
        floor_pre: &SeamFloorProbe,
        floor_post: &SeamFloorProbe,
    ) -> Self {
        Self::from_parts_with_floor_ok(
            chosen_pre,
            chosen_post,
            floor_pre,
            floor_post,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
        )
    }

    /// Like [`from_parts`] but uses a custom `floor_ok_db` (calibration sweeps).
    pub fn from_parts_with_floor_ok(
        chosen_pre: &SeamFloorProbe,
        chosen_post: &SeamFloorProbe,
        floor_pre: &SeamFloorProbe,
        floor_post: &SeamFloorProbe,
        floor_ok_db: f64,
    ) -> Self {
        Self::from_parts_with_placement(
            chosen_pre,
            chosen_post,
            floor_pre,
            floor_post,
            floor_ok_db,
            0,
            0,
        )
    }

    /// Production path: includes placement slide and lag radius for reach abstention.
    pub fn from_parts_with_placement(
        chosen_pre: &SeamFloorProbe,
        chosen_post: &SeamFloorProbe,
        floor_pre: &SeamFloorProbe,
        floor_post: &SeamFloorProbe,
        floor_ok_db: f64,
        placement_slide_frames: u64,
        max_lag_frames: i64,
    ) -> Self {
        Self {
            chosen_pre_db: chosen_pre.residual_db,
            chosen_post_db: chosen_post.residual_db,
            floor_pre_db: floor_pre.residual_db,
            floor_post_db: floor_post.residual_db,
            floor_source_pre: floor_pre.source,
            floor_source_post: floor_post.source,
            informative: residual_verdict_informative(floor_pre, floor_post, floor_ok_db),
            placement_slide_frames,
            max_lag_frames,
        }
    }

    /// Assemble from per-channel residuals (energy-selected channels; see
    /// [`seam_chosen_and_floor_multichannel`]). The scalar side summaries report the **worst-headroom
    /// channel** on each side (so `worst_headroom_db()` is the conservative max over channels × sides
    /// for the veto, and the skip message names the channel that drove it), while `informative`
    /// follows the **best-cancelling (min-floor) channel** per side — a noisy surround must not flip
    /// the same-master regime off (Non-goal §2, residual-channel-alignment-plan §4d/§4e).
    pub fn from_channel_residuals(
        pre: &[SeamChannelResidual],
        post: &[SeamChannelResidual],
        floor_ok_db: f64,
        placement_slide_frames: u64,
        max_lag_frames: i64,
    ) -> Self {
        let (chosen_pre_db, floor_pre_db, floor_source_pre) = side_worst_headroom_summary(pre);
        let (chosen_post_db, floor_post_db, floor_source_post) = side_worst_headroom_summary(post);

        let pre_inf = side_floor_informative(pre, floor_ok_db);
        let post_inf = side_floor_informative(post, floor_ok_db);
        let informative = match (pre_inf, post_inf) {
            (None, None) => false,
            _ => pre_inf.unwrap_or(true) && post_inf.unwrap_or(true),
        };

        Self {
            chosen_pre_db,
            chosen_post_db,
            floor_pre_db,
            floor_post_db,
            floor_source_pre,
            floor_source_post,
            informative,
            placement_slide_frames,
            max_lag_frames,
        }
    }

    /// Placement slide exceeds the unified lag radius — residual gate abstains.
    pub fn beyond_lag_reach(&self) -> bool {
        self.max_lag_frames > 0 && self.placement_slide_frames as i64 > self.max_lag_frames
    }

    /// Worst-side headroom at the chosen placement (`chosen − floor`); larger = worse match.
    ///
    /// Ignores sides where either value is non-finite (unmeasured floor or chosen).
    pub fn worst_headroom_db(&self) -> f64 {
        let headrooms = [self.chosen_pre_db - self.floor_pre_db, self.chosen_post_db - self.floor_post_db]
            .into_iter()
            .filter(|h| h.is_finite());
        headrooms.fold(f64::NAN, |acc, h| {
            if acc.is_nan() {
                h
            } else {
                acc.max(h)
            }
        })
    }

    /// Worst-side chosen-placement residual (higher = less cancellation).
    pub fn worst_chosen_db(&self) -> f64 {
        worst_finite_max([self.chosen_pre_db, self.chosen_post_db])
    }

    /// Worst-side nominal floor (higher = weaker cancellation).
    pub fn worst_floor_db(&self) -> f64 {
        worst_finite_max([self.floor_pre_db, self.floor_post_db])
    }
}

fn worst_finite_max(values: [f64; 2]) -> f64 {
    values
        .into_iter()
        .filter(|v| v.is_finite())
        .fold(f64::NAN, |acc, v| {
            if acc.is_nan() {
                v
            } else {
                acc.max(v)
            }
        })
}

/// Side summary for the scalar verdict fields: the worst-headroom channel's `(chosen_db, floor_db,
/// source)`. Ignores channels where headroom is non-finite; `(NaN, NaN, None)` when the side has no
/// channel with both probes measured.
fn side_worst_headroom_summary(side: &[SeamChannelResidual]) -> (f64, f64, SeamFloorSource) {
    let mut best: Option<(f64, &SeamChannelResidual)> = None;
    for c in side {
        let headroom = c.chosen.residual_db - c.floor.residual_db;
        if headroom.is_finite() && best.is_none_or(|(h, _)| headroom > h) {
            best = Some((headroom, c));
        }
    }
    match best {
        Some((_, c)) => (c.chosen.residual_db, c.floor.residual_db, c.floor.source),
        None => (f64::NAN, f64::NAN, SeamFloorSource::None),
    }
}

/// Whether a side's **best-cancelling** (min-floor) selected channel established same-master
/// cancellation (`floor_db ≤ floor_ok_db`). `None` when the side was not measured (no channel with a
/// finite, sourced floor) — so an unmeasured side neither asserts nor blocks `informative`.
fn side_floor_informative(side: &[SeamChannelResidual], floor_ok_db: f64) -> Option<bool> {
    side.iter()
        .filter(|c| c.floor.source != SeamFloorSource::None && c.floor.residual_db.is_finite())
        .map(|c| c.floor.residual_db)
        .reduce(f64::min)
        .map(|best_floor| best_floor <= floor_ok_db)
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

fn blend_samples(a: f32, b: f32, a_weight: f32, b_weight: f32) -> f32 {
    (a_weight * a + b_weight * b).clamp(-1.0, 1.0)
}

/// Splice `b_fill` into `a_samples` at the gap, crossfading against A's real border audio.
///
/// Pre-seam: equal-power crossfade bleeds the fill head into the last `cf` pre-gap frames only;
/// the gap interior starts at full `b_fill[cf]` so there is no silence ramp inside the dropout.
/// Post-seam: blends fill tail with post-gap head across the boundary (same value on both sides).
///
/// `gap_start_frame` / `gap_end_frame` are frame indices (not interleaved sample indices).
pub fn apply_seam_crossfade(
    a_samples: &mut [f32],
    b_fill: &[f32],
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
            let a_val = a_samples[pre_idx];
            let b_val = b_fill[i * channels + ch];
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
            let b_val = b_fill[b_frame * channels + ch];
            let post_idx = (gap_end_frame + i) * channels + ch;
            let a_val = a_samples[post_idx];
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

    fn mono_pcm(rate: u32, samples: Vec<f32>) -> MultiChannelPcm {
        MultiChannelPcm {
            sample_rate: rate,
            channels: 1,
            samples,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        }
    }

    #[test]
    fn empty_clip_is_silent() {
        assert!(is_silent(&[], 0.01, 0.0));
    }

    #[test]
    fn all_zeros_is_silent() {
        assert!(is_silent(&vec![0.0f32; 1000], 0.01, 0.0));
    }

    #[test]
    fn loud_sine_is_not_silent() {
        let samples: Vec<f32> = (0..1000)
            .map(|i| f32::sin(i as f32 * 0.1) * 0.305)
            .collect();
        assert!(!is_silent(&samples, 0.01, 0.0));
    }

    #[test]
    fn single_spike_in_sea_of_zeros_is_silent() {
        // Peak ≈ 0.00305, threshold = peak * 0.01 ≈ 0.0000305.
        // 1 spike in 11025 zeros: RMS = peak / sqrt(11025) ≈ 0.0000291 < threshold → silent.
        let mut samples = vec![0.0f32; 11_025];
        samples[0] = 100.0 / 32767.0;
        assert!(is_silent(&samples, 0.01, 0.0));
    }

    #[test]
    fn absolute_floor_catches_low_level_codec_noise() {
        // All samples at ±(1/32767): peak ≈ 3.05e-5, RMS ≈ peak.
        // Relative check alone (RMS ≈ peak → fraction ≈ 1.0 >> 0.01) would NOT flag as silent.
        // Peak-floor check: peak < floor(2/32767) → silent.
        let v = 1.0_f32 / 32767.0;
        let floor = 2.0_f32 / 32767.0;
        let samples: Vec<f32> = (0..11_025).map(|i| if i % 2 == 0 { v } else { -v }).collect();
        assert!(!is_silent(&samples, 0.01, 0.0), "no floor: should not be silent");
        assert!(is_silent(&samples, 0.01, floor), "floor: peak < floor → silent");
        let loud_v = 5.0_f32 / 32767.0;
        let loud_samples: Vec<f32> = (0..11_025).map(|i| if i % 2 == 0 { loud_v } else { -loud_v }).collect();
        assert!(!is_silent(&loud_samples, 0.01, floor), "floor: peak > floor → not silent by floor");
    }

    fn sine_samples(rate: u32, secs: f64) -> Vec<f32> {
        let count = (rate as f64 * secs).round() as usize;
        (0..count)
            .map(|i| f32::sin(i as f32 * 0.3) * 0.244)
            .collect()
    }

    #[test]
    fn silence_run_scanner_detects_three_second_gap() {
        let rate = 11_025u32;
        let block_secs = 0.25;
        let mut samples = sine_samples(rate, 5.0);
        samples.extend(std::iter::repeat_n(0.0f32, (rate as f64 * 3.0).round() as usize));
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

        let first = mono_pcm(rate, vec![0.0f32; (rate as f64 * 2.0).round() as usize]);
        let second = mono_pcm(rate, vec![0.0f32; (rate as f64 * 2.0).round() as usize]);

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
        samples.extend(vec![0.0f32; (rate as f64 * 0.5).round() as usize]);
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
        let mut samples = vec![0.0f32; block_samples * 8];
        samples.extend(sine_samples(rate, block_secs));
        samples.extend(vec![0.0f32; block_samples * 8]);

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

        let mut samples = vec![0.0f32; block_samples * 8];
        samples.extend(sine_samples(rate, block_secs));
        samples.extend(vec![0.0f32; block_samples * 8]);

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
            let tone = f32::sin(i as f32 * 0.3) * 0.244;
            samples.push(tone);
            samples.push(0.0f32);
        }
        assert!(
            !is_silent_interleaved(&samples, 2, 0.01, 0.0),
            "one hot channel should prevent silence"
        );
    }

    #[test]
    fn stereo_both_channels_quiet_is_silent() {
        let samples = vec![0.0f32; 11_025 * 2];
        assert!(is_silent_interleaved(&samples, 2, 0.01, 0.0));
    }

    #[test]
    fn rms_interleaved_of_constant() {
        let v = 1000.0_f32 / 32767.0;
        let samples = vec![v; 100];
        let result = rms_interleaved(&samples);
        assert!((result - v).abs() < 0.001, "rms of constant {v} should be ~{v}, got {result}");
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
        let a_samples: Vec<f32> = [100.0, 200.0, 300.0, 400.0, 0.0, 0.0, 500.0, 600.0, 0.0, 0.0]
            .iter()
            .map(|&v| v / 32767.0)
            .collect();
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
        let loud: f32 = 8_000.0 / 32767.0;
        let fill_level: f32 = 4_000.0 / 32767.0;

        let mut a = vec![0.0f32; 30];
        for s in &mut a[0..gap_start] {
            *s = loud;
        }
        for s in &mut a[gap_end..] {
            *s = loud;
        }

        let b_fill = vec![fill_level; gap_frames];
        apply_seam_crossfade(&mut a, &b_fill, 1, gap_start, gap_end, cf);

        assert!(
            (a[gap_start - cf - 1] - loud).abs() < 1e-5,
            "pre-gap audio before the crossfade window should be untouched"
        );
        assert!(
            (a[gap_start - cf] - loud).abs() < 1e-5,
            "crossfade should start from pure pre-gap audio"
        );
        assert!(
            (a[gap_start] - fill_level).abs() < 1e-5,
            "gap should start at full fill level, not a silence ramp"
        );
        assert!((a[gap_start + 1] - fill_level).abs() < 1e-5, "gap interior should be pure fill");
        assert!(
            a[gap_start - 1] > 3_000.0 / 32767.0,
            "pre-gap tail should bleed into fill before the gap boundary"
        );
    }

    #[test]
    fn apply_seam_crossfade_is_continuous_at_pre_seam() {
        let cf = 4usize;
        let gap_start = 10usize;
        let gap_end = 20usize;
        let gap_frames = gap_end - gap_start;
        let loud: f32 = 8_000.0 / 32767.0;
        let fill_level: f32 = 4_000.0 / 32767.0;

        let mut a = vec![0.0f32; 30];
        for s in &mut a[0..gap_start] {
            *s = loud;
        }
        for s in &mut a[gap_end..] {
            *s = loud;
        }

        let b_fill = vec![fill_level; gap_frames];
        apply_seam_crossfade(&mut a, &b_fill, 1, gap_start, gap_end, cf);

        let diff = (a[gap_start] - a[gap_start - 1]).abs();
        assert!(
            diff <= 4_500.0 / 32767.0,
            "jump of {diff} across pre seam ({} -> {})",
            a[gap_start - 1],
            a[gap_start]
        );
    }

    #[test]
    fn refine_gap_frames_retracts_through_leading_silence_before_reported_start() {
        let channels = 2usize;
        let loud = 8_000.0_f32 / 32767.0;
        // [loud 5][silent 10][loud 5] — reported gap starts two frames late.
        let mut samples = Vec::new();
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }
        for _ in 0..10 {
            samples.extend([0.0f32, 0.0f32]);
        }
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }

        let refined = refine_gap_frames(&samples, channels, 7, 14, 0.01, 0.0, 10);
        assert_eq!(refined.start_frame, 5);
        assert_eq!(refined.end_frame, 15);
    }

    #[test]
    fn refine_gap_frames_advances_past_leading_audio_and_extends_trailing_silence() {
        let channels = 2usize;
        let loud = 8_000.0_f32 / 32767.0;
        // [loud 5][silent 10][loud 5] — reported gap starts one frame too early and ends one frame too early.
        let mut samples = Vec::new();
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }
        for _ in 0..10 {
            samples.extend([0.0f32, 0.0f32]);
        }
        for _ in 0..5 {
            samples.extend([loud, loud]);
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
        let loud = 8_000.0_f32 / 32767.0;
        samples.extend([loud; 2]);
        samples.extend([3.0_f32 / 32767.0; 8]);
        samples.extend([loud; 2]);

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
        let loud = 8_000.0_f32 / 32767.0;
        let low = 200.0_f32 / 32767.0;
        let mut samples = Vec::new();
        samples.extend(vec![loud; 8]);
        samples.extend(vec![low; 4]);
        samples.extend(vec![0.0f32; 4]);
        samples.extend(vec![low; 4]);
        samples.extend(vec![loud; 8]);

        let (pre, post) = border_templates_for_gap(&samples, channels, &test_border_spec(16, 20, 12, 0));
        assert!(!pre.is_empty());
        assert!(pre.iter().all(|&v| v.abs() > 1_000.0 / 32767.0));
        assert!(!post.is_empty());
        assert!(post.iter().all(|&v| v.abs() > 1_000.0 / 32767.0));
    }

    #[test]
    fn border_standoff_excludes_audio_adjacent_to_dropout() {
        let channels = 1usize;
        let loud = 8_000.0_f32 / 32767.0;
        let mut samples = vec![loud; 20];
        samples.extend(vec![0.0f32; 5]);
        samples.extend(vec![loud; 20]);

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
        let loud = 8_000.0_f32 / 32767.0;
        let mut samples = vec![loud; 5];
        samples.extend(vec![0.0f32; 5]);
        samples.extend(vec![loud; 5]);

        let (pre, post) = border_templates_for_gap(&samples, channels, &test_border_spec(5, 10, 5, 0));
        assert_eq!(pre.len(), 5);
        assert!(pre.iter().all(|&v| (v - loud as f64).abs() < 0.001));
        assert_eq!(post.len(), 5);
        assert!(post.iter().all(|&v| (v - loud as f64).abs() < 0.001));
    }

    #[test]
    fn effective_repeat_window_caps_to_gap_and_seam_on_short_brackets() {
        assert_eq!(
            effective_repeat_window_frames(96_000, 48_000, 79_200, 12_000),
            12_000
        );
        assert_eq!(effective_repeat_window_frames(96_000, 48_000, 79_200, 0), 48_000);
        // Previously `repeat_window > a_post.len()` disabled repeat_post entirely.
        assert_eq!(effective_repeat_window_frames(96_000, 48_000, 12_000, 12_000), 12_000);
    }

    #[test]
    fn fill_repeat_post_detects_speech_onset_in_fill_tail_on_one_second_gap() {
        let gap_frames = 48_000usize;
        let seam_window = 12_000usize;
        let border_frames = 96_000usize;
        let a_post: Vec<f64> = (0..seam_window)
            .map(|i| (i as f64 * 0.12).sin())
            .collect();
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
        let (_, repeat_speech) =
            fill_repeat_correlations(&templates, placement, border_frames);
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
        let a_post: Vec<f64> = (0..seam_window)
            .map(|i| (i as f64 * 0.15).cos())
            .collect();
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
        let (_, repeat_interior) =
            fill_repeat_correlations(&templates, placement, border_frames);
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
        let front_b: Vec<f64> = (0..20).map(|i| if i % 2 == 0 { -5.0 } else { 5.0 }).collect();
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
        assert!(pre > 0.9, "pre seam should track the center channel, got {pre}");
        assert!(post > 0.9, "post seam should track the center channel, got {post}");
    }

    #[test]
    fn seam_residual_cancels_identical_scaled_source() {
        // Same-master: A border is B at half level → deep cancellation and gain ≈ 0.5.
        let pre_window = 16usize;
        let post_window = 16usize;
        let gap_frames = 8usize;
        let start = 64usize;

        let b_mono: Vec<f64> = (0..200)
            .map(|i| (i as f64 * 0.21).sin() * 1000.0 + (i as f64 * 0.07).cos() * 400.0)
            .collect();
        let a_pre: Vec<f64> = b_mono[start - pre_window..start].iter().map(|s| s * 0.5).collect();
        let a_post: Vec<f64> = b_mono[start + gap_frames..start + gap_frames + post_window]
            .iter()
            .map(|s| s * 0.5)
            .collect();

        let pre = seam_residual_for_side(&a_pre, &b_mono, |lag| {
            let lo = start as i64 - pre_window as i64 + lag;
            let hi = start as i64 + lag;
            if lo < 0 || hi > b_mono.len() as i64 || hi <= lo {
                return None;
            }
            Some((lo as usize, hi as usize))
        }, 512, 0)
        .expect("pre lag fit");
        let post = seam_residual_for_side(&a_post, &b_mono, |lag| {
            let tail = (start + gap_frames) as i64;
            let lo = tail + lag;
            let hi = tail + post_window as i64 + lag;
            if lo < 0 || hi > b_mono.len() as i64 {
                return None;
            }
            Some((lo as usize, hi as usize))
        }, 512, 0)
        .expect("post lag fit");
        assert_eq!(pre.best_lag, 0, "true lag is 0, got {}", pre.best_lag);
        assert!(pre.residual_db < -60.0, "expected deep cancellation, got {} dB", pre.residual_db);
        assert!((pre.gain - 0.5).abs() < 1e-6, "expected gain ~0.5, got {}", pre.gain);
        assert!(post.residual_db < -60.0, "expected deep cancellation, got {} dB", post.residual_db);
    }

    #[test]
    fn seam_residual_recovers_integer_lag() {
        // A's border equals B shifted by +3 samples; lag search should report best_lag = 3.
        let pre_window = 16usize;
        let start = 64usize;
        let true_lag = 3i64;

        let b_mono: Vec<f64> = (0..200).map(|i| (i as f64 * 0.3).sin() * 1000.0).collect();
        let lo = (start as i64 - pre_window as i64 + true_lag) as usize;
        let hi = (start as i64 + true_lag) as usize;
        let a_pre: Vec<f64> = b_mono[lo..hi].to_vec();

        let pre = seam_residual_for_side(&a_pre, &b_mono, |lag| {
            let b_lo = start as i64 - pre_window as i64 + lag;
            let b_hi = start as i64 + lag;
            if b_lo < 0 || b_hi > b_mono.len() as i64 || b_hi <= b_lo {
                return None;
            }
            Some((b_lo as usize, b_hi as usize))
        }, 64, 0)
        .expect("pre lag fit");
        assert_eq!(pre.best_lag, true_lag, "expected lag {true_lag}, got {}", pre.best_lag);
        assert!(pre.residual_db < -60.0, "shifted copy should cancel, got {} dB", pre.residual_db);
    }

    #[test]
    fn seam_floor_probe_uses_border_when_energetic() {
        // A and B share the same master; the immediate border is energetic, so the floor probe
        // anchors on it (source = Border) and cancels deeply.
        let rate_frames = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;
        let standoff = 16usize;

        let b_mono: Vec<f64> = (0..rate_frames)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        // A = same master, half level (nominal map is exact → delta 0).
        let a_samples: Vec<f32> = b_mono
            .iter()
            .map(|&s| (s * 0.5 / 4000.0) as f32)
            .collect();

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: standoff,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: rate_frames,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let pre = seam_floor_probe(&params, SeamSide::Pre, gap_start, gap_end);
        assert_eq!(pre.source, SeamFloorSource::Border);
        assert!(pre.residual_db < -60.0, "border floor should cancel, got {}", pre.residual_db);

        let post = seam_floor_probe(&params, SeamSide::Post, gap_start, gap_end);
        assert_eq!(post.source, SeamFloorSource::Border);
        assert!(post.residual_db < -60.0, "post floor should cancel, got {}", post.residual_db);
    }

    #[test]
    fn seam_floor_probe_walks_past_quiet_border() {
        // The immediate pre-border is silent; the probe must walk outward to energetic audio.
        let total = 2000usize;
        let gap_start = 1200usize;
        let gap_end = 1400usize;
        let window = 128usize;
        let standoff = 16usize;

        let b_mono: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.23).sin() * 4000.0)
            .collect();
        let mut a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();
        // Silence the region just before the gap (the immediate border), forcing an outward walk.
        let quiet_lo = gap_start - standoff - window;
        for s in a_samples.iter_mut().take(gap_start).skip(quiet_lo) {
            *s = 0.0;
        }

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: standoff,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let pre = seam_floor_probe(&params, SeamSide::Pre, gap_start, gap_end);
        assert_eq!(pre.source, SeamFloorSource::Walked, "should walk past the quiet border");
        assert!(pre.residual_db < -60.0, "walked floor should still cancel, got {}", pre.residual_db);
    }

    #[test]
    fn seam_chosen_and_floor_same_window_zero_headroom_when_nominal_correct() {
        // Same-master: A's border equals B (scaled). With chosen_delta == nominal_delta (correct
        // placement), chosen and floor are measured on the same raw window at the same mapping, so
        // both cancel deeply and headroom ≈ 0.
        let rate = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let b_mono: Vec<f64> = (0..rate)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0, // nominal == truth (same timeline)
            step_frames: window,
            max_walk_frames: rate,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let (chosen, floor) =
            seam_chosen_and_floor(&params, SeamSide::Pre, gap_start, gap_end, 0);
        assert_eq!(chosen.source, SeamFloorSource::Border);
        assert!(chosen.residual_db < -60.0, "chosen should cancel: {}", chosen.residual_db);
        assert!(floor.residual_db < -60.0, "floor should cancel: {}", floor.residual_db);

        let verdict = SeamResidualVerdict::from_parts(&chosen, &chosen, &floor, &floor);
        assert!(
            verdict.worst_headroom_db().abs() < 1.0,
            "headroom should be ~0 at correct placement, got {}",
            verdict.worst_headroom_db()
        );
        assert!(verdict.informative, "same-master floor should be informative");
    }

    #[test]
    fn seam_chosen_and_floor_lag_center_low_headroom_within_reach() {
        // Same-master with a modest placement slide inside the lag radius: lag-centered chosen
        // should cancel like the floor (headroom ≈ 0).
        let rate = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;
        let slide = 50i64;

        let b_mono: Vec<f64> = (0..rate)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: rate,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let (chosen, floor) =
            seam_chosen_and_floor(&params, SeamSide::Pre, gap_start, gap_end, slide);
        assert!(floor.residual_db < -60.0, "floor should cancel: {}", floor.residual_db);
        assert!(
            chosen.residual_db < -60.0,
            "lag-centered chosen should cancel within reach: {}",
            chosen.residual_db
        );

        let verdict = SeamResidualVerdict::from_parts_with_placement(
            &chosen,
            &chosen,
            &floor,
            &floor,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
            slide as u64,
            512,
        );
        assert!(
            verdict.worst_headroom_db().abs() < 1.0,
            "headroom should be ~0 with lag centering, got {}",
            verdict.worst_headroom_db()
        );
        assert!(!verdict.beyond_lag_reach());
    }

    #[test]
    fn seam_chosen_and_floor_headroom_large_when_chosen_wrong() {
        // Two-region B: the reference window (near gap_start) is region-1 content; the floor maps to
        // region 1 (cancels) while a wrong chosen delta maps into region-2 (different) content that
        // does not cancel at any lag → large headroom.
        let half = 2000usize;
        let total = half * 2;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let b_mono: Vec<f64> = (0..total)
            .map(|i| {
                if i < half {
                    (i as f64 * 0.23).sin() * 4000.0
                } else {
                    (i as f64 * 0.71).sin() * 4000.0
                }
            })
            .collect();
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0, // nominal maps the region-1 window to region-1 → cancels
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        // Chosen delta maps the region-1 reference window into region-2 content (in bounds, ±512 lag
        // stays within region 2) → no cancellation.
        let (chosen, floor) =
            seam_chosen_and_floor(&params, SeamSide::Pre, gap_start, gap_end, half as i64);
        assert!(floor.residual_db < -60.0, "floor should cancel at nominal: {}", floor.residual_db);
        assert!(
            chosen.residual_db > -6.0,
            "chosen should not cancel into different content: {}",
            chosen.residual_db
        );
        let verdict = SeamResidualVerdict::from_parts(&chosen, &chosen, &floor, &floor);
        assert!(
            verdict.worst_headroom_db() > 40.0,
            "headroom should be large: {}",
            verdict.worst_headroom_db()
        );
        assert!(verdict.informative, "floor still cancels at nominal");
    }

    // ---- Per-channel (multichannel) residual ----------------------------------------------------

    /// Build interleaved f32 A samples (normalized) from per-channel f64 timelines (raw level).
    fn interleave_a(channels_f64: &[Vec<f64>], norm: f64) -> Vec<f32> {
        let channels = channels_f64.len();
        let total = channels_f64[0].len();
        let mut out = vec![0.0f32; total * channels];
        for frame in 0..total {
            for (ch, timeline) in channels_f64.iter().enumerate() {
                out[frame * channels + ch] = (timeline[frame] / norm) as f32;
            }
        }
        out
    }

    fn probe_at(db: f64) -> SeamFloorProbe {
        SeamFloorProbe { source: SeamFloorSource::Border, residual_db: db, gain: 1.0, best_lag: 0 }
    }

    #[test]
    fn seam_chosen_and_floor_multichannel_follows_center_when_fronts_are_noise() {
        // Center-dominant 5.1-style: FC carries same-master signal; FL/FR carry noise that does NOT
        // cancel against B's (different) FL/FR noise. Per-channel residual on the selected center
        // cancels deeply; the mono downmix is polluted by the surrounds and cancels far worse.
        let total = 2000usize;
        let channels = 3usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let fc_b: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        let fl_b: Vec<f64> = (0..total).map(|i| (i as f64 * 0.53).sin() * 2000.0).collect();
        let fr_b: Vec<f64> = (0..total).map(|i| (i as f64 * 0.91).cos() * 2000.0).collect();
        let b_ch = vec![fl_b.clone(), fr_b.clone(), fc_b.clone()];
        let b_mono: Vec<f64> =
            (0..total).map(|i| (fl_b[i] + fr_b[i] + fc_b[i]) / 3.0).collect();

        // A: FC is B's center at half level (same master). FL/FR are *different* noise.
        let fc_a: Vec<f64> = fc_b.iter().map(|s| s * 0.5).collect();
        let fl_a: Vec<f64> = (0..total).map(|i| (i as f64 * 0.37).cos() * 2000.0).collect();
        let fr_a: Vec<f64> = (0..total).map(|i| (i as f64 * 0.71).sin() * 2000.0).collect();
        let a_samples = interleave_a(&[fl_a, fr_a, fc_a], 4000.0);

        let params = |window: usize| SeamFloorParams {
            a_samples: &a_samples,
            channels,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window.max(1),
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };

        // Selection follows the center (FL/FR noise is >20 dB below FC), as Pearson would score it.
        let a_pre_ch = vec![
            vec![0.01; window],
            vec![0.01; window],
            (0..window).map(|i| i as f64 + 1.0).collect(),
        ];
        assert_eq!(
            seam_score_channel_indices(&a_pre_ch, &a_pre_ch),
            vec![2],
            "center channel should be the only selected channel"
        );

        let mc_pre = seam_chosen_and_floor_multichannel(
            &params(window), &b_ch, &[2], SeamSide::Pre, gap_start, gap_end, 0,
        );
        let mc_post = seam_chosen_and_floor_multichannel(
            &params(window), &b_ch, &[2], SeamSide::Post, gap_start, gap_end, 0,
        );
        let mc = SeamResidualVerdict::from_channel_residuals(
            &mc_pre, &mc_post, DEFAULT_RESIDUAL_FLOOR_OK_DB, 0, 512,
        );

        let (chosen_pre, floor_pre) =
            seam_chosen_and_floor(&params(window), SeamSide::Pre, gap_start, gap_end, 0);
        let (chosen_post, floor_post) =
            seam_chosen_and_floor(&params(window), SeamSide::Post, gap_start, gap_end, 0);
        let mono = SeamResidualVerdict::from_parts(&chosen_pre, &chosen_post, &floor_pre, &floor_post);

        assert!(
            mc.worst_floor_db() < -40.0,
            "center channel should cancel deeply, got {}",
            mc.worst_floor_db()
        );
        assert!(
            mono.worst_floor_db() > mc.worst_floor_db() + 20.0,
            "mono downmix should cancel far worse than the center channel (mono={}, mc={})",
            mono.worst_floor_db(),
            mc.worst_floor_db()
        );
        assert!(mc.informative, "center cancellation establishes the same-master regime");
        assert!(
            mc.worst_headroom_db().abs() < 1.0,
            "headroom at truth should be ~0, got {}",
            mc.worst_headroom_db()
        );
    }

    #[test]
    fn seam_chosen_and_floor_multichannel_stereo_equal_matches_mono() {
        // Stereo, both channels same-master and equal energy → both selected, and the per-channel
        // result matches the mono-downmix path (both cancel deeply, headroom ≈ 0).
        let total = 2000usize;
        let channels = 2usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let l_b: Vec<f64> = (0..total).map(|i| (i as f64 * 0.17).sin() * 4000.0).collect();
        let r_b: Vec<f64> = (0..total).map(|i| (i as f64 * 0.4).cos() * 4000.0).collect();
        let b_ch = vec![l_b.clone(), r_b.clone()];
        let b_mono: Vec<f64> = (0..total).map(|i| (l_b[i] + r_b[i]) / 2.0).collect();
        let a_samples = interleave_a(
            &[l_b.iter().map(|s| s * 0.5).collect(), r_b.iter().map(|s| s * 0.5).collect()],
            4000.0,
        );

        let params = |window: usize| SeamFloorParams {
            a_samples: &a_samples,
            channels,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window.max(1),
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };

        let mc_pre = seam_chosen_and_floor_multichannel(
            &params(window), &b_ch, &[0, 1], SeamSide::Pre, gap_start, gap_end, 0,
        );
        let mc_post = seam_chosen_and_floor_multichannel(
            &params(window), &b_ch, &[0, 1], SeamSide::Post, gap_start, gap_end, 0,
        );
        let mc = SeamResidualVerdict::from_channel_residuals(
            &mc_pre, &mc_post, DEFAULT_RESIDUAL_FLOOR_OK_DB, 0, 512,
        );
        assert_eq!(mc_pre.len(), 2, "both stereo channels measured");

        let (cp, fp) = seam_chosen_and_floor(&params(window), SeamSide::Pre, gap_start, gap_end, 0);
        let (cq, fq) = seam_chosen_and_floor(&params(window), SeamSide::Post, gap_start, gap_end, 0);
        let mono = SeamResidualVerdict::from_parts(&cp, &cq, &fp, &fq);

        assert!(mc.worst_floor_db() < -40.0, "per-channel cancels: {}", mc.worst_floor_db());
        assert!(mono.worst_floor_db() < -40.0, "mono cancels: {}", mono.worst_floor_db());
        assert!(mc.worst_headroom_db().abs() < 1.0, "mc headroom ~0: {}", mc.worst_headroom_db());
        assert!(mono.worst_headroom_db().abs() < 1.0, "mono headroom ~0: {}", mono.worst_headroom_db());
        assert!(mc.informative && mono.informative);
    }

    #[test]
    fn seam_chosen_and_floor_multichannel_empty_selection_is_mono_fallback() {
        // Empty selection → single mono-downmix entry identical to `seam_chosen_and_floor`.
        let total = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let b_mono: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();
        let b_ch = vec![b_mono.clone()];

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };

        let mc = seam_chosen_and_floor_multichannel(
            &params, &b_ch, &[], SeamSide::Pre, gap_start, gap_end, 0,
        );
        let (chosen, floor) = seam_chosen_and_floor(&params, SeamSide::Pre, gap_start, gap_end, 0);

        assert_eq!(mc.len(), 1, "empty selection collapses to one mono entry");
        assert!((mc[0].floor.residual_db - floor.residual_db).abs() < 1e-9);
        assert!((mc[0].chosen.residual_db - chosen.residual_db).abs() < 1e-9);
        assert_eq!(mc[0].floor.source, floor.source);
    }

    #[test]
    fn seam_chosen_and_floor_multichannel_shared_lag_follows_matching_channel() {
        // The gap content lives in ch2 with a true A→B lag of +3; ch0 is *louder* but its B content
        // does not match A at all. The shared lag must come from the matching channel (ch2), and the
        // non-matching loud channel must be measured at that same shared lag — proving alignment is
        // not hijacked by the loudest channel. A naive mono downmix would let ch0's energy corrupt it.
        let total = 2200usize;
        let channels = 3usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;
        let true_lag = 3i64;

        // Broadband pseudo-random noise → sharply peaked autocorrelation (single, unambiguous lag).
        let prng = |seed: u32| -> Vec<f64> {
            let mut x = seed;
            (0..total)
                .map(|_| {
                    x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                    (x >> 8) as f64 / f64::from(1u32 << 24) * 8000.0 - 4000.0
                })
                .collect()
        };
        let sig = prng(12345);
        // ch2 B = signal; ch0 B = a *different* loud broadband waveform; ch1 silent.
        let b_ch2 = sig.clone();
        let b_ch0 = prng(999);
        let b_ch1 = vec![0.0f64; total];
        let b_ch = vec![b_ch0.clone(), b_ch1.clone(), b_ch2.clone()];
        let b_mono: Vec<f64> =
            (0..total).map(|i| (b_ch0[i] + b_ch1[i] + b_ch2[i]) / 3.0).collect();

        // A: ch2 is B's signal at half level, shifted by +3 frames (the true lag); ch0 is loud,
        // unrelated to ch0's B; ch1 silent.
        let shift = true_lag as usize;
        let a_ch2: Vec<f64> = (0..total)
            .map(|i| if i + shift < total { sig[i + shift] * 0.5 } else { 0.0 })
            .collect();
        let a_ch0 = prng(7777);
        let a_samples = interleave_a(&[a_ch0, b_ch1.clone(), a_ch2], 4000.0);

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };

        // Both loud channels are selected (within 20 dB); ch1 is silent and excluded by the caller.
        let mc = seam_chosen_and_floor_multichannel(
            &params, &b_ch, &[0, 2], SeamSide::Pre, gap_start, gap_end, 0,
        );
        let by_ch = |ch: usize| mc.iter().find(|c| c.channel == ch).expect("channel present");

        // Shared lag came from the matching channel: BOTH channels were measured at lag +3.
        assert_eq!(by_ch(2).floor.best_lag, true_lag, "matching channel sets the shared lag");
        assert_eq!(
            by_ch(0).floor.best_lag,
            true_lag,
            "the loud non-matching channel is measured at the shared lag, not its own"
        );
        assert!(
            by_ch(2).floor.residual_db < -40.0,
            "matching channel cancels deeply at the shared lag, got {}",
            by_ch(2).floor.residual_db
        );
        assert!(
            by_ch(0).floor.residual_db > -6.0,
            "non-matching channel does not cancel, got {}",
            by_ch(0).floor.residual_db
        );

        let mc_post = seam_chosen_and_floor_multichannel(
            &params, &b_ch, &[0, 2], SeamSide::Post, gap_start, gap_end, 0,
        );
        let verdict =
            SeamResidualVerdict::from_channel_residuals(&mc, &mc_post, DEFAULT_RESIDUAL_FLOOR_OK_DB, 0, 512);
        assert!(verdict.informative, "matching channel establishes the same-master regime");
    }

    #[test]
    fn from_channel_residuals_worst_headroom_and_best_floor_informative() {
        // Aggregation: a well-cancelling channel and a channel that cancels at nominal (low floor)
        // but not at the chosen placement (high chosen) → the bad channel drives `worst_headroom_db`.
        let good = SeamChannelResidual { channel: 0, chosen: probe_at(-50.0), floor: probe_at(-50.0) };
        let bad = SeamChannelResidual { channel: 2, chosen: probe_at(-2.0), floor: probe_at(-45.0) };
        let v = SeamResidualVerdict::from_channel_residuals(&[good, bad], &[good, bad], -15.0, 0, 0);
        assert!(
            (v.worst_headroom_db() - 43.0).abs() < 0.5,
            "worst channel should drive headroom, got {}",
            v.worst_headroom_db()
        );

        // Decoupling: a noisy surround whose floor never cancels (−3 dB > floor_ok) must NOT flip
        // `informative` off when another selected channel established the regime (best floor −50 dB).
        let center = SeamChannelResidual { channel: 2, chosen: probe_at(-50.0), floor: probe_at(-50.0) };
        let surround = SeamChannelResidual { channel: 4, chosen: probe_at(-3.0), floor: probe_at(-3.0) };
        let v2 = SeamResidualVerdict::from_channel_residuals(
            &[center, surround], &[center, surround], -15.0, 0, 0,
        );
        assert!(
            v2.informative,
            "best-floor channel establishes the regime; a noisy surround must not veto informative"
        );
    }

    #[test]
    fn residual_verdict_informative_boundary_at_floor_ok() {
        let deep = SeamFloorProbe {
            source: SeamFloorSource::Border,
            residual_db: -20.0,
            gain: 1.0,
            best_lag: 0,
        };
        let shallow = SeamFloorProbe {
            source: SeamFloorSource::Border,
            residual_db: -10.0,
            gain: 1.0,
            best_lag: 0,
        };
        let none = SeamFloorProbe::none();
        let floor_ok = -15.0;

        assert!(residual_verdict_informative(&deep, &deep, floor_ok));
        assert!(!residual_verdict_informative(&shallow, &deep, floor_ok));
        assert!(!residual_verdict_informative(&deep, &shallow, floor_ok));
        assert!(!residual_verdict_informative(&none, &none, floor_ok));
        assert!(residual_verdict_informative(&deep, &none, floor_ok));
    }

    #[test]
    fn seam_floor_probe_none_when_all_quiet() {
        // No energetic reference anywhere → source None.
        let total = 1000usize;
        let b_mono: Vec<f64> = (0..total).map(|i| (i as f64 * 0.2).sin() * 4000.0).collect();
        let a_samples = vec![0.0f32; total];
        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window: 128,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: 128,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let pre = seam_floor_probe(&params, SeamSide::Pre, 600, 800);
        assert_eq!(pre.source, SeamFloorSource::None);
        assert!(pre.residual_db.is_nan());
        let none = SeamFloorProbe::none();
        let verdict = SeamResidualVerdict::from_parts(&none, &none, &none, &none);
        assert!(!verdict.informative);
    }

    #[test]
    fn lsq_residual_ratio_abstains_when_b_silent() {
        let a: Vec<f64> = (0..16).map(|i| (i as f64 * 0.3).sin() * 1000.0).collect();
        let b = vec![0.0; 16];
        assert!(lsq_residual_ratio(&a, &b).is_none());
    }

    #[test]
    fn seam_residual_abstains_when_b_silent_at_placement() {
        // L13: silent B at every lag must not read as ~0 dB "no cancellation".
        let pre_window = 16usize;
        let start = 64usize;

        let b_mono = vec![0.0; 200];
        let a_pre: Vec<f64> = (0..pre_window)
            .map(|i| (i as f64 * 0.3).sin() * 1000.0)
            .collect();

        let fit = seam_residual_for_side(&a_pre, &b_mono, |lag| {
            let lo = start as i64 - pre_window as i64 + lag;
            let hi = start as i64 + lag;
            if lo < 0 || hi > b_mono.len() as i64 || hi <= lo {
                return None;
            }
            Some((lo as usize, hi as usize))
        }, 64, 0);
        assert!(fit.is_none(), "silent B should abstain, not report ~0 dB residual");
    }

    #[test]
    fn seam_residual_high_for_unrelated_audio() {
        // A's border is unrelated to B at the placement: residual stays near 0 dB (no cancellation).
        let pre_window = 16usize;
        let start = 64usize;

        let b_mono: Vec<f64> = (0..200).map(|i| (i as f64 * 0.3).sin() * 1000.0).collect();
        let a_pre: Vec<f64> = (0..pre_window)
            .map(|i| (i as f64 * 1.7 + 0.5).cos() * 1000.0)
            .collect();

        let pre = seam_residual_for_side(&a_pre, &b_mono, |lag| {
            let lo = start as i64 - pre_window as i64 + lag;
            let hi = start as i64 + lag;
            if lo < 0 || hi > b_mono.len() as i64 || hi <= lo {
                return None;
            }
            Some((lo as usize, hi as usize))
        }, 64, 0)
        .expect("pre lag fit");
        assert!(pre.residual_db > -6.0, "unrelated audio should not cancel, got {} dB", pre.residual_db);
    }
}
