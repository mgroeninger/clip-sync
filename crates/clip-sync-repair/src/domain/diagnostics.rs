//! Repair-run warnings and thresholds for timeline / duration sanity checks.

use crate::domain::AudioTimelineSkew;

/// Symmetric overlap on A is expected to start near 0:00 on a normal recording.
pub const OVERLAP_START_WARN_SECS: f64 = 1.0;

/// Warn when PTS and sequential sample clocks diverge beyond this (seconds).
pub const TIMELINE_SKEW_WARN_SECS: f64 = 1.0;

/// Warn when patched PCM length differs from container duration beyond this (seconds).
pub const PCM_CONTAINER_WARN_SECS: f64 = 2.0;

/// Refuse mux when patched PCM and video duration differ beyond this (seconds).
pub const MUX_DURATION_ERROR_SECS: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PcmContainerDurationSkew {
    pub pcm_secs: f64,
    pub container_secs: f64,
    pub delta_secs: f64,
}

pub fn pcm_container_duration_skew(pcm_secs: f64, container_secs: f64) -> PcmContainerDurationSkew {
    PcmContainerDurationSkew {
        pcm_secs,
        container_secs,
        delta_secs: (pcm_secs - container_secs).abs(),
    }
}

pub fn collect_repair_warnings(
    overlap_a_start_secs: Option<f64>,
    query_mode: bool,
    audio_timeline_skew: Option<AudioTimelineSkew>,
    pcm_skew: Option<PcmContainerDurationSkew>,
) -> Vec<String> {
    let mut warnings = Vec::new();

    if !query_mode {
        if let Some(start) = overlap_a_start_secs {
            if let Some(line) = format_overlap_start_warning(start) {
                warnings.push(line);
            }
        }
    }

    if let Some(skew) = audio_timeline_skew {
        if skew.delta_secs > TIMELINE_SKEW_WARN_SECS {
            warnings.push(format_timeline_skew_warning(skew));
        }
    }

    if let Some(skew) = pcm_skew {
        if skew.delta_secs > PCM_CONTAINER_WARN_SECS {
            warnings.push(format_pcm_container_warning(skew));
        }
    }

    warnings
}

pub fn format_overlap_start_warning(start_secs: f64) -> Option<String> {
    if start_secs <= OVERLAP_START_WARN_SECS {
        return None;
    }
    Some(format!(
        "Warning: video A shared overlap starts at {start_secs:.1}s (not 0:00) — gap times are on the decoded-sample clock and may not match ffmpeg/container timestamps; prefer a clean source file or MKV"
    ))
}

pub fn format_timeline_skew_warning(skew: AudioTimelineSkew) -> String {
    format!(
        "Warning: audio timeline mismatch on video A (PTS {:.1}s vs decoded-sample clock {:.1}s, Δ {:.1}s) — gap positions may be shifted relative to ffmpeg silencedetect",
        skew.pts_secs, skew.sample_clock_secs, skew.delta_secs
    )
}

pub fn format_pcm_container_warning(skew: PcmContainerDurationSkew) -> String {
    format!(
        "Warning: patched audio length {:.1}s differs from container duration {:.1}s by {:.1}s — mux may fail or truncate",
        skew.pcm_secs, skew.container_secs, skew.delta_secs
    )
}

pub fn format_mux_duration_error(pcm_secs: f64, video_secs: f64) -> String {
    let delta = (pcm_secs - video_secs).abs();
    format!(
        "patched audio ({pcm_secs:.1}s) and video ({video_secs:.1}s) differ by {delta:.1}s (>{MUX_DURATION_ERROR_SECS}s); use --wav to inspect audio or fix source timestamps"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_warning_when_start_above_threshold() {
        let line = format_overlap_start_warning(4.97).expect("warning");
        assert!(line.contains("5.0"));
        assert!(line.contains("0:00"));
    }

    #[test]
    fn overlap_warning_suppressed_near_zero() {
        assert!(format_overlap_start_warning(0.5).is_none());
    }

    #[test]
    fn mux_error_includes_durations() {
        let msg = format_mux_duration_error(100.0, 90.0);
        assert!(msg.contains("10.0s"));
        assert!(msg.contains("--wav"));
    }
}
