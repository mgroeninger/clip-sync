//! Production-scale energy corpus helpers (`docs/dev/archive/energy-corpus-plan.md`).

use std::path::Path;

use clip_sync::SymphoniaMediaReader;

use crate::test_align::{oracle_injected_alignment, zero_offset_alignment, NeverCalledAligner};
use crate::NoOpProgressReporter;

use crate::energy_signature_fixtures::{
    gap_report_times, write_fixture_wavs, EnergySignatureFixture,
};
use clip_sync_repair::application::align_bridge::scan_alignment_from_result;
use clip_sync_repair::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use clip_sync_repair::application::PatchAudioRequest;
use clip_sync_repair::domain::{GapReport, GapSignatureMode};
use clip_sync_repair::infrastructure::config::RepairConfig;

/// Minimum fixture length for a valid production patch matrix context (lead-in + gap + tail).
///
/// Layout: `anchor + min_gap + context + border + margin` where
/// `anchor = context + border + margin` (see `gap_anchor_secs`).
pub fn min_total_secs_for_signature_context(context_secs: f64) -> f64 {
    const BORDER: f64 = 10.0;
    const MARGIN: f64 = 1.0;
    const MIN_GAP: f64 = 1.0;
    let anchor = context_secs + BORDER + MARGIN;
    anchor + MIN_GAP + context_secs + BORDER + MARGIN
}

/// Production matrix contexts valid for a fixture of `total_secs` (skips e.g. context 30 @ 60 s).
pub fn production_matrix_contexts(total_secs: f64) -> Vec<f64> {
    [3.0, 10.0, 30.0]
        .into_iter()
        .filter(|&context| {
            total_secs + f64::EPSILON >= min_total_secs_for_signature_context(context)
        })
        .collect()
}

pub fn gap_report_from_energy_fixture(temp: &Path, fixture: &EnergySignatureFixture) -> GapReport {
    use clip_sync_repair::domain::gap::Gap;

    let (path_a, path_b) = write_fixture_wavs(temp, fixture);
    let (a_start, a_end, b_start, b_end, total_secs) = gap_report_times(fixture);
    GapReport {
        video_a: path_a,
        video_b: path_b,
        track_compatibility: Some(clip_sync_repair::domain::TrackCompatibility {
            a_channels: fixture.channels as u16,
            b_channels: fixture.channels as u16,
            a_sample_rate: fixture.sample_rate,
            b_sample_rate: fixture.sample_rate,
            channels_match: true,
            rate_match: true,
            verdict: clip_sync_repair::domain::CompatibilityVerdict::Compatible,
        }),
        alignment: scan_alignment_from_result(&oracle_injected_alignment(total_secs)),
        gaps: vec![Gap {
            video_a_start_secs: a_start,
            video_a_end_secs: a_end,
            video_b_start_secs: Some(b_start),
            video_b_end_secs: Some(b_end),
            b_has_energy: true,
        }],
        gap_equivalence: Vec::new(),
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        scan_block_ms: 250,
        silence_peak_fraction: 0.01,
        limit_fill_to_mapped_region: true,
        b_scanned_end_secs: None,
        b_scan_truncated: false,
        audio_timeline_skew: None,
    }
}

