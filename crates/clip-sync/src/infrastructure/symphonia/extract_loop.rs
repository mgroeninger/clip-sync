use std::io;
use std::path::Path;
use std::time::Duration;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration as MediaDuration, TimeBase, Timestamp};
use tracing::{debug, warn};

use crate::application::error::MediaError;
use crate::application::ports::ProgressReporter;
use crate::domain::{AudioTrack, ClipWindow, MonoPcmClip, MultiChannelPcm};
use crate::infrastructure::symphonia::error_mapping::{
    decode_failed, fail_media, log_media_success, map_decode_loop_error, warn_partial_decode,
};
use crate::infrastructure::symphonia::extract::{
    append_frames_in_window, append_interleaved_frames_in_window, decode_shortfall_limit,
    media_duration_to_frames, seek_with_recovery, seek_to_window_start, tail_partial_clip_acceptable,
    timestamp_to_sample, timestamp_to_std_duration, window_sample_bounds, InterleavedCollectContext,
    WindowCollectContext, MAX_CONSECUTIVE_DECODE_ERRORS,
};
use crate::infrastructure::symphonia::session::{ensure_track_decoder, MediaIoState};

pub(super) struct ExtractLoopParams<'a> {
    pub path: &'a Path,
    pub state: &'a mut MediaIoState,
    pub track: &'a AudioTrack,
    pub window: &'a ClipWindow,
    pub progress: &'a dyn ProgressReporter,
    pub label: &'a str,
}

pub(super) struct ExtractFinalizeContext<'a> {
    pub path: &'a Path,
    pub track: &'a AudioTrack,
    pub window: &'a ClipWindow,
    pub progress: &'a dyn ProgressReporter,
    pub label: &'a str,
}

pub(super) trait ExtractSink {
    type Output;

    /// Reset output buffer and re-estimate capacity for a new seek attempt.
    fn reset_attempt(
        &mut self,
        sample_rate_hint: Option<u32>,
        window: &ClipWindow,
        path: &Path,
        track_index: u32,
        seek_start_secs: f64,
    );

    /// Called once on the first non-empty decoded buffer. Discovers rate (and channels for
    /// interleaved), reserves the output buffer, and sets the target unit count.
    fn on_first_decode(
        &mut self,
        rate: u32,
        channels: usize,
        window: &ClipWindow,
        path: &Path,
        track_index: u32,
    ) -> Result<(), MediaError>;

    /// True when the sink has all the per-format metadata it needs from the first decoded packet
    /// (rate for mono; rate AND channels for interleaved). The driver stops calling
    /// `on_first_decode` once this returns true.
    fn metadata_ready(&self) -> bool;
    fn resolved_rate(&self) -> Option<u32>;
    fn target_reached(&self) -> bool;

    /// Append in-window frames from a decoded packet. Returns `true` when the window end is
    /// reached.
    fn append_packet(
        &mut self,
        decoded: GenericAudioBufferRef<'_>,
        packet_start_unit: u64,
        trim_start_frames: u32,
        window: &ClipWindow,
        scratch: &mut Vec<f32>,
    ) -> bool;

    fn collected_units(&self) -> usize;
    fn target_units(&self) -> Option<usize>;
    fn has_output(&self) -> bool;

    fn finalize(
        &mut self,
        ctx: &ExtractFinalizeContext<'_>,
        allow_tail_padding: bool,
        decode_error_skips: u32,
    ) -> Result<Self::Output, MediaError>;
}

// ---------------------------------------------------------------------------
// Packet-loop helpers
// ---------------------------------------------------------------------------

pub(super) enum PacketRead {
    Got(Packet),
    Eof,
}

/// Read the next format packet, handling `ResetRequired` by resetting and retrying internally.
fn read_next_packet(
    state: &mut MediaIoState,
    path: &Path,
    track_id: u32,
    track_index: u32,
) -> Result<PacketRead, MediaError> {
    loop {
        match state.format.next_packet() {
            Ok(Some(pkt)) => return Ok(PacketRead::Got(pkt)),
            Ok(None) => return Ok(PacketRead::Eof),
            Err(SymphoniaError::ResetRequired) => {
                state.decoders.get_mut(&track_id).expect("decoder cached").decoder.reset();
            }
            Err(e) => return Err(map_decode_loop_error(path, track_index, e)),
        }
    }
}

