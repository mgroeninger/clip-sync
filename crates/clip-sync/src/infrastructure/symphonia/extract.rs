use std::io;
use std::path::Path;
use std::time::Duration;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{SeekMode, SeekTo};
use symphonia::core::units::{Duration as MediaDuration, Time, TimeBase, Timestamp};
use tracing::{debug, warn};

use crate::application::error::MediaError;
use crate::application::ports::ProgressReporter;
use crate::domain::{AudioTrack, ClipWindow, MonoPcmClip, MultiChannelPcm};
use crate::infrastructure::symphonia::duration::symphonia_time_to_std;
use crate::infrastructure::symphonia::error_mapping::{
    decode_failed, fail_media, log_media_success, map_decode_loop_error, map_seek_error,
};
use crate::infrastructure::symphonia::session::{ensure_track_decoder, MediaIoState};

/// Fail extract after this many consecutive packet decode errors.
pub(crate) const MAX_CONSECUTIVE_DECODE_ERRORS: u32 = 64;

pub(crate) fn extract_mono_with_state(
    path: &Path,
    state: &mut MediaIoState,
    track: &AudioTrack,
    window: &ClipWindow,
    progress: &dyn ProgressReporter,
    label: &str,
) -> Result<MonoPcmClip, MediaError> {
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
    let sample_rate = state
        .format
        .tracks()
        .iter()
        .find(|candidate| candidate.id == track_id)
        .and_then(|media_track| match &media_track.codec_params {
            Some(CodecParameters::Audio(params)) => params
                .sample_rate
                .filter(|rate| *rate > 0)
                .or_else(|| Some(track.sample_rate).filter(|rate| *rate > 0)),
            _ => None,
        })
        .or_else(|| Some(track.sample_rate).filter(|rate| *rate > 0));

    let max_attempts = if window.start.is_zero() { 1 } else { 2 };
    let mut scratch = Vec::<f32>::new();
    let mut mono_samples = Vec::new();
    let mut resolved_rate = None::<u32>;
    let mut target_samples = None::<usize>;
    let mut decode_error_skips = 0_u32;
    let mut allow_tail_padding = false;

    for attempt in 0..max_attempts {
        if attempt > 0 {
            debug!(
                path = %path.display(),
                track = track.index,
                window_start_secs = window.start.as_secs_f64(),
                "seek-based extract produced no audio; retrying via sequential scan from start"
            );
        }

        let seek_start = if attempt == 0 {
            window.start
        } else {
            Duration::ZERO
        };
        seek_to_window_start(path, state.format.as_mut(), track_id, seek_start)?;
        state
            .decoders
            .get_mut(&track_id)
            .expect("decoder cached")
            .decoder
            .reset();

        resolved_rate = sample_rate;
        target_samples = resolved_rate.map(|rate| {
            let (start, end) = window_sample_bounds(window, rate);
            end.saturating_sub(start) as usize
        });
        mono_samples.clear();
        if let Some(rate) = resolved_rate {
            mono_samples.reserve(window.sample_count_at(rate));
        }
        if let (Some(rate), Some(expected)) = (resolved_rate, target_samples) {
            debug!(
                path = %path.display(),
                track = track.index,
                window_start_secs = window.start.as_secs_f64(),
                window_end_secs = window.end.as_secs_f64(),
                expected_samples = expected,
                sample_rate = rate,
                seek_start_secs = seek_start.as_secs_f64(),
                "extracting mono clip"
            );
        }

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
            if let Some(target) = target_samples {
                if mono_samples.len() >= target {
                    break;
                }
            }

            let packet = match state.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    allow_tail_padding = true;
                    break;
                }
                Err(SymphoniaError::ResetRequired) => {
                    state
                        .decoders
                        .get_mut(&track_id)
                        .expect("decoder cached")
                        .decoder
                        .reset();
                    continue;
                }
                Err(error) => {
                    return Err(map_decode_loop_error(path, track.index, error));
                }
            };

            if packet.track_id != track_id {
                continue;
            }

            if let Some(time_base) = time_base {
                if let Some(rate) = resolved_rate {
                    let (start_sample, end_sample) = window_sample_bounds(window, rate);
                    let packet_start_sample = timestamp_to_sample(packet.pts, time_base, rate);
                    let packet_end_sample = timestamp_to_sample(
                        packet.pts.saturating_add(packet.dur),
                        time_base,
                        rate,
                    );
                    if packet_end_sample <= start_sample {
                        continue;
                    }
                    if packet_start_sample >= end_sample {
                        allow_tail_padding = true;
                        break;
                    }
                } else if let (Some(packet_start), Some(packet_end)) = (
                    timestamp_to_std_duration(packet.pts, time_base),
                    timestamp_to_std_duration(packet.pts.saturating_add(packet.dur), time_base),
                ) {
                    if packet_end <= window.start {
                        continue;
                    }
                    if packet_start >= window.end {
                        allow_tail_padding = true;
                        break;
                    }
                }
            }

            let decoded = match state
                .decoders
                .get_mut(&track_id)
                .expect("decoder cached")
                .decoder
                .decode(&packet)
            {
                Ok(decoded) => {
                    consecutive_decode_errors = 0;
                    decoded
                }
                Err(SymphoniaError::DecodeError(detail)) => {
                    decode_error_skips += 1;
                    consecutive_decode_errors += 1;
                    debug!(
                        path = %path.display(),
                        track = track.index,
                        skip_count = decode_error_skips,
                        consecutive = consecutive_decode_errors,
                        detail = %detail,
                        "skipped corrupt decode packet"
                    );
                    if consecutive_decode_errors >= MAX_CONSECUTIVE_DECODE_ERRORS {
                        return Err(fail_media(
                            path,
                            "extract",
                            Some(track.index),
                            decode_failed(
                                track.index,
                                format!(
                                    "too many consecutive decode errors ({decode_error_skips} packets skipped)"
                                ),
                            ),
                        ));
                    }
                    continue;
                }
                Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                    allow_tail_padding = true;
                    break;
                }
                Err(SymphoniaError::ResetRequired) => {
                    state
                        .decoders
                        .get_mut(&track_id)
                        .expect("decoder cached")
                        .decoder
                        .reset();
                    continue;
                }
                Err(error) => {
                    return Err(map_decode_loop_error(path, track.index, error));
                }
            };

            if decoded.frames() == 0 {
                continue;
            }

            if resolved_rate.is_none() {
                resolved_rate = Some(decoded.spec().rate());
                let rate = resolved_rate.unwrap_or(0);
                if rate == 0 {
                    return Err(fail_media(
                        path,
                        "extract",
                        Some(track.index),
                        decode_failed(track.index, "missing sample rate"),
                    ));
                }
                let (start_sample, end_sample) = window_sample_bounds(window, rate);
                let expected = end_sample.saturating_sub(start_sample) as usize;
                if expected == 0 {
                    return Err(fail_media(
                        path,
                        "extract",
                        Some(track.index),
                        decode_failed(track.index, "clip window is too short to decode"),
                    ));
                }
                target_samples = Some(expected);
                mono_samples.reserve(expected);
                debug!(
                    path = %path.display(),
                    track = track.index,
                    window_start_secs = window.start.as_secs_f64(),
                    window_end_secs = window.end.as_secs_f64(),
                    expected_samples = expected,
                    sample_rate = rate,
                    "extracting mono clip"
                );
            }

            let rate = resolved_rate.unwrap_or(0);
            let target = target_samples.unwrap_or(0);
            let (start_sample, end_sample) = window_sample_bounds(window, rate);
            let packet_start_sample = time_base
                .map(|base| timestamp_to_sample(packet.pts, base, rate))
                .unwrap_or(start_sample);
            let trim_start_frames = time_base
                .map(|base| media_duration_to_frames(packet.trim_start, base, rate))
                .unwrap_or(0);

            finished = append_frames_in_window(
                decoded,
                &mut WindowCollectContext {
                    packet_start_sample,
                    window_start_sample: start_sample,
                    window_end_sample: end_sample,
                    trim_start_frames,
                    mono_samples: &mut mono_samples,
                    target_samples: target,
                },
                &mut scratch,
            );

            if mono_samples.len().saturating_sub(last_reported as usize) >= rate as usize / 2 {
                progress.progress(
                    label,
                    mono_samples.len().min(target) as u64,
                    target as u64,
                );
                last_reported = mono_samples.len().min(target) as u64;
            }
        }

        if !mono_samples.is_empty() {
            break;
        }
    }

    let rate = resolved_rate.ok_or_else(|| {
        fail_media(
            path,
            "extract",
            Some(track.index),
            decode_failed(track.index, "missing sample rate"),
        )
    })?;
    let target = target_samples.unwrap_or(0);

    mono_samples.truncate(target);
    progress.progress(label, mono_samples.len() as u64, target as u64);
    let decoded_sample_count = mono_samples.len();

    if mono_samples.is_empty() {
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

    if mono_samples.len() < target {
        let shortfall = target - mono_samples.len();
        let limit = decode_shortfall_limit(rate, target, allow_tail_padding);
        if shortfall > limit {
            return Err(fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(
                    track.index,
                    format!(
                        "partial clip decoded: got {} of {} samples for window [{:.3}s–{:.3}s)",
                        mono_samples.len(),
                        target,
                        window.start.as_secs_f64(),
                        window.end.as_secs_f64()
                    ),
                ),
            ));
        }

        debug!(
            path = %path.display(),
            track = track.index,
            shortfall,
            target,
            allow_tail_padding,
            limit,
            "padding end-of-window decode gap with silence"
        );
        mono_samples.resize(target, 0);
    }

    if decode_error_skips > 0 {
        warn!(
            path = %path.display(),
            track = track.index,
            decode_error_skips,
            decoded_samples = mono_samples.len(),
            target_samples = target,
            "extract completed after skipping corrupt decode packets"
        );
    }

    log_media_success(path, "extract");
    debug!(
        path = %path.display(),
        track = track.index,
        sample_rate = rate,
        samples = mono_samples.len(),
        "extracted mono clip"
    );

    Ok(MonoPcmClip {
        sample_rate: rate,
        samples: mono_samples,
        decode_error_skips,
        decoded_sample_count: (decoded_sample_count < target).then_some(decoded_sample_count),
    })
}