/// Oracle `GapReport` for real-media floor calibration pairs.
///
/// Refines A gap edges on decoded mono PCM; B fill times match A (same-master, zero offset).
pub fn gap_report_from_floor_oracle(
    path_a: &Path,
    path_b: &Path,
    decoded_a_mono: &[f32],
    sample_rate: u32,
    gap_start_frame: usize,
    gap_end_frame: usize,
) -> GapReport {
    use clip_sync_repair::domain::gap::Gap;
    use clip_sync_repair::domain::policies::refine_gap_frames;

    const PATCH_GAP_EDGE_REFINE_SECS: f64 = 0.75;
    let max_refine_frames = (PATCH_GAP_EDGE_REFINE_SECS * sample_rate as f64).round() as usize;
    let refined_a = refine_gap_frames(
        decoded_a_mono,
        1,
        gap_start_frame,
        gap_end_frame,
        0.01,
        0.0,
        max_refine_frames,
    );
    let rate = sample_rate as f64;
    let a_start = refined_a.start_frame as f64 / rate;
    let a_end = refined_a.end_frame as f64 / rate;
    let total_secs = decoded_a_mono.len() as f64 / rate;

    GapReport {
        video_a: path_a.to_path_buf(),
        video_b: path_b.to_path_buf(),
        track_compatibility: Some(clip_sync_repair::domain::TrackCompatibility {
            a_channels: 1,
            b_channels: 1,
            a_sample_rate: sample_rate,
            b_sample_rate: sample_rate,
            channels_match: true,
            rate_match: true,
            verdict: clip_sync_repair::domain::CompatibilityVerdict::Compatible,
        }),
        alignment: scan_alignment_from_result(&oracle_injected_alignment(total_secs)),
        gaps: vec![Gap {
            video_a_start_secs: a_start,
            video_a_end_secs: a_end,
            video_b_start_secs: Some(a_start),
            video_b_end_secs: Some(a_end),
            b_has_energy: true,
        }],
        gap_equivalence: Vec::new(),
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        scan_block_ms: 250,
        silence_peak_fraction: 0.01,
        limit_fill_to_mapped_region: true,
        b_scanned_end_secs: None,
        b_scan_truncated: false,
        audio_timeline_skew: None,
    }
}

/// Repair config for signature matrix runs: production geometry with acceptance-style
/// structure isolation (decoy fixtures need zero nominal bias and structure-heavy fit).
pub fn production_repair_config(
    gap_signature_mode: GapSignatureMode,
    gap_signature_context_secs: f64,
) -> RepairConfig {
    RepairConfig {
        gap_signature_mode,
        gap_signature_context_secs,
        fill_border_search_secs: 10.0,
        fill_align_margin_secs: 1.0,
        fill_fit_structure_weight: 1.0,
        fill_fit_waveform_weight: 0.0,
        fill_fit_nominal_bias_scale: 0.0,
        fill_fit_energy_nominal_bias_scale: 0.0, // coupling-neutral: matches base
        fill_fit_late_start_penalty_scale: 0.0,
        min_structure_match_score: 0.0,
        min_fill_correlation: 0.0,
        fill_absolute_floor: -0.05,
        gap_end_extend_on_post_seam_fail: true,
        gap_start_extend_on_pre_seam_fail: true,
        ..Default::default()
    }
}

/// Variant of [`production_repair_config`] with **production fit weights**
/// (`structure 0.35 / waveform 0.65`) and production nominal bias, instead of structure
/// isolation. Used to test whether the waveform tier — rather than structure +
/// `snap_fill_to_gap` — changes mode discrimination on decoy fixtures (EC-6 follow-up).
/// Gating floors stay corpus-relaxed (`min_fill_correlation = 0`, `fill_absolute_floor =
/// -0.05`) so we isolate the weights/bias variable from seam-gate rejection.
pub fn production_fit_weights_config(
    gap_signature_mode: GapSignatureMode,
    gap_signature_context_secs: f64,
) -> RepairConfig {
    RepairConfig {
        fill_fit_structure_weight: 0.35,
        fill_fit_waveform_weight: 0.65,
        fill_fit_nominal_bias_scale: 1.0,
        fill_fit_energy_nominal_bias_scale: 1.0, // coupling-neutral: the masked prodw baseline
        ..production_repair_config(gap_signature_mode, gap_signature_context_secs)
    }
}

