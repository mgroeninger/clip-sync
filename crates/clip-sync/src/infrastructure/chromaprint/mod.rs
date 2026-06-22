pub mod aligner;
pub mod config;
pub mod fingerprinter;
pub(crate) mod matching;
pub(crate) mod repetition;

pub use aligner::ChromaprintAligner;
pub use fingerprinter::ChromaprintFingerprinter;
pub use repetition::ChromaprintClipRepetitionDetector;
