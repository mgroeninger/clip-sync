#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrack {
    pub index: u32,
    pub codec: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub bitrate: Option<u32>,
}
