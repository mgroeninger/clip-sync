use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::Track;
use symphonia::core::meta::{ChapterGroup, ChapterGroupItem};
use symphonia::core::units::{Duration as MediaDuration, Time, TimeBase, Timestamp};
use tracing::{debug, info};

use crate::application::error::MediaError;
use crate::infrastructure::symphonia::error_mapping::{fail_media, map_decode_loop_error};
use crate::infrastructure::symphonia::probe::is_audio_track;

pub(crate) fn duration_from_chapters(chapters: Option<&ChapterGroup>) -> Option<Duration> {
    let mut max_end = Time::ZERO;

    fn visit(group: &ChapterGroup, max_end: &mut Time) {
        for item in &group.items {
            match item {
                ChapterGroupItem::Chapter(chapter) => {
                    if let Some(end) = chapter.end_time {
                        if end.as_nanos() > max_end.as_nanos() {
                            *max_end = end;
                        }
                    } else if chapter.start_time.as_nanos() > max_end.as_nanos() {
                        *max_end = chapter.start_time;
                    }
                }
                ChapterGroupItem::Group(nested) => visit(nested, max_end),
            }
        }
    }

    visit(chapters?, &mut max_end);
    if max_end.is_zero() {
        None
    } else {
        Some(symphonia_time_to_std(max_end))
    }
}

pub(crate) fn scan_container_audio_duration(
    path: &Path,
    format: &mut dyn symphonia::core::formats::FormatReader,
) -> Result<Duration, MediaError> {
    let audio_time_bases: HashMap<u32, TimeBase> = format
        .tracks()
        .iter()
        .filter(|track| is_audio_track(track))
        .filter_map(|track| track.time_base.map(|time_base| (track.id, time_base)))
        .collect();

    if audio_time_bases.is_empty() {
        return Err(fail_media(
            path,
            "probe",
            None,
            MediaError::OpenFailed(format!(
                "could not determine duration for {}",
                path.display()
            )),
        ));
    }

    info!(
        path = %path.display(),
        "no duration metadata; scanning container packets to estimate length"
    );

    let mut max_duration = Duration::ZERO;
    loop {
        match format.next_packet() {
            Ok(Some(packet)) => {
                if let Some(time_base) = audio_time_bases.get(&packet.track_id) {
                    let end = packet.pts.saturating_add(packet.dur);
                    if let Some(time) = time_base.calc_time(end) {
                        max_duration = max_duration.max(symphonia_time_to_std(time));
                    }
                }
            }
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => continue,
            Err(error) => {
                let track_id = audio_time_bases.keys().copied().next().unwrap_or(0);
                return Err(map_decode_loop_error(path, track_id, error));
            }
        }
    }

    if max_duration.is_zero() {
        return Err(fail_media(
            path,
            "probe",
            None,
            MediaError::OpenFailed(format!(
                "could not determine duration for {}",
                path.display()
            )),
        ));
    }

    debug!(
        path = %path.display(),
        duration_secs = max_duration.as_secs_f64(),
        "estimated duration from container scan"
    );
    Ok(max_duration)
}

pub(crate) fn track_duration_from_track(track: &Track) -> Option<Duration> {
    let mut candidates = Vec::new();

    if let Some(time_base) = track.time_base {
        if let Some(media_duration) = track.duration {
            if let Some(time) = media_ticks_to_time(media_duration, time_base) {
                candidates.push(symphonia_time_to_std(time));
            }
        }

        if let Some(num_frames) = track.num_frames {
            if let Some(CodecParameters::Audio(params)) = &track.codec_params {
                if let Some(rate) = params.sample_rate.filter(|rate| *rate > 0) {
                    candidates.push(Duration::from_secs_f64(num_frames as f64 / f64::from(rate)));
                }
            } else if let Some(time) = media_ticks_to_time(MediaDuration::new(num_frames), time_base)
            {
                candidates.push(symphonia_time_to_std(time));
            }
        }
    }

    if let Some(CodecParameters::Audio(params)) = &track.codec_params {
        if let (Some(num_frames), Some(rate)) = (track.num_frames, params.sample_rate) {
            if rate > 0 {
                candidates.push(Duration::from_secs_f64(num_frames as f64 / f64::from(rate)));
            }
        }
    }

    candidates.into_iter().min()
}

pub(crate) fn media_ticks_to_time(ticks: MediaDuration, time_base: TimeBase) -> Option<Time> {
    Timestamp::try_from(ticks.get())
        .ok()
        .and_then(|ts| time_base.calc_time(ts))
}

pub(crate) fn format_media_duration(
    format: &dyn symphonia::core::formats::FormatReader,
) -> Option<Duration> {
    let info = format.media_info();
    let time_base = info.time_base?;
    let ticks = info.duration?;
    media_ticks_to_time(ticks, time_base).map(symphonia_time_to_std)
}

pub(crate) fn symphonia_time_to_std(time: Time) -> Duration {
    let (seconds, nanos) = time.parts();
    Duration::new(seconds.max(0) as u64, nanos)
}
