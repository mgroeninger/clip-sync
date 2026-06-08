pub mod aligner;
pub mod config;
pub mod fingerprinter;
#[cfg(test)]
mod repetition_spike;

pub use aligner::ChromaprintAligner;
pub use fingerprinter::ChromaprintFingerprinter;
