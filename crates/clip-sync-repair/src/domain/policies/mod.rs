//! Repair domain policies: silence, borders, seam scoring/residual, splice.
//!
//! Public paths stay `crate::domain::policies::*` via re-exports from submodules.
mod seam_residual;
pub use seam_residual::{
    floor_probe_informative, residual_verdict_informative, seam_chosen_and_floor,
    seam_chosen_and_floor_multichannel, seam_floor_probe, DEFAULT_RESIDUAL_FLOOR_OK_DB,
    SeamChannelResidual, SeamFloorParams, SeamFloorProbe, SeamFloorSource, SeamResidualVerdict,
    SeamSide,
};

use crate::domain::metrics::normalized_correlation;
use crate::domain::pcm::InterleavedSamples;
use crate::domain::seam_local::seam_correlation_over_bases;


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
                    self.run_start =
                        Some(timeline_start_secs + edge as f64 / f64::from(rate));
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
pub(crate) fn seam_score_channel_indices(a_pre_ch: &[Vec<f64>], a_post_ch: &[Vec<f64>]) -> Vec<usize> {
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

/// The **loudest** energy-passing seam channel — argmax of per-channel border energy (max of pre/post
/// mean-square), or `None` when every channel is near-silent. Use this for a single-channel
/// *representative* (e.g. the lag/probe diagnostic) instead of `selected_seam_channels().first()`, which
/// returns the **lowest-index** passing channel — index order, an arbitrary pick that lands on L over a
/// louder C in a center-dominant mix. The gate's multichannel decision still uses the full
/// `selected_seam_channels` set; this only changes which single channel a diagnostic follows.
pub fn loudest_seam_channel(a_samples: &[f32], channels: usize, spec: &GapBorderSpec) -> Option<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    let n = a_pre_ch.len().min(a_post_ch.len());
    (0..n)
        .map(|ch| (ch, template_mean_square(&a_pre_ch[ch]).max(template_mean_square(&a_post_ch[ch]))))
        .filter(|&(_, e)| e > f64::EPSILON)
        .max_by(|(_, x), (_, y)| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(ch, _)| ch)
}