pub(super) enum PacketWindowPos {
    Before,
    Past,
    Within,
}

/// Classify a packet's position relative to the extract window.
///
/// Uses sample-based bounds when `resolved_rate` is known; falls back to duration-based bounds
/// when rate is not yet resolved (before the first successful decode). Returns `Within` when
/// `time_base` is `None` or when neither boundary is crossed.
fn packet_window_pos(
    pts: Timestamp,
    dur: MediaDuration,
    time_base: Option<TimeBase>,
    resolved_rate: Option<u32>,
    window: &ClipWindow,
) -> PacketWindowPos {
    let Some(tb) = time_base else { return PacketWindowPos::Within };
    if let Some(rate) = resolved_rate {
        let (start_sample, end_sample) = window_sample_bounds(window, rate);
        let pkt_start = timestamp_to_sample(pts, tb, rate);
        let pkt_end = timestamp_to_sample(pts.saturating_add(dur), tb, rate);
        if pkt_end <= start_sample {
            return PacketWindowPos::Before;
        }
        if pkt_start >= end_sample {
            return PacketWindowPos::Past;
        }
    } else if let (Some(pkt_start), Some(pkt_end)) = (
        timestamp_to_std_duration(pts, tb),
        timestamp_to_std_duration(pts.saturating_add(dur), tb),
    ) {
        if pkt_end <= window.start {
            return PacketWindowPos::Before;
        }
        if pkt_start >= window.end {
            return PacketWindowPos::Past;
        }
    }
    PacketWindowPos::Within
}

pub(super) enum DecodeOutcome<'a> {
    Decoded(GenericAudioBufferRef<'a>),
    /// Corrupt packet skipped; caller should `continue` the packet loop.
    Skipped,
    /// Unexpected EOF during decode; caller should set `allow_tail_padding` and `break`.
    Eof,
    /// `ResetRequired` from the decoder; caller should reset and `continue`.
    NeedReset,
}

/// Decode one packet, classifying non-fatal errors so the driver loop can act uniformly.
///
/// Updates `decode_error_skips` and `consecutive_decode_errors`; returns `Err` when consecutive
/// errors exceed `MAX_CONSECUTIVE_DECODE_ERRORS`. `ResetRequired` is signalled as `NeedReset`
/// so the caller can reborrow `state` for the reset after this function returns.
fn decode_packet_or_skip<'dec>(
    state: &'dec mut MediaIoState,
    packet: &Packet,
    path: &Path,
    track_id: u32,
    track_index: u32,
    decode_error_skips: &mut u32,
    consecutive_decode_errors: &mut u32,
) -> Result<DecodeOutcome<'dec>, MediaError> {
    match state.decoders.get_mut(&track_id).expect("decoder cached").decoder.decode(packet) {
        Ok(decoded) => {
            *consecutive_decode_errors = 0;
            Ok(DecodeOutcome::Decoded(decoded))
        }
        Err(SymphoniaError::DecodeError(detail)) => {
            *decode_error_skips += 1;
            *consecutive_decode_errors += 1;
            debug!(
                path = %path.display(),
                track = track_index,
                skip_count = *decode_error_skips,
                consecutive = *consecutive_decode_errors,
                detail = %detail,
                "skipped corrupt decode packet"
            );
            if *consecutive_decode_errors >= MAX_CONSECUTIVE_DECODE_ERRORS {
                return Err(fail_media(
                    path,
                    "extract",
                    Some(track_index),
                    decode_failed(
                        track_index,
                        format!(
                            "too many consecutive decode errors ({} packets skipped)",
                            decode_error_skips
                        ),
                    ),
                ));
            }
            Ok(DecodeOutcome::Skipped)
        }
        Err(SymphoniaError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
            Ok(DecodeOutcome::Eof)
        }
        Err(SymphoniaError::ResetRequired) => Ok(DecodeOutcome::NeedReset),
        Err(e) => Err(map_decode_loop_error(path, track_index, e)),
    }
}