/// Native-rate, all-channels counterpart to [`extract_mono_with_state`].
///
/// Tracking is in **frames** (one frame = one sample per channel); the interleaved output buffer
/// holds `frames * channels` samples. Seek/retry, decode-skip, and tail-padding logic mirror the
/// mono path so both extracts behave identically at window edges.
pub(crate) fn extract_interleaved_with_state(
    path: &Path,
    state: &mut MediaIoState,
    track: &AudioTrack,
    window: &ClipWindow,
    progress: &dyn ProgressReporter,
    label: &str,
) -> Result<MultiChannelPcm, MediaError> {
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
    let sample_rate = state
        .format
        .tracks()
        .iter()
        .find(|candidate| candidate.id == track_id)
        .and_then(|media_track| match &media_track.codec_params {
            Some(CodecParameters::Audio(params)) => params
                .sample_rate
                .filter(|rate| *rate > 0)
                .or_else(|| Some(track.sample_rate).filter(|rate| *rate > 0)),
            _ => None,
        })
        .or_else(|| Some(track.sample_rate).filter(|rate| *rate > 0));

    let channels_hint = (track.channels as usize > 0).then_some(track.channels as usize);
    let max_attempts = if window.start.is_zero() { 1 } else { 2 };
    let mut scratch = Vec::<f32>::new();
    let mut out: Vec<i16> = Vec::new();
    let mut resolved_rate = None::<u32>;
    let mut channels = channels_hint;
    let mut target_frames = None::<usize>;
    let mut decode_error_skips = 0_u32;
    let mut allow_tail_padding = false;

    for attempt in 0..max_attempts {
        if attempt > 0 {
            debug!(
                path = %path.display(),
                track = track.index,
                window_start_secs = window.start.as_secs_f64(),
                "seek-based interleaved extract produced no audio; retrying via sequential scan from start"
            );
        }

        let seek_start = if attempt == 0 {
            window.start
        } else {
            Duration::ZERO
        };
        seek_to_window_start(path, state.format.as_mut(), track_id, seek_start)?;
        state
            .decoders
            .get_mut(&track_id)
            .expect("decoder cached")
            .decoder
            .reset();

        resolved_rate = sample_rate;
        target_frames = resolved_rate.map(|rate| {
            let (start, end) = window_sample_bounds(window, rate);
            end.saturating_sub(start) as usize
        });
        channels = channels_hint;
        out.clear();
        if let (Some(tf), Some(ch)) = (target_frames, channels) {
            out.reserve(tf.saturating_mul(ch));
            debug!(
                path = %path.display(),
                track = track.index,
                window_start_secs = window.start.as_secs_f64(),
                window_end_secs = window.end.as_secs_f64(),
                expected_frames = tf,
                sample_rate = resolved_rate.unwrap_or(0),
                channels = ch,
                "extracting interleaved clip"
            );
        }

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
            if let (Some(target), Some(ch)) = (target_frames, channels) {
                if out.len() >= target * ch {
                    break;
                }
            }

            let packet = match state.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    allow_tail_padding = true;
                    break;
                }
                Err(SymphoniaError::ResetRequired) => {
                    state
                        .decoders
                        .get_mut(&track_id)
                        .expect("decoder cached")
                        .decoder
                        .reset();
                    continue;
                }
                Err(error) => {
                    return Err(map_decode_loop_error(path, track.index, error));
                }
            };

            if packet.track_id != track_id {
                continue;
            }

            if let Some(time_base) = time_base {
                if let Some(rate) = resolved_rate {
                    let (start_sample, end_sample) = window_sample_bounds(window, rate);
                    let packet_start_sample = timestamp_to_sample(packet.pts, time_base, rate);
                    let packet_end_sample = timestamp_to_sample(
                        packet.pts.saturating_add(packet.dur),
                        time_base,
                        rate,
                    );
                    if packet_end_sample <= start_sample {
                        continue;
                    }
                    if packet_start_sample >= end_sample {
                        allow_tail_padding = true;
                        break;
                    }
                } else if let (Some(packet_start), Some(packet_end)) = (
                    timestamp_to_std_duration(packet.pts, time_base),
                    timestamp_to_std_duration(packet.pts.saturating_add(packet.dur), time_base),
                ) {
                    if packet_end <= window.start {
                        continue;
                    }
                    if packet_start >= window.end {
                        allow_tail_padding = true;
                        break;
                    }
                }
            }

            let decoded = match state
                .decoders
                .get_mut(&track_id)
                .expect("decoder cached")
                .decoder
                .decode(&packet)
            {
                Ok(decoded) => {
                    consecutive_decode_errors = 0;
                    decoded
                }
                Err(SymphoniaError::DecodeError(detail)) => {
                    decode_error_skips += 1;
                    consecutive_decode_errors += 1;
                    debug!(
                        path = %path.display(),
                        track = track.index,
                        skip_count = decode_error_skips,
                        consecutive = consecutive_decode_errors,
                        detail = %detail,
                        "skipped corrupt decode packet"
                    );
                    if consecutive_decode_errors >= MAX_CONSECUTIVE_DECODE_ERRORS {
                        return Err(fail_media(
                            path,
                            "extract",
                            Some(track.index),
                            decode_failed(
                                track.index,
                                format!(
                                    "too many consecutive decode errors ({decode_error_skips} packets skipped)"
                                ),
                            ),
                        ));
                    }
                    continue;
                }
                Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                    allow_tail_padding = true;
                    break;
                }
                Err(SymphoniaError::ResetRequired) => {
                    state
                        .decoders
                        .get_mut(&track_id)
                        .expect("decoder cached")
                        .decoder
                        .reset();
                    continue;
                }
                Err(error) => {
                    return Err(map_decode_loop_error(path, track.index, error));
                }
            };

            if decoded.frames() == 0 {
                continue;
            }

            if resolved_rate.is_none() {
                let rate = decoded.spec().rate();
                if rate == 0 {
                    return Err(fail_media(
                        path,
                        "extract",
                        Some(track.index),
                        decode_failed(track.index, "missing sample rate"),
                    ));
                }
                resolved_rate = Some(rate);
                let (start_sample, end_sample) = window_sample_bounds(window, rate);
                let expected = end_sample.saturating_sub(start_sample) as usize;
                if expected == 0 {
                    return Err(fail_media(
                        path,
                        "extract",
                        Some(track.index),
                        decode_failed(track.index, "clip window is too short to decode"),
                    ));
                }
                target_frames = Some(expected);
            }

            if channels.is_none() {
                let ch = decoded.spec().channels().count().max(1);
                channels = Some(ch);
                out.reserve(target_frames.unwrap_or(0).saturating_mul(ch));
                debug!(
                    path = %path.display(),
                    track = track.index,
                    window_start_secs = window.start.as_secs_f64(),
                    window_end_secs = window.end.as_secs_f64(),
                    expected_frames = target_frames.unwrap_or(0),
                    sample_rate = resolved_rate.unwrap_or(0),
                    channels = ch,
                    "extracting interleaved clip"
                );
            }

            let rate = resolved_rate.unwrap_or(0);
            let ch = channels.unwrap_or(1);
            let target = target_frames.unwrap_or(0);
            let (start_sample, end_sample) = window_sample_bounds(window, rate);
            let packet_start_sample = time_base
                .map(|base| timestamp_to_sample(packet.pts, base, rate))
                .unwrap_or(start_sample);
            let trim_start_frames = time_base
                .map(|base| media_duration_to_frames(packet.trim_start, base, rate))
                .unwrap_or(0);

            finished = append_interleaved_frames_in_window(
                decoded,
                &mut InterleavedCollectContext {
                    packet_start_frame: packet_start_sample,
                    window_start_frame: start_sample,
                    window_end_frame: end_sample,
                    trim_start_frames,
                    out: &mut out,
                    channels: ch,
                    target_frames: target,
                },
                &mut scratch,
            );

            let frames_collected = out.len() / ch;
            if frames_collected.saturating_sub(last_reported as usize) >= rate as usize / 2 {
                progress.progress(label, frames_collected.min(target) as u64, target as u64);
                last_reported = frames_collected.min(target) as u64;
            }
        }

        if !out.is_empty() {
            break;
        }
    }

    let rate = resolved_rate.ok_or_else(|| {
        fail_media(
            path,
            "extract",
            Some(track.index),
            decode_failed(track.index, "missing sample rate"),
        )
    })?;
    let ch = channels.unwrap_or(1);
    let target = target_frames.unwrap_or(0);

    // Capture the true decoded frame count before truncation: this is what gets stored in
    // MultiChannelPcm.decoded_frame_count to indicate whether silence padding was applied.
    let decoded_frame_count = out.len() / ch;
    out.truncate(target.saturating_mul(ch));
    progress.progress(label, decoded_frame_count.min(target) as u64, target as u64);

    if out.is_empty() {
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
        if shortfall > limit {
            return Err(fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(
                    track.index,
                    format!(
                        "partial clip decoded: got {} of {} frames for window [{:.3}s–{:.3}s)",
                        decoded_frame_count,
                        target,
                        window.start.as_secs_f64(),
                        window.end.as_secs_f64()
                    ),
                ),
            ));
        }

        debug!(
            path = %path.display(),
            track = track.index,
            shortfall,
            target,
            allow_tail_padding,
            limit,
            "padding end-of-window interleaved decode gap with silence"
        );
        out.resize(target.saturating_mul(ch), 0);
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
        frames = out.len() / ch,
        "extracted interleaved clip"
    );

    Ok(MultiChannelPcm {
        sample_rate: rate,
        channels: ch as u16,
        samples: out,
        decode_error_skips,
        decoded_frame_count: (decoded_frame_count < target).then_some(decoded_frame_count),
    })
}

