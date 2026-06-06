use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrack {
    pub index: u32,
    pub codec: String,
    pub channels: u16,
    pub sample_rate: u32,
    /// Container-reported average bitrate in bits per second, when known.
    pub bitrate: Option<u32>,
    /// Decodable duration for this track, when the container reports frame count and time base.
    pub duration: Option<Duration>,
    /// Whether Symphonia can construct a decoder for this track at probe time.
    pub decodable: bool,
}