// ---------------------------------------------------------------------------
// Shared driver
// ---------------------------------------------------------------------------

/// Shared seek/retry/decode/progress driver. Steps 1–10 of the duplication map are owned here;
/// per-mode differences (buffer, target unit, finalize order) are delegated to `sink`.
pub(super) fn run_extract_decode_loop<S: ExtractSink>(
    params: ExtractLoopParams<'_>,
    sink: &mut S,
    scratch: &mut Vec<f32>,
) -> Result<S::Output, MediaError> {
    let ExtractLoopParams { path, state, track, window, progress, label } = params;

    if window.end <= window.start {
        return Err(fail_media(
            path,
            "extract",
            Some(track.index),
            decode_failed(track.index, "clip window is empty"),
        ));
    }

    let track_id = track.index;
    ensure_track_decoder(path, state, track)?;

    let cached = state.decoders.get(&track_id).expect("decoder cached");
    let time_base = cached.time_base;
    let sample_rate_hint = state
        .format
        .tracks()
        .iter()
        .find(|candidate| candidate.id == track_id)
        .and_then(|media_track| match &media_track.codec_params {
            Some(CodecParameters::Audio(codec_params)) => codec_params
                .sample_rate
                .filter(|rate| *rate > 0)
                .or_else(|| Some(track.sample_rate).filter(|rate| *rate > 0)),
            _ => None,
        })
        .or_else(|| Some(track.sample_rate).filter(|rate| *rate > 0));

    let max_attempts = if window.start.is_zero() { 1 } else { 2 };
    let mut allow_tail_padding = false;
    let mut decode_error_skips = 0_u32;

    for attempt in 0..max_attempts {
        if attempt > 0 {
            debug!(
                path = %path.display(),
                track = track.index,
                window_start_secs = window.start.as_secs_f64(),
                "seek-based extract produced no audio; retrying via sequential scan from start"
            );
            // Reopen state before the sequential-from-zero fallback: the reader that just
            // produced no audio may be in a confused state and shouldn't be reused.
            *state = MediaIoState::open(path)?;
            ensure_track_decoder(path, state, track)?;
        }

        let seek_start = if attempt == 0 { window.start } else { Duration::ZERO };
        if attempt == 0 {
            seek_with_recovery(path, state, track, seek_start)?;
        } else {
            seek_to_window_start(path, state.format.as_mut(), track_id, seek_start, track.duration)?;
        }
        state.decoders.get_mut(&track_id).expect("decoder cached").decoder.reset();

        sink.reset_attempt(
            sample_rate_hint,
            window,
            path,
            track.index,
            seek_start.as_secs_f64(),
        );

        let mut last_reported = 0_u64;
        let mut finished = false;
        allow_tail_padding = false;
        decode_error_skips = 0_u32;
        let mut consecutive_decode_errors = 0_u32;

        loop {
            if finished {
                allow_tail_padding = true;
                break;
            }
            if sink.target_reached() {
                break;
            }

            let packet = match read_next_packet(state, path, track_id, track.index)? {
                PacketRead::Got(pkt) => pkt,
                PacketRead::Eof => {
                    allow_tail_padding = true;
                    break;
                }
            };

            if packet.track_id != track_id {
                continue;
            }

            match packet_window_pos(
                packet.pts,
                packet.dur,
                time_base,
                sink.resolved_rate(),
                window,
            ) {
                PacketWindowPos::Before => continue,
                PacketWindowPos::Past => {
                    allow_tail_padding = true;
                    break;
                }
                PacketWindowPos::Within => {}
            }

            let decoded = match decode_packet_or_skip(
                state,
                &packet,
                path,
                track_id,
                track.index,
                &mut decode_error_skips,
                &mut consecutive_decode_errors,
            )? {
                DecodeOutcome::Decoded(buf) => buf,
                DecodeOutcome::Skipped => continue,
                DecodeOutcome::Eof => {
                    allow_tail_padding = true;
                    break;
                }
                DecodeOutcome::NeedReset => {
                    state.decoders.get_mut(&track_id).expect("decoder cached").decoder.reset();
                    continue;
                }
            };

            if decoded.frames() == 0 {
                continue;
            }

            if !sink.metadata_ready() {
                let rate = decoded.spec().rate();
                let channels = decoded.spec().channels().count();
                sink.on_first_decode(rate, channels, window, path, track.index)?;
            }

            let rate = sink.resolved_rate().unwrap_or(0);
            let (start_sample, _) = window_sample_bounds(window, rate);
            let packet_start_unit = time_base
                .map(|tb| timestamp_to_sample(packet.pts, tb, rate))
                .unwrap_or(start_sample);
            let trim_start_frames = time_base
                .map(|tb| media_duration_to_frames(packet.trim_start, tb, rate))
                .unwrap_or(0);

            finished =
                sink.append_packet(decoded, packet_start_unit, trim_start_frames, window, scratch);

            let collected = sink.collected_units();
            let target = sink.target_units().unwrap_or(0);
            if collected.saturating_sub(last_reported as usize) >= rate as usize / 2 {
                progress.progress(label, collected.min(target) as u64, target as u64);
                last_reported = collected.min(target) as u64;
            }
        }

        if sink.has_output() {
            break;
        }
    }

    sink.finalize(
        &ExtractFinalizeContext { path, track, window, progress, label },
        allow_tail_padding,
        decode_error_skips,
    )
}

