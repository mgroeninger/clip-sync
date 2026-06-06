#[derive(Debug, Clone, PartialEq)]
pub struct Fingerprint {
    pub data: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchSegment {
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlignmentResult {
    pub offset_secs: f64,
    pub confidence: f32,
    pub per_clip_offsets: Vec<f64>,
    pub segments: Vec<MatchSegment>,
}
