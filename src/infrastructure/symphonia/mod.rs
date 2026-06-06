mod codec_registry;
mod duration;
mod extract;
pub mod error_mapping;
mod probe;
mod session;

#[cfg(feature = "he-aac")]
mod fdk_aac;

#[cfg(test)]
mod media_reader_tests;

pub use session::SymphoniaMediaReader;