// ---------------------------------------------------------------------------
// Mono sink
// ---------------------------------------------------------------------------

pub(super) struct MonoExtractSink {
    mono_samples: Vec<i16>,
    resolved_rate: Option<u32>,
    target_samples: Option<usize>,
}

impl MonoExtractSink {
    pub(super) fn new() -> Self {
        Self { mono_samples: Vec::new(), resolved_rate: None, target_samples: None }
    }
}

impl ExtractSink for MonoExtractSink {
    type Output = MonoPcmClip;

    fn reset_attempt(
        &mut self,
        sample_rate_hint: Option<u32>,
        window: &ClipWindow,
        path: &Path,
        track_index: u32,
        seek_start_secs: f64,
    ) {
        self.resolved_rate = sample_rate_hint;
        self.target_samples = self.resolved_rate.map(|rate| {
            let (start, end) = window_sample_bounds(window, rate);
            end.saturating_sub(start) as usize
        });
        self.mono_samples.clear();
        if let Some(rate) = self.resolved_rate {
            self.mono_samples.reserve(window.sample_count_at(rate));
        }
        if let (Some(rate), Some(expected)) = (self.resolved_rate, self.target_samples) {
            debug!(
                path = %path.display(),
                track = track_index,
                window_start_secs = window.start.as_secs_f64(),
                window_end_secs = window.end.as_secs_f64(),
                expected_samples = expected,
                sample_rate = rate,
                seek_start_secs,
                "extracting mono clip"
            );
        }
    }

    fn on_first_decode(
        &mut self,
        rate: u32,
        _channels: usize,
        window: &ClipWindow,
        path: &Path,
        track_index: u32,
    ) -> Result<(), MediaError> {
        if rate == 0 {
            return Err(fail_media(
                path,
                "extract",
                Some(track_index),
                decode_failed(track_index, "missing sample rate"),
            ));
        }
        self.resolved_rate = Some(rate);
        let (start_sample, end_sample) = window_sample_bounds(window, rate);
        let expected = end_sample.saturating_sub(start_sample) as usize;
        if expected == 0 {
            return Err(fail_media(
                path,
                "extract",
                Some(track_index),
                decode_failed(track_index, "clip window is too short to decode"),
            ));
        }
        self.target_samples = Some(expected);
        self.mono_samples.reserve(expected);
        debug!(
            path = %path.display(),
            track = track_index,
            window_start_secs = window.start.as_secs_f64(),
            window_end_secs = window.end.as_secs_f64(),
            expected_samples = expected,
            sample_rate = rate,
            "extracting mono clip"
        );
        Ok(())
    }

    fn metadata_ready(&self) -> bool {
        self.resolved_rate.is_some()
    }

    fn resolved_rate(&self) -> Option<u32> {
        self.resolved_rate
    }

    fn target_reached(&self) -> bool {
        self.target_samples.is_some_and(|target| self.mono_samples.len() >= target)
    }

