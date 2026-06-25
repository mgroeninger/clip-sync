use std::time::Duration;

use crate::domain::BitDepth;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrack {
    pub index: u32,
    pub codec: String,
    pub channels: u16,
    pub sample_rate: u32,
    /// Decodable duration for this track, when the container reports frame count and time base.
    pub duration: Option<Duration>,
    /// Whether Symphonia can construct a decoder for this track at probe time.
    pub decodable: bool,
    /// Source sample format / bit depth as reported by the container codec parameters.
    /// `None` for lossy codecs (AAC, MP3, AC-3, Opus, Vorbis) that don't carry this information.
    pub bit_depth: Option<BitDepth>,
}

/// Human-readable channel layout for logging (e.g. `stereo`, `5.1`).
pub fn channel_layout_label(channels: u16) -> String {
    match channels {
        1 => "mono".into(),
        2 => "stereo".into(),
        3 => "3.0".into(),
        4 => "4.0".into(),
        5 => "5.0".into(),
        6 => "5.1".into(),
        7 => "6.1".into(),
        8 => "7.1".into(),
        n => format!("{n}ch"),
    }
}

impl AudioTrack {
    /// Codec, rate, channel layout, and decodability for stderr / verbose reports.
    pub fn format_description(&self) -> String {
        format!(
            "{} @ {} Hz, {} ({})",
            self.codec,
            self.sample_rate,
            channel_layout_label(self.channels),
            if self.decodable {
                "decodable"
            } else {
                "not decodable"
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_description_includes_codec_and_layout() {
        let track = AudioTrack {
            index: 2,
            codec: "ac3".into(),
            channels: 6,
            sample_rate: 48_000,
            duration: None,
            decodable: true,
            bit_depth: None,
        };
        assert_eq!(
            track.format_description(),
            "ac3 @ 48000 Hz, 5.1 (decodable)"
        );
    }
}
