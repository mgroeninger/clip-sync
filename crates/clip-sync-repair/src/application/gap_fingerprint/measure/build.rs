//! `build` slice of the gap_fingerprint measure folder split.

use crate::application::gap_fingerprint::contract::*;
use crate::application::gap_fingerprint::schema::*;

use crate::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorSeamParams, AnchorSource,
};
use crate::domain::gap_energy::energy_bins;
use crate::domain::gap_signature::{build_gap_signature, GapSignatureMode};
use crate::domain::pcm::{interleaved_to_channels, interleaved_to_mono};
use crate::domain::policies::{refine_gap_frames, RefinedGapFrames};

use super::config::*;
use super::lag::*;
use super::pcm::*;
use super::placement::*;

pub(in crate::application::gap_fingerprint::measure) fn classify_bracket_stage(
    structure_pre: f64,
    structure_post: f64,
    seam_pre: f64,
    seam_post: f64,
    cfg: &FingerprintConfig,
) -> Option<FailureStage> {
    if structure_pre.min(structure_post) < cfg.min_structure_match_score {
        return Some(FailureStage::StructureFloor);
    }
    if seam_pre.min(seam_post) < f64::from(cfg.fill_absolute_floor) {
        return Some(FailureStage::WaveformFloor);
    }
    None
}
/// Build a fingerprint for one gap from decoded windows.
///
/// When `skip_baseline_placement` is true (the pre-gate summary pass before the gate overlay in
/// [`characterize_gaps_from_decode`]), the expensive unified `place_on_b` search is omitted — the gate
/// overlay supplies authoritative seams and brackets (D12 §3 step 1).
pub fn build_gap_fingerprint(
    index: usize,
    inputs: &GapInputs<'_>,
    tier: DetailTier,
    skip_baseline_placement: bool,
) -> GapFingerprint {
    let cfg = &inputs.config;
    let ch = inputs.channels.max(1);
    let rate = f64::from(inputs.sample_rate).max(1.0);
    let total_a = inputs.a_samples.len() / ch;

    let bin_frames = ((cfg.gap_signature_bin_ms as f64 / 1000.0) * rate)
        .round()
        .max(1.0) as usize;
    let context_frames = (cfg.gap_signature_context_secs * rate).round() as usize;
    let noise_collar_frames = (cfg.noise_collar_secs * rate).round() as usize;
    let max_refine_frames = (cfg.max_refine_secs * rate).round() as usize;

    let refined = refine_gap_frames(
        inputs.a_samples,
        ch,
        inputs.reported_start_frame.min(total_a),
        inputs.reported_end_frame.min(total_a),
        cfg.silence_peak_fraction,
        cfg.absolute_silence_rms,
        max_refine_frames,
    );
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);

    let geometry = GapGeometry {
        a_start_secs: inputs.reported_start_frame as f64 / rate,
        a_end_secs: inputs.reported_end_frame as f64 / rate,
        a_refined_start_secs: refined.start_frame as f64 / rate,
        a_refined_end_secs: refined.end_frame as f64 / rate,
        duration_secs: gap_frames as f64 / rate,
        b_mapped_start_secs: inputs
            .b_haystack
            .map(|_| refined.start_frame as f64 / rate + inputs.gap_offset_secs),
        b_mapped_end_secs: inputs
            .b_haystack
            .map(|_| refined.end_frame as f64 / rate + inputs.gap_offset_secs),
        fill_offset_secs: inputs.b_haystack.map(|_| inputs.gap_offset_secs),
    };

    // --- intrinsic (A-side) ---
    let pre_start = refined.start_frame.saturating_sub(context_frames);
    let post_end = (refined.end_frame + context_frames).min(total_a);
    let (levels, _context_bins) = level_profile(
        |f, end| mono_rms(inputs.a_samples, ch, f, end),
        LevelProfileSpan {
            gap_start: refined.start_frame,
            gap_end: refined.end_frame,
            context_start: pre_start,
            context_end: post_end,
        },
        bin_frames,
        cfg.gap_signature_bin_ms as u32,
    );

    let collar_start = refined.start_frame.saturating_sub(noise_collar_frames);
    let collar_rms = mono_rms(inputs.a_samples, ch, collar_start, refined.start_frame);
    let collar_peak = mono_peak(inputs.a_samples, ch, collar_start, refined.start_frame);
    let collar_ratio = if collar_peak > 0.0 {
        collar_rms / collar_peak
    } else {
        0.0
    };
    let silence = Some(SilenceProfile {
        collar_rms_peak_ratio: collar_ratio,
        collar_above_relative_floor: collar_ratio >= cfg.silence_peak_fraction,
        silence_peak_fraction: cfg.silence_peak_fraction,
    });

    let sig_params = structure_params_for(cfg, gap_frames.max(1), bin_frames, 0, 0);
    let signature = build_gap_signature(
        inputs.a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        context_frames,
        &sig_params,
        GapSignatureMode::Energy,
    );
    let pre_env = energy_bins(
        inputs.a_samples,
        ch,
        pre_start,
        refined.start_frame,
        bin_frames.max(1),
        cfg.silence_peak_fraction,
        cfg.absolute_silence_rms,
    );
    let post_env = energy_bins(
        inputs.a_samples,
        ch,
        refined.end_frame,
        post_end,
        bin_frames.max(1),
        cfg.silence_peak_fraction,
        cfg.absolute_silence_rms,
    );
    let contour = Some(ContourInfo {
        has_anchor_seam_contour: signature.has_anchor_seam_contour(),
        pre_flatness: flatness(&pre_env),
        post_flatness: flatness(&post_env),
    });

    let bracket_params = AnchorSeamParams {
        context_frames,
        max_anchors_per_side: cfg.max_anchors_per_side,
        max_bracket_frames: (cfg.max_anchor_bracket_secs * rate).round().max(1.0) as usize,
        min_prominence: cfg.anchor_seam_min_prominence,
        structure: structure_params_for(cfg, gap_frames.max(1), bin_frames, 0, 0),
    };
    let candidates = list_anchor_candidates_a(inputs.a_samples, ch, refined, &bracket_params);
    let map_anchor = |c: &crate::domain::gap_anchor_seam::AnchorCandidate| AnchorPoint {
        time_secs: c.frame as f64 / rate,
        source: match c.source {
            AnchorSource::ScanRefined => AnchorSourceKind::ScanRefined,
            AnchorSource::EnergyPeak => AnchorSourceKind::EnergyPeak,
            AnchorSource::BoolTransition => AnchorSourceKind::BoolTransition,
        },
        prominence: c.prominence,
        rms_db: to_db((c.rms.exp() - 1.0).max(0.0)),
    };
    let anchors = Some(AnchorSet {
        pre: candidates.pre.iter().map(map_anchor).collect(),
        post: candidates.post.iter().map(map_anchor).collect(),
    });
    let raw_brackets = list_feasible_anchor_brackets(&candidates, refined, &bracket_params);

    // --- pairwise (B present) ---
    let mut structure = None;
    let mut seams = None;
    let mut lag_editorial = None;
    let mut lag_decision = None;
    // Per-bracket scoring in Full tier; Summary tier only needs baseline structure/seam at `b_mapped`.
    let mut brackets: Vec<BracketInfo> = raw_brackets
        .iter()
        .map(|b| BracketInfo {
            pre_time_secs: b.pre.frame as f64 / rate,
            post_time_secs: b.post.frame as f64 / rate,
            span_secs: (b.post.frame.saturating_sub(b.pre.frame)) as f64 / rate,
            move_frames: b.move_frames,
            structure_pre: None,
            structure_post: None,
            seam_pre: None,
            seam_post: None,
            start_frame: None,
            fill_frames: None,
            failure_stage: None,
            residual_margin_db: None,
        })
        .collect();

    if let Some(b_haystack) = inputs.b_haystack {
        let b_mono = interleaved_to_mono(b_haystack, ch);
        let b_ch = interleaved_to_channels(b_haystack, ch);
        let search_radius_frames =
            ((cfg.fill_border_search_secs.max(cfg.fill_align_margin_secs)) * rate).round() as usize;
        // Each A boundary maps to its own B nominal: a_time + gap_offset, in haystack frame coords.
        let gap_offset_secs = inputs.gap_offset_secs;
        let b_mapped_start = |a_start_frame: usize| -> usize {
            b_mapped_frame_in_haystack(
                a_start_frame,
                rate,
                gap_offset_secs,
                inputs.b_extract_start_secs,
            )
        };

        if !skip_baseline_placement {
            if let Some(base) = place_on_b(&PlaceOnBInput {
                a_samples: inputs.a_samples,
                channels: ch,
                refined,
                b_haystack,
                b_mono: &b_mono,
                b_ch: &b_ch,
                nominal_fill_start: b_mapped_start(refined.start_frame),
                context_frames,
                bin_frames,
                search_radius_frames,
                cfg,
            }) {
                structure = Some(StructureScores {
                    baseline_pre: base.structure_pre,
                    baseline_post: base.structure_post,
                });
                seams = Some(SeamScores {
                    baseline_pre: base.seam_pre,
                    baseline_post: base.seam_post,
                    selected_channels: base.selected_channels,
                    per_channel: base.per_channel,
                    mono_pre: base.mono_pre,
                    mono_post: base.mono_post,
                });
            }
        }

        if tier == DetailTier::Full {
            // Decision-seam lag at `b_mapped` nominal + ±600 ms sweep (ledger A2) — not structure throat.
            lag_decision = Some(lag_at_placement(&LagAtPlacementInput {
                a_samples: inputs.a_samples,
                channels: ch,
                refined,
                b_mono: &b_mono,
                b_ch: &b_ch,
                selected: None,
                start_frame: b_mapped_start(refined.start_frame),
                cfg,
                sample_rate: inputs.sample_rate,
            }));
            // Per-bracket scoring; remember the best-structure energy-peak bracket's placement for lag.
            let mut best: Option<(f64, usize, RefinedGapFrames, Option<usize>)> = None;
            for (i, br) in raw_brackets.iter().enumerate() {
                let refined_b = br.refined;
                if let Some(p) = place_on_b(&PlaceOnBInput {
                    a_samples: inputs.a_samples,
                    channels: ch,
                    refined: refined_b,
                    b_haystack,
                    b_mono: &b_mono,
                    b_ch: &b_ch,
                    nominal_fill_start: b_mapped_start(refined_b.start_frame),
                    context_frames,
                    bin_frames,
                    search_radius_frames,
                    cfg,
                }) {
                    brackets[i].structure_pre = Some(p.structure_pre);
                    brackets[i].structure_post = Some(p.structure_post);
                    brackets[i].seam_pre = Some(p.seam_pre);
                    brackets[i].seam_post = Some(p.seam_post);
                    brackets[i].start_frame = Some(p.start_frame);
                    brackets[i].fill_frames = Some(p.fill_frames);
                    brackets[i].failure_stage = classify_bracket_stage(
                        p.structure_pre,
                        p.structure_post,
                        p.seam_pre,
                        p.seam_post,
                        cfg,
                    );
                    let energy_pair = br.pre.source == AnchorSource::EnergyPeak
                        && br.post.source == AnchorSource::EnergyPeak;
                    let smin = p.structure_pre.min(p.structure_post);
                    if energy_pair && best.is_none_or(|(bs, ..)| smin > bs) {
                        best = Some((
                            smin,
                            p.start_frame,
                            refined_b,
                            p.selected_channels.first().copied(),
                        ));
                    }
                } else {
                    brackets[i].failure_stage = Some(FailureStage::StructureAlign);
                }
            }
            if let Some((_, start_frame, refined_b, selected)) = best {
                lag_editorial = Some(lag_at_placement(&LagAtPlacementInput {
                    a_samples: inputs.a_samples,
                    channels: ch,
                    refined: refined_b,
                    b_mono: &b_mono,
                    b_ch: &b_ch,
                    selected,
                    start_frame,
                    cfg,
                    sample_rate: inputs.sample_rate,
                }));
            }
        }
    }

    if tier != DetailTier::Full {
        brackets.clear();
    }

    GapFingerprint {
        index,
        tier,
        sample_rate: inputs.sample_rate,
        channels: inputs.channels as u16,
        geometry,
        levels,
        silence,
        contour,
        anchors,
        brackets,
        structure,
        seams,
        lag_editorial: stamp(lag_editorial, LAG_EDITORIAL_CONTRACT),
        lag_decision: stamp(lag_decision, LAG_DECISION_CONTRACT),
        residual: None,
        seam_probe: None,
        donor_interior_aligned: None,
        donor_interior_nominal: None,
        b_levels: None,
        splice: None,
        wide_envelope: None,
        splice_dualfit: None,
        post_window_ladder: None,
        outcome: None,
        equivalence_diagnostic: None,
        equivalence_production: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    fn write_speech(buf: &mut [f32], start: usize, end: usize, freq: f64, amp: f32) {
        let n = (end - start) as f64;
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            let t = (f - start) as f64;
            let env = 0.5 - 0.5 * (std::f64::consts::TAU * t / n).cos();
            let s = (std::f64::consts::TAU * freq * t / 48_000.0).sin();
            *slot = (env * s) as f32 * amp;
        }
    }

    fn write_noise(buf: &mut [f32], start: usize, end: usize, seed: u64, amp: f32) {
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            *slot = noise(seed, f) as f32 * amp;
        }
    }
    #[test]
    fn builder_characterizes_noise_collar_gap() {
        let rate = 48_000u32;
        let ch = 1usize;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(5.0);
        let (sp1, n1, gap, n2, sp2) = (
            (secs(0.50), secs(0.85)),
            (secs(0.85), secs(1.85)),
            (secs(1.85), secs(3.35)),
            (secs(3.35), secs(4.35)),
            (secs(4.35), secs(4.70)),
        );
        let mut a = vec![0f32; total];
        let mut b = vec![0f32; total];
        // Same-master speech bursts (identical), decorrelated collars, B fill in the gap.
        write_speech(&mut a, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut a, sp2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, sp2.0, sp2.1, 440.0, 0.079);
        write_noise(&mut a, n1.0, n1.1, 1, 0.0056);
        write_noise(&mut b, n1.0, n1.1, 11, 0.0056);
        write_noise(&mut a, n2.0, n2.1, 3, 0.0056);
        write_noise(&mut b, n2.0, n2.1, 13, 0.0056);
        write_noise(&mut b, gap.0, gap.1, 5, 0.0056);

        let config = FingerprintConfig {
            gap_signature_context_secs: 1.5,
            fill_border_search_secs: 1.0,
            noise_collar_secs: 0.3,
            ..Default::default()
        };
        let inputs = GapInputs {
            a_samples: &a,
            b_haystack: Some(&b),
            channels: ch,
            sample_rate: rate,
            reported_start_frame: gap.0,
            reported_end_frame: gap.1,
            b_extract_start_secs: 0.0,
            gap_offset_secs: 0.0,
            config,
        };
        let fp = build_gap_fingerprint(0, &inputs, DetailTier::Full, false);

        assert!(
            (fp.geometry.duration_secs - 1.5).abs() < 0.05,
            "duration {}",
            fp.geometry.duration_secs
        );
        assert!(
            fp.anchors
                .as_ref()
                .expect("measured anchors")
                .pre
                .iter()
                .any(|p| p.source == AnchorSourceKind::EnergyPeak),
            "expected a pre energy-peak anchor: {:?}",
            fp.anchors.as_ref().map(|a| &a.pre)
        );
        assert!(
            fp.contour
                .as_ref()
                .expect("measured contour")
                .has_anchor_seam_contour,
            "speech bursts give contour"
        );

        let s = fp.structure.expect("structure present with B");
        assert!(
            s.baseline_pre > 0.5,
            "structure aligns at the throat: {}",
            s.baseline_pre
        );
        let seam = fp.seams.expect("seams present with B");
        assert!(
            seam.baseline_pre < 0.2 && seam.baseline_post < 0.2,
            "throat seam collapses in decorrelated noise: pre={} post={}",
            seam.baseline_pre,
            seam.baseline_post
        );
        assert!(
            fp.brackets.iter().any(
                |b| b.seam_pre.is_some_and(|v| v > 0.5) && b.seam_post.is_some_and(|v| v > 0.5)
            ),
            "a speech-anchored bracket should score a strong seam where the throat cannot"
        );
        assert!(
            fp.lag_editorial.is_some(),
            "lag computed at the best speech bracket"
        );
    }
}
