//! Crate-internal test support that any layer may use.
//!
//! Unlike `application::testing` (application-layer fakes and fixtures), helpers here sit
//! outside the layer stack, so infrastructure tests can depend on them without pointing a
//! dependency arrow at the application module.

// `test` is part of the gate because the only consumer
// (`infrastructure::symphonia::ac3_oxideav_characterization_tests`) is itself `cfg(test)`. Without it
// this module compiles alone — with nothing calling it — whenever `clip-sync` is built as a
// *dependency* with these features on, where `cfg(test)` is false. That produced four dead-code
// warnings for helpers that are genuinely used.
#[cfg(all(test, feature = "ac3", feature = "ffmpeg-tests"))]
pub mod ac3_pcm_analysis;
pub mod audio_fixtures;
pub mod ffmpeg_util;
