use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// How the residual/floor headroom gate composes with Pearson waveform tiers (fit mode only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidualGateMode {
    /// No residual measurement for gating; zero behavior change unless `measure_residual` or debug.
    Off,
    /// Anti-echo veto when informative floor and headroom exceed margin.
    #[default]
    Veto,
    /// Veto plus false-skip rescue into marginal when headroom is clean but Pearson is in the dead zone.
    VetoRescue,
}

impl ResidualGateMode {
    pub fn rescue_enabled(self) -> bool {
        matches!(self, ResidualGateMode::VetoRescue)
    }

    pub fn is_active(self) -> bool {
        !matches!(self, ResidualGateMode::Off)
    }
}

impl FromStr for ResidualGateMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(ResidualGateMode::Off),
            "veto" => Ok(ResidualGateMode::Veto),
            "veto_rescue" => Ok(ResidualGateMode::VetoRescue),
            _ => Err(format!(
                "invalid residual gate mode: {s} (expected off, veto, or veto_rescue)"
            )),
        }
    }
}

/// Default unified seam/floor lag search radius (seconds).
pub const DEFAULT_RESIDUAL_LAG_SECS: f64 = 0.010;

/// Default headroom margin for residual veto/rescue (dB).
pub const DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB: f64 = 6.0;

/// Integer sample lag radius from configured seconds and sample rate.
pub fn residual_max_lag_frames(sample_rate: u32, residual_lag_secs: f64) -> i64 {
    if sample_rate == 0 || residual_lag_secs <= 0.0 {
        return 0;
    }
    (residual_lag_secs * f64::from(sample_rate)).round().max(0.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_residual_gate_is_veto() {
        assert_eq!(ResidualGateMode::default(), ResidualGateMode::Veto);
    }
}
