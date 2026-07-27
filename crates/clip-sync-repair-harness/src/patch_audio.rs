//! Shared patch integration helpers (sine fixtures, `PatchTestOptions`, energy-signature diagnostics).

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::{ClipLabel, ClipMatch, SymphoniaMediaReader};
use clip_sync_repair::application::align_bridge::scan_alignment_from_result;
use clip_sync_repair::application::{
    PatchAudio, PatchAudioRequest, PatchAudioResult, PatchRequestSettings,
};
use clip_sync_repair::domain::gap::{Gap, GapReport};
use clip_sync_repair::domain::{
    CompatibilityVerdict, FillMode, FillOffsetMode, FitBoundarySearch, GapPatchStatus,
    GapSignatureMode, RepairProfile, TrackCompatibility,
};
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    gap_report_times, structure_heavy_weights, structure_slide_secs, EnergySignatureFixture,
};
use clip_sync_repair_fixtures::patch_geometry_preview::{
    preview_patch_geometry, PatchGeometryParams,
};
use hound::{SampleFormat, WavSpec, WavWriter};

pub const SINE_SAMPLE_RATE: u32 = 44_100;
pub const SINE_CHANNELS: u16 = 2;

#[derive(Clone)]
pub struct PatchTestOptions {
    pub disable_structure_trust: bool,
    pub strong_structure_trust: f64,
    pub fill_offset_mode: FillOffsetMode,
    pub short_gap_one_strong_seam_fallback: bool,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
    pub short_gap_mean_correlation_secs: f64,
    pub max_fill_align_adjustment_secs: f64,
    pub partial_structure_waveform_soften: f64,
    pub fill_mode: FillMode,
    pub fill_marginal_margin: f32,
    pub fill_absolute_floor: f32,
    pub fill_fit_structure_weight: f64,
    pub fill_fit_waveform_weight: f64,
    pub fill_fit_nominal_bias_scale: f64,
    pub fill_fit_late_start_penalty_scale: f64,
    pub fill_border_search_secs: f64,
    pub fill_align_margin_secs: f64,
    pub gap_signature_context_secs: f64,
    pub fill_length_slack_secs: f64,
    /// B haystack tail slack; mirrors `RepairConfig.fill_extract_tail_slack_secs`.
    pub fill_extract_tail_slack_secs: f64,
    pub gap_end_extend_max_ms: u64,
    pub gap_end_extend_step_ms: u64,
    pub fill_repeat_penalty_weight: f64,
    /// Match production default (`RepairConfig.fft_seam_search`); lever 1 FFT seam band.
    pub fft_seam_search: bool,
    /// Match production default (`RepairConfig.fft_repeat_band`); lever 1b(b) FFT repeat-window band.
    pub fft_repeat_band: bool,
    pub fill_anchor_retry_marginal: bool,
    pub min_structure_match_score: f32,
    pub min_border_discovery_secs: f64,
    pub gap_signature_mode: GapSignatureMode,
    pub profile: RepairProfile,
    pub fit_boundary_search: FitBoundarySearch,
    /// Match production default (`RepairConfig.dual_fit`).
    pub dual_fit: bool,
}

impl Default for PatchTestOptions {
    fn default() -> Self {
        Self {
            disable_structure_trust: false,
            strong_structure_trust: 0.90,
            fill_offset_mode: FillOffsetMode::Recommended,
            short_gap_one_strong_seam_fallback: true,
            gap_end_extend_on_post_seam_fail: true,
            gap_start_extend_on_pre_seam_fail: true,
            short_gap_mean_correlation_secs: 2.0,
            max_fill_align_adjustment_secs: 1.0,
            partial_structure_waveform_soften: 0.85,
            fill_mode: FillMode::Gate,
            fill_marginal_margin: 0.08,
            fill_absolute_floor: 0.12,
            fill_fit_structure_weight: 0.35,
            fill_fit_waveform_weight: 0.65,
            fill_fit_nominal_bias_scale: 1.0,
            fill_fit_late_start_penalty_scale: 1.0,
            fill_border_search_secs: 30.0,
            fill_align_margin_secs: 1.0,
            gap_signature_context_secs: 3.0,
            fill_length_slack_secs: 1.0,
            fill_extract_tail_slack_secs: 5.0,
            gap_end_extend_max_ms: 500,
            gap_end_extend_step_ms: 20,
            fill_repeat_penalty_weight: 0.4,
            fft_seam_search: clip_sync_repair::infrastructure::config::RepairConfig::default()
                .fft_seam_search,
            fft_repeat_band: clip_sync_repair::infrastructure::config::RepairConfig::default()
                .fft_repeat_band,
            fill_anchor_retry_marginal: false,
            min_structure_match_score: 0.55,
            min_border_discovery_secs: 2.0,
            gap_signature_mode: GapSignatureMode::Auto,
            profile: RepairProfile::Default,
            fit_boundary_search: FitBoundarySearch::BaselineOnly,
            dual_fit: clip_sync_repair::infrastructure::config::RepairConfig::default().dual_fit,
        }
    }
}

