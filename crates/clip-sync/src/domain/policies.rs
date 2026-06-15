use std::time::Duration;

use crate::domain::audio_track::AudioTrack;
use crate::domain::clip_plan::ClipPlan;
use crate::domain::clip_window::{ClipLabel, ClipWindow};
use crate::domain::error::DomainError;
use crate::domain::media_extent::MediaExtent;
use crate::domain::mono_pcm_clip::MonoPcmClip;

/// Pick the first decodable audio track in container order.
///
/// Multi-track files often mux the main program before commentary or effects tracks.
/// Higher sample rate on a secondary track is not a reliable signal for "best" audio.
pub fn select_best_track(tracks: &[AudioTrack]) -> Result<&AudioTrack, DomainError> {
    if tracks.is_empty() {
        return Err(DomainError::NoAudioTracks);
    }

    tracks
        .iter()
        .find(|track| track.decodable)
        .ok_or(DomainError::NoDecodableAudioTracks)
}

/// Pick the best decodable B track to use as a reference for A.
///
/// Prefers a track whose channel count matches A's exactly; falls back to
/// `select_best_track` (first decodable in container order) when no channel
/// match exists. This matters for dual-track containers (e.g. 2ch AAC + 6ch
/// AC-3) where the surround track is the correct repair source.
pub fn select_track_for_reference<'a>(
    a: &AudioTrack,
    tracks: &'a [AudioTrack],
) -> Result<&'a AudioTrack, DomainError> {
    if tracks.is_empty() {
        return Err(DomainError::NoAudioTracks);
    }

    tracks
        .iter()
        .find(|t| t.decodable && t.channels == a.channels)
        .or_else(|| tracks.iter().find(|t| t.decodable))
        .ok_or(DomainError::NoDecodableAudioTracks)
}

/// Optional bounds when placing multi-clip windows near the file tail.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClipPlanningOptions {
    /// Inset before [`MediaExtent::effective`] for the end clip window.
    pub end_tail_inset: Duration,
}

pub fn effective_timeline_end(extent: &MediaExtent, end_tail_inset: Duration) -> Duration {
    extent
        .effective()
        .saturating_sub(end_tail_inset)
        .max(Duration::from_secs(1))
        .min(extent.declared)
}

pub fn clip_windows_with_options(
    extent: &MediaExtent,
    plan: &ClipPlan,
    options: ClipPlanningOptions,
) -> Result<Vec<ClipWindow>, DomainError> {
    if extent.declared.is_zero() {
        return Err(DomainError::InvalidDuration);
    }

    let duration = extent.declared;
    let clip_length = plan.clip_length;
    let effective_num_clips = if duration < clip_length {
        1
    } else {
        plan.num_clips
    };

    if effective_num_clips == 1 {
        let end = duration.min(clip_length);
        if end.is_zero() {
            return Err(DomainError::EmptyClip);
        }
        return Ok(vec![ClipWindow::new(
            Duration::ZERO,
            end,
            ClipLabel::Start,
        )]);
    }

    let timeline_end = effective_timeline_end(extent, options.end_tail_inset);
    if timeline_end < clip_length {
        let end = timeline_end.min(clip_length);
        if end.is_zero() {
            return Err(DomainError::EmptyClip);
        }
        return Ok(vec![ClipWindow::new(
            Duration::ZERO,
            end,
            ClipLabel::Start,
        )]);
    }

    let n = effective_num_clips;
    let mut windows = Vec::with_capacity(n as usize);

    windows.push(ClipWindow::new(
        Duration::ZERO,
        clip_length,
        ClipLabel::Start,
    ));

    if n > 2 {
        let timeline_secs = timeline_end.as_secs_f64();
        let clip_secs = clip_length.as_secs_f64();

        for i in 1..(n - 1) {
            let seg_start_secs = timeline_secs * f64::from(i) / f64::from(n);
            let seg_end_secs = timeline_secs * f64::from(i + 1) / f64::from(n);
            let center_secs = (seg_start_secs + seg_end_secs) / 2.0;
            let half = clip_secs / 2.0;
            let start_secs = (center_secs - half).max(0.0);
            let end_secs = (start_secs + clip_secs).min(timeline_secs);

            windows.push(ClipWindow::new(
                secs_to_duration(start_secs),
                secs_to_duration(end_secs),
                ClipLabel::Interior,
            ));
        }
    }

    let end_start = timeline_end.saturating_sub(clip_length);
    windows.push(ClipWindow::new(
        end_start,
        timeline_end,
        ClipLabel::End,
    ));

    for window in &windows {
        if window.duration().is_zero() {
            return Err(DomainError::EmptyClip);
        }
    }

    Ok(windows)
}

