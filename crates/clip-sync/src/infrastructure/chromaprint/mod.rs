pub mod aligner;
pub mod config;
pub mod fingerprinter;
pub(crate) mod matching;
pub(crate) mod repetition;
#[cfg(test)]
mod repetition_spike;

pub use aligner::ChromaprintAligner;
pub use fingerprinter::ChromaprintFingerprinter;