/// Shipped mode-coupled behavior: production fit weights with the **base** nominal bias for bool
/// (`1.0`) but the **lowered** energy bias (`0.25`). Demonstrates the energy/bool split surviving
/// at production weights once the coupling is in effect (EC-6 follow-up).
pub fn production_mode_coupled_config(
    gap_signature_mode: GapSignatureMode,
    gap_signature_context_secs: f64,
) -> RepairConfig {
    RepairConfig {
        fill_fit_structure_weight: 0.35,
        fill_fit_waveform_weight: 0.65,
        fill_fit_nominal_bias_scale: 1.0,
        fill_fit_energy_nominal_bias_scale: 0.25,
        ..production_repair_config(gap_signature_mode, gap_signature_context_secs)
    }
}

/// [`production_repair_config`] with explicit fit weights + nominal bias, for the EC-6 weight
/// sweep (find where the energy/bool split survives between structure isolation and production
/// fit weights). Gating floors stay corpus-relaxed so we isolate weights/bias from seam-gate
/// rejection.
pub fn production_weight_sweep_config(
    gap_signature_mode: GapSignatureMode,
    gap_signature_context_secs: f64,
    structure_weight: f64,
    waveform_weight: f64,
    nominal_bias_scale: f64,
) -> RepairConfig {
    RepairConfig {
        fill_fit_structure_weight: structure_weight,
        fill_fit_waveform_weight: waveform_weight,
        fill_fit_nominal_bias_scale: nominal_bias_scale,
        // Sweep controls the actual bias for both modes (coupling-neutral).
        fill_fit_energy_nominal_bias_scale: nominal_bias_scale,
        ..production_repair_config(gap_signature_mode, gap_signature_context_secs)
    }
}

/// Scan A/B WAVs written from a synthetic fixture (production scan defaults).
pub fn scan_gaps_for_fixture(fixture: &EnergySignatureFixture, temp: &Path) -> GapReport {
    let (path_a, path_b) = write_fixture_wavs(temp, fixture);
    let (_, _, _, _, total_secs) = gap_report_times(fixture);
    let repair = RepairConfig::default();
    let request = ScanGapsRequest {
        video_a: path_a,
        video_b: path_b,
        align: Default::default(),
        decode_chunk_secs: repair.decode_chunk_secs,
        scan_block_secs: repair.scan_block_secs(),
        silence_peak_fraction: repair.silence_peak_fraction,
        absolute_silence_rms: repair.absolute_silence_rms,
        silence_hold_blocks: repair.silence_hold_blocks(),
        min_gap_secs: repair.min_gap_secs(),
        scan_both: false,
        gap_offset_tolerance_secs: repair.gap_offset_tolerance_secs,
        limit_fill_to_mapped_region: true,
    };
    let media_reader = SymphoniaMediaReader;
    let progress = NoOpProgressReporter;
    let scan = ScanGaps::new(&media_reader, &progress, &NeverCalledAligner);
    let mut report = scan
        .scan_after_alignment(request, zero_offset_alignment(total_secs))
        .expect("scan energy fixture WAV")
        .report;
    inject_oracle_alignment(&mut report, total_secs);
    report
}

pub fn patch_request_from_repair(report: GapReport, repair: &RepairConfig) -> PatchAudioRequest {
    repair
        .patch_settings()
        .into_request(report)
        .expect("default All gap selection")
}

/// Patch geometry params mirroring [`production_repair_config`] for haystack diagnostics.
pub fn production_geometry_params(
    repair: &RepairConfig,
) -> crate::patch_geometry_preview::PatchGeometryParams {
    use crate::patch_geometry_preview::PatchGeometryParams;
    use clip_sync_repair::domain::FillMode;

    PatchGeometryParams {
        fill_border_search_secs: repair.fill_border_search_secs,
        fill_align_margin_secs: repair.fill_align_margin_secs,
        gap_signature_context_secs: repair.gap_signature_context_secs,
        fill_length_slack_secs: repair.fill_length_slack_secs,
        fill_extract_tail_slack_secs: repair.fill_extract_tail_slack_secs,
        gap_end_extend_max_ms: repair.gap_end_extend_max_ms,
        gap_end_extend_on_post_seam_fail: repair.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: repair.gap_start_extend_on_pre_seam_fail,
        fit_boundary_search: repair.fit_boundary_search,
        fill_offset_mode: repair.fill_offset_mode,
        fill_mode_fit: repair.fill_mode == FillMode::Fit,
        gap_signature_bin_ms: repair.gap_signature_bin_ms as u32,
    }
}