/// Drop silence padding appended when a tail extract ended before the planned window end.
pub fn truncate_padded_tail(mut clip: MonoPcmClip) -> MonoPcmClip {
    if let Some(decoded) = clip.decoded_sample_count {
        if decoded < clip.samples.len() {
            clip.samples.truncate(decoded);
        }
        clip.decoded_sample_count = None;
    }
    clip
}

/// Whether a hold-out extract decoded enough samples for the requested segment length.
pub fn holdout_extract_sufficient(
    clip: &MonoPcmClip,
    segment_length: Duration,
    min_decode_fraction: f64,
    max_decode_skips: u32,
) -> bool {
    if clip.decode_error_skips > max_decode_skips {
        return false;
    }
    let rate = clip.sample_rate.max(1);
    let expected = ((segment_length.as_secs_f64() * f64::from(rate))
        .floor()
        .max(1.0)) as usize;
    if expected == 0 {
        return false;
    }
    let decoded = clip.effective_decoded_sample_count();
    let threshold = min_decode_fraction.clamp(0.0, 1.0);
    (decoded as f64) >= (expected as f64) * threshold
}

/// Whether an end-clip extract is too incomplete or corrupt for alignment.
pub fn end_clip_extract_unreliable(
    clip: &MonoPcmClip,
    window: &ClipWindow,
    min_decode_fraction: f64,
    max_decode_skips: u32,
) -> bool {
    if window.label != ClipLabel::End {
        return false;
    }
    if clip.decode_error_skips > max_decode_skips {
        return true;
    }
    let rate = clip.sample_rate.max(1);
    let expected = window.sample_count_at(rate);
    if expected == 0 {
        return true;
    }
    let decoded = clip.effective_decoded_sample_count();
    let threshold = min_decode_fraction.clamp(0.0, 1.0);
    (decoded as f64) < (expected as f64) * threshold
}

fn secs_to_duration(secs: f64) -> Duration {
    Duration::from_secs_f64(secs.max(0.0))
}

/// Pick a hold-out window on the shorter file's timeline, avoiding discovery windows when possible.
pub fn pick_holdout_window(
    duration: Duration,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
) -> Option<ClipWindow> {
    if duration < segment_length {
        return None;
    }

    let segment_secs = segment_length.as_secs_f64();
    let duration_secs = duration.as_secs_f64();

    if discovery_windows.len() <= 1 {
        let start_secs = duration_secs / 3.0;
        let end_secs = (start_secs + segment_secs).min(duration_secs);
        if end_secs - start_secs < segment_secs {
            return None;
        }
        return Some(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(end_secs),
            ClipLabel::Interior,
        ));
    }

    let gap_start = discovery_windows.first()?.end.as_secs_f64();
    let gap_end = discovery_windows.last()?.start.as_secs_f64();
    let start_secs = if gap_end - gap_start >= segment_secs {
        gap_start + (gap_end - gap_start - segment_secs) / 2.0
    } else {
        (duration_secs - segment_secs) / 2.0
    };
    let end_secs = start_secs + segment_secs;
    if end_secs > duration_secs {
        return None;
    }

    Some(ClipWindow::new(
        secs_to_duration(start_secs),
        secs_to_duration(end_secs),
        ClipLabel::Interior,
    ))
}