fn sine_sample(sample_rate: u32, index: u64, freq: f32, amplitude: f32) -> i16 {
    let t = index as f32 / sample_rate as f32;
    (f32::sin(TAU * freq * t) * amplitude).round() as i16
}

/// Write a stereo WAV with a sine wave and a silent gap.
pub fn write_stereo_sine_with_gap(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    gap_start_secs: u32,
    gap_end_secs: u32,
    freq: f32,
    amplitude: f32,
) {
    let spec = WavSpec {
        channels: SINE_CHANNELS,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(sample_rate) * u64::from(total_secs);
    let gap_start_frame = u64::from(sample_rate) * u64::from(gap_start_secs);
    let gap_end_frame = u64::from(sample_rate) * u64::from(gap_end_secs);

    for frame in 0..total_frames {
        let s = if frame >= gap_start_frame && frame < gap_end_frame {
            0i16
        } else {
            sine_sample(sample_rate, frame, freq, amplitude)
        };
        writer.write_sample(s).expect("write L");
        writer.write_sample(s).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

fn make_alignment(offset: f64) -> clip_sync_repair::domain::ScanAlignment {
    scan_alignment_from_result(&clip_sync::AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: 10.0,
            aligned: true,
            offset_secs: Some(offset),
            confidence: 0.9,
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
            repetition: None,
            video_b_window_start_secs: None,
            video_b_window_end_secs: None,
        }],
        start_aligned: true,
        end_aligned: None,
        recommended_offset_secs: Some(offset),
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
        alignment_mode_used: None,
        query_localization: None,
        end_clip_anchor: None,
    })
}

pub fn stereo_identical_compat(sample_rate: u32) -> TrackCompatibility {
    TrackCompatibility {
        a_channels: SINE_CHANNELS,
        b_channels: SINE_CHANNELS,
        a_sample_rate: sample_rate,
        b_sample_rate: sample_rate,
        channels_match: true,
        rate_match: true,
        verdict: CompatibilityVerdict::Identical,
    }
}

fn default_sine_gap() -> Gap {
    Gap {
        video_a_start_secs: 3.0,
        video_a_end_secs: 6.0,
        video_b_start_secs: Some(3.0),
        video_b_end_secs: Some(6.0),
        b_has_energy: true,
    }
}

fn make_report(path_a: PathBuf, path_b: PathBuf, compat: TrackCompatibility) -> GapReport {
    GapReport {
        video_a: path_a,
        video_b: path_b,
        track_compatibility: Some(compat),
        alignment: make_alignment(0.0),
        gaps: vec![default_sine_gap()],
        gap_equivalence: Vec::new(),
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        scan_block_ms: 250,
        silence_peak_fraction: 0.01,
        limit_fill_to_mapped_region: true,
        audio_timeline_skew: None,
    }
}

/// Fit-mode options for energy-signature acceptance fixtures (I1–I4).
pub fn energy_sig_patch_options(mode: GapSignatureMode) -> PatchTestOptions {
    PatchTestOptions {
        fill_mode: FillMode::Fit,
        fill_border_search_secs: 3.5,
        max_fill_align_adjustment_secs: 2.0,
        fill_align_margin_secs: 0.2,
        gap_signature_context_secs: 0.5,
        fill_length_slack_secs: 0.05,
        fill_extract_tail_slack_secs: 0.05,
        gap_end_extend_max_ms: 0,
        gap_end_extend_step_ms: 40,
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        short_gap_one_strong_seam_fallback: true,
        fill_fit_structure_weight: 1.0,
        fill_fit_waveform_weight: 0.0,
        fill_fit_nominal_bias_scale: 0.0,
        fill_fit_late_start_penalty_scale: 0.0,
        fill_marginal_margin: 0.08,
        fill_absolute_floor: -0.05,
        min_structure_match_score: 0.0,
        min_border_discovery_secs: 0.25,
        gap_signature_mode: mode,
        ..Default::default()
    }
}

