//! Crate-internal test support that any layer may use.
//!
//! Unlike `application::testing` (application-layer fakes and fixtures), helpers here sit
//! outside the layer stack, so infrastructure tests can depend on them without pointing a
//! dependency arrow at the application module.

pub mod audio_fixtures;
pub mod ffmpeg_util;