pub(crate) struct WindowCollectContext<'a> {
    pub packet_start_sample: u64,
    pub window_start_sample: u64,
    pub window_end_sample: u64,
    pub trim_start_frames: u32,
    pub mono_samples: &'a mut Vec<i16>,
    pub target_samples: usize,
}

pub(crate) struct InterleavedCollectContext<'a> {
    pub packet_start_frame: u64,
    pub window_start_frame: u64,
    pub window_end_frame: u64,
    pub trim_start_frames: u32,
    pub out: &'a mut Vec<i16>,
    pub channels: usize,
    pub target_frames: usize,
}

/// Appends in-window frames to `ctx.out` as interleaved `i16`, fixed at `ctx.channels` per frame.
/// Packets with more source channels are truncated; fewer are zero-padded. Returns `true` when the
/// target frame count is reached.
///
/// `scratch` is a caller-owned buffer reused across packets to avoid per-call heap allocation;
/// its contents on entry are irrelevant and will be overwritten.
pub(crate) fn append_interleaved_frames_in_window(
    decoded: GenericAudioBufferRef<'_>,
    ctx: &mut InterleavedCollectContext<'_>,
    scratch: &mut Vec<f32>,
) -> bool {
    let frame_count = decoded.frames();
    if frame_count == 0 {
        return false;
    }

    let source_channels = decoded.spec().channels().count().max(1);
    scratch.clear();
    decoded.copy_to_vec_interleaved(scratch);
    let interleaved = &*scratch;

    let target_samples = ctx.target_frames.saturating_mul(ctx.channels);
    let trim_start = ctx.trim_start_frames as usize;
    for frame_idx in trim_start..frame_count {
        if ctx.out.len() >= target_samples {
            return true;
        }

        let frame_index = ctx.packet_start_frame + (frame_idx - trim_start) as u64;
        if frame_index >= ctx.window_end_frame {
            return true;
        }
        if frame_index < ctx.window_start_frame {
            continue;
        }

        let frame_start = frame_idx * source_channels;
        let frame = &interleaved[frame_start..frame_start + source_channels];
        for channel in 0..ctx.channels {
            let sample = frame.get(channel).copied().unwrap_or(0.0);
            ctx.out.push(float_to_i16(sample));
        }
    }

    false
}

