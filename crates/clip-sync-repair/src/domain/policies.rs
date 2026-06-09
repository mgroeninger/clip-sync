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

/// Slide a candidate B window to maximize agreement with A's borders at both gap seams.
///
/// `nominal_start_frame` is where the coarse offset maps the fill inside `b_extended`.
/// Search is limited to ±`max_adjustment_frames` around that position.
pub fn align_fill_segment(
    a_pre_border: &[f64],
    a_post_border: &[f64],
    b_extended: &[f64],
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

    let mut best: Option<FillAlignment> = None;

    for delta in -(max_adjustment_frames as i64)..=(max_adjustment_frames as i64) {
        let start = nominal_start_frame as i64 + delta;
        if start < 0 {
            continue;
        }
        let start = start as usize;
        if start + gap_frames > b_extended.len() || start + gap_frames + post_window > b_extended.len()
        {
            continue;
        }

        if start < pre_window {
            continue;
        }

        let pre_corr = normalized_correlation(
            &a_pre_border[a_pre_border.len() - pre_window..],
            &b_extended[start - pre_window..start],
        );
        let post_corr = normalized_correlation(
            &a_post_border[..post_window],
            &b_extended[start + gap_frames..start + gap_frames + post_window],
        );
        let score = (pre_corr + post_corr) * 0.5;

        let is_better = |candidate: &FillAlignment| -> bool {
            let candidate_score = (candidate.pre_correlation + candidate.post_correlation) * 0.5;
            if score > candidate_score + f64::EPSILON {
                return true;
            }
            if (score - candidate_score).abs() > f64::EPSILON {
                return false;
            }
            let candidate_delta = candidate.start_frame.abs_diff(nominal_start_frame);
            let new_delta = start.abs_diff(nominal_start_frame);
            new_delta < candidate_delta
        };

        if best.as_ref().is_none_or(is_better) {
            best = Some(FillAlignment {
                start_frame: start,
                pre_correlation: pre_corr,
                post_correlation: post_corr,
            });
        }
    }

    best
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
}
