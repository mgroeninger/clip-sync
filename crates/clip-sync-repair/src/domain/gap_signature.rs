//! Gap context signatures: bool structure vs energy envelope.

use serde::{Deserialize, Serialize};

use crate::domain::gap_energy::{
    build_gap_energy_signature, score_post_energy_match, score_pre_energy_match,
    EnergyTimeline, GapEnergySignature,
};
use crate::domain::gap_structure::{
    self, build_gap_context_signature, score_post_match, score_pre_match, ActivityTimeline,
    GapContextSignature, StructureMatchParams,
};

/// Which structure representation to use for gap fill search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapSignatureMode {
    /// Active/silent bins around the gap (`fill_mode = fit` structure tier).
    #[default]
    Bool,
    /// RMS energy envelope Pearson match (fit path).
    Energy,
    /// Use energy when the bool signature is empty; otherwise bool.
    Auto,
}

/// Active/silent bool bins or gated energy envelope around a gap.
#[derive(Debug, Clone, PartialEq)]
pub enum GapSignature {
    Bool(GapContextSignature),
    Energy(GapEnergySignature),
}

impl GapSignature {
    pub fn is_empty(&self) -> bool {
        match self {
            GapSignature::Bool(sig) => sig.pre_bins.is_empty() || sig.post_bins.is_empty(),
            GapSignature::Energy(sig) => sig.pre_energy.is_empty() || sig.post_energy.is_empty(),
        }
    }

    pub fn mode_label(&self) -> &'static str {
        match self {
            GapSignature::Bool(_) => "bool",
            GapSignature::Energy(_) => "energy",
        }
    }
}

/// Precomputed B haystack timeline for structure scoring.
pub(crate) enum StructureTimeline<'a> {
    Bool(&'a ActivityTimeline),
    Energy(&'a EnergyTimeline),
}

pub fn build_gap_signature(
    samples: &[i16],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    context_frames: usize,
    params: &StructureMatchParams,
    mode: GapSignatureMode,
) -> GapSignature {
    match effective_mode(samples, channels, gap_start_frame, gap_end_frame, context_frames, params, mode)
    {
        GapSignatureMode::Bool => GapSignature::Bool(build_gap_context_signature(
            samples,
            channels,
            gap_start_frame,
            gap_end_frame,
            context_frames,
            params,
        )),
        GapSignatureMode::Energy | GapSignatureMode::Auto => GapSignature::Energy(
            build_gap_energy_signature(
                samples,
                channels,
                gap_start_frame,
                gap_end_frame,
                context_frames,
                params,
            ),
        ),
    }
}

fn effective_mode(
    samples: &[i16],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    context_frames: usize,
    params: &StructureMatchParams,
    mode: GapSignatureMode,
) -> GapSignatureMode {
    if mode != GapSignatureMode::Auto {
        return mode;
    }
    let energy = build_gap_energy_signature(
        samples,
        channels,
        gap_start_frame,
        gap_end_frame,
        context_frames,
        params,
    );
    let pre_max = energy.pre_energy.iter().copied().fold(0.0f32, f32::max);
    let post_max = energy.post_energy.iter().copied().fold(0.0f32, f32::max);
    if pre_max <= f32::EPSILON || post_max <= f32::EPSILON {
        tracing::debug!("gap signature auto: flat envelope → bool");
        GapSignatureMode::Bool
    } else {
        GapSignatureMode::Energy
    }
}

pub(crate) fn score_pre_for_signature(
    signature: &GapSignature,
    timeline: &StructureTimeline<'_>,
    fill_start: usize,
    params: &StructureMatchParams,
) -> f64 {
    match (signature, timeline) {
        (GapSignature::Bool(sig), StructureTimeline::Bool(timeline)) => {
            score_pre_match(sig, timeline, fill_start, params)
        }
        (GapSignature::Energy(sig), StructureTimeline::Energy(timeline)) => {
            score_pre_energy_match(sig, timeline, fill_start, params)
        }
        _ => f64::NEG_INFINITY,
    }
}

pub(crate) fn score_post_for_signature(
    signature: &GapSignature,
    timeline: &StructureTimeline<'_>,
    fill_end: usize,
    params: &StructureMatchParams,
) -> f64 {
    match (signature, timeline) {
        (GapSignature::Bool(sig), StructureTimeline::Bool(timeline)) => {
            score_post_match(sig, timeline, fill_end, params)
        }
        (GapSignature::Energy(sig), StructureTimeline::Energy(timeline)) => {
            score_post_energy_match(sig, timeline, fill_end, params)
        }
        _ => f64::NEG_INFINITY,
    }
}

pub(crate) fn snap_fill_to_gap(
    alignment: &mut crate::domain::policies::FillAlignment,
    signature: &GapSignature,
    timeline: &StructureTimeline<'_>,
    params: &StructureMatchParams,
) {
    match (signature, timeline) {
        (GapSignature::Bool(sig), StructureTimeline::Bool(timeline)) => {
            gap_structure::snap_structure_fill_to_gap(alignment, sig, timeline, params);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_params() -> StructureMatchParams {
        StructureMatchParams {
            gap_frames: 20,
            bin_frames: 10,
            search_radius_frames: 10,
            fill_length_slack_frames: 2,
            max_fine_adjustment_frames: 2,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        }
    }

    #[test]
    fn auto_falls_back_to_bool_on_flat_envelope() {
        let samples = vec![0i16; 400];
        let sig = build_gap_signature(&samples, 1, 100, 120, 50, &flat_params(), GapSignatureMode::Auto);
        assert!(matches!(sig, GapSignature::Bool(_)));
    }
}
