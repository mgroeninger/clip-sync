use std::io;
use std::path::Path;
use std::time::Duration;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::{Error as SymphoniaError, SeekErrorKind};
use symphonia::core::formats::{SeekMode, SeekTo};
use symphonia::core::units::{Duration as MediaDuration, Time, TimeBase, Timestamp};

use crate::application::error::MediaError;
use crate::application::ports::ProgressReporter;
use crate::domain::{
    AudioTrack, AudioTimelineSkew, ClipWindow, InterleavedScanBucket, MonoPcmClip, MonoScanBucket,
    MultiChannelPcm,
};
use crate::infrastructure::symphonia::duration::symphonia_time_to_std;
use crate::infrastructure::symphonia::error_mapping::{
    decode_failed, fail_media, log_media_success, map_decode_loop_error, map_seek_error,
    NEAR_TRACK_END_TOLERANCE_SECS,
};
use crate::infrastructure::symphonia::session::{ensure_track_decoder, MediaIoState};
use super::extract_loop;
use tracing::{debug, warn};

/// Tracks the maximum |PTS − sample-clock| observed during a sequential scan.
#[derive(Debug, Default)]
pub(crate) struct TimelineSkewTracker {
    max_skew: Option<AudioTimelineSkew>,
}

impl TimelineSkewTracker {
    pub fn observe_packet_start(
        &mut self,
        packet_pts: Timestamp,
        time_base: Option<TimeBase>,
        sample_rate: u32,
        sample_clock_frame: u64,
    ) {
        let Some(tb) = time_base else {
            return;
        };
        if sample_rate == 0 {
            return;
        }
        let pts_frame = timestamp_to_sample(packet_pts, tb, sample_rate);
        let pts_secs = pts_frame as f64 / f64::from(sample_rate);
        let sample_clock_secs = sample_clock_frame as f64 / f64::from(sample_rate);
        let delta_secs = (pts_secs - sample_clock_secs).abs();
        let update = self
            .max_skew
            .as_ref()
            .map(|current| delta_secs > current.delta_secs)
            .unwrap_or(true);
        if update {
            self.max_skew = Some(AudioTimelineSkew {
                pts_secs,
                sample_clock_secs,
                delta_secs,
            });
        }
    }

    pub fn finish(self) -> Option<AudioTimelineSkew> {
        self.max_skew
    }
}

/// Inputs for [`scan_interleaved_buckets_with_state`].
pub(crate) struct InterleavedBucketScan<'a> {
    pub path: &'a Path,
    pub state: &'a mut MediaIoState,
    pub track: &'a AudioTrack,
    pub bucket_secs: f64,
    pub progress: &'a dyn ProgressReporter,
    pub label: &'a str,
}

/// Fail extract after this many consecutive packet decode errors.
pub(crate) const MAX_CONSECUTIVE_DECODE_ERRORS: u32 = 64;

/// Reopen-and-reseek retries when an interior window decodes most of a clip but falls short
/// (timestamp/sample boundary mismatch after a successful seek).
pub(crate) const MAX_INTERIOR_PARTIAL_DECODE_RETRIES: u32 = 1;

/// Outcome of comparing decoded sample/frame count to the clip window target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortfallDisposition {
    Pad { shortfall: usize },
    HardFail { near_track_end: bool },
}

/// Classify decode shortfall relative to tolerance and tail-clip padding rules.
pub(crate) fn shortfall_disposition(
    decoded: usize,
    target: usize,
    allow_tail_padding: bool,
    window_end_secs: f64,
    track_duration_secs: Option<f64>,
    sample_rate: u32,
) -> ShortfallDisposition {
    debug_assert!(decoded < target);
    let shortfall = target - decoded;
    let limit = decode_shortfall_limit(sample_rate, target, allow_tail_padding);
    if shortfall > limit
        && !tail_partial_clip_acceptable(
            decoded,
            target,
            allow_tail_padding,
            window_end_secs,
            track_duration_secs,
        )
    {
        let near_track_end = track_duration_secs
            .map(|duration| window_end_secs >= duration - NEAR_TRACK_END_TOLERANCE_SECS)
            .unwrap_or(false);
        return ShortfallDisposition::HardFail { near_track_end };
    }
    ShortfallDisposition::Pad { shortfall }
}

