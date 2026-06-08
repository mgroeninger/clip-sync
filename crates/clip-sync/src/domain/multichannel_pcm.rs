/// Native-rate, all-channels PCM for a single window — the repair fill path counterpart to
/// [`MonoPcmClip`](crate::domain::MonoPcmClip), which downmixes to mono at the fingerprint rate.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiChannelPcm {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved `i16` frames: `samples.len() == frames * channels`.
    pub samples: Vec<i16>,
    /// Corrupt packets skipped during decode (`0` for synthetic / unknown sources).
    pub decode_error_skips: u32,
    /// Frames decoded before end-of-window silence padding, when padding was applied.
    pub decoded_frame_count: Option<usize>,
}

impl MultiChannelPcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate.max(1))
    }

    pub fn effective_decoded_frame_count(&self) -> usize {
        self.decoded_frame_count.unwrap_or_else(|| self.frames())
    }
}
