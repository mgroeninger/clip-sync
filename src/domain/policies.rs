use std::time::Duration;

use crate::domain::audio_track::AudioTrack;
use crate::domain::clip_plan::ClipPlan;
use crate::domain::clip_window::{ClipLabel, ClipWindow};
use crate::domain::error::DomainError;

pub fn select_best_track(tracks: &[AudioTrack]) -> Result<&AudioTrack, DomainError> {
    tracks
        .iter()
        .max_by(|a, b| {
            a.sample_rate
                .cmp(&b.sample_rate)
                .then(a.channels.cmp(&b.channels))
                .then(a.bitrate.cmp(&b.bitrate))
        })
        .ok_or(DomainError::NoAudioTracks)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::audio_track::AudioTrack;

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    #[test]
    fn select_best_track_prefers_higher_sample_rate() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 44_100,
                bitrate: Some(128_000),
                duration: Some(mins(60)),
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,
                bitrate: Some(128_000),
                duration: Some(mins(60)),
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
}
