#[derive(Debug, Clone, PartialEq)]
pub struct MonoPcmClip {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
    /// Corrupt packets skipped during decode (`0` for synthetic / unknown sources).
    pub decode_error_skips: u32,
}

impl MonoPcmClip {
    pub fn new(sample_rate: u32, samples: Vec<i16>) -> Self {
        Self {
            sample_rate,
            samples,
            decode_error_skips: 0,
        }
    }
}