pub fn energy_sig_geometry_params(options: &PatchTestOptions) -> PatchGeometryParams {
    PatchGeometryParams {
        fill_border_search_secs: options.fill_border_search_secs,
        fill_align_margin_secs: options.fill_align_margin_secs,
        gap_signature_context_secs: options.gap_signature_context_secs,
        fill_length_slack_secs: options.fill_length_slack_secs,
        fill_extract_tail_slack_secs: options.fill_extract_tail_slack_secs,
        gap_end_extend_max_ms: options.gap_end_extend_max_ms,
        gap_end_extend_on_post_seam_fail: options.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: options.gap_start_extend_on_pre_seam_fail,
        fit_boundary_search: options.fit_boundary_search,
        fill_offset_mode: options.fill_offset_mode,
        fill_mode_fit: options.fill_mode == FillMode::Fit,
        gap_signature_bin_ms: 50,
    }
}

pub fn patch_request_with_options(
    report: GapReport,
    normalize_fill: bool,
    normalize_window_secs: f64,
    min_fill_correlation: f32,
    options: PatchTestOptions,
) -> PatchAudioRequest {
    // Seeded from production (`..RepairConfig::default().patch_settings()`), so a newly added
    // knob is inherited rather than silently hardcoded here. Config-bundles plan P3 step 1
    // (`docs/dev/archive/TEMP-repair-config-bundles-plan.md`) — structural only; production
    // defaults are exercised separately via `patch_request_from_repair`. Everything below is a caller arg, a
    // `PatchTestOptions` knob, or a deliberate synthetic-fixture deviation.
    //
    // Of the 56 settings fields, 40 stay explicit (4 caller args, 28 options knobs, 8
    // deviations) and 16 are absorbed by the seed. Each absorbed field already equaled its
    // production default in the previous hand-written literal, so the built request is
    // value-identical to that literal.
    PatchRequestSettings {
        // ---- caller arguments ----
        normalize_fill,
        normalize_window_secs,
        min_fill_correlation,
        fill_anchor_min_correlation: min_fill_correlation,

        // ---- test knobs (PatchTestOptions) ----
        fill_align_margin_secs: options.fill_align_margin_secs,
        min_border_discovery_secs: options.min_border_discovery_secs,
        short_gap_mean_correlation_secs: options.short_gap_mean_correlation_secs,
        fill_length_slack_secs: options.fill_length_slack_secs,
        fill_extract_tail_slack_secs: options.fill_extract_tail_slack_secs,
        gap_signature_context_secs: options.gap_signature_context_secs,
        min_structure_match_score: options.min_structure_match_score,
        strong_structure_trust: options.strong_structure_trust,
        disable_structure_trust: options.disable_structure_trust,
        partial_structure_waveform_soften: options.partial_structure_waveform_soften,
        fill_offset_mode: options.fill_offset_mode,
        gap_end_extend_on_post_seam_fail: options.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: options.gap_start_extend_on_pre_seam_fail,
        gap_end_extend_max_ms: options.gap_end_extend_max_ms,
        gap_end_extend_step_ms: options.gap_end_extend_step_ms,
        short_gap_one_strong_seam_fallback: options.short_gap_one_strong_seam_fallback,
        fill_fit_structure_weight: options.fill_fit_structure_weight,
        fill_fit_waveform_weight: options.fill_fit_waveform_weight,
        fill_fit_nominal_bias_scale: options.fill_fit_nominal_bias_scale,
        fill_fit_late_start_penalty_scale: options.fill_fit_late_start_penalty_scale,
        fill_marginal_margin: options.fill_marginal_margin,
        fill_absolute_floor: options.fill_absolute_floor,
        fill_repeat_penalty_weight: options.fill_repeat_penalty_weight,
        fft_seam_search: options.fft_seam_search,
        fft_repeat_band: options.fft_repeat_band,
        fill_anchor_retry_marginal: options.fill_anchor_retry_marginal,
        gap_signature_mode: options.gap_signature_mode,
        profile: options.profile,
        fit_boundary_search: options.fit_boundary_search,
        dual_fit: options.dual_fit,

        // ---- deliberate deviations from production ----
        // Test-local settings for synthetic fixtures; production is covered elsewhere via
        // `patch_request_from_repair`. Pending justify-or-drop in the undated P3 step 3 — do not
        // "clean up" without reading `docs/dev/archive/TEMP-repair-config-bundles-plan.md` §4.
        //
        // Via PatchTestOptions defaults: fill_mode (Gate vs production Fit),
        // fill_border_search_secs (30.0 vs 10.0), max_fill_align_adjustment_secs (1.0 vs 0.5).
        fill_mode: options.fill_mode,
        fill_border_search_secs: options.fill_border_search_secs,
        max_fill_align_adjustment_secs: options.max_fill_align_adjustment_secs,
        // Hypothesis: synthetic fixtures contain true digital silence, so production's real-world
        // noise floor and codec-edge standoff do not apply.
        absolute_silence_rms: 0.0,
        border_standoff_secs: 0.0,
        // Hypothesis: tests want every gap processed and the patch observable, not deduped/vetoed.
        skip_equivalent_gaps: false,
        residual_gate: clip_sync_repair::domain::ResidualGateMode::Off,
        // No energy field on PatchTestOptions, so energy bias can only mirror nominal. Production
        // deliberately splits them 4× (1.0 / 0.25). Inert under Gate-mode defaults — see
        // `gate_mode_ignores_fill_fit_knobs`.
        fill_fit_energy_nominal_bias_scale: options.fill_fit_nominal_bias_scale,

        ..clip_sync_repair::infrastructure::config::RepairConfig::default().patch_settings()
    }
    .into_request(report)
}