/// Minimum fraction of a tail clip that must decode before we pad the remainder (fingerprinting).
const TAIL_CLIP_MIN_DECODE_PERCENT: usize = 95;

pub(super) fn tail_partial_clip_acceptable(
    decoded_count: usize,
    target_count: usize,
    allow_tail_padding: bool,
    window_end_secs: f64,
    track_duration_secs: Option<f64>,
) -> bool {
    if !allow_tail_padding || target_count == 0 {
        return false;
    }
    let Some(duration) = track_duration_secs else {
        return false;
    };
    if window_end_secs < duration - NEAR_TRACK_END_TOLERANCE_SECS {
        return false;
    }
    decoded_count * 100 >= target_count * TAIL_CLIP_MIN_DECODE_PERCENT
}

pub(crate) fn extract_mono_with_state(
    path: &Path,
    state: &mut MediaIoState,
    track: &AudioTrack,
    window: &ClipWindow,
    progress: &dyn ProgressReporter,
    label: &str,
) -> Result<MonoPcmClip, MediaError> {
    let mut sink = extract_loop::MonoExtractSink::new();
    let mut scratch = Vec::new();
    extract_loop::run_extract_decode_loop(
        extract_loop::ExtractLoopParams { path, state, track, window, progress, label },
        &mut sink,
        &mut scratch,
    )
}

