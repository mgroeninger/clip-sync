pub mod align_bridge;
pub mod error;
pub mod fit_routing;
pub mod gap_equivalence;
pub mod gap_fingerprint;
#[doc(hidden)]
pub mod gate_oracle;
pub mod mux_bitrate;
pub mod patch_audio;
pub mod patch_region;
pub mod pcm_bridge;
pub mod ports;
pub mod repair_videos;
pub mod run_repair;
pub mod scan_gaps;
#[doc(hidden)]
pub mod test_support;

pub use error::RepairError;
pub use patch_audio::{
    AbSources, FillWindowFrames, PatchAudio, PatchAudioRequest, PatchAudioResult,
    PatchRequestSettings, SourceDescriptor,
};
pub use repair_videos::{RepairVideos, RepairWriteRequest};
pub use scan_gaps::{ScanGaps, ScanGapsRequest};
pub use test_support::{
    no_op_alignment, oracle_injected_alignment, start_clip_alignment, zero_offset_alignment,
    NeverCalledAligner,
};