pub fn run_patch(request: PatchAudioRequest, crossfade_ms: u64) -> PatchAudioResult {
    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    PatchAudio::new(&media_reader, &progress)
        .execute(request, crossfade_ms)
        .expect("patch should succeed")
}

/// Validation smoke: production-default fit patches a clean stereo sine gap.
pub fn run_fit_production_defaults_smoke() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(&path_a, SINE_SAMPLE_RATE, 10, 3, 6, 440.0, 16_000.0);
    // B is continuous — donor audio at the nominal span (A-only gap; G5 must not skip).
    write_stereo_sine_with_gap(&path_b, SINE_SAMPLE_RATE, 10, 0, 0, 440.0, 16_000.0);

    let report = make_report(path_a, path_b, stereo_identical_compat(SINE_SAMPLE_RATE));

    let options = PatchTestOptions {
        fill_mode: FillMode::Fit,
        fill_border_search_secs: 10.0,
        short_gap_one_strong_seam_fallback: false,
        ..Default::default()
    };

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, options),
        10,
    );
    assert_eq!(
        patched.summary.patched_count, 1,
        "production-like fit config should patch clean fixture: {:?}",
        patched.summary.gaps
    );
}

/// CSV / stderr diagnostic for energy-signature patch geometry (I1 / I3).
pub fn energy_sig_patch_diagnostic(
    fixture: &EnergySignatureFixture,
    report: &GapReport,
    options: PatchTestOptions,
    label: &str,
) {
    let (a_start, a_end, b_start, b_end, _) = gap_report_times(fixture);
    let weights = structure_heavy_weights();
    let preview = preview_patch_geometry(
        fixture,
        &report.alignment,
        a_start,
        a_end,
        b_start,
        b_end,
        &energy_sig_geometry_params(&options),
    );
    eprintln!("=== {label} patch geometry diagnostic ({}) ===", fixture.id);
    eprintln!("{}", preview.format_diagnostic(fixture));

    let domain = fixture
        .unified_match(GapSignatureMode::Energy, weights)
        .expect("domain oracle");
    eprintln!(
        "domain unified: start={} slide={:.6}s (truth slide {:.6}s)",
        domain.alignment.start_frame,
        structure_slide_secs(fixture, domain.alignment.start_frame),
        structure_slide_secs(fixture, fixture.true_fill_start),
    );

    match preview.unified_match_on_haystack(fixture, GapSignatureMode::Energy, weights) {
        Some(haystack) => {
            let extract_start =
                (preview.b_extract_start_secs * fixture.sample_rate as f64).round() as usize;
            eprintln!(
                "haystack unified: start={} full_B={} slide={:.6}s",
                haystack.alignment.start_frame,
                haystack.alignment.start_frame + extract_start,
                structure_slide_secs(fixture, haystack.alignment.start_frame + extract_start,),
            );
        }
        None => eprintln!("haystack unified: None (structure search failed on sliced B)"),
    }

    let result = run_patch(
        patch_request_with_options(report.clone(), false, 0.25, 0.0, options),
        10,
    );
    eprintln!(
        "patch result: patched_count={} skipped_count={}",
        result.summary.patched_count, result.summary.skipped_count,
    );
    match &result.summary.gaps[0].status {
        GapPatchStatus::Skipped { reason } => eprintln!("patch skip reason: {reason:?}"),
        GapPatchStatus::Patched {
            align_adjustment_secs,
            pre_correlation,
            post_correlation,
            ..
        } => eprintln!(
            "patch patched: slide={align_adjustment_secs:.6}s pre={pre_correlation} post={post_correlation}",
        ),
        other => eprintln!("patch status: {other:?}"),
    }
}