/// `scratch` is a caller-owned buffer reused across packets to avoid per-call heap allocation;
/// its contents on entry are irrelevant and will be overwritten.
pub(crate) fn append_frames_in_window(
    decoded: GenericAudioBufferRef<'_>,
    ctx: &mut WindowCollectContext<'_>,
    scratch: &mut Vec<f32>,
) -> bool {
    let frame_count = decoded.frames();
    if frame_count == 0 {
        return false;
    }

    let channel_count = decoded.spec().channels().count().max(1);
    scratch.clear();
    decoded.copy_to_vec_interleaved(scratch);
    let interleaved = &*scratch;

    let trim_start = ctx.trim_start_frames as usize;
    for frame_idx in trim_start..frame_count {
        if ctx.mono_samples.len() >= ctx.target_samples {
            return true;
        }

        let sample_index = ctx.packet_start_sample + (frame_idx - trim_start) as u64;
        if sample_index >= ctx.window_end_sample {
            return true;
        }
        if sample_index < ctx.window_start_sample {
            continue;
        }

        let frame_start = frame_idx * channel_count;
        let frame = &interleaved[frame_start..frame_start + channel_count];
        let mono = if frame.is_empty() {
            0.0
        } else {
            frame.iter().sum::<f32>() / frame.len() as f32
        };
        ctx.mono_samples.push(float_to_i16(mono));
    }

    false
}

