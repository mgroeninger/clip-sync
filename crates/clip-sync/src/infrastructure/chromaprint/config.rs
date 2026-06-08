use rusty_chromaprint::Configuration;

use crate::application::config::ChromaprintPreset;

/// Chromaprint algorithm configuration used for both fingerprinting and matching.
pub fn configuration_for_preset(preset: ChromaprintPreset) -> Configuration {
    match preset {
        ChromaprintPreset::Test2 => Configuration::preset_test2(),
        ChromaprintPreset::Test3 => Configuration::preset_test3(),
        ChromaprintPreset::Test4 => Configuration::preset_test4(),
        ChromaprintPreset::Test5 => Configuration::preset_test5(),
    }
}

/// Match scores below this threshold indicate similar segments (see `rusty-chromaprint`).
pub const MATCH_SCORE_THRESHOLD: f64 = 10.0;

/// Minimum matching segment length (items) for full length confidence.
pub const MIN_RELIABLE_ITEMS: usize = 40;