    fn append_packet(
        &mut self,
        decoded: GenericAudioBufferRef<'_>,
        packet_start_unit: u64,
        trim_start_frames: u32,
        window: &ClipWindow,
        scratch: &mut Vec<f32>,
    ) -> bool {
        let rate = self.resolved_rate.unwrap_or(0);
        let target = self.target_samples.unwrap_or(0);
        let (start_sample, end_sample) = window_sample_bounds(window, rate);
        append_frames_in_window(
            decoded,
            &mut WindowCollectContext {
                packet_start_sample: packet_start_unit,
                window_start_sample: start_sample,
                window_end_sample: end_sample,
                trim_start_frames,
                mono_samples: &mut self.mono_samples,
                target_samples: target,
            },
            scratch,
        )
    }

    fn collected_units(&self) -> usize {
        self.mono_samples.len()
    }

    fn target_units(&self) -> Option<usize> {
        self.target_samples
    }

    fn has_output(&self) -> bool {
        !self.mono_samples.is_empty()
    }

    /// Mono finalize order: truncate first, then capture decoded count (after truncate).
    fn finalize(
        &mut self,
        ctx: &ExtractFinalizeContext<'_>,
        allow_tail_padding: bool,
        decode_error_skips: u32,
    ) -> Result<MonoPcmClip, MediaError> {
        let ExtractFinalizeContext { path, track, window, progress, label } = ctx;
        let rate = self.resolved_rate.ok_or_else(|| {
            fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(track.index, "missing sample rate"),
            )
        })?;
        let target = self.target_samples.unwrap_or(0);

        self.mono_samples.truncate(target);
        progress.progress(label, self.mono_samples.len() as u64, target as u64);
        let decoded_sample_count = self.mono_samples.len();

        if self.mono_samples.is_empty() {
            return Err(fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(
                    track.index,
                    format!(
                        "no audio decoded for window [{:.3}s–{:.3}s)",
                        window.start.as_secs_f64(),
                        window.end.as_secs_f64()
                    ),
                ),
            ));
        }

        if self.mono_samples.len() < target {
            let shortfall = target - self.mono_samples.len();
            let limit = decode_shortfall_limit(rate, target, allow_tail_padding);
            let window_end_secs = window.end.as_secs_f64();
            let track_duration_secs = track.duration.map(|d| d.as_secs_f64());
            if shortfall > limit
                && !tail_partial_clip_acceptable(
                    self.mono_samples.len(),
                    target,
                    allow_tail_padding,
                    window_end_secs,
                    track_duration_secs,
                )
            {
                let error = decode_failed(
                    track.index,
                    format!(
                        "partial clip decoded: got {} of {} samples for window [{:.3}s–{:.3}s)",
                        self.mono_samples.len(),
                        target,
                        window.start.as_secs_f64(),
                        window_end_secs
                    ),
                );
                return Err(if allow_tail_padding {
                    warn_partial_decode(
                        path,
                        "extract",
                        Some(track.index),
                        error,
                        window_end_secs,
                        track_duration_secs,
                    )
                } else {
                    fail_media(path, "extract", Some(track.index), error)
                });
            }

            if shortfall > limit {
                warn!(
                    path = %path.display(),
                    track = track.index,
                    shortfall,
                    target,
                    decoded = self.mono_samples.len(),
                    window_end_secs,
                    "near-track-end partial clip; padding remainder with silence"
                );
            } else {
                debug!(
                    path = %path.display(),
                    track = track.index,
                    shortfall,
                    target,
                    allow_tail_padding,
                    limit,
                    "padding end-of-window decode gap with silence"
                );
            }
            self.mono_samples.resize(target, 0);
        }

        if decode_error_skips > 0 {
            warn!(
                path = %path.display(),
                track = track.index,
                decode_error_skips,
                decoded_samples = self.mono_samples.len(),
                target_samples = target,
                "extract completed after skipping corrupt decode packets"
            );
        }

        log_media_success(path, "extract");
        debug!(
            path = %path.display(),
            track = track.index,
            sample_rate = rate,
            samples = self.mono_samples.len(),
            "extracted mono clip"
        );

        Ok(MonoPcmClip {
            sample_rate: rate,
            samples: std::mem::take(&mut self.mono_samples),
            decode_error_skips,
            decoded_sample_count: (decoded_sample_count < target).then_some(decoded_sample_count),
        })
    }
}

