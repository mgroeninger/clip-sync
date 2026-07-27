use serde::Serialize;

/// Minimal audio track description needed for compatibility assessment.
pub struct TrackDescriptor {
    pub channels: u16,
    pub sample_rate: u32,
}

/// How compatible video B's audio track is with video A's, for patching purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityVerdict {
    /// Same channel count and sample rate — patch directly.
    Identical,
    /// Same channel layout, different sample rate — patch after resampling B to A.
    Compatible,
    /// Channel counts differ — fill must be skipped (no auto up/downmix in v1).
    Mismatch,
}

/// Result of comparing A's and B's selected audio tracks (surround vs stereo, rate match).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrackCompatibility {
    pub a_channels: u16,
    pub b_channels: u16,
    pub a_sample_rate: u32,
    pub b_sample_rate: u32,
    pub channels_match: bool,
    pub rate_match: bool,
    pub verdict: CompatibilityVerdict,
}

/// Compare A's and B's tracks. Channel-count mismatch is the only hard blocker for fill in v1;
/// a sample-rate difference is `Compatible` (B is resampled to A on fill).
pub fn assess_track_compatibility(a: TrackDescriptor, b: TrackDescriptor) -> TrackCompatibility {
    let channels_match = a.channels == b.channels;
    let rate_match = a.sample_rate == b.sample_rate;

    let verdict = if !channels_match {
        CompatibilityVerdict::Mismatch
    } else if rate_match {
        CompatibilityVerdict::Identical
    } else {
        CompatibilityVerdict::Compatible
    };

    TrackCompatibility {
        a_channels: a.channels,
        b_channels: b.channels,
        a_sample_rate: a.sample_rate,
        b_sample_rate: b.sample_rate,
        channels_match,
        rate_match,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(channels: u16, sample_rate: u32) -> TrackDescriptor {
        TrackDescriptor {
            channels,
            sample_rate,
        }
    }

    #[test]
    fn identical_tracks_are_identical() {
        let compat = assess_track_compatibility(descriptor(2, 48_000), descriptor(2, 48_000));
        assert_eq!(compat.verdict, CompatibilityVerdict::Identical);
        assert!(compat.channels_match);
        assert!(compat.rate_match);
    }

    #[test]
    fn same_layout_different_rate_is_compatible() {
        let compat = assess_track_compatibility(descriptor(6, 48_000), descriptor(6, 44_100));
        assert_eq!(compat.verdict, CompatibilityVerdict::Compatible);
        assert!(compat.channels_match);
        assert!(!compat.rate_match);
    }

    #[test]
    fn channel_count_difference_is_mismatch() {
        // Surround A vs stereo B: cannot fill without a channel map.
        let compat = assess_track_compatibility(descriptor(6, 48_000), descriptor(2, 48_000));
        assert_eq!(compat.verdict, CompatibilityVerdict::Mismatch);
        assert!(!compat.channels_match);
        assert_eq!(compat.a_channels, 6);
        assert_eq!(compat.b_channels, 2);
    }

    #[test]
    fn channel_mismatch_dominates_even_when_rate_matches() {
        let compat = assess_track_compatibility(descriptor(1, 48_000), descriptor(2, 48_000));
        assert_eq!(compat.verdict, CompatibilityVerdict::Mismatch);
        assert!(compat.rate_match);
    }
}
