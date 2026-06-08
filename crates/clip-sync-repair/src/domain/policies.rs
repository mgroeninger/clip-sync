use clip_sync::MonoPcmClip;

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
    pub fn feed(&mut self, pcm: &MonoPcmClip, timeline_start_secs: f64) {
        if self.block_secs <= 0.0 || pcm.samples.is_empty() {
            return;
        }

        let block_samples = (self.block_secs * f64::from(pcm.sample_rate))
            .round()
            .max(1.0) as usize;
        let rate = pcm.sample_rate;

        let mut offset = 0usize;
        while offset < pcm.samples.len() {
            let end = (offset + block_samples).min(pcm.samples.len());
            let block_start_secs = timeline_start_secs + offset as f64 / f64::from(rate);
            let block_end_secs = timeline_start_secs + end as f64 / f64::from(rate);
            let block = &pcm.samples[offset..end];

            if is_silent(block, self.silence_peak_fraction, self.absolute_rms_floor) {
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

            offset = end;
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
/// - all samples are zero, or
/// - `RMS < absolute_rms_floor` (catches codec noise in otherwise-silent gaps), or
/// - `RMS < peak × silence_peak_fraction` (catches sparse transients in a sea of zeros).
///
/// Pass `absolute_rms_floor = 0.0` to disable the absolute check.
pub fn is_silent(samples: &[i16], silence_peak_fraction: f32, absolute_rms_floor: f32) -> bool {
    if samples.is_empty() {
        return true;
    }

    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32;

    if peak == 0.0 {
        return true;
    }

    let rms = rms_i16(samples);

    if absolute_rms_floor > 0.0 && rms < absolute_rms_floor {
        return true;
    }

    rms < peak * silence_peak_fraction
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
        // All samples at ±1 — relative check (RMS ≈ 1 vs peak = 1) would NOT flag as silent,
        // but the absolute floor should.
        let samples: Vec<i16> = (0..11_025).map(|i| if i % 2 == 0 { 1 } else { -1 }).collect();
        assert!(!is_silent(&samples, 0.01, 0.0), "no floor: should not be silent");
        assert!(is_silent(&samples, 0.01, 2.0), "floor=2: RMS≈1 < 2 → silent");
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

        let pcm = MonoPcmClip {
            sample_rate: rate,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };

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

        let first = MonoPcmClip {
            sample_rate: rate,
            samples: vec![0i16; (rate as f64 * 2.0).round() as usize],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let second = MonoPcmClip {
            sample_rate: rate,
            samples: vec![0i16; (rate as f64 * 2.0).round() as usize],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };

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

        let pcm = MonoPcmClip {
            sample_rate: rate,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };

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

        let pcm = MonoPcmClip { sample_rate: rate, samples, decode_error_skips: 0, decoded_sample_count: None };

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

        let pcm = MonoPcmClip { sample_rate: rate, samples, decode_error_skips: 0, decoded_sample_count: None };

        let mut scanner = SilenceRunScanner::new(block_secs, 0.01, 1.0, 0, 0.0);
        scanner.feed(&pcm, 0.0);
        assert_eq!(scanner.finish().len(), 2, "hold=0 should split at the noisy block");
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
}
