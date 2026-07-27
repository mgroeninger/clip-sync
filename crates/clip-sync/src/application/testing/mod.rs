pub mod alignment_fixtures;
pub mod anchored_end_oracles;
#[cfg(any(test, feature = "test-utils"))]
pub mod corpus_fixtures;
#[cfg(any(test, feature = "test-utils"))]
pub mod corpus_sources;
pub mod fakes;

// Kept at `clip_sync::testing::{audio_fixtures, ffmpeg_util}` for external consumers (e.g.
// repair's integration tests); the modules themselves live in `test_support` so infrastructure
// tests don't import through the application layer.
pub use crate::test_support::audio_fixtures;
#[cfg(feature = "test-utils")]
pub use crate::test_support::ffmpeg_util;