// ---------------------------------------------------------------------------
// Interleaved sink
// ---------------------------------------------------------------------------

pub(super) struct InterleavedExtractSink {
    out: Vec<i16>,
    channels_hint: Option<usize>,
    channels: Option<usize>,
    resolved_rate: Option<u32>,
    target_frames: Option<usize>,
}

impl InterleavedExtractSink {
    pub(super) fn new(track: &AudioTrack) -> Self {
        let channels_hint = (track.channels as usize > 0).then_some(track.channels as usize);
        Self {
            out: Vec::new(),
            channels_hint,
            channels: None,
            resolved_rate: None,
            target_frames: None,
        }
    }
}

impl ExtractSink for InterleavedExtractSink {
    type Output = MultiChannelPcm;

    fn reset_attempt(
        &mut self,
        sample_rate_hint: Option<u32>,
        window: &ClipWindow,
        path: &Path,
        track_index: u32,
        _seek_start_secs: f64,
    ) {
        self.resolved_rate = sample_rate_hint;
        self.target_frames = self.resolved_rate.map(|rate| {
            let (start, end) = window_sample_bounds(window, rate);
            end.saturating_sub(start) as usize
        });
        self.channels = self.channels_hint;
        self.out.clear();
        if let (Some(tf), Some(ch)) = (self.target_frames, self.channels) {
            self.out.reserve(tf.saturating_mul(ch));
            debug!(
                path = %path.display(),
                track = track_index,
                window_start_secs = window.start.as_secs_f64(),
                window_end_secs = window.end.as_secs_f64(),
                expected_frames = tf,
                sample_rate = self.resolved_rate.unwrap_or(0),
                channels = ch,
                "extracting interleaved clip"
            );
        }
    }

    fn on_first_decode(
        &mut self,
        rate: u32,
        channels: usize,
        window: &ClipWindow,
        path: &Path,
        track_index: u32,
    ) -> Result<(), MediaError> {
        if self.resolved_rate.is_none() {
            if rate == 0 {
                return Err(fail_media(
                    path,
                    "extract",
                    Some(track_index),
                    decode_failed(track_index, "missing sample rate"),
                ));
            }
            self.resolved_rate = Some(rate);
            let (start_sample, end_sample) = window_sample_bounds(window, rate);
            let expected = end_sample.saturating_sub(start_sample) as usize;
            if expected == 0 {
                return Err(fail_media(
                    path,
                    "extract",
                    Some(track_index),
                    decode_failed(track_index, "clip window is too short to decode"),
                ));
            }
            self.target_frames = Some(expected);
        }

        if self.channels.is_none() {
            let ch = channels.max(1);
            self.channels = Some(ch);
            self.out.reserve(self.target_frames.unwrap_or(0).saturating_mul(ch));
            debug!(
                path = %path.display(),
                track = track_index,
                window_start_secs = window.start.as_secs_f64(),
                window_end_secs = window.end.as_secs_f64(),
                expected_frames = self.target_frames.unwrap_or(0),
                sample_rate = self.resolved_rate.unwrap_or(0),
                channels = ch,
                "extracting interleaved clip"
            );
        }

        Ok(())
    }

    fn metadata_ready(&self) -> bool {
        self.resolved_rate.is_some() && self.channels.is_some()
    }

    fn resolved_rate(&self) -> Option<u32> {
        self.resolved_rate
    }

    fn target_reached(&self) -> bool {
        match (self.target_frames, self.channels) {
            (Some(target), Some(ch)) => self.out.len() >= target * ch,
            _ => false,
        }
    }

    fn append_packet(
        &mut self,
        decoded: GenericAudioBufferRef<'_>,
        packet_start_unit: u64,
        trim_start_frames: u32,
        window: &ClipWindow,
        scratch: &mut Vec<f32>,
    ) -> bool {
        let rate = self.resolved_rate.unwrap_or(0);
        let ch = self.channels.unwrap_or(1);
        let target = self.target_frames.unwrap_or(0);
        let (start_sample, end_sample) = window_sample_bounds(window, rate);
        append_interleaved_frames_in_window(
            decoded,
            &mut InterleavedCollectContext {
                packet_start_frame: packet_start_unit,
                window_start_frame: start_sample,
                window_end_frame: end_sample,
                trim_start_frames,
                out: &mut self.out,
                channels: ch,
                target_frames: target,
            },
            scratch,
        )
    }