/// Decode a track sequentially from the start and emit fixed-size sample buckets.
///
/// Buckets are defined by **decoded sample count** (`bucket_secs * sample_rate`), not packet
/// timestamps, so gap scans avoid per-window seek imprecision on MKV/AAC.
pub(crate) fn scan_mono_buckets_with_state(
    path: &Path,
    state: &mut MediaIoState,
    track: &AudioTrack,
    bucket_secs: f64,
    progress: &dyn ProgressReporter,
    label: &str,
    on_bucket: &mut dyn FnMut(MonoScanBucket) -> Result<(), MediaError>,
) -> Result<(), MediaError> {
    if bucket_secs <= 0.0 {
        return Err(fail_media(
            path,
            "scan",
            Some(track.index),
            decode_failed(track.index, "bucket duration must be positive"),
        ));
    }

    let track_id = track.index;
    ensure_track_decoder(path, state, track)?;

    seek_to_window_start(
        path,
        state.format.as_mut(),
        track_id,
        Duration::ZERO,
        track.duration,
    )?;
    state
        .decoders
        .get_mut(&track_id)
        .expect("decoder cached")
        .decoder
        .reset();

    let time_base = state
        .decoders
        .get(&track_id)
        .expect("decoder cached")
        .time_base;
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

    let mut scratch = Vec::<f32>::new();
    let mut bucket_buf = Vec::<i16>::new();
    let mut resolved_rate = None::<u32>;
    let mut bucket_capacity = None::<usize>;
    let mut bucket_index = 0_u64;
    let mut decode_error_skips = 0_u32;
    let mut consecutive_decode_errors = 0_u32;
    let mut last_reported = 0_u64;

    let estimated_total_samples = track.duration.and_then(|duration| {
        sample_rate.map(|rate| {
            (duration.as_secs_f64() * f64::from(rate)).ceil().max(0.0) as u64
        })
    });

    let emit_full_bucket = |buf: &mut Vec<i16>,
                            capacity: usize,
                            index: &mut u64,
                            rate: u32,
                            skips: u32,
                            on_bucket: &mut dyn FnMut(MonoScanBucket) -> Result<(), MediaError>|
     -> Result<(), MediaError> {
        while buf.len() >= capacity {
            let samples: Vec<i16> = buf.drain(..capacity).collect();
            let start_secs = *index as f64 * bucket_secs;
            let end_secs = start_secs + bucket_secs;
            *index += 1;
            on_bucket(MonoScanBucket {
                start_secs,
                end_secs,
                pcm: MonoPcmClip {
                    sample_rate: rate,
                    samples,
                    decode_error_skips: skips,
                    decoded_sample_count: None,
                },
            })?;
        }
        Ok(())
    };

    loop {
        let packet = match state.format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                state
                    .decoders
                    .get_mut(&track_id)
                    .expect("decoder cached")
                    .decoder
                    .reset();
                continue;
            }
            Err(error) => return Err(map_decode_loop_error(path, track.index, error)),
        };

        if packet.track_id != track_id {
            continue;
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
                    "skipped corrupt decode packet during sequential scan"
                );
                if consecutive_decode_errors >= MAX_CONSECUTIVE_DECODE_ERRORS {
                    return Err(fail_media(
                        path,
                        "scan",
                        Some(track.index),
                        decode_failed(
                            track.index,
                            format!(
                                "too many consecutive decode errors during sequential scan ({decode_error_skips} packets skipped)"
                            ),
                        ),
                    ));
                }
                continue;
            }
            Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
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
            Err(error) => return Err(map_decode_loop_error(path, track.index, error)),
        };

        if decoded.frames() == 0 {
            continue;
        }

        if resolved_rate.is_none() {
            let rate = decoded.spec().rate();
            if rate == 0 {
                return Err(fail_media(
                    path,
                    "scan",
                    Some(track.index),
                    decode_failed(track.index, "missing sample rate during sequential scan"),
                ));
            }
            resolved_rate = Some(rate);
            let capacity = (bucket_secs * f64::from(rate)).round() as usize;
            if capacity == 0 {
                return Err(fail_media(
                    path,
                    "scan",
                    Some(track.index),
                    decode_failed(track.index, "bucket duration too small for sample rate"),
                ));
            }
            bucket_capacity = Some(capacity);
            bucket_buf.reserve(capacity);
            debug!(
                path = %path.display(),
                track = track.index,
                bucket_secs,
                bucket_samples = capacity,
                sample_rate = rate,
                "starting sequential mono bucket scan"
            );
        }

        let rate = resolved_rate.unwrap_or(0);
        let capacity = bucket_capacity.unwrap_or(0);
        let trim_start_frames = time_base
            .map(|base| media_duration_to_frames(packet.trim_start, base, rate))
            .unwrap_or(0);

        append_mono_frames_sequential(decoded, &mut bucket_buf, &mut scratch, trim_start_frames);

        emit_full_bucket(
            &mut bucket_buf,
            capacity,
            &mut bucket_index,
            rate,
            decode_error_skips,
            on_bucket,
        )?;

        if let Some(estimated) = estimated_total_samples {
            if bucket_buf.len().saturating_add(bucket_index as usize * capacity)
                .saturating_sub(last_reported as usize)
                >= rate as usize / 2
            {
                let current = bucket_index
                    .saturating_mul(capacity as u64)
                    .saturating_add(bucket_buf.len() as u64);
                progress.progress(label, current.min(estimated), estimated);
                last_reported = current;
            }
        }
    }

    let rate = resolved_rate.ok_or_else(|| {
        fail_media(
            path,
            "scan",
            Some(track.index),
            decode_failed(track.index, "no audio decoded during sequential scan"),
        )
    })?;
    if !bucket_buf.is_empty() {
        let start_secs = bucket_index as f64 * bucket_secs;
        let end_secs = start_secs + bucket_buf.len() as f64 / f64::from(rate);
        on_bucket(MonoScanBucket {
            start_secs,
            end_secs,
            pcm: MonoPcmClip {
                sample_rate: rate,
                samples: std::mem::take(&mut bucket_buf),
                decode_error_skips,
                decoded_sample_count: None,
            },
        })?;
    }

    if let Some(estimated) = estimated_total_samples {
        progress.progress(label, estimated, estimated);
    }

    if decode_error_skips > 0 {
        warn!(
            path = %path.display(),
            track = track.index,
            decode_error_skips,
            "sequential bucket scan completed after skipping corrupt decode packets"
        );
    }

    log_media_success(path, "scan");
    Ok(())
}

