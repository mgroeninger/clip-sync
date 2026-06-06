mod codec_registry;
pub mod error_mapping;
pub mod media_reader;

#[cfg(feature = "he-aac")]
mod fdk_aac;

pub use media_reader::SymphoniaMediaReader;
