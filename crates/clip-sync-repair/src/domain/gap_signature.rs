//! Gap context signatures: bool structure vs energy envelope.

use std::str::FromStr;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapSignatureMode {
    /// Active/silent bins around the gap (`fill_mode = fit` structure tier).
    Bool,
    /// RMS energy envelope Pearson match (fit path).
    Energy,
    /// Energy when pre/post envelope has contour; otherwise bool (flat / near-silence).
    #[default]
    Auto,
}

impl FromStr for GapSignatureMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bool" => Ok(GapSignatureMode::Bool),
            "energy" => Ok(GapSignatureMode::Energy),
            "auto" => Ok(GapSignatureMode::Auto),
            _ => Err(format!(
                "invalid gap signature mode: {s} (expected bool, energy, or auto)"
            )),
        }
    }
}

/// Active/silent bool bins or gated energy envelope around a gap.
#[derive(Debug, Clone, PartialEq)]
pub enum GapSignature {
    Bool(GapContextSignature),
    Energy(GapEnergySignature),
}

impl GapSignature {
    /// True when the resolved signature has structure an anchor seam can latch onto (`auto` trigger).
    pub(crate) fn has_anchor_seam_contour(&self) -> bool {
        match self {
            GapSignature::Energy(sig) => {
                !energy_envelope_is_flat(&sig.pre_energy)
                    || !energy_envelope_is_flat(&sig.post_energy)
            }
            GapSignature::Bool(sig) => {
                bool_bins_have_anchor_contour(&sig.pre_bins, &sig.post_bins)
            }
        }
    }

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
    samples: &[f32],
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

/// Activity transitions or mixed active/silent context (not flat silence or uniform activity).
fn bool_bins_have_anchor_contour(pre: &[bool], post: &[bool]) -> bool {
    let side_has_transition = |bins: &[bool]| bins.windows(2).any(|w| w[0] != w[1]);
    if side_has_transition(pre) || side_has_transition(post) {
        return true;
    }
    let mut any_active = false;
    let mut any_silent = false;
    for &active in pre.iter().chain(post.iter()) {
        any_active |= active;
        any_silent |= !active;
        if any_active && any_silent {
            return true;
        }
    }
    false
}

pub(crate) fn energy_envelope_is_flat(bins: &[f32]) -> bool {
    if bins.is_empty() {
        return true;
    }
    let peak = bins.iter().copied().fold(0.0f32, f32::max);
    if peak <= f32::EPSILON {
        return true;
    }
    let mut min = f32::MAX;
    let mut max = 0.0f32;
    for &v in bins {
        let n = v / peak;
        min = min.min(n);
        max = max.max(n);
    }
    (max - min) <= 0.05
}

fn effective_mode(
    samples: &[f32],
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
    if energy_envelope_is_flat(&energy.pre_energy) || energy_envelope_is_flat(&energy.post_energy) {
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
    if let (GapSignature::Bool(sig), StructureTimeline::Bool(timeline)) = (signature, timeline) {
        gap_structure::snap_structure_fill_to_gap(alignment, sig, timeline, params);
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
        let samples = vec![0.0f32; 400];
        let sig = build_gap_signature(&samples, 1, 100, 120, 50, &flat_params(), GapSignatureMode::Auto);
        assert!(matches!(sig, GapSignature::Bool(_)));
    }

    #[test]
    fn auto_falls_back_to_bool_on_steady_drone() {
        let mut samples = vec![6_000.0_f32 / 32767.0; 400];
        for sample in samples.iter_mut().take(120).skip(100) {
            *sample = 0.0;
        }
        let sig = build_gap_signature(&samples, 1, 100, 120, 50, &flat_params(), GapSignatureMode::Auto);
        assert!(matches!(sig, GapSignature::Bool(_)));
    }

    #[test]
    fn flat_bool_signature_has_no_anchor_contour() {
        let samples = vec![0.0f32; 400];
        let sig = build_gap_signature(&samples, 1, 100, 120, 50, &flat_params(), GapSignatureMode::Bool);
        assert!(!sig.has_anchor_seam_contour());
    }

    #[test]
    fn bool_onset_signature_has_anchor_contour() {
        let mut samples = vec![0.0f32; 400];
        for sample in samples.iter_mut().skip(130) {
            *sample = 8_000.0_f32 / 32767.0;
        }
        let sig = build_gap_signature(&samples, 1, 100, 120, 50, &flat_params(), GapSignatureMode::Bool);
        assert!(sig.has_anchor_seam_contour());
    }

    #[test]
    fn speech_peaks_bool_signature_has_anchor_contour() {
        use crate::test_support::energy_signature_fixtures::build_speech_peaks_offset_from_throat;

        let fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
        let sig = fixture.signature(GapSignatureMode::Bool);
        assert!(
            sig.has_anchor_seam_contour(),
            "speech peaks fixture should expose bool activity contour for anchor auto"
        );
    }
}