/// Decode a track sequentially from the start and emit fixed-duration interleaved PCM buckets.
///
/// Buckets are defined by **decoded frame count** (`bucket_secs * sample_rate`); each bucket holds
/// `frames * channels` interleaved samples. Used by gap scans for multichannel silence detection.
pub(crate) fn scan_interleaved_buckets_with_state(
    scan: InterleavedBucketScan<'_>,
    on_bucket: &mut dyn FnMut(InterleavedScanBucket) -> Result<(), MediaError>,
    timeline_skew: &mut TimelineSkewTracker,
) -> Result<(), MediaError> {
    let InterleavedBucketScan {
        path,
        state,
        track,
        bucket_secs,
        progress,
        label,
    } = scan;

    if bucket_secs <= 0.0 {
        return Err(fail_media(
            path,
            "scan",
            Some(track.index),
            decode_failed(track.index, "bucket duration must be positive"),
        ));
    }

    let track_id = track.index;
    ensure_track_decoder(path, state, track)?;

    seek_to_window_start(
        path,
        state.format.as_mut(),
        track_id,
        Duration::ZERO,
        track.duration,
    )?;
    state
        .decoders
        .get_mut(&track_id)
        .expect("decoder cached")
        .decoder
        .reset();

    let time_base = state
        .decoders
        .get(&track_id)
        .expect("decoder cached")
        .time_base;
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

    let channels_hint = (track.channels > 0).then_some(track.channels as usize);
    let mut scratch = Vec::<f32>::new();
    let mut bucket_buf = Vec::<i16>::new();
    let mut resolved_rate = None::<u32>;
    let mut channels = channels_hint;
    let mut bucket_frame_capacity = None::<usize>;
    let mut bucket_index = 0_u64;
    let mut decode_error_skips = 0_u32;
    let mut consecutive_decode_errors = 0_u32;
    let mut last_reported = 0_u64;

    let estimated_total_frames = track.duration.and_then(|duration| {
        sample_rate.map(|rate| {
            (duration.as_secs_f64() * f64::from(rate)).ceil().max(0.0) as u64
        })
    });

    let emit_full_bucket = |buf: &mut Vec<i16>,
                            frame_capacity: usize,
                            ch: usize,
                            index: &mut u64,
                            rate: u32,
                            skips: u32,
                            on_bucket: &mut dyn FnMut(InterleavedScanBucket) -> Result<(), MediaError>|
     -> Result<(), MediaError> {
        let sample_capacity = frame_capacity.saturating_mul(ch);
        while buf.len() >= sample_capacity {
            let samples: Vec<i16> = buf.drain(..sample_capacity).collect();
            let start_secs = *index as f64 * bucket_secs;
            let end_secs = start_secs + bucket_secs;
            *index += 1;
            on_bucket(InterleavedScanBucket {
                start_secs,
                end_secs,
                pcm: MultiChannelPcm {
                    sample_rate: rate,
                    channels: ch as u16,
                    samples,
                    decode_error_skips: skips,
                    decoded_frame_count: None,
                    compressed_bytes: None,
                },
            })?;
        }
        Ok(())
    };

    loop {
        let packet = match state.format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                state
                    .decoders
                    .get_mut(&track_id)
                    .expect("decoder cached")
                    .decoder
                    .reset();
                continue;
            }
            Err(error) => return Err(map_decode_loop_error(path, track.index, error)),
        };

        if packet.track_id != track_id {
            continue;
        }

        let rate_for_skew = resolved_rate.or(sample_rate).unwrap_or(0);
        let ch_for_skew = channels.unwrap_or(channels_hint.unwrap_or(1)).max(1);
        let sample_clock_frame = bucket_index
            .saturating_mul(bucket_frame_capacity.unwrap_or(0) as u64)
            .saturating_add((bucket_buf.len() / ch_for_skew) as u64);
        if rate_for_skew > 0 {
            timeline_skew.observe_packet_start(
                packet.pts,
                time_base,
                rate_for_skew,
                sample_clock_frame,
            );
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
                    "skipped corrupt decode packet during sequential interleaved scan"
                );
                if consecutive_decode_errors >= MAX_CONSECUTIVE_DECODE_ERRORS {
                    return Err(fail_media(
                        path,
                        "scan",
                        Some(track.index),
                        decode_failed(
                            track.index,
                            format!(
                                "too many consecutive decode errors during sequential scan ({decode_error_skips} packets skipped)"
                            ),
                        ),
                    ));
                }
                continue;
            }
            Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
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
            Err(error) => return Err(map_decode_loop_error(path, track.index, error)),
        };

        if decoded.frames() == 0 {
            continue;
        }

        if resolved_rate.is_none() {
            let rate = decoded.spec().rate();
            if rate == 0 {
                return Err(fail_media(
                    path,
                    "scan",
                    Some(track.index),
                    decode_failed(track.index, "missing sample rate during sequential scan"),
                ));
            }
            resolved_rate = Some(rate);
            let frame_capacity = (bucket_secs * f64::from(rate)).round() as usize;
            if frame_capacity == 0 {
                return Err(fail_media(
                    path,
                    "scan",
                    Some(track.index),
                    decode_failed(track.index, "bucket duration too small for sample rate"),
                ));
            }
            bucket_frame_capacity = Some(frame_capacity);
        }

        if channels.is_none() {
            let ch = decoded.spec().channels().count().max(1);
            channels = Some(ch);
            if let Some(frame_capacity) = bucket_frame_capacity {
                bucket_buf.reserve(frame_capacity.saturating_mul(ch));
            }
            debug!(
                path = %path.display(),
                track = track.index,
                bucket_secs,
                bucket_frames = bucket_frame_capacity.unwrap_or(0),
                sample_rate = resolved_rate.unwrap_or(0),
                channels = ch,
                "starting sequential interleaved bucket scan"
            );
        }

        let rate = resolved_rate.unwrap_or(0);
        let ch = channels.unwrap_or(1);
        let frame_capacity = bucket_frame_capacity.unwrap_or(0);
        let trim_start_frames = time_base
            .map(|base| media_duration_to_frames(packet.trim_start, base, rate))
            .unwrap_or(0);

        append_interleaved_frames_sequential(
            decoded,
            &mut bucket_buf,
            ch,
            &mut scratch,
            trim_start_frames,
        );

        emit_full_bucket(
            &mut bucket_buf,
            frame_capacity,
            ch,
            &mut bucket_index,
            rate,
            decode_error_skips,
            on_bucket,
        )?;

        if let Some(estimated) = estimated_total_frames {
            let frames_buffered = bucket_buf.len() / ch;
            let current_frames = bucket_index
                .saturating_mul(frame_capacity as u64)
                .saturating_add(frames_buffered as u64);
            if current_frames.saturating_sub(last_reported) >= u64::from(rate) / 2 {
                progress.progress(label, current_frames.min(estimated), estimated);
                last_reported = current_frames.min(estimated);
            }
        }
    }

    let rate = resolved_rate.ok_or_else(|| {
        fail_media(
            path,
            "scan",
            Some(track.index),
            decode_failed(track.index, "no audio decoded during sequential scan"),
        )
    })?;
    let ch = channels.unwrap_or(1);
    if !bucket_buf.is_empty() {
        let frames = bucket_buf.len() / ch;
        let start_secs = bucket_index as f64 * bucket_secs;
        let end_secs = start_secs + frames as f64 / f64::from(rate);
        on_bucket(InterleavedScanBucket {
            start_secs,
            end_secs,
            pcm: MultiChannelPcm {
                sample_rate: rate,
                channels: ch as u16,
                samples: std::mem::take(&mut bucket_buf),
                decode_error_skips,
                decoded_frame_count: None,
                compressed_bytes: None,
            },
        })?;
    }

    if let Some(estimated) = estimated_total_frames {
        progress.progress(label, estimated, estimated);
    }

    if decode_error_skips > 0 {
        warn!(
            path = %path.display(),
            track = track.index,
            decode_error_skips,
            "sequential interleaved bucket scan completed after skipping corrupt decode packets"
        );
    }

    log_media_success(path, "scan");
    Ok(())
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
    let mut sink = extract_loop::InterleavedExtractSink::new(track);
    let mut scratch = Vec::new();
    extract_loop::run_extract_decode_loop(
        extract_loop::ExtractLoopParams { path, state, track, window, progress, label },
        &mut sink,
        &mut scratch,
    )
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