/// Hold-out placement candidates, most preferred first. Includes fallbacks for inflated duration metadata.
pub fn holdout_window_candidates(
    duration: Duration,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
    offset_secs: f64,
) -> Vec<ClipWindow> {
    if duration < segment_length {
        return Vec::new();
    }

    let duration_secs = duration.as_secs_f64();
    let segment_secs = segment_length.as_secs_f64();
    let mut candidates = Vec::new();

    let mut push_unique = |window: ClipWindow| {
        if window.end <= window.start {
            return;
        }
        if candidates
            .iter()
            .any(|existing: &ClipWindow| {
                (existing.start.as_secs_f64() - window.start.as_secs_f64()).abs() < 0.001
            })
        {
            return;
        }
        candidates.push(window);
    };

    // Overlap-safe near-start windows first (avoids broken mid-file seeks on some MKV tracks).
    let min_a_start = (-offset_secs).max(0.0);
    if min_a_start + segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            secs_to_duration(min_a_start),
            secs_to_duration(min_a_start + segment_secs),
            ClipLabel::Interior,
        ));
    }
    let overlap_interior = min_a_start + 30.0;
    if overlap_interior + segment_secs <= duration_secs && overlap_interior > min_a_start + segment_secs {
        push_unique(ClipWindow::new(
            secs_to_duration(overlap_interior),
            secs_to_duration(overlap_interior + segment_secs),
            ClipLabel::Interior,
        ));
    }

    if let Some(window) = pick_holdout_window(duration, discovery_windows, segment_length) {
        push_unique(window);
    }

    let early_start = duration_secs / 6.0;
    if early_start + segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            secs_to_duration(early_start),
            secs_to_duration(early_start + segment_secs),
            ClipLabel::Interior,
        ));
    }

    if let Some(first) = discovery_windows.first() {
        let after_discovery = first.end.as_secs_f64();
        if after_discovery + segment_secs <= duration_secs {
            push_unique(ClipWindow::new(
                secs_to_duration(after_discovery),
                secs_to_duration(after_discovery + segment_secs),
                ClipLabel::Interior,
            ));
        }
    }

    if segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            Duration::ZERO,
            secs_to_duration(segment_secs),
            ClipLabel::Interior,
        ));
    }

    candidates
}

/// Calendar-parallel hold-out windows (same `[T, T+L)` on A and B) for periodic offset recheck.
/// Edge `T = 0` is listed first.
pub fn parallel_holdout_window_candidates(
    duration_a: Duration,
    duration_b: Duration,
    segment_length: Duration,
) -> Vec<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return Vec::new();
    }

    let max_start = duration_a
        .as_secs_f64()
        .min(duration_b.as_secs_f64())
        - segment_secs;
    if max_start < 0.0 {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push_unique = |start_secs: f64| {
        if start_secs < 0.0 || start_secs > max_start + 0.001 {
            return;
        }
        if candidates.iter().any(|w: &ClipWindow| {
            (w.start.as_secs_f64() - start_secs).abs() < 0.001
        }) {
            return;
        }
        candidates.push(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(start_secs + segment_secs),
            ClipLabel::Interior,
        ));
    };

    push_unique(0.0);
    if max_start > 0.0 {
        push_unique(max_start);
    }

    candidates
}

