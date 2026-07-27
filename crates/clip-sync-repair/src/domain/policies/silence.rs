//! Silence scanning, RMS helpers, and fill-gain matching.
//!
//! Leaf module of `domain::policies` — depends only on `domain::pcm`. `is_silent_frame` lives here
//! because both the scanner and `gap_borders`' border refinement consume it.
use crate::domain::pcm::InterleavedSamples;

/// A contiguous silent region on a media timeline (seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilentRun {
    pub start_secs: f64,
    pub end_secs: f64,
    /// Block-confirmed silent interior — the span of fully-silent analysis blocks, never widened
    /// by the sub-block edge refinement that produces `[start_secs, end_secs]`. Equals the reported
    /// boundaries for runs whose onset/offset happen to land on block edges. The gap-equivalence
    /// gate classifies on this core (not the refined extent) so the fade-shoulder frames pulled in
    /// by refinement never pollute the A-side dropout-depth measurement (`aggregate_rms_db` is an
    /// energy mean dominated by the loudest included block, so one partial-signal block flips a
    /// deep dropout to `ambient-quiet`).
    pub core_start_secs: f64,
    pub core_end_secs: f64,
}

/// One analysis block's RMS level (dBFS) on a media timeline — the lightweight per-block timeline the
/// gap-equivalence gate reads (`docs/gap-scan.md`). Retained only when the scanner is
/// built [`SilenceRunScanner::retain_block_levels`]; the scan already computes the RMS, so this just keeps
/// it instead of discarding it. `rms_db` is floored at [`BLOCK_LEVEL_FLOOR_DB`] (never `-inf`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockLevel {
    pub start_secs: f64,
    pub end_secs: f64,
    pub rms_db: f64,
}