/// Append all frames from a decoded packet as interleaved `i16` (decode order).
pub(crate) fn append_interleaved_frames_sequential(
    decoded: GenericAudioBufferRef<'_>,
    out: &mut Vec<i16>,
    channels: usize,
    scratch: &mut Vec<f32>,
    trim_start_frames: u32,
) {
    let frame_count = decoded.frames();
    if frame_count == 0 {
        return;
    }

    let source_channels = decoded.spec().channels().count().max(1);
    let channels = channels.max(1);
    scratch.clear();
    decoded.copy_to_vec_interleaved(scratch);
    let interleaved = &*scratch;

    let trim_start = trim_start_frames as usize;
    for frame_idx in trim_start..frame_count {
        let frame_start = frame_idx * source_channels;
        let frame = &interleaved[frame_start..frame_start + source_channels];
        for channel in 0..channels {
            let sample = frame.get(channel).copied().unwrap_or(0.0);
            out.push(float_to_i16(sample));
        }
    }
}

/// Append all frames from a decoded packet as downmixed mono samples (decode order).
pub(crate) fn append_mono_frames_sequential(
    decoded: GenericAudioBufferRef<'_>,
    out: &mut Vec<i16>,
    scratch: &mut Vec<f32>,
    trim_start_frames: u32,
) {
    let frame_count = decoded.frames();
    if frame_count == 0 {
        return;
    }

    let channel_count = decoded.spec().channels().count().max(1);
    scratch.clear();
    decoded.copy_to_vec_interleaved(scratch);
    let interleaved = &*scratch;

    let trim_start = trim_start_frames as usize;
    for frame_idx in trim_start..frame_count {
        let frame_start = frame_idx * channel_count;
        let frame = &interleaved[frame_start..frame_start + channel_count];
        let mono = if frame.is_empty() {
            0.0
        } else {
            frame.iter().sum::<f32>() / frame.len() as f32
        };
        out.push(float_to_i16(mono));
    }
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

pub(super) fn timestamp_to_std_duration(ts: Timestamp, time_base: TimeBase) -> Option<Duration> {
    time_base.calc_time(ts).map(symphonia_time_to_std)
}

pub(super) fn timestamp_to_sample(ts: Timestamp, time_base: TimeBase, sample_rate: u32) -> u64 {
    timestamp_to_std_duration(ts, time_base)
        .map(|duration| time_to_sample(duration, sample_rate))
        .unwrap_or(0)
}

pub(super) fn media_duration_to_frames(dur: MediaDuration, time_base: TimeBase, sample_rate: u32) -> u32 {
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

/// Scan packets from a tail seek through EOF to find the last decodable timestamp on `track`.
pub(crate) fn scan_track_decodable_extent(
    path: &Path,
    state: &mut MediaIoState,
    track: &AudioTrack,
    container_duration: Duration,
) -> Result<Option<Duration>, MediaError> {
    const TAIL_PROBE_SECS: f64 = 120.0;

    let track_id = track.index;
    let media_track = state
        .format
        .tracks()
        .iter()
        .find(|candidate| candidate.id == track_id)
        .ok_or_else(|| {
            fail_media(
                path,
                "extent_scan",
                Some(track_id),
                decode_failed(track_id, format!("track {track_id} not found")),
            )
        })?;

    let time_base = match media_track.time_base {
        Some(time_base) => time_base,
        None => return Ok(None),
    };

    let probe_start = container_duration
        .saturating_sub(Duration::from_secs_f64(TAIL_PROBE_SECS))
        .min(container_duration);

    seek_with_recovery(path, state, track, probe_start)?;

    let mut max_end = probe_start;
    loop {
        match state.format.next_packet() {
            Ok(Some(packet)) => {
                if packet.track_id != track_id {
                    continue;
                }
                let end_ts = packet.pts.saturating_add(packet.dur);
                if let Some(time) = time_base.calc_time(end_ts) {
                    max_end = max_end.max(
                        crate::infrastructure::symphonia::duration::symphonia_time_to_std(time),
                    );
                }
            }
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => continue,
            Err(error) => return Err(map_decode_loop_error(path, track_id, error)),
        }
    }

    if max_end <= probe_start && probe_start > Duration::ZERO {
        return Ok(None);
    }

    debug!(
        path = %path.display(),
        track = track_id,
        container_secs = container_duration.as_secs_f64(),
        decodable_secs = max_end.as_secs_f64(),
        "tail packet scan decodable extent"
    );

    if max_end > container_duration {
        warn!(
            path = %path.display(),
            track = track_id,
            container_secs = container_duration.as_secs_f64(),
            observed_secs = max_end.as_secs_f64(),
            "container under-reports duration; clamping decodable extent to declared"
        );
    }

    Ok(Some(max_end.min(container_duration)))
}

/// Raw seek that returns the Symphonia error unmapped, so callers can recover before mapping.
fn raw_seek(
    format: &mut dyn symphonia::core::formats::FormatReader,
    track_id: u32,
    start: Duration,
) -> Result<(), SymphoniaError> {
    // Duration values from media operations fit comfortably in i64 seconds.
    let secs = start.as_secs().min(i64::MAX as u64) as i64;
    let time = Time::try_new(secs, start.subsec_nanos())
        .unwrap_or_else(|| Time::try_new(secs, 0).expect("valid time"));
    format
        .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(track_id) })
        .map(|_| ())
}

