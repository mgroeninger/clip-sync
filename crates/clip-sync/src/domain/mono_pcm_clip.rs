#[derive(Debug, Clone, PartialEq)]
pub struct MonoPcmClip {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
    /// Corrupt packets skipped during decode (`0` for synthetic / unknown sources).
    pub decode_error_skips: u32,
    /// Samples decoded before end-of-window silence padding, when padding was applied.
    pub decoded_sample_count: Option<usize>,
}

impl MonoPcmClip {
    pub fn effective_decoded_sample_count(&self) -> usize {
        self.decoded_sample_count.unwrap_or(self.samples.len())
    }
}
