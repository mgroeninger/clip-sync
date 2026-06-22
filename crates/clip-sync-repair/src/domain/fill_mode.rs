use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// How gap-fill placement is chosen after structure match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillMode {
    /// Legacy: score waveform at the structure winner; threshold gates and shortcuts apply.
    Gate,
    /// Search waveform seams around the structure winner; pass when `min(pre, post)` meets floor.
    #[default]
    Fit,
}

impl FromStr for FillMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gate" => Ok(FillMode::Gate),
            "fit" => Ok(FillMode::Fit),
            _ => Err(format!("invalid fill mode: {s} (expected gate or fit)")),
        }
    }
}