/// Seek to `start`, recovering once by reopening `MediaIoState` on seek failure.
/// `OutOfRange` errors (expected tail-boundary seeks) are not retried.
/// Returns the original (mapped) seek error if reopen fails or the retry also fails.
pub(super) fn seek_with_recovery(
    path: &Path,
    state: &mut MediaIoState,
    track: &AudioTrack,
    start: Duration,
) -> Result<(), MediaError> {
    let track_id = track.index;
    let first_err = match raw_seek(state.format.as_mut(), track_id, start) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };

    if matches!(first_err, SymphoniaError::SeekError(SeekErrorKind::OutOfRange)) {
        return Err(map_seek_error(
            path,
            track_id,
            start.as_secs_f64(),
            first_err,
            track.duration.map(|d| d.as_secs_f64()),
        ));
    }

    debug!(
        path = %path.display(),
        track = track_id,
        start_secs = start.as_secs_f64(),
        "seek failed; reopening io state and retrying once"
    );

    let mapped = map_seek_error(
        path,
        track_id,
        start.as_secs_f64(),
        first_err,
        track.duration.map(|d| d.as_secs_f64()),
    );
    if let Ok(fresh) = MediaIoState::open(path) {
        *state = fresh;
        if ensure_track_decoder(path, state, track).is_ok()
            && raw_seek(state.format.as_mut(), track_id, start).is_ok()
        {
            return Ok(());
        }
    }
    Err(mapped)
}