/// Baseline throat Pearson at the **nominal** B map (no unified slide) — W5 oracle preflight.
pub fn oracle_nominal_throat_pearson(
    fixture: &EnergySignatureFixture,
    repair: &RepairConfig,
) -> (f64, f64) {
    use crate::patch_geometry_preview::{preview_patch_geometry, slice_b_interleaved};
    use clip_sync_repair::domain::gap_fill_fit::WaveformSeamContext;
    use clip_sync_repair::domain::pcm::{interleaved_to_channels, interleaved_to_mono};
    use clip_sync_repair::domain::policies::{
        border_templates_for_gap, border_templates_per_channel_for_gap, fill_seam_correlations,
        GapBorderSpec, SeamPlacement, SeamTemplates,
    };

    let (a_start, a_end, b_start, b_end, total_secs) = gap_report_times(fixture);
    let alignment = scan_alignment_from_result(&oracle_injected_alignment(total_secs));
    let preview = preview_patch_geometry(
        fixture,
        &alignment,
        a_start,
        a_end,
        b_start,
        b_end,
        &production_geometry_params(repair),
    );
    let ch = fixture.channels.max(1);
    let gap_frames = preview
        .refined
        .end_frame
        .saturating_sub(preview.refined.start_frame);
    let border_frames = preview.patch_structure_params.bin_frames * 3;
    let border_spec = GapBorderSpec {
        gap_start_frame: preview.refined.start_frame,
        gap_end_frame: preview.refined.end_frame,
        border_frames,
        border_standoff_frames: 0,
        silence_peak_fraction: preview.patch_structure_params.silence_peak_fraction,
        absolute_rms_floor: preview.patch_structure_params.absolute_silence_rms,
    };
    let haystack = slice_b_interleaved(
        &fixture.b_samples,
        ch,
        fixture.sample_rate,
        preview.b_extract_start_secs,
        preview.b_extract_end_secs,
    );
    let (a_pre, a_post) = border_templates_for_gap(&fixture.a_samples, ch, &border_spec);
    let (a_pre_ch, a_post_ch) =
        border_templates_per_channel_for_gap(&fixture.a_samples, ch, &border_spec);
    let b_mono = interleaved_to_mono(&haystack, ch);
    let b_ch = interleaved_to_channels(&haystack, ch);
    let pre_window = a_pre.len().max(1);
    let post_window = a_post.len().max(1);
    let templates = SeamTemplates {
        a_pre: &a_pre,
        a_post: &a_post,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        b_mono: &b_mono,
        b_ch: &b_ch,
    };
    let _waveform = WaveformSeamContext {
        templates: &templates,
        gap_frames,
        pre_window,
        post_window,
        b_total_frames: b_mono.len(),
        repeat_window_frames: preview.patch_structure_params.bin_frames.max(1),
        repeat_penalty_weight: 0.0,
    };
    fill_seam_correlations(
        &templates,
        SeamPlacement {
            start: preview.offset_nominal_start,
            gap_frames,
            pre_window,
            post_window,
        },
    )
}

/// Baseline throat Pearson from unified haystack search (best B slide at scan throat).
pub fn oracle_baseline_throat_pearson(
    fixture: &EnergySignatureFixture,
    repair: &RepairConfig,
) -> (f64, f64) {
    oracle_baseline_throat_pearson_opt(fixture, repair)
        .expect("oracle baseline unified match on haystack")
}

