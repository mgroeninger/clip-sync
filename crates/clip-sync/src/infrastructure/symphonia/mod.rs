mod codec_registry;
mod duration;
mod extract;
pub mod error_mapping;
mod probe;
mod session;

#[cfg(feature = "he-aac")]
mod fdk_aac;

#[cfg(feature = "ac3")]
mod oxideav_ac3;

#[cfg(test)]
mod media_reader_tests;

pub use session::SymphoniaMediaReader;
