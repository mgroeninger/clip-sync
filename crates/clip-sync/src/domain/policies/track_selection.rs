//! Audio-track selection policy for alignment.

use crate::domain::audio_track::AudioTrack;
use crate::domain::error::DomainError;

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

/// Order decodable A×B track pairs for `try_all_tracks`: channel-matched layouts first.
pub fn order_track_pairs_for_alignment<'a>(
    decodable_a: &[&'a AudioTrack],
    decodable_b: &[&'a AudioTrack],
) -> Vec<(&'a AudioTrack, &'a AudioTrack)> {
    let mut pairs: Vec<(&'a AudioTrack, &'a AudioTrack)> = decodable_a
        .iter()
        .flat_map(|track_a| decodable_b.iter().map(move |track_b| (*track_a, *track_b)))
        .collect();
    pairs.sort_by(|(a1, b1), (a2, b2)| {
        let matched1 = a1.channels == b1.channels;
        let matched2 = a2.channels == b2.channels;
        matched2
            .cmp(&matched1)
            .then_with(|| a1.index.cmp(&a2.index))
            .then_with(|| b1.index.cmp(&b2.index))
    });
    pairs
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

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
            bit_depth: None,
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
    fn order_track_pairs_for_alignment_prefers_channel_matched_pairs() {
        let a6 = track(2, 6, true);
        let b2 = track(1, 2, true);
        let b6 = track(2, 6, true);
        let pairs = order_track_pairs_for_alignment(&[&a6], &[&b2, &b6]);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], (&a6, &b6));
        assert_eq!(pairs[1], (&a6, &b2));
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
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
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
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
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
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: false,
                bit_depth: None,
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
            bit_depth: None,
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
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
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
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 44_100,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 1);
    }
}