    fn collected_units(&self) -> usize {
        self.out.len() / self.channels.unwrap_or(1)
    }

    fn target_units(&self) -> Option<usize> {
        self.target_frames
    }

    fn has_output(&self) -> bool {
        !self.out.is_empty()
    }

    /// Interleaved finalize order: capture decoded count BEFORE truncate, then truncate.
    fn finalize(
        &mut self,
        ctx: &ExtractFinalizeContext<'_>,
        allow_tail_padding: bool,
        decode_error_skips: u32,
    ) -> Result<MultiChannelPcm, MediaError> {
        let ExtractFinalizeContext { path, track, window, progress, label } = ctx;
        let rate = self.resolved_rate.ok_or_else(|| {
            fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(track.index, "missing sample rate"),
            )
        })?;
        let ch = self.channels.unwrap_or(1);
        let target = self.target_frames.unwrap_or(0);

        let decoded_frame_count = self.out.len() / ch;
        self.out.truncate(target.saturating_mul(ch));
        progress.progress(label, decoded_frame_count.min(target) as u64, target as u64);

        if self.out.is_empty() {
            return Err(fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(
                    track.index,
                    format!(
                        "no audio decoded for window [{:.3}s–{:.3}s)",
                        window.start.as_secs_f64(),
                        window.end.as_secs_f64()
                    ),
                ),
            ));
        }

        if decoded_frame_count < target {
            let shortfall = target - decoded_frame_count;
            let limit = decode_shortfall_limit(rate, target, allow_tail_padding);
            let window_end_secs = window.end.as_secs_f64();
            let track_duration_secs = track.duration.map(|d| d.as_secs_f64());
            if shortfall > limit
                && !tail_partial_clip_acceptable(
                    decoded_frame_count,
                    target,
                    allow_tail_padding,
                    window_end_secs,
                    track_duration_secs,
                )
            {
                let error = decode_failed(
                    track.index,
                    format!(
                        "partial clip decoded: got {} of {} frames for window [{:.3}s–{:.3}s)",
                        decoded_frame_count,
                        target,
                        window.start.as_secs_f64(),
                        window_end_secs
                    ),
                );
                return Err(if allow_tail_padding {
                    warn_partial_decode(
                        path,
                        "extract",
                        Some(track.index),
                        error,
                        window_end_secs,
                        track_duration_secs,
                    )
                } else {
                    fail_media(path, "extract", Some(track.index), error)
                });
            }

            if shortfall > limit {
                warn!(
                    path = %path.display(),
                    track = track.index,
                    shortfall,
                    target,
                    decoded = decoded_frame_count,
                    window_end_secs,
                    "near-track-end partial clip; padding remainder with silence"
                );
            } else {
                debug!(
                    path = %path.display(),
                    track = track.index,
                    shortfall,
                    target,
                    allow_tail_padding,
                    limit,
                    "padding end-of-window interleaved decode gap with silence"
                );
            }
            self.out.resize(target.saturating_mul(ch), 0);
        }

        if decode_error_skips > 0 {
            warn!(
                path = %path.display(),
                track = track.index,
                decode_error_skips,
                decoded_frames = decoded_frame_count,
                target_frames = target,
                "interleaved extract completed after skipping corrupt decode packets"
            );
        }

        log_media_success(path, "extract");
        debug!(
            path = %path.display(),
            track = track.index,
            sample_rate = rate,
            channels = ch,
            frames = self.out.len() / ch,
            "extracted interleaved clip"
        );

        Ok(MultiChannelPcm {
            sample_rate: rate,
            channels: ch as u16,
            samples: std::mem::take(&mut self.out),
            decode_error_skips,
            decoded_frame_count: (decoded_frame_count < target).then_some(decoded_frame_count),
        })
    }
}