/// Energy-selected seam channels for a gap, computed from the same per-channel border templates
/// Pearson scores — the single entry point so residual/floor measurement follows the *same*
/// channels as seam scoring (see `seam-scoring.md`). Returns empty when every channel is
/// near-silent, so the caller falls back to the mono downmix.
pub fn selected_seam_channels(
    a_samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> Vec<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    seam_score_channel_indices(&a_pre_ch, &a_post_ch)
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
    let SeamTemplates { a_pre, a_post, a_pre_ch, a_post_ch, b_mono, b_ch } = *templates;

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
    let template: &[f64] = if tail { &a[a.len() - window..] } else { &a[..window] };
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
                ch_bands.iter().map(|b| b[i]).fold(f64::NEG_INFINITY, f64::max)
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
    use crate::domain::pcm::InterleavedPcm;


    fn det_noise(seed: u64, n: usize) -> Vec<f64> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
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
        let b_ch: Vec<Vec<f64>> = (0..nch).map(|c| det_noise(300 + c as u64, total_b)).collect();
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
            &templates, gap_frames, pre_window, post_window, &score_channels, start_lo, start_hi,
        )
        .expect("band applies for interior starts");
        assert_eq!(band.len(), start_hi - start_lo + 1);
        for (i, &(pre, post)) in band.iter().enumerate() {
            let (npre, npost) = fill_seam_correlations_with_channels(
                &templates,
                SeamPlacement { start: start_lo + i, gap_frames, pre_window, post_window },
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
            &mono_templates, gap_frames, pre_window, post_window, &[], start_lo, start_hi,
        )
        .expect("mono band applies");
        for (i, &(pre, post)) in mband.iter().enumerate() {
            let (npre, npost) = fill_seam_correlations_with_channels(
                &mono_templates,
                SeamPlacement { start: start_lo + i, gap_frames, pre_window, post_window },
                &[],
            );
            assert!(
                (pre - npre).abs() < 1e-8 && (post - npost).abs() < 1e-8,
                "mono start {}: pre {pre} vs {npre}, post {post} vs {npost}",
                start_lo + i
            );
        }
    }

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
    fn retain_block_levels_records_a_db_timeline() {
        let rate = 11_025u32;
        let block_secs = 0.25;
        // 1 s of sine (loud) then 1 s of digital silence.
        let mut samples = sine_samples(rate, 1.0);
        samples.extend(std::iter::repeat_n(0.0f32, rate as usize));
        let pcm = mono_pcm(rate, samples);

        let mut scanner = SilenceRunScanner::new(block_secs, 0.01, 1.0, 0, 0.0).retain_block_levels();
        scanner.feed(&pcm, 0.0);
        let (_runs, levels) = scanner.finish_with_levels();

        // ~2 s / 0.25 s ⇒ 8 full blocks plus a tiny trailing partial (block frames don't divide evenly).
        assert!(levels.len() >= 8, "one block per 0.25 s: {}", levels.len());
        // First block is loud; a mid-silence block reads the floor exactly.
        assert!(levels[0].rms_db > -30.0, "loud block: {:?}", levels[0]);
        assert_eq!(levels[6].rms_db, BLOCK_LEVEL_FLOOR_DB, "silent block floors: {:?}", levels[6]);
        assert!((levels[0].start_secs - 0.0).abs() < 1e-9);
        assert_eq!(levels.last().unwrap().rms_db, BLOCK_LEVEL_FLOOR_DB, "tail is silent");
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

        assert_eq!(runs.len(), 1, "sub-block-margin silence should be detected, not dropped");
        let dur = runs[0].end_secs - runs[0].start_secs;
        assert!(dur >= min_gap, "refined span {dur}s must clear min_gap {min_gap}s");
        assert!((runs[0].start_secs - 2.05).abs() < 0.001, "start {} ≈ 2.05", runs[0].start_secs);
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
            single_lag_alignment: true,
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
                single_lag_alignment: true,
            },
        );

        assert!(pre_cf > pre_no_cf + 0.5, "pre should score bleed tail on A timeline");
        assert!(post_cf > post_no_cf + 0.5, "post should score fade head on A timeline");
        assert!(pre_cf > 0.9 && post_cf > 0.9);
    }

    /// A **dual-fit** fill's shoulders are independently matched at their own seam-local lag — the
    /// fill is NOT expected to sit at lag 0 against A's raw neighboring samples the way an ordinary
    /// rigid-lag splice fill is. Regression for the 2026-07-03 (`7a26a17`) / 2026-07-05 production
    /// bug: with `single_lag_alignment: true`, the crossfade-window branch compares the fill's own
    /// head/tail against raw A at the literal gap boundary and collapses to a strongly NEGATIVE
    /// correlation even though the fill matches the border template (what `try_dual_fit`'s own
    /// seam-local search validated) almost perfectly. `single_lag_alignment: false` must bypass that
    /// branch and score the border template instead.
    #[test]
    fn splice_seam_correlation_ignores_crossfade_lag0_window_when_not_single_lag_aligned() {
        let pre_window = 4usize;
        let post_window = 4usize;
        let cf = 2usize;
        let gap_start = 10usize;
        let gap_end = 14usize;
        let total = 20usize;

        // Raw A around the gap boundary: a short spike-then-drop right at the edge, unrelated to
        // the fill's own trend — this is what a genuine per-shoulder lag looks like: the fill's
        // content was matched further along B, not against A's literal immediate neighbor.
        let mut a_samples = vec![0.0f32; total];
        a_samples[gap_start - 2] = 100.0 / 32767.0;
        a_samples[gap_start - 1] = 0.0;
        a_samples[gap_end] = 0.0;
        a_samples[gap_end + 1] = 100.0 / 32767.0;

        // The fill's own head/tail ramp — matches the border templates (what seam-local search
        // validated) almost perfectly, but is monotonically opposite the raw A spike-then-drop above.
        let fill = vec![10.0, 20.0, 30.0, 40.0, 0.0, 0.0, 0.0, 0.0, 40.0, 30.0, 20.0, 10.0];
        let a_pre = vec![1.0, 2.0, 3.0, 4.0];
        let a_post = vec![4.0, 3.0, 2.0, 1.0];

        let dual_fit_ctx = SpliceSeamContext {
            seam_cf: cf,
            gap_start_frame: gap_start,
            gap_end_frame: gap_end,
            a_samples: &a_samples,
            channels: 1,
            single_lag_alignment: false,
        };
        let (pre_border, post_border) = fill_splice_seam_correlations(
            &fill,
            &a_pre,
            &a_post,
            pre_window,
            post_window,
            dual_fit_ctx,
        );
        assert!(
            pre_border > 0.9 && post_border > 0.9,
            "dual-fit fill must be scored against the border template it was actually matched to: pre={pre_border} post={post_border}"
        );

        let single_lag_ctx = SpliceSeamContext {
            seam_cf: cf,
            gap_start_frame: gap_start,
            gap_end_frame: gap_end,
            a_samples: &a_samples,
            channels: 1,
            single_lag_alignment: true,
        };
        let (pre_lag0, post_lag0) = fill_splice_seam_correlations(
            &fill,
            &a_pre,
            &a_post,
            pre_window,
            post_window,
            single_lag_ctx,
        );
        assert!(
            pre_lag0 < 0.0 && post_lag0 < 0.0,
            "sanity check: the lag-0 crossfade window must actually diverge from the border score \
             for this fixture (pre={pre_lag0} post={post_lag0}) — otherwise this test isn't exercising the bug"
        );
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


    /// Pearson (`seam_channel_diagnostics`) and residual (`selected_seam_channels`) must agree on
    /// which A-side channels carry border energy for a gap.
    fn assert_channel_selection_parity(
        a_samples: &[f32],
        channels: usize,
        spec: &GapBorderSpec,
        b_ch: &[Vec<f64>],
        placement: SeamPlacement,
    ) {
        let (a_pre, a_post) = border_templates_for_gap(a_samples, channels, spec);
        let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
        let b_mono: Vec<f64> = b_ch
            .first()
            .map(|_| {
                (0..b_ch[0].len())
                    .map(|i| b_ch.iter().map(|ch| ch[i]).sum::<f64>() / b_ch.len() as f64)
                    .collect()
            })
            .unwrap_or_default();
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch,
        };
        let pearson = seam_channel_diagnostics(&templates, placement).selected;
        let residual = selected_seam_channels(a_samples, channels, spec);
        assert_eq!(
            pearson, residual,
            "Pearson diagnostics and residual must select the same channels"
        );
    }

    #[test]
    fn selected_seam_channels_matches_pearson_diagnostics() {
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let border_frames = 128usize;
        let standoff = 16usize;
        let window = 128usize;
        let start = gap_start;
        let gap_frames = gap_end - gap_start;
        let placement = || SeamPlacement {
            start,
            gap_frames,
            pre_window: window,
            post_window: window,
        };

        // Stereo, equal energy on both channels → both selected.
        {
            let total = 2000usize;
            let channels = 2usize;
            let l: Vec<f64> = (0..total).map(|i| (i as f64 * 0.17).sin() * 4000.0).collect();
            let r: Vec<f64> = (0..total).map(|i| (i as f64 * 0.4).cos() * 4000.0).collect();
            let a_samples = interleave_a(&[l.clone(), r.clone()], 4000.0);
            let b_ch = vec![l, r];
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(
                &a_samples,
                channels,
                &spec,
                &b_ch,
                placement(),
            );
            assert_eq!(selected_seam_channels(&a_samples, channels, &spec), vec![0, 1]);
        }

        // Center-dominant 3ch: only FC crosses the ~20 dB energy gate in the border templates.
        {
            let total = 2000usize;
            let channels = 3usize;
            let fc_b: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
                .collect();
            let fl_b: Vec<f64> = (0..total).map(|i| (i as f64 * 0.53).sin() * 2000.0).collect();
            let fr_b: Vec<f64> = (0..total).map(|i| (i as f64 * 0.91).cos() * 2000.0).collect();
            let b_ch = vec![fl_b.clone(), fr_b.clone(), fc_b.clone()];
            let fc_a: Vec<f64> = fc_b.iter().map(|s| s * 0.5).collect();
            // Fronts well below FC in the border region (>20 dB down) so only the center is selected.
            let fl_a: Vec<f64> = vec![5.0; total];
            let fr_a: Vec<f64> = vec![5.0; total];
            let a_samples = interleave_a(&[fl_a, fr_a, fc_a], 4000.0);
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(
                &a_samples,
                channels,
                &spec,
                &b_ch,
                placement(),
            );
            assert_eq!(selected_seam_channels(&a_samples, channels, &spec), vec![2]);
        }

        // Near-silent borders → empty selection (mono fallback for both paths).
        {
            let total = 2000usize;
            let channels = 2usize;
            let a_samples = vec![0.0f32; total * channels];
            let b_ch = vec![vec![0.0f64; total], vec![0.0f64; total]];
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(
                &a_samples,
                channels,
                &spec,
                &b_ch,
                placement(),
            );
            assert!(selected_seam_channels(&a_samples, channels, &spec).is_empty());
        }
    }

    #[test]
    fn loudest_seam_channel_picks_by_energy_not_index() {
        let (gap_start, gap_end, border_frames, standoff) = (800usize, 1000usize, 128usize, 16usize);
        let total = 2000usize;
        let channels = 3usize;
        let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);

        // ch0 (L) moderate, ch1 (R) silent, ch2 (C) loudest. Both 0 and 2 pass the −20 dB energy gate,
        // so `selected_seam_channels` returns [0, 2] in index order and `.first()` = 0 (the *quieter*
        // channel). `loudest_seam_channel` must instead return 2 (the channel that carries the level).
        let l_a: Vec<f64> = (0..total).map(|i| (i as f64 * 0.17).sin() * 2000.0).collect();
        let r_a: Vec<f64> = vec![5.0; total];
        let c_a: Vec<f64> = (0..total).map(|i| (i as f64 * 0.31).sin() * 4000.0).collect();
        let a_samples = interleave_a(&[l_a, r_a, c_a], 4000.0);

        assert_eq!(selected_seam_channels(&a_samples, channels, &spec), vec![0, 2]);
        assert_eq!(
            selected_seam_channels(&a_samples, channels, &spec).first().copied(),
            Some(0),
            ".first() picks the lowest-index passing channel (the quieter L)"
        );
        assert_eq!(
            loudest_seam_channel(&a_samples, channels, &spec),
            Some(2),
            "loudest_seam_channel follows the level to the center channel"
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