/// Non-panicking baseline throat Pearson: `None` when the unified haystack match fails (degenerate
/// geometry — e.g. an empty extract window for some discovery cells). Used by the W5 grid sweep so
/// one bad cell does not abort the whole map.
pub fn oracle_baseline_throat_pearson_opt(
    fixture: &EnergySignatureFixture,
    repair: &RepairConfig,
) -> Option<(f64, f64)> {
    use crate::energy_signature_fixtures::gap_report_times;
    use crate::patch_geometry_preview::preview_patch_geometry;
    use clip_sync_repair::domain::gap_fill_fit::UnifiedFitWeights;

    let (a_start, a_end, b_start, b_end, total_secs) = gap_report_times(fixture);
    let alignment = scan_alignment_from_result(&oracle_injected_alignment(total_secs));
    let preview = preview_patch_geometry(
        fixture,
        &alignment,
        a_start,
        a_end,
        b_start,
        b_end,
        &production_geometry_params(repair),
    );
    let nominal_bias_scale = match repair.gap_signature_mode {
        GapSignatureMode::Energy => repair.fill_fit_energy_nominal_bias_scale,
        GapSignatureMode::Bool | GapSignatureMode::Auto => repair.fill_fit_nominal_bias_scale,
    };
    let weights = UnifiedFitWeights {
        structure_weight: repair.fill_fit_structure_weight,
        waveform_weight: repair.fill_fit_waveform_weight,
        nominal_bias_scale,
        late_start_penalty_scale: repair.fill_fit_late_start_penalty_scale,
    };
    let matched = preview.unified_match_on_haystack(fixture, repair.gap_signature_mode, weights)?;
    Some((
        matched.alignment.pre_correlation,
        matched.alignment.post_correlation,
    ))
}

/// Repair knobs for the W5 anchor-rescue oracle (A6).
pub fn w5_anchor_rescue_repair(
    anchor_seam_mode: clip_sync_repair::domain::AnchorSeamMode,
    fill_border_search_secs: f64,
) -> RepairConfig {
    let mut repair = production_fit_weights_config(GapSignatureMode::Energy, 3.0);
    repair.fill_mode = clip_sync_repair::domain::FillMode::Fit;
    repair.fit_boundary_search = clip_sync_repair::domain::FitBoundarySearch::BaselineOnly;
    repair.gap_signature_mode = GapSignatureMode::Energy;
    repair.anchor_seam_mode = anchor_seam_mode;
    repair.residual_gate = clip_sync_repair::domain::ResidualGateMode::Off;
    repair.min_fill_correlation = 0.35;
    repair.fill_marginal_margin = 0.10;
    repair.strong_structure_trust = 0.90;
    repair.normalize_fill = false;
    repair.fill_length_slack_secs = 0.05;
    repair.fill_extract_tail_slack_secs = 0.05;
    repair.min_border_discovery_secs = 0.25;
    repair.border_standoff_secs = 0.0;
    repair.fill_border_search_secs = fill_border_search_secs;
    repair.gap_end_extend_on_post_seam_fail = false;
    repair.gap_start_extend_on_pre_seam_fail = false;
    repair.gap_end_extend_max_ms = 0;
    repair
}

/// Replace scan alignment with I1-style oracle injection (start + end clips, zero drift).
pub fn inject_oracle_alignment(report: &mut GapReport, total_secs: f64) {
    report.alignment = scan_alignment_from_result(&oracle_injected_alignment(total_secs));
}

/// After scan, remap B gap bounds using fixture nominal refine (matches [`gap_report_times`] B leg).
pub fn normalize_scan_gap_b_mapping(report: &mut GapReport, fixture: &EnergySignatureFixture) {
    let (_, _, b_start, b_end, total_secs) = gap_report_times(fixture);
    inject_oracle_alignment(report, total_secs);
    for gap in &mut report.gaps {
        if gap.is_fillable() {
            gap.video_b_start_secs = Some(b_start);
            gap.video_b_end_secs = Some(b_end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_matrix_contexts_skip_thirty_on_sixty_sec_fixture() {
        let contexts = production_matrix_contexts(60.0);
        assert_eq!(contexts, vec![3.0, 10.0]);
        assert_eq!(production_matrix_contexts(120.0), vec![3.0, 10.0, 30.0]);
    }
}
