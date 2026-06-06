use std::time::Duration;

use crate::domain::audio_track::AudioTrack;
use crate::domain::clip_plan::ClipPlan;
use crate::domain::clip_window::{ClipLabel, ClipWindow};
use crate::domain::error::DomainError;
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

pub fn clip_windows(duration: Duration, plan: &ClipPlan) -> Result<Vec<ClipWindow>, DomainError> {
    if duration.is_zero() {
        return Err(DomainError::InvalidDuration);
    }

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

    let n = effective_num_clips;
    let mut windows = Vec::with_capacity(n as usize);

    windows.push(ClipWindow::new(
        Duration::ZERO,
        clip_length,
        ClipLabel::Start,
    ));

    if n > 2 {
        let duration_secs = duration.as_secs_f64();
        let clip_secs = clip_length.as_secs_f64();

        for i in 1..(n - 1) {
            let seg_start_secs = duration_secs * f64::from(i) / f64::from(n);
            let seg_end_secs = duration_secs * f64::from(i + 1) / f64::from(n);
            let center_secs = (seg_start_secs + seg_end_secs) / 2.0;
            let half = clip_secs / 2.0;
            let start_secs = (center_secs - half).max(0.0);
            let end_secs = (start_secs + clip_secs).min(duration_secs);

            windows.push(ClipWindow::new(
                secs_to_duration(start_secs),
                secs_to_duration(end_secs),
                ClipLabel::Interior,
            ));
        }
    }

    let end_start = duration.saturating_sub(clip_length);
    windows.push(ClipWindow::new(
        end_start,
        duration,
        ClipLabel::End,
    ));

    for window in &windows {
        if window.duration().is_zero() {
            return Err(DomainError::EmptyClip);
        }
    }

    Ok(windows)
}

fn secs_to_duration(secs: f64) -> Duration {
    Duration::from_secs_f64(secs.max(0.0))
}

/// Latest timeline position covered by actually decoded samples (ignores end-of-window silence padding).
pub fn decoded_timeline_extent(windows: &[ClipWindow], clips: &[MonoPcmClip]) -> Duration {
    windows
        .iter()
        .zip(clips)
        .map(|(window, clip)| {
            let rate = f64::from(clip.sample_rate.max(1));
            let decoded_secs = clip.effective_decoded_sample_count() as f64 / rate;
            window.start.as_secs_f64() + decoded_secs
        })
        .map(Duration::from_secs_f64)
        .max()
        .unwrap_or(Duration::ZERO)
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

    #[test]
    fn select_best_track_prefers_first_decodable_in_container_order() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 44_100,
                bitrate: None,
                duration: Some(mins(60)),
                decodable: true,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,
                bitrate: None,
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
                bitrate: None,
                duration: Some(mins(60)),
                decodable: true,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,
                bitrate: None,
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
                bitrate: None,
                duration: Some(mins(60)),
                decodable: true,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,
                bitrate: None,
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
            bitrate: None,
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
                bitrate: None,
                duration: Some(mins(60)),
                decodable: true,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,
                bitrate: None,
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
                bitrate: None,
                duration: Some(mins(60)),
                decodable: false,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 44_100,
                bitrate: None,
                duration: Some(mins(60)),
                decodable: true,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 1);
    }

    #[test]
    fn clip_windows_short_media_single_start_clip() {
        let plan = ClipPlan::new(mins(15), 2);
        let windows = clip_windows(mins(12), &plan).unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(12));
        assert_eq!(windows[0].label, ClipLabel::Start);
    }

    #[test]
    fn clip_windows_two_clips_start_and_end() {
        let plan = ClipPlan::new(mins(15), 2);
        let windows = clip_windows(mins(45), &plan).unwrap();

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(15));
        assert_eq!(windows[1].start, mins(30));
        assert_eq!(windows[1].end, mins(45));
    }

    #[test]
    fn clip_windows_num_clips_one_on_long_media() {
        let plan = ClipPlan::new(mins(15), 1);
        let windows = clip_windows(mins(60), &plan).unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].end, mins(15));
    }

    #[test]
    fn clip_windows_three_clips_with_interior() {
        let plan = ClipPlan::new(mins(10), 3);
        let windows = clip_windows(mins(60), &plan).unwrap();

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
    fn clip_windows_rejects_zero_duration() {
        let plan = ClipPlan::new(mins(15), 2);
        assert_eq!(
            clip_windows(Duration::ZERO, &plan),
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
    fn decoded_timeline_extent_ignores_silence_padding() {
        let window = ClipWindow::new(Duration::ZERO, Duration::from_secs(600), ClipLabel::Start);
        let clip = MonoPcmClip {
            sample_rate: 44_100,
            samples: vec![0; 441_000],
            decode_error_skips: 0,
            decoded_sample_count: Some(203 * 44_100),
        };
        let extent = decoded_timeline_extent(&[window], &[clip]);
        assert!((extent.as_secs_f64() - 203.0).abs() < 0.01);
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