pub fn holdout_window_feasible(
    window_start_secs: f64,
    segment_length_secs: f64,
    offset_secs: f64,
    duration_a_secs: f64,
    duration_b_secs: f64,
) -> bool {
    window_start_secs >= 0.0
        && window_start_secs + segment_length_secs <= duration_a_secs
        && window_start_secs + offset_secs >= 0.0
        && window_start_secs + segment_length_secs + offset_secs <= duration_b_secs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::audio_track::AudioTrack;

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    fn track(index: u32, channels: u16, decodable: bool) -> AudioTrack {
        AudioTrack {
            index,
            codec: "aac".into(),
            channels,
            sample_rate: 48_000,

            duration: Some(mins(60)),
            decodable,
        }
    }

    #[test]
    fn select_track_for_reference_picks_channel_match_over_first_decodable() {
        let a = track(0, 6, true);
        let tracks = vec![track(0, 2, true), track(1, 6, true)];
        assert_eq!(select_track_for_reference(&a, &tracks).unwrap().index, 1);
    }

    #[test]
    fn select_track_for_reference_falls_back_to_first_decodable_when_no_match() {
        let a = track(0, 6, true);
        let tracks = vec![track(0, 2, true), track(1, 2, true)];
        assert_eq!(select_track_for_reference(&a, &tracks).unwrap().index, 0);
    }

    #[test]
    fn select_track_for_reference_ignores_undecodable_channel_match() {
        let a = track(0, 6, true);
        let tracks = vec![track(0, 6, false), track(1, 2, true)];
        assert_eq!(select_track_for_reference(&a, &tracks).unwrap().index, 1);
    }

    #[test]
    fn select_track_for_reference_mono_a_unchanged() {
        let a = track(0, 1, true);
        let tracks = vec![track(0, 1, true), track(1, 6, true)];
        assert_eq!(select_track_for_reference(&a, &tracks).unwrap().index, 0);
    }

    #[test]
    fn select_track_for_reference_errors_when_empty() {
        let a = track(0, 2, true);
        assert_eq!(
            select_track_for_reference(&a, &[]),
            Err(DomainError::NoAudioTracks)
        );
    }

    #[test]
    fn select_track_for_reference_errors_when_none_decodable() {
        let a = track(0, 6, true);
        let tracks = vec![track(0, 6, false), track(1, 2, false)];
        assert_eq!(
            select_track_for_reference(&a, &tracks),
            Err(DomainError::NoDecodableAudioTracks)
        );
    }

    #[test]
    fn select_best_track_prefers_first_decodable_in_container_order() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 44_100,

                duration: Some(mins(60)),
                decodable: true,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 0);
    }

    #[test]
    fn select_best_track_prefers_program_when_decoy_has_higher_sample_rate() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 11_025,

                duration: Some(mins(60)),
                decodable: true,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 0);
    }

    #[test]
    fn select_best_track_prefers_decodable_over_sample_rate() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "ac3".into(),
                channels: 6,
                sample_rate: 44_100,

                duration: Some(mins(60)),
                decodable: true,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: false,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 0);
    }

    #[test]
    fn select_best_track_errors_when_none_are_decodable() {
        let tracks = vec![AudioTrack {
            index: 2,
            codec: "aac".into(),
            channels: 6,
            sample_rate: 48_000,

            duration: Some(mins(60)),
            decodable: false,
        }];

        assert_eq!(
            select_best_track(&tracks),
            Err(DomainError::NoDecodableAudioTracks)
        );
    }

    #[test]
    fn select_best_track_prefers_first_track_when_sample_rates_match() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: 6,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 0);
    }

    #[test]
    fn select_best_track_skips_undecodable_leading_tracks() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "ac3".into(),
                channels: 6,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: false,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 44_100,

                duration: Some(mins(60)),
                decodable: true,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 1);
    }

    #[test]
    fn clip_windows_short_media_single_start_clip() {
        let plan = ClipPlan::new(mins(15), 2);
        let windows =
            clip_windows_with_options(&MediaExtent::from_declared(mins(12)), &plan, ClipPlanningOptions::default()).unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(12));
        assert_eq!(windows[0].label, ClipLabel::Start);
    }

    #[test]
    fn clip_windows_two_clips_start_and_end() {
        let plan = ClipPlan::new(mins(15), 2);
        let windows =
            clip_windows_with_options(&MediaExtent::from_declared(mins(45)), &plan, ClipPlanningOptions::default()).unwrap();

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(15));
        assert_eq!(windows[1].start, mins(30));
        assert_eq!(windows[1].end, mins(45));
    }

    #[test]
    fn clip_windows_num_clips_one_on_long_media() {
        let plan = ClipPlan::new(mins(15), 1);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(60)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].end, mins(15));
    }

    #[test]
    fn clip_windows_three_clips_with_interior() {
        let plan = ClipPlan::new(mins(10), 3);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(60)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();

        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].label, ClipLabel::Start);
        assert_eq!(windows[1].label, ClipLabel::Interior);
        assert_eq!(windows[2].label, ClipLabel::End);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(10));
        assert_eq!(windows[1].start, mins(25));
        assert_eq!(windows[1].end, mins(35));
        assert_eq!(windows[2].start, mins(50));
        assert_eq!(windows[2].end, mins(60));
    }

    #[test]
    fn clip_windows_clamps_end_to_decodable_extent_with_inset() {
        let plan = ClipPlan::new(mins(15), 2);
        let container = Duration::from_secs_f64(6180.033);
        let extent = Duration::from_secs_f64(6176.0);
        let windows = clip_windows_with_options(
            &MediaExtent::new(container, Some(extent)),
            &plan,
            ClipPlanningOptions {
                end_tail_inset: Duration::from_secs(1),
            },
        )
        .unwrap();

        assert_eq!(windows.len(), 2);
        let end = &windows[1];
        assert_eq!(end.label, ClipLabel::End);
        assert!((end.end.as_secs_f64() - 6175.0).abs() < 0.01);
        assert!((end.start.as_secs_f64() - (6175.0 - 900.0)).abs() < 0.01);
    }

    #[test]
    fn end_clip_extract_unreliable_when_tail_padding_exceeds_threshold() {
        let window = ClipWindow::new(Duration::from_secs(5280), Duration::from_secs(6180), ClipLabel::End);
        let expected = window.sample_count_at(48_000);
        let decoded = (expected as f64 * 0.94) as usize;
        let clip = MonoPcmClip {
            sample_rate: 48_000,
            samples: vec![0; expected],
            decode_error_skips: 0,
            decoded_sample_count: Some(decoded),
        };
        assert!(end_clip_extract_unreliable(&clip, &window, 0.95, 8));
    }

    #[test]
    fn truncate_padded_tail_removes_synthetic_silence() {
        let clip = MonoPcmClip {
            sample_rate: 48_000,
            samples: vec![1; 100],
            decode_error_skips: 0,
            decoded_sample_count: Some(80),
        };
        let trimmed = truncate_padded_tail(clip);
        assert_eq!(trimmed.samples.len(), 80);
        assert!(trimmed.decoded_sample_count.is_none());
    }

    #[test]
    fn clip_windows_rejects_zero_duration() {
        let plan = ClipPlan::new(mins(15), 2);
        assert_eq!(
            clip_windows_with_options(
                &MediaExtent::from_declared(Duration::ZERO),
                &plan,
                ClipPlanningOptions::default(),
            ),
            Err(DomainError::InvalidDuration)
        );
    }

    #[test]
    fn pick_holdout_window_middle_for_single_clip() {
        let discovery = vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(15), ClipLabel::Start)];
        let window =
            pick_holdout_window(Duration::from_secs(60), &discovery, Duration::from_secs(3))
                .unwrap();
        assert_eq!(window.start, Duration::from_secs(20));
        assert_eq!(window.end, Duration::from_secs(23));
    }

    #[test]
    fn pick_holdout_window_fits_two_clip_gap() {
        let discovery = vec![
            ClipWindow::new(Duration::ZERO, Duration::from_secs(15), ClipLabel::Start),
            ClipWindow::new(Duration::from_secs(45), Duration::from_secs(60), ClipLabel::End),
        ];
        let window =
            pick_holdout_window(Duration::from_secs(60), &discovery, Duration::from_secs(3))
                .unwrap();
        assert!((window.start.as_secs_f64() - 28.5).abs() < 0.001);
        assert!((window.end.as_secs_f64() - 31.5).abs() < 0.001);
    }

    #[test]
    fn pick_holdout_window_none_when_shorter_than_segment() {
        let discovery = vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(2), ClipLabel::Start)];
        assert!(pick_holdout_window(Duration::from_secs(2), &discovery, Duration::from_secs(3)).is_none());
    }

    #[test]
    fn holdout_window_feasible_respects_offset() {
        assert!(holdout_window_feasible(10.0, 3.0, 3.0, 120.0, 120.0));
        assert!(!holdout_window_feasible(10.0, 3.0, 3.0, 120.0, 12.0));
    }

    #[test]
    fn holdout_extract_sufficient_requires_full_segment_decode() {
        let segment = Duration::from_secs(60);
        let full = MonoPcmClip {
            sample_rate: 11_025,
            samples: vec![0_i16; 11_025 * 60],
            decode_error_skips: 0,
            decoded_sample_count: Some(11_025 * 60),
        };
        assert!(holdout_extract_sufficient(&full, segment, 0.95, 8));

        let short = MonoPcmClip {
            sample_rate: 11_025,
            samples: vec![0_i16; 11_025 * 30],
            decode_error_skips: 0,
            decoded_sample_count: Some(11_025 * 30),
        };
        assert!(!holdout_extract_sufficient(&short, segment, 0.95, 8));
    }

    #[test]
    fn high_rate_refine_skips_when_window_infeasible() {
        let discovery = vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(60),
            ClipLabel::Start,
        )];
        let candidates = holdout_window_candidates(
            Duration::from_secs(60),
            &discovery,
            Duration::from_secs(3),
            3.0,
        );
        assert!(!candidates.is_empty());
        assert!(
            candidates.iter().all(|window| {
                !holdout_window_feasible(
                    window.start.as_secs_f64(),
                    3.0,
                    3.0,
                    120.0,
                    5.0,
                )
            }),
            "short B duration should make every candidate infeasible"
        );
    }

    #[test]
    fn holdout_candidates_include_early_fallback() {
        let discovery =
            vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(600), ClipLabel::Start)];
        let candidates = holdout_window_candidates(
            Duration::from_secs(203),
            &discovery,
            Duration::from_secs(3),
            0.0,
        );
        assert!(candidates.len() >= 2);
        assert!(candidates.iter().any(|window| window.start.as_secs_f64() < 50.0));
    }

    #[test]
    fn holdout_candidates_prefer_overlap_safe_start_for_negative_offset() {
        let discovery =
            vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(600), ClipLabel::Start)];
        let candidates = holdout_window_candidates(
            Duration::from_secs(600),
            &discovery,
            Duration::from_secs(3),
            -11.019,
        );
        let first = candidates.first().expect("candidate");
        assert!((first.start.as_secs_f64() - 11.019).abs() < 0.001);
    }
}
