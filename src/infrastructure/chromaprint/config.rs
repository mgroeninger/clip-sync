use rusty_chromaprint::Configuration;

/// Chromaprint algorithm configuration used for both fingerprinting and matching.
pub fn default_configuration() -> Configuration {
    Configuration::preset_test2()
}

/// Match scores below this threshold indicate similar segments (see `rusty-chromaprint`).
pub const MATCH_SCORE_THRESHOLD: f64 = 10.0;