/// dBFS a fully-silent analysis block floors to — same convention as the fingerprint's `level_profile`
/// (`SILENCE_FLOOR_DB`), so a scan-derived noise floor is on the same scale as the fingerprint's.
pub const BLOCK_LEVEL_FLOOR_DB: f64 = -120.0;

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
    /// Block-confirmed silent interior of the open run, tracked in parallel with the refined
    /// `run_start`/`silent_tail`. `core_start` is the onset block's leading edge (no backward
    /// walk); `core_end` is the last fully-silent block's end (no forward walk). Emitted as
    /// `SilentRun::core_*` for the equivalence gate.
    core_start: Option<f64>,
    core_end: Option<f64>,
    runs: Vec<SilentRun>,
    /// When `true`, [`feed`](Self::feed) retains a per-block [`BlockLevel`] timeline in `levels`.
    retain_levels: bool,
    levels: Vec<BlockLevel>,
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
            core_start: None,
            core_end: None,
            runs: Vec::new(),
            retain_levels: false,
            levels: Vec::new(),
        }
    }

    /// Opt in to retaining a per-block [`BlockLevel`] RMS timeline (the gap-equivalence signal source).
    /// Off by default so the existing scan callers pay nothing; the gap scan turns it on for A and B.
    pub fn retain_block_levels(mut self) -> Self {
        self.retain_levels = true;
        self
    }

    /// Classify `pcm` (starting at `timeline_start_secs` on the file timeline) into blocks.
    ///
    /// Silence requires every channel in a block to pass [`is_silent_interleaved`] (ffmpeg
    /// `silencedetect` default: all channels quiet simultaneously).
    ///
    /// Run boundaries are refined to sub-block precision at the frame level: the blocks that
    /// straddle a silence's onset/offset read non-silent (their peak comes from the loud
    /// shoulder), so a pure block-quantized run undercounts the true silent span by up to ~2
    /// blocks — enough to push a genuine `≥ min_gap_secs` dropout under the emit threshold in
    /// [`close_open_run`]. Frame-level edge walks (bounded to the adjacent straddling block) fix
    /// this without buffering, so `min_gap_secs` is tested against the true extent.
    pub fn feed<P: InterleavedSamples>(&mut self, pcm: &P, timeline_start_secs: f64) {
        if self.block_secs <= 0.0 || pcm.samples().is_empty() {
            return;
        }

        let channels = pcm.channels().max(1) as usize;
        let rate = pcm.sample_rate();
        let block_frames = (self.block_secs * f64::from(rate)).round().max(1.0) as usize;
        let total_frames = pcm.frames();

        let mut offset_frames = 0usize;
        while offset_frames < total_frames {
            let end_frames = (offset_frames + block_frames).min(total_frames);
            let block_start_secs = timeline_start_secs + offset_frames as f64 / f64::from(rate);
            let block_end_secs = timeline_start_secs + end_frames as f64 / f64::from(rate);
            let block_start = offset_frames * channels;
            let block_end = end_frames * channels;
            let block = &pcm.samples()[block_start..block_end];

            if self.retain_levels {
                self.levels.push(BlockLevel {
                    start_secs: block_start_secs,
                    end_secs: block_end_secs,
                    rms_db: block_rms_db(block),
                });
            }

            if is_silent_interleaved(
                block,
                channels,
                self.silence_peak_fraction,
                self.absolute_rms_floor,
            ) {
                self.held_count = 0;
                if self.run_start.is_none() {
                    // Sub-block leading-edge refinement: a block straddling the silence onset
                    // takes its peak from the loud shoulder and reads non-silent, so `run_start`
                    // lands up to one block *late*. Walk backward through the preceding block's
                    // trailing silent frames (bounded — that block has ≥1 non-silent frame, or it
                    // would already be in the run) so the run starts at the true onset, not the
                    // block edge. Only refinable within this bucket; a run that opens on the first
                    // block of a bucket falls back to the block edge (prior samples are gone).
                    let mut edge = offset_frames;
                    while edge > 0
                        && is_silent_frame(
                            pcm.samples(),
                            channels,
                            edge - 1,
                            self.silence_peak_fraction,
                            self.absolute_rms_floor,
                        )
                    {
                        edge -= 1;
                    }
                    self.run_start = Some(timeline_start_secs + edge as f64 / f64::from(rate));
                    // Core onset is the block edge (unwalked) — this whole block is silent.
                    self.core_start = Some(block_start_secs);
                }
                // Advance the confirmed-silent boundary past any previously held blocks.
                self.silent_tail = Some(block_end_secs);
                self.core_end = Some(block_end_secs);
            } else if self.run_start.is_some() {
                // Sub-block trailing-edge refinement: the block that closes a run straddles the
                // silence offset, so `silent_tail` (last fully-silent block end) lands up to one
                // block *early*. On the first non-silent block after silence (`held_count == 0`),
                // walk forward through its leading silent frames and extend `silent_tail` to the
                // true offset. Bounded — this block has ≥1 non-silent frame. If the run later
                // continues, a subsequent silent block overwrites `silent_tail` with its own end.
                if self.held_count == 0 {
                    let mut edge = offset_frames;
                    while edge < end_frames
                        && is_silent_frame(
                            pcm.samples(),
                            channels,
                            edge,
                            self.silence_peak_fraction,
                            self.absolute_rms_floor,
                        )
                    {
                        edge += 1;
                    }
                    if edge > offset_frames {
                        self.silent_tail =
                            Some(timeline_start_secs + edge as f64 / f64::from(rate));
                    }
                }
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

    /// Close any open run and return both the detected intervals and the retained per-block level timeline
    /// (empty unless built with [`retain_block_levels`](Self::retain_block_levels)). Used by the gap scan,
    /// which needs the levels to derive per-gap equivalence signals.
    pub fn finish_with_levels(mut self) -> (Vec<SilentRun>, Vec<BlockLevel>) {
        self.close_open_run();
        (self.runs, self.levels)
    }

    /// Break an open silent run when decoded PCM has a timeline hole (e.g. skipped decode chunk).
    pub fn note_pcm_discontinuity(&mut self) {
        self.held_count = 0;
        self.close_open_run();
    }

    fn close_open_run(&mut self) {
        let (Some(start), Some(end)) = (self.run_start.take(), self.silent_tail.take()) else {
            self.core_start = None;
            self.core_end = None;
            return;
        };
        let core_start = self.core_start.take().unwrap_or(start);
        let core_end = self.core_end.take().unwrap_or(end);
        // Gate on the refined extent so a genuine ≥ min_gap dropout straddling block edges is not
        // dropped by quantization; report the refined boundaries but preserve the block-confirmed
        // core for the equivalence gate.
        if end - start >= self.min_gap_secs {
            self.runs.push(SilentRun {
                start_secs: start,
                end_secs: end,
                core_start_secs: core_start,
                core_end_secs: core_end,
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

/// One analysis block's overall RMS in dBFS, floored at [`BLOCK_LEVEL_FLOOR_DB`] (a silent block reads the
/// floor, not `-inf`). Downmix-agnostic: the RMS is taken over all interleaved samples, matching the
/// scan's own `is_silent` energy.
fn block_rms_db(block: &[f32]) -> f64 {
    let rms = f64::from(rms_f32(block));
    if rms <= 1e-9 {
        BLOCK_LEVEL_FLOOR_DB
    } else {
        20.0 * rms.log10()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::pcm::InterleavedPcm;

    fn mono_pcm(rate: u32, samples: Vec<f32>) -> InterleavedPcm {
        InterleavedPcm {
            sample_rate: rate,
            channels: 1,
            samples,
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
        let samples: Vec<f32> = (0..11_025)
            .map(|i| if i % 2 == 0 { v } else { -v })
            .collect();
        assert!(
            !is_silent(&samples, 0.01, 0.0),
            "no floor: should not be silent"
        );
        assert!(
            is_silent(&samples, 0.01, floor),
            "floor: peak < floor → silent"
        );
        let loud_v = 5.0_f32 / 32767.0;
        let loud_samples: Vec<f32> = (0..11_025)
            .map(|i| if i % 2 == 0 { loud_v } else { -loud_v })
            .collect();
        assert!(
            !is_silent(&loud_samples, 0.01, floor),
            "floor: peak > floor → not silent by floor"
        );
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
        samples.extend(std::iter::repeat_n(
            0.0f32,
            (rate as f64 * 3.0).round() as usize,
        ));
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
    fn retain_block_levels_records_a_db_timeline() {
        let rate = 11_025u32;
        let block_secs = 0.25;
        // 1 s of sine (loud) then 1 s of digital silence.
        let mut samples = sine_samples(rate, 1.0);
        samples.extend(std::iter::repeat_n(0.0f32, rate as usize));
        let pcm = mono_pcm(rate, samples);

        let mut scanner =
            SilenceRunScanner::new(block_secs, 0.01, 1.0, 0, 0.0).retain_block_levels();
        scanner.feed(&pcm, 0.0);
        let (_runs, levels) = scanner.finish_with_levels();

        // ~2 s / 0.25 s ⇒ 8 full blocks plus a tiny trailing partial (block frames don't divide evenly).
        assert!(levels.len() >= 8, "one block per 0.25 s: {}", levels.len());
        // First block is loud; a mid-silence block reads the floor exactly.
        assert!(levels[0].rms_db > -30.0, "loud block: {:?}", levels[0]);
        assert_eq!(
            levels[6].rms_db, BLOCK_LEVEL_FLOOR_DB,
            "silent block floors: {:?}",
            levels[6]
        );
        assert!((levels[0].start_secs - 0.0).abs() < 1e-9);
        assert_eq!(
            levels.last().unwrap().rms_db,
            BLOCK_LEVEL_FLOOR_DB,
            "tail is silent"
        );
    }

    #[test]
    fn levels_empty_unless_retained() {
        let rate = 11_025u32;
        let pcm = mono_pcm(rate, sine_samples(rate, 1.0));
        let mut scanner = SilenceRunScanner::new(0.25, 0.01, 1.0, 0, 0.0);
        scanner.feed(&pcm, 0.0);
        let (_runs, levels) = scanner.finish_with_levels();
        assert!(levels.is_empty(), "retain not requested ⇒ no timeline");
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
    fn silence_run_scanner_detects_sub_block_margin_gap() {
        // Regression: a 543 ms silence at 100 ms blocks / min_gap 500 ms. The silence starts and
        // ends mid-block, so only 4 blocks are *fully* silent (400 ms) — block-quantized detection
        // undercounts to < min_gap and drops the gap (the pair-16 1:28:31 miss ffmpeg's
        // silencedetect=noise=-60dB:d=0.5 catches). Sub-block edge refinement recovers the true
        // 543 ms span so the run clears min_gap.
        let rate = 48_000u32;
        let block_secs = 0.1;
        let min_gap = 0.5;

        // 2.05 s sine → 0.543 s of exact zeros → 2.0 s sine. The 2.05 s lead pushes the silence
        // onset to mid-block (block 20), and the 543 ms length ends mid-block (block 25).
        let mut samples = sine_samples(rate, 2.05);
        let silence_frames = (rate as f64 * 0.543).round() as usize;
        samples.extend(vec![0.0f32; silence_frames]);
        samples.extend(sine_samples(rate, 2.0));
        let pcm = mono_pcm(rate, samples);

        let mut scanner = SilenceRunScanner::new(block_secs, 0.01, min_gap, 0, 0.0);
        scanner.feed(&pcm, 0.0);
        let runs = scanner.finish();

        assert_eq!(
            runs.len(),
            1,
            "sub-block-margin silence should be detected, not dropped"
        );
        let dur = runs[0].end_secs - runs[0].start_secs;
        assert!(
            dur >= min_gap,
            "refined span {dur}s must clear min_gap {min_gap}s"
        );
        assert!(
            (runs[0].start_secs - 2.05).abs() < 0.001,
            "start {} ≈ 2.05",
            runs[0].start_secs
        );
        assert!((dur - 0.543).abs() < 0.001, "duration {dur} ≈ 0.543");
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

        assert_eq!(
            runs.len(),
            1,
            "hold=1 should merge across the single noisy block"
        );
        // Total span is 17 blocks = 4.25s; duration should be close to that.
        assert!(
            runs[0].end_secs - runs[0].start_secs >= 4.0,
            "merged run should span most of the 4.25s signal, got {}s",
            runs[0].end_secs - runs[0].start_secs
        );
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
        assert_eq!(
            scanner.finish().len(),
            2,
            "hold=0 should split at the noisy block"
        );
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
        assert!(
            (result - v).abs() < 0.001,
            "rms of constant {v} should be ~{v}, got {result}"
        );
    }

    #[test]
    fn compute_fill_gain_clamps_to_max_db() {
        // a=1000, b=1 => raw gain=1000; max_db=12 => max_gain=10^(12/20)≈3.981
        let gain = compute_fill_gain(1000.0, 1.0, 12.0);
        let expected_max = 10f32.powf(12.0 / 20.0);
        assert!(
            (gain - expected_max).abs() < 0.001,
            "gain should be clamped to {expected_max}, got {gain}"
        );
    }

    #[test]
    fn compute_fill_gain_unity_when_rms_zero() {
        assert_eq!(compute_fill_gain(0.0, 500.0, 12.0), 1.0);
        assert_eq!(compute_fill_gain(500.0, 0.0, 12.0), 1.0);
    }
}
