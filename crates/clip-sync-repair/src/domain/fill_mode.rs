use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// How gap-fill placement is chosen after structure match.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum FillMode {
    /// Legacy: score waveform at the structure winner; threshold gates and shortcuts apply.
    Gate,
    /// Search waveform seams around the structure winner; pass when `min(pre, post)` meets floor.
    #[default]
    Fit,
}