fn gap_align_slide_secs(result: &PatchAudioResult, gap_idx: usize) -> Option<f64> {
    match &result.summary.gaps[gap_idx].status {
        GapPatchStatus::Patched {
            align_adjustment_secs,
            ..
        } => Some(*align_adjustment_secs),
        _ => None,
    }
}

fn slide_within_bin(
    fixture: &EnergySignatureFixture,
    slide_secs: f64,
    truth_slide_secs: f64,
) -> bool {
    let bin_secs = fixture.bin_frames() as f64 / fixture.sample_rate as f64;
    (slide_secs - truth_slide_secs).abs() <= bin_secs * 1.1
}

/// Domain energy unified match lands within bin tolerance of truth fill start.
pub fn assert_domain_energy_finds_truth(fixture: &EnergySignatureFixture) {
    let matched = fixture
        .unified_match(GapSignatureMode::Energy, structure_heavy_weights())
        .expect("domain energy unified match");
    assert!(
        fixture.within_bin_tolerance(matched.alignment.start_frame, fixture.true_fill_start),
        "domain energy start {} truth {}",
        matched.alignment.start_frame,
        fixture.true_fill_start,
    );
}

/// Haystack energy unified match (post geometry preview) lands within bin tolerance.
pub fn assert_haystack_energy_finds_truth(
    fixture: &EnergySignatureFixture,
    report: &GapReport,
    options: &PatchTestOptions,
) {
    let (a_start, a_end, b_start, b_end, _) = gap_report_times(fixture);
    let preview = preview_patch_geometry(
        fixture,
        &report.alignment,
        a_start,
        a_end,
        b_start,
        b_end,
        &energy_sig_geometry_params(options),
    );
    let matched = preview
        .unified_match_on_haystack(fixture, GapSignatureMode::Energy, structure_heavy_weights())
        .expect("haystack oracle should match patch geometry");
    let extract_start =
        (preview.b_extract_start_secs * fixture.sample_rate as f64).round() as usize;
    let full_b_start = matched.alignment.start_frame.saturating_add(extract_start);
    assert!(
        fixture.within_bin_tolerance(full_b_start, fixture.true_fill_start),
        "haystack start {} (full B {}) truth {}",
        matched.alignment.start_frame,
        full_b_start,
        fixture.true_fill_start,
    );
}

/// SP integration: domain + haystack oracles and one successful fit patch (I1 / I3).
pub fn assert_energy_integration_patch(
    fixture: &EnergySignatureFixture,
    report: GapReport,
    options: PatchTestOptions,
    test_id: &str,
    expected_slide_secs: Option<f64>,
) {
    assert_domain_energy_finds_truth(fixture);
    assert_haystack_energy_finds_truth(fixture, &report, &options);

    let result = run_patch(
        patch_request_with_options(report, false, 0.25, 0.0, options),
        10,
    );
    assert_eq!(
        result.summary.patched_count, 1,
        "{test_id}: expected patch, got {:?}",
        result.summary.gaps
    );
    let slide =
        gap_align_slide_secs(&result, 0).unwrap_or_else(|| panic!("{test_id}: patched gap slide"));
    let truth = expected_slide_secs
        .unwrap_or_else(|| structure_slide_secs(fixture, fixture.true_fill_start));
    assert!(
        slide_within_bin(fixture, slide, truth),
        "{test_id}: slide {slide}s not within bin of truth {truth}s",
    );
}
