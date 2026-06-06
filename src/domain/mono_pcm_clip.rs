#[derive(Debug, Clone, PartialEq)]
pub struct MonoPcmClip {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

impl MonoPcmClip {
    pub fn new(sample_rate: u32, samples: Vec<i16>) -> Self {
        Self {
            sample_rate,
            samples,
        }
    }
}