fn timestamp_to_std_duration(ts: Timestamp, time_base: TimeBase) -> Option<Duration> {
    time_base.calc_time(ts).map(symphonia_time_to_std)
}

fn timestamp_to_sample(ts: Timestamp, time_base: TimeBase, sample_rate: u32) -> u64 {
    timestamp_to_std_duration(ts, time_base)
        .map(|duration| time_to_sample(duration, sample_rate))
        .unwrap_or(0)
}

fn media_duration_to_frames(dur: MediaDuration, time_base: TimeBase, sample_rate: u32) -> u32 {
    let ts = Timestamp::try_from(dur.get()).unwrap_or(Timestamp::ZERO);
    timestamp_to_sample(ts, time_base, sample_rate).min(u32::MAX as u64) as u32
}

fn time_to_sample(time: Duration, sample_rate: u32) -> u64 {
    (time.as_secs_f64() * f64::from(sample_rate)).floor() as u64
}

pub(crate) fn window_sample_bounds(window: &ClipWindow, sample_rate: u32) -> (u64, u64) {
    (
        time_to_sample(window.start, sample_rate),
        time_to_sample(window.end, sample_rate),
    )
}

fn seek_to_window_start(
    path: &Path,
    format: &mut dyn symphonia::core::formats::FormatReader,
    track_id: u32,
    start: Duration,
) -> Result<(), MediaError> {
    let time = Time::try_new(start.as_secs() as i64, start.subsec_nanos()).ok_or_else(|| {
        MediaError::SeekFailed(format!(
            "{}: seek to {:.3}s on track {track_id} failed: invalid time",
            path.display(),
            start.as_secs_f64()
        ))
    })?;

    let seek_to = SeekTo::Time {
        time,
        track_id: Some(track_id),
    };

    format
        .seek(SeekMode::Accurate, seek_to)
        .map_err(|error| map_seek_error(path, track_id, start.as_secs_f64(), error))?;

    Ok(())
}

pub(crate) fn sample_count_tolerance(sample_rate: u32) -> usize {
    // ~20 ms baseline; allow up to two HE-AAC SBR output frames (2048 samples each) for
    // container duration vs decodable sample boundary mismatch at window edges.
    const HE_AAC_FRAME_SAMPLES: usize = 2048;
    (sample_rate as usize / 50)
        .max(HE_AAC_FRAME_SAMPLES * 2)
        .max(64)
}

pub(crate) fn decode_shortfall_limit(
    sample_rate: u32,
    target_samples: usize,
    allow_tail_padding: bool,
) -> usize {
    let frame = sample_count_tolerance(sample_rate);
    if !allow_tail_padding {
        return frame;
    }

    // Container timestamps often extend past the last decodable sample at window edges.
    const EOF_MAX_SECS: f64 = 2.0;
    let eof_cap = (f64::from(sample_rate) * EOF_MAX_SECS).ceil() as usize;
    let percent_cap = target_samples / 200; // 0.5%
    frame.max(eof_cap.min(percent_cap))
}

pub(crate) fn float_to_i16(sample: f32) -> i16 {
    let scaled = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i32;
    scaled.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}
