//! Crate-internal test support that any layer may use.
//!
//! Unlike `application::testing` (application-layer fakes and fixtures), helpers here sit
//! outside the layer stack, so infrastructure tests can depend on them without pointing a
//! dependency arrow at the application module.

#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
pub mod ac3_pcm_analysis;
pub mod audio_fixtures;
pub mod ffmpeg_util;
