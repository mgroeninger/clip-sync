mod codec_registry;
mod duration;
mod extract;
mod extract_loop;
pub mod error_mapping;
mod probe;
mod session;

#[cfg(feature = "he-aac")]
mod fdk_aac;

#[cfg(feature = "ac3")]
mod oxideav_ac3;

#[cfg(test)]
mod media_reader_tests;

#[cfg(test)]
mod extract_window_regression;

#[cfg(all(test, feature = "ac3", feature = "ffmpeg-tests"))]
mod ac3_oxideav_characterization_tests;

pub use session::SymphoniaMediaReader;