pub(super) fn seek_to_window_start(
    path: &Path,
    format: &mut dyn symphonia::core::formats::FormatReader,
    track_id: u32,
    start: Duration,
    track_duration: Option<Duration>,
) -> Result<(), MediaError> {
    let time = Time::try_new(start.as_secs() as i64, start.subsec_nanos()).ok_or_else(|| {
        MediaError::seek_failed(format!(
            "{}: seek to {:.3}s on track {track_id} failed: invalid time",
            path.display(),
            start.as_secs_f64()
        ))
    })?;

    let seek_to = SeekTo::Time {
        time,
        track_id: Some(track_id),
    };

    format.seek(SeekMode::Accurate, seek_to).map_err(|error| {
        map_seek_error(
            path,
            track_id,
            start.as_secs_f64(),
            error,
            track_duration.map(|d| d.as_secs_f64()),
        )
    })?;

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

#[cfg(test)]
mod tail_clip_tests {
    use super::tail_partial_clip_acceptable;

    #[test]
    fn accepts_mkv_style_tail_shortfall_for_end_clip() {
        // Observed on long MKV/AAC end-clip extract: ~99.6% decoded, ~3.9s short of 15m window.
        assert!(tail_partial_clip_acceptable(
            44_454_896,
            44_640_000,
            true,
            6180.033,
            Some(6180.0),
        ));
    }

    #[test]
    fn rejects_midfile_partial_even_when_eof_flag_set() {
        assert!(!tail_partial_clip_acceptable(
            44_454_896,
            44_640_000,
            true,
            3000.0,
            Some(6180.0),
        ));
    }

    #[test]
    fn rejects_tail_clip_with_too_little_decoded_audio() {
        assert!(!tail_partial_clip_acceptable(
            20_000_000,
            44_640_000,
            true,
            6180.0,
            Some(6180.0),
        ));
    }

    #[test]
    fn interior_partial_shortfall_is_hard_fail_not_near_track_end() {
        // Observed on MKV interior clip 2/4: ~99.78% of a 15m window, ~2.1s short at 48kHz.
        let disposition = super::shortfall_disposition(
            45_978_881,
            46_079_999,
            false,
            4304.625,
            Some(10_200.0),
            48_000,
        );
        assert_eq!(
            disposition,
            super::ShortfallDisposition::HardFail {
                near_track_end: false
            }
        );
    }

    #[test]
    fn tail_shortfall_within_eof_cap_pads() {
        let disposition = super::shortfall_disposition(
            46_000_000,
            46_079_999,
            true,
            6180.0,
            Some(6180.0),
            48_000,
        );
        assert!(matches!(disposition, super::ShortfallDisposition::Pad { .. }));
    }
}
