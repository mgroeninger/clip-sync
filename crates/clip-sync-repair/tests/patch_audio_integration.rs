//! Sine-seam patch integration (~15 min debug).
//!
//! Tier: **integration**. Stereo sine + gap WAV fixtures; fit/gate modes, marginal tier, extension
//! grid, anchored retry. **SP04** (`i4_f3_auto_matches_bool_outcome`) is the only energy-signature
//! row here; SP01–SP03 live in `integration_energy_patch.rs`.
//!
//! PR: **no** — `.\scripts\test-tier.ps1 -Tier pr-repair-extended` (or full `integration` tier).
//!
//! Run: `cargo test -p clip-sync-repair --test patch_audio_integration`

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::MultiChannelPcm;
use clip_sync::{
    AlignmentResult, ClipLabel, ClipMatch, MediaReader, MediaSession, SymphoniaMediaReader,
};
use clip_sync_repair::application::align_bridge::scan_alignment_from_result;
use clip_sync_repair::application::ports::PatchedAudioWriter;
use clip_sync_repair::application::{PatchAudio, PatchAudioRequest};
use clip_sync_repair::domain::gap::{Gap, GapReport};
use clip_sync_repair::domain::gap_tags::DualFitRoute;
use clip_sync_repair::domain::policies;
use clip_sync_repair::domain::FillConfidence;
use clip_sync_repair::domain::{CompatibilityVerdict, TrackCompatibility};
use clip_sync_repair::domain::{
    FillMode, FillOffsetMode, FitBoundarySearch, GapSelectionMode, GapSignatureMode, RepairProfile,
};
use clip_sync_repair::domain::{
    GapFillSkipReason, GapPatchSkipReason, GapPatchStatus, PatchSummary,
};
use clip_sync_repair::infrastructure::aligner::SymphoniaAligner;
use clip_sync_repair::infrastructure::wav_writer::WavPatchedAudioWriter;
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    build_f3_drone_integration, structure_heavy_weights,
};
use clip_sync_repair_harness::patch_audio::{
    patch_request_with_options, run_patch, PatchTestOptions,
};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

const SAMPLE_RATE: u32 = 44_100;
const RATE_B: u32 = 48_000;
const CHANNELS: u16 = 2;

const GAP_START: f64 = 3.0;
const GAP_END: f64 = 6.0;
const TOTAL_SECS: u32 = 10;

fn sine_sample(sample_rate: u32, index: u64, freq: f32, amplitude: f32) -> i16 {
    let t = index as f32 / sample_rate as f32;
    (f32::sin(TAU * freq * t) * amplitude).round() as i16
}

/// Write a stereo WAV with a sine wave for `total_secs`.
fn write_stereo_sine_wav(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    freq: f32,
    amplitude: f32,
) {
    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(sample_rate) * u64::from(total_secs);
    for frame in 0..total_frames {
        let s = sine_sample(sample_rate, frame, freq, amplitude);
        writer.write_sample(s).expect("write L");
        writer.write_sample(s).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

/// Write a stereo WAV with a sine wave and a silent gap.
fn write_stereo_sine_with_gap(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    gap_start_secs: u32,
    gap_end_secs: u32,
    freq: f32,
    amplitude: f32,
) {
    let spec = WavSpec {
        channels: CHANNELS,
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

/// Like [`write_stereo_sine_wav`] but phase-inverts a frame range (per channel).
fn write_stereo_sine_with_inverted_region(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    invert_start_secs: f64,
    invert_end_secs: f64,
    freq: f32,
    amplitude: f32,
) {
    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(sample_rate) * u64::from(total_secs);
    let invert_start = (invert_start_secs * sample_rate as f64) as u64;
    let invert_end = (invert_end_secs * sample_rate as f64) as u64;

    for frame in 0..total_frames {
        let mut s = sine_sample(sample_rate, frame, freq, amplitude);
        if frame >= invert_start && frame < invert_end {
            s = s.saturating_neg();
        }
        writer.write_sample(s).expect("write L");
        writer.write_sample(s).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

/// Blend sine samples toward inverted in `[start_secs, end_secs)`.
/// `mix_inverted = 0` matches A; `1` fully inverts (Pearson ≈ −1).
struct SeamDistortionWindow {
    start_secs: f64,
    end_secs: f64,
    mix_inverted: f32,
}

struct SineTone {
    freq: f32,
    amplitude: f32,
}

fn write_stereo_sine_with_seam_distortion(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    distortion: SeamDistortionWindow,
    tone: SineTone,
) {
    let SeamDistortionWindow {
        start_secs: distort_start_secs,
        end_secs: distort_end_secs,
        mix_inverted,
    } = distortion;
    let SineTone { freq, amplitude } = tone;
    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(sample_rate) * u64::from(total_secs);
    let distort_start = (distort_start_secs * sample_rate as f64) as u64;
    let distort_end = (distort_end_secs * sample_rate as f64) as u64;
    let mix = mix_inverted.clamp(0.0, 1.0);

    for frame in 0..total_frames {
        let s = sine_sample(sample_rate, frame, freq, amplitude);
        let sample = if frame >= distort_start && frame < distort_end {
            let blended = s as f32 * (1.0 - 2.0 * mix);
            blended.round() as i16
        } else {
            s
        };
        writer.write_sample(sample).expect("write L");
        writer.write_sample(sample).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

/// Write a stereo WAV with multiple silent gaps (each `(start_secs, end_secs)` on this file's timeline).
fn write_stereo_sine_with_gaps(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    gaps: &[(f64, f64)],
    freq: f32,
    amplitude: f32,
) {
    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(sample_rate) * u64::from(total_secs);

    for frame in 0..total_frames {
        let t = frame as f64 / sample_rate as f64;
        let silent = gaps.iter().any(|&(start, end)| t >= start && t < end);
        let s = if silent {
            0i16
        } else {
            sine_sample(sample_rate, frame, freq, amplitude)
        };
        writer.write_sample(s).expect("write L");
        writer.write_sample(s).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

/// Position-indexed pseudo-noise sample: a splitmix64-style avalanche hash of `(seed, idx)`, in
/// `[-amplitude, amplitude]`. Unlike [`channel_noise`] (a near-linear congruential map — two positions
/// a *constant* distance apart stay strongly correlated, r≈0.89 measured at a 15,435-frame shift, since
/// the map is affine in the frame index), this decorrelates sharply between any two distinct indices
/// (r≈0.03 at the same shift) — needed so a shifted copy of this content is uncorrelated with the
/// original at any lag except the true shift.
fn hash_noise_sample(seed: u32, idx: i64, amplitude: f32) -> i16 {
    let mut x = idx.wrapping_add(i64::from(seed)) as u64;
    x = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    let unit = (x >> 40) as f64 / f64::from(1u32 << 24); // [0, 1)
    (((unit * 2.0 - 1.0) as f32) * amplitude).round() as i16
}

/// Write a stereo WAV: sine tone everywhere, except a band of [`hash_noise_sample`] noise in
/// `[region_start_secs + shift_secs, region_end_secs + shift_secs)`, and (if `gap` is `Some`) true
/// silence in `[gap_start, gap_end)`.
///
/// The noise value written at absolute frame `f` depends only on
/// `f - (region_start_secs + shift_secs) * sample_rate` — so two calls sharing
/// `seed`/`region_start_secs`/`region_end_secs` but differing `shift_secs` produce the *same* noise
/// content, just translated in time by the difference in shift. That lets a caller build an A/B pair
/// whose shoulders correlate strongly at a single constant lag (`shift_b - shift_a`) and not at all
/// at lag 0 (a lag-0 comparison lines up unrelated noise samples) — a rigid constant-shift fixture,
/// distinct from [`write_stereo_sine_with_inverted_region`]'s single-shoulder distortion (which
/// produces a *step*, not a shared lag).
#[allow(clippy::too_many_arguments)]
fn write_stereo_with_shifted_noise_band(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    region_start_secs: f64,
    region_end_secs: f64,
    shift_secs: f64,
    seed: u32,
    noise_amplitude: f32,
    gap: Option<(f64, f64)>,
    tone_freq: f32,
    tone_amplitude: f32,
) {
    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(sample_rate) * u64::from(total_secs);
    let band_start = ((region_start_secs + shift_secs) * sample_rate as f64).round() as i64;
    let band_end = ((region_end_secs + shift_secs) * sample_rate as f64).round() as i64;
    let gap_frames = gap.map(|(start, end)| {
        (
            (start * sample_rate as f64).round() as i64,
            (end * sample_rate as f64).round() as i64,
        )
    });

    for frame in 0..total_frames as i64 {
        let mut s = if frame >= band_start && frame < band_end {
            hash_noise_sample(seed, frame - band_start, noise_amplitude)
        } else {
            sine_sample(sample_rate, frame as u64, tone_freq, tone_amplitude)
        };
        if let Some((gap_start, gap_end)) = gap_frames {
            if frame >= gap_start && frame < gap_end {
                s = 0;
            }
        }
        writer.write_sample(s).expect("write L");
        writer.write_sample(s).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

/// Linear clip-offset interpolation at `a_secs` on a timeline of `timeline_secs`.
fn interpolated_fill_offset_secs(
    a_secs: f64,
    start_offset: f64,
    end_offset: f64,
    timeline_secs: f64,
) -> f64 {
    start_offset + (a_secs / timeline_secs) * (end_offset - start_offset)
}

fn read_stereo_i16(path: &Path) -> (WavSpec, Vec<i16>) {
    let mut reader = WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("sample"))
        .collect();
    (spec, samples)
}

fn write_stereo_i16(path: &Path, spec: WavSpec, samples: &[i16]) {
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for &s in samples {
        writer.write_sample(s).expect("write");
    }
    writer.finalize().expect("finalize wav");
}

/// Overwrite a stereo WAV region with deterministic uncorrelated noise (per channel).
fn patch_wav_noise_region(
    path: &Path,
    sample_rate: u32,
    start_secs: f64,
    end_secs: f64,
    seed: u64,
    amplitude: f32,
) {
    let (spec, mut samples) = read_stereo_i16(path);
    let channels = spec.channels as usize;
    let start_frame = (start_secs * sample_rate as f64) as u64;
    let end_frame = (end_secs * sample_rate as f64) as u64;
    let mut state = seed;
    for frame in start_frame..end_frame {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let n = ((state >> 33) as f32 / (1u64 << 31) as f32) * amplitude;
        let s = n.round() as i16;
        let base = frame as usize * channels;
        for ch in 0..channels {
            samples[base + ch] = s;
        }
    }
    write_stereo_i16(path, spec, &samples);
}

/// Zero-fill a stereo WAV region (per channel).
fn patch_wav_silence_region(path: &Path, sample_rate: u32, start_secs: f64, end_secs: f64) {
    let (spec, mut samples) = read_stereo_i16(path);
    let channels = spec.channels as usize;
    let start = (start_secs * sample_rate as f64) as usize * channels;
    let end = (end_secs * sample_rate as f64) as usize * channels;
    for sample in samples.iter_mut().take(end).skip(start) {
        *sample = 0;
    }
    write_stereo_i16(path, spec, &samples);
}

/// Overwrite a stereo WAV region with in-phase sine (per channel).
fn patch_wav_sine_region(
    path: &Path,
    sample_rate: u32,
    start_secs: f64,
    end_secs: f64,
    freq: f32,
    amplitude: f32,
) {
    let (spec, mut samples) = read_stereo_i16(path);
    let channels = spec.channels as usize;
    let start_frame = (start_secs * sample_rate as f64) as u64;
    let end_frame = (end_secs * sample_rate as f64) as u64;
    for frame in start_frame..end_frame {
        let s = sine_sample(sample_rate, frame, freq, amplitude);
        let base = frame as usize * channels;
        for ch in 0..channels {
            samples[base + ch] = s;
        }
    }
    write_stereo_i16(path, spec, &samples);
}

fn make_gap_on_a(a_start: f64, a_end: f64, report_b_offset: f64) -> Gap {
    Gap {
        video_a_start_secs: a_start,
        video_a_end_secs: a_end,
        video_b_start_secs: Some(a_start + report_b_offset),
        video_b_end_secs: Some(a_end + report_b_offset),
        b_has_energy: true,
        collar_level: None,
    }
}

/// Fit-mode options for anchored-retry drift fixtures: small haystack so pass 1 can fail
/// when clip interpolation undershoots local B shift, while nearby easy gaps still patch.
fn anchored_retry_drift_patch_options() -> PatchTestOptions {
    PatchTestOptions {
        fill_mode: FillMode::Fit,
        fill_offset_mode: FillOffsetMode::AnchoredRetry,
        fill_border_search_secs: 0.32,
        fill_align_margin_secs: 0.1,
        gap_signature_context_secs: 1.0,
        fill_length_slack_secs: 0.0,
        fill_extract_tail_slack_secs: 0.0,
        gap_end_extend_max_ms: 0,
        gap_end_extend_step_ms: 40,
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        short_gap_one_strong_seam_fallback: false,
        short_gap_mean_correlation_secs: 0.5,
        fill_absolute_floor: 0.78,
        ..Default::default()
    }
}

/// Gate-mode drift options: fit-sized haystack; `strong_structure_trust` above 1.0 forces
/// waveform gate (anchors) instead of structure-trust skips; extension helps pass 2 on hard tail.
fn anchored_retry_drift_gate_patch_options() -> PatchTestOptions {
    let mut opts = anchored_retry_drift_patch_options();
    opts.fill_mode = FillMode::Gate;
    opts.strong_structure_trust = 1.01;
    opts.partial_structure_waveform_soften = 0.85;
    opts.fill_absolute_floor = 0.12;
    opts.gap_end_extend_on_post_seam_fail = true;
    opts.gap_start_extend_on_pre_seam_fail = true;
    opts.short_gap_one_strong_seam_fallback = true;
    opts
}

const DRIFT_ANCHOR_TIMELINE_SECS: u32 = 60;
const DRIFT_ANCHOR_START_OFFSET: f64 = 0.0;
const DRIFT_ANCHOR_END_OFFSET: f64 = 1.0;
const DRIFT_ANCHOR_MIN_CORRELATION: f32 = 0.78;

struct DriftAnchorRetryFixture {
    _temp: tempfile::TempDir,
    report: GapReport,
    hard_gap_span: (f64, f64),
}

/// How the hard-gap donor on B is decorated (G5-safe; pass-1 fail, pass-2 recover).
enum HardTailBDecor {
    /// 880 Hz pocket at interpolated nominal — fit-mode correlation fail.
    WrongFreqInterp,
    /// Silent pocket at true shift — gate-mode program-quiet / post-seam fail.
    SilenceAtShift,
}

fn build_drift_anchor_retry_fixture() -> DriftAnchorRetryFixture {
    build_drift_anchor_retry_fixture_with_hard_shift(1.42, HardTailBDecor::WrongFreqInterp)
}

/// Same drift timeline as the fit anchored-retry test.
fn build_gate_anchor_retry_fixture() -> DriftAnchorRetryFixture {
    build_drift_anchor_retry_fixture_with_hard_shift(1.42, HardTailBDecor::SilenceAtShift)
}

fn build_drift_anchor_retry_fixture_with_hard_shift(
    hard_b_shift: f64,
    hard_tail: HardTailBDecor,
) -> DriftAnchorRetryFixture {
    build_drift_anchor_retry_fixture_from_specs(
        &[
            ((36.0, 39.0), 0.35),
            ((44.0, 46.0), 0.76),
            ((47.0, 49.0), 1.05),
            ((50.0, 52.0), 1.35),
            ((53.0, 56.0), hard_b_shift),
        ],
        hard_tail,
    )
}

fn build_drift_anchor_retry_fixture_from_specs(
    gap_specs: &[((f64, f64), f64)],
    hard_tail: HardTailBDecor,
) -> DriftAnchorRetryFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");

    let a_gaps: Vec<(f64, f64)> = gap_specs.iter().map(|&(span, _)| span).collect();
    write_stereo_sine_with_gaps(
        &path_a,
        SAMPLE_RATE,
        DRIFT_ANCHOR_TIMELINE_SECS,
        &a_gaps,
        440.0,
        16_000.0,
    );

    write_stereo_sine_wav(
        &path_b,
        SAMPLE_RATE,
        DRIFT_ANCHOR_TIMELINE_SECS,
        440.0,
        16_000.0,
    );
    if let Some(&((hard_start, hard_end), hard_shift)) = gap_specs.last() {
        match hard_tail {
            HardTailBDecor::WrongFreqInterp => {
                let off = interpolated_fill_offset_secs(
                    hard_start,
                    DRIFT_ANCHOR_START_OFFSET,
                    DRIFT_ANCHOR_END_OFFSET,
                    f64::from(DRIFT_ANCHOR_TIMELINE_SECS),
                );
                patch_wav_sine_region(
                    &path_b,
                    SAMPLE_RATE,
                    hard_start + off,
                    hard_end + off,
                    880.0,
                    16_000.0,
                );
            }
            HardTailBDecor::SilenceAtShift => {
                patch_wav_silence_region(
                    &path_b,
                    SAMPLE_RATE,
                    hard_start + hard_shift,
                    hard_end + hard_shift,
                );
            }
        }
    }

    let gaps: Vec<Gap> = gap_specs
        .iter()
        .map(|&((start, end), _)| make_gap_on_a(start, end, DRIFT_ANCHOR_START_OFFSET))
        .collect();

    let alignment = make_drift_alignment(
        DRIFT_ANCHOR_START_OFFSET,
        DRIFT_ANCHOR_END_OFFSET,
        f64::from(DRIFT_ANCHOR_TIMELINE_SECS),
    );
    let report = make_report_with_alignment(
        path_a,
        path_b,
        stereo_identical_compat(SAMPLE_RATE),
        alignment,
        gaps,
    );

    DriftAnchorRetryFixture {
        _temp: temp,
        hard_gap_span: gap_specs.last().expect("gap specs").0,
        report,
    }
}

fn assert_hard_gap_skipped_interpolated_only(summary: &clip_sync_repair::domain::PatchSummary) {
    let hard_index = summary.gaps.len() - 1;
    match &summary.gaps[hard_index].status {
        GapPatchStatus::Skipped { reason, .. } => {
            assert!(
                matches!(
                    reason,
                    GapPatchSkipReason::CorrelationBelowThreshold { .. }
                        | GapPatchSkipReason::BoundaryAlignmentFailed
                        | GapPatchSkipReason::ProgramQuiet
                ),
                "hard gap should fail when true B shift exceeds search window, got {reason:?}"
            );
        }
        other => panic!("expected hard gap to skip on interpolated-only pass, got {other:?}"),
    }
}

fn assert_fit_interpolated_only_patches_bridge_gaps(
    summary: &clip_sync_repair::domain::PatchSummary,
) {
    assert_eq!(
        summary.patched_count, 4,
        "fit bridge gaps should patch under interpolated offset, hard gap should not, got {:?}",
        summary.gaps
    );
    assert_hard_gap_skipped_interpolated_only(summary);
    assert_fit_terms_reported_on_bracket_patches(summary);
}

/// End-to-end proof that `fit_terms` is POPULATED, not merely declared. The unit tests cover the
/// breakdown's arithmetic and the golden covers its serialized shape; neither would notice if the value
/// stopped reaching the report — the field is `skip_serializing_if = "Option::is_none"`, so a break
/// looks exactly like the dual-fit path's legitimate absence. These are fit-mode bracket patches, so
/// every one of them must carry terms.
fn assert_fit_terms_reported_on_bracket_patches(summary: &clip_sync_repair::domain::PatchSummary) {
    for (index, outcome) in summary.gaps.iter().enumerate() {
        let GapPatchStatus::Patched {
            dual_fit_used,
            fit_terms,
            ..
        } = &outcome.status
        else {
            continue;
        };
        if *dual_fit_used {
            continue; // dual-fit never runs the unified objective — absence is correct there
        }
        let terms = fit_terms.unwrap_or_else(|| {
            panic!(
                "gap #{} is a bracket patch but reports no fit_terms",
                index + 1
            )
        });
        assert!(
            terms.total.is_finite(),
            "gap #{} reported an unrankable total: {terms:?}",
            index + 1
        );
        for (name, value) in [
            ("repeat_pre", terms.repeat_pre),
            ("repeat_post", terms.repeat_post),
        ] {
            assert!(
                value.is_finite() && (-1.0..=1.0).contains(&value),
                "gap #{} {name} is not a Pearson: {terms:?}",
                index + 1
            );
        }
    }
}

fn assert_patch_anchors_exclude_structure_trusted(
    summary: &clip_sync_repair::domain::PatchSummary,
) {
    let trusted_indices: Vec<usize> = summary
        .gaps
        .iter()
        .enumerate()
        .filter_map(|(index, outcome)| match &outcome.status {
            GapPatchStatus::Patched {
                structure_trusted: true,
                ..
            } => Some(index),
            _ => None,
        })
        .collect();

    let anchors = match summary.patch_anchors_used.as_ref() {
        Some(anchors) => anchors,
        None => return,
    };

    for anchor in anchors {
        assert!(
            !trusted_indices.contains(&anchor.source_gap_index),
            "structure-trusted gap #{} must not become an anchor, trusted={trusted_indices:?}, anchors={anchors:?}",
            anchor.source_gap_index + 1,
        );
        match &summary.gaps[anchor.source_gap_index].status {
            GapPatchStatus::Patched {
                structure_trusted: false,
                ..
            } => {}
            other => panic!(
                "anchor source gap #{} should be a waveform-measured patch, got {other:?}",
                anchor.source_gap_index + 1,
            ),
        }
    }
}

fn rms_region(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    start_secs: f64,
    end_secs: f64,
) -> f32 {
    let start = (start_secs * sample_rate as f64) as usize * channels as usize;
    let end = (end_secs * sample_rate as f64) as usize * channels as usize;
    let end = end.min(samples.len());
    if start >= end {
        return 0.0;
    }
    let slice = &samples[start..end];
    let sum_sq: f64 = slice
        .iter()
        .map(|&s| {
            let v = s as f64;
            v * v
        })
        .sum();
    (sum_sq / slice.len() as f64).sqrt() as f32
}

fn make_alignment(offset: f64) -> AlignmentResult {
    AlignmentResult {
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
    }
}

fn stereo_identical_compat(sample_rate: u32) -> TrackCompatibility {
    TrackCompatibility {
        a_channels: CHANNELS,
        b_channels: CHANNELS,
        a_sample_rate: sample_rate,
        b_sample_rate: sample_rate,
        channels_match: true,
        rate_match: true,
        verdict: CompatibilityVerdict::Identical,
    }
}

fn stereo_compatible_diff_rate() -> TrackCompatibility {
    TrackCompatibility {
        a_channels: CHANNELS,
        b_channels: CHANNELS,
        a_sample_rate: SAMPLE_RATE,
        b_sample_rate: RATE_B,
        channels_match: true,
        rate_match: false,
        verdict: CompatibilityVerdict::Compatible,
    }
}

fn default_gap() -> Gap {
    Gap {
        video_a_start_secs: GAP_START,
        video_a_end_secs: GAP_END,
        video_b_start_secs: Some(GAP_START),
        video_b_end_secs: Some(GAP_END),
        b_has_energy: true,
        collar_level: None,
    }
}

fn fast_fit_patch_options() -> PatchTestOptions {
    PatchTestOptions {
        fill_mode: FillMode::Fit,
        fill_border_search_secs: 0.0,
        fill_align_margin_secs: 0.1,
        gap_signature_context_secs: 1.0,
        fill_length_slack_secs: 0.0,
        fill_extract_tail_slack_secs: 0.0,
        gap_end_extend_max_ms: 40,
        gap_end_extend_step_ms: 40,
        short_gap_one_strong_seam_fallback: false,
        fit_boundary_search: FitBoundarySearch::FullGrid,
        ..Default::default()
    }
}

fn patch_request(
    report: GapReport,
    normalize_fill: bool,
    normalize_window_secs: f64,
    min_fill_correlation: f32,
) -> PatchAudioRequest {
    patch_request_with_options(
        report,
        normalize_fill,
        normalize_window_secs,
        min_fill_correlation,
        PatchTestOptions::default(),
    )
}

fn make_drift_alignment(start_offset: f64, end_offset: f64, timeline_secs: f64) -> AlignmentResult {
    let half = timeline_secs / 2.0;
    AlignmentResult {
        clips: vec![
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: half,
                aligned: true,
                offset_secs: Some(start_offset),
                confidence: 0.95,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
            ClipMatch {
                label: ClipLabel::End,
                window_start_secs: half,
                window_end_secs: timeline_secs,
                aligned: true,
                offset_secs: Some(end_offset),
                confidence: 0.95,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
        ],
        start_aligned: true,
        end_aligned: Some(true),
        recommended_offset_secs: Some(start_offset),
        offsets_consistent: false,
        offset_drift_secs: Some(end_offset - start_offset),
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
        alignment_mode_used: None,
        query_localization: None,
        end_clip_anchor: None,
    }
}

fn make_report_with_alignment(
    path_a: PathBuf,
    path_b: PathBuf,
    compat: TrackCompatibility,
    alignment: AlignmentResult,
    gaps: Vec<Gap>,
) -> GapReport {
    GapReport {
        local_pair_collar_secs: None,
        video_a: path_a,
        video_b: path_b,
        track_compatibility: Some(compat),
        alignment: scan_alignment_from_result(&alignment),
        gaps,
        gap_equivalence: Vec::new(),
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
        limit_fill_to_mapped_region: true,
        b_scanned_end_secs: None,
        b_scan_truncated: false,
        b_stall: None,
        audio_timeline_skew: None,
        program_levels: None,
    }
}

fn make_report(path_a: PathBuf, path_b: PathBuf, compat: TrackCompatibility) -> GapReport {
    GapReport {
        local_pair_collar_secs: None,
        video_a: path_a,
        video_b: path_b,
        track_compatibility: Some(compat),
        alignment: scan_alignment_from_result(&make_alignment(0.0)),
        gaps: vec![default_gap()],
        gap_equivalence: Vec::new(),
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
        limit_fill_to_mapped_region: true,
        b_scanned_end_secs: None,
        b_scan_truncated: false,
        b_stall: None,
        audio_timeline_skew: None,
        program_levels: None,
    }
}

fn expect_pcm(result: &clip_sync_repair::application::PatchAudioResult) -> &MultiChannelPcm {
    result
        .pcm
        .as_ref()
        .expect("expected patch to decode A when fill regions exist")
}

fn patch_to_samples(request: PatchAudioRequest, crossfade_ms: u64) -> (Vec<f32>, u32, u16) {
    let (samples, rate, channels, _) = patch_to_samples_and_summary(request, crossfade_ms);
    (samples, rate, channels)
}

/// As [`patch_to_samples`], but also hands back the summary — needed by the normalize tests, which
/// assert on the written audio *and* on the `normalize_gain` recorded for the gap.
fn patch_to_samples_and_summary(
    request: PatchAudioRequest,
    crossfade_ms: u64,
) -> (Vec<f32>, u32, u16, PatchSummary) {
    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;

    let result = PatchAudio::new(&media_reader, &progress)
        .execute(request, crossfade_ms)
        .expect("patch should succeed");
    let summary = result.summary.clone();

    let temp = tempfile::tempdir().expect("tempdir");
    let path_out = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(expect_pcm(&result), &path_out)
        .expect("write should succeed");

    let mut reader = WavReader::open(&path_out).expect("open out.wav");
    let spec = reader.spec();
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.expect("read sample") as f32 / 32767.0)
        .collect();
    (samples, spec.sample_rate, spec.channels, summary)
}

/// A `ProgramLevels` carrying one paired offset, for the whole-program gain tests. `offset_db` is
/// **B minus A**, so a negative value means B is the quieter master and the fill must come up.
fn program_levels_offset(
    offset_db: f64,
    spread_db: f64,
) -> clip_sync_repair::domain::program_levels::ProgramLevels {
    use clip_sync_repair::domain::program_levels::{
        AbLevelOffset, ProgramLevelContext, ProgramLevels,
    };
    let ctx = ProgramLevelContext {
        mid_db: Some(-20.0),
        floor_db: Some(-70.0),
        n_active: 600,
    };
    ProgramLevels {
        a: ctx,
        b: Some(ctx),
        channels_comparable: true,
        offset: Some(AbLevelOffset {
            offset_db,
            spread_db,
            p10_db: offset_db,
            p90_db: offset_db,
            n_pairs: 500,
            n_active: 600,
            n_steady: 550,
            first_pair_secs: 0.0,
            last_pair_secs: 600.0,
            channels_comparable: true,
            chunks: Vec::new(),
        }),
    }
}

/// The `normalize_gain` recorded for a single-gap patch run.
/// A synthetic per-gap collar reading, thick and tight enough to clear the admission test
/// (`n_pairs >= 8`, both sides present, `se_db <= 1.0`) so a test can choose the `offset_db` it
/// wants applied. **B minus A**, same polarity as the whole-program offset: positive means B is the
/// louder master here and the fill must come down.
fn collar_reading(offset_db: f64) -> clip_sync_repair::domain::program_levels::CollarLevelOffset {
    clip_sync_repair::domain::program_levels::CollarLevelOffset {
        offset_db,
        spread_db: 0.05,
        n_pairs: 24,
        n_active: 48,
        n_steady: 26,
        n_pre: 12,
        n_post: 12,
        pre_offset_db: Some(offset_db),
        post_offset_db: Some(offset_db),
        first_pair_secs: GAP_START - 2.0,
        last_pair_secs: GAP_END + 2.0,
        map_lag_ms: Some(0.0),
    }
}

/// `make_report` with a collar stamped on its one gap — what a scan produces for a pair whose
/// shoulders paired. Without this the report looks like a pair where nothing paired, and the local
/// half abstains.
fn report_with_collar(
    path_a: PathBuf,
    path_b: PathBuf,
    compat: TrackCompatibility,
    offset_db: f64,
) -> GapReport {
    let mut report = make_report(path_a, path_b, compat);
    report.local_pair_collar_secs = Some(4.0);
    report.gaps[0].collar_level = Some(collar_reading(offset_db));
    report
}

fn only_gap_local_gain(summary: &PatchSummary) -> (f32, clip_sync_repair::domain::LocalGainReason) {
    match &summary.gaps[0].status {
        GapPatchStatus::Patched {
            local_gain,
            local_gain_reason,
            ..
        } => (*local_gain, *local_gain_reason),
        other => panic!("expected a patched gap, got {other:?}"),
    }
}

fn only_gap_normalize_gain(summary: &PatchSummary) -> f32 {
    match &summary.gaps[0].status {
        GapPatchStatus::Patched { normalize_gain, .. } => *normalize_gain,
        other => panic!("expected a patched gap, got {other:?}"),
    }
}

struct SineGapFixture {
    path_a: PathBuf,
    path_b: PathBuf,
}

fn sine_gap_fixture(
    temp: &Path,
    rate_a: u32,
    rate_b: u32,
    b_freq: f32,
    b_amplitude: f32,
) -> SineGapFixture {
    let path_a = temp.join("a.wav");
    let path_b = temp.join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        rate_a,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    write_stereo_sine_wav(&path_b, rate_b, TOTAL_SECS, b_freq, b_amplitude);
    SineGapFixture { path_a, path_b }
}

#[test]
fn patch_audio_fills_gap_in_stereo_wav() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let request = patch_request(
        make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        ),
        false,
        5.0,
        // Both A's pre-gap border and B's fill are 440 Hz sine; correlation is ~1.0.
        0.35,
    );

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let result = PatchAudio::new(&media_reader, &progress)
        .execute(request, 10)
        .expect("patch should succeed");
    let pcm = expect_pcm(&result);
    assert_eq!(pcm.sample_rate, SAMPLE_RATE);
    assert_eq!(pcm.channels, CHANNELS);
    assert_eq!(result.summary.patched_count, 1);

    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "gap region should have audio after fill, got rms={gap_rms}"
    );

    let pre_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 0.0, 2.0);
    assert!(
        pre_rms > 100.0 / 32767.0,
        "pre-gap region should have audio, got rms={pre_rms}"
    );

    let post_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 7.0, 9.0);
    assert!(
        post_rms > 100.0 / 32767.0,
        "post-gap region should retain A audio after splice, got rms={post_rms}"
    );
}

/// Config-bundles plan P5 (`docs/dev/archive/TEMP-repair-config-bundles-plan.md` §2/§5):
/// the five `fill_fit_*` knobs are dead under `fill_mode: Gate`
/// (they are only read on the Fit candidate path). Perturbing them must leave Gate-mode PCM
/// byte-identical — documents the coupling and fails if a fit knob later leaks into Gate.
#[test]
fn gate_mode_ignores_fill_fit_knobs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);
    let report = make_report(
        fixture.path_a,
        fixture.path_b,
        stereo_identical_compat(SAMPLE_RATE),
    );

    let gate = PatchTestOptions {
        fill_mode: FillMode::Gate,
        ..Default::default()
    };

    let baseline = patch_request_with_options(report.clone(), false, 5.0, 0.35, gate.clone());
    assert_eq!(baseline.fill_mode, FillMode::Gate);

    let mut perturbed = patch_request_with_options(report, false, 5.0, 0.35, gate);
    perturbed.settings.fill_fit_structure_weight = 0.99;
    perturbed.settings.fill_fit_waveform_weight = 0.01;
    perturbed.settings.fill_fit_nominal_bias_scale = 50.0;
    perturbed.settings.fill_fit_energy_nominal_bias_scale = 50.0;
    perturbed.settings.fill_fit_late_start_penalty_scale = 50.0;

    let baseline_result = run_patch(baseline, 10);
    let perturbed_result = run_patch(perturbed, 10);

    assert_eq!(
        baseline_result.summary.patched_count, 1,
        "baseline Gate patch should succeed: {:?}",
        baseline_result.summary.gaps
    );
    assert_eq!(
        perturbed_result.summary.patched_count, 1,
        "perturbed Gate patch should succeed: {:?}",
        perturbed_result.summary.gaps
    );

    let baseline_pcm = expect_pcm(&baseline_result);
    let perturbed_pcm = expect_pcm(&perturbed_result);
    assert_eq!(
        baseline_pcm.samples, perturbed_pcm.samples,
        "fill_fit_* knobs must not change Gate-mode PCM"
    );
}

#[test]
fn patch_audio_trusts_structure_when_waveform_seams_disagree() {
    let temp = tempfile::tempdir().expect("tempdir");
    // A borders are 440 Hz; B fill is 2200 Hz — waveform Pearson fails, but the
    // active/silent edit pattern still matches (structure-first placement).
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 2200.0, 16_000.0);

    let request = patch_request(
        make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        ),
        false,
        5.0,
        0.35,
    );

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let result = PatchAudio::new(&media_reader, &progress)
        .execute(request, 10)
        .expect("patch should succeed");

    let pcm = expect_pcm(&result);
    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "gap should be filled when structure match is strong, got rms={gap_rms}"
    );

    let pre_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 0.0, 2.0);
    assert!(
        pre_rms > 100.0 / 32767.0,
        "pre-gap audio should be preserved, got rms={pre_rms}"
    );

    assert_eq!(result.summary.patched_count, 1);
    assert_eq!(result.summary.skipped_count, 0);
    match &result.summary.gaps[0].status {
        GapPatchStatus::Patched {
            structure_trusted: true,
            ..
        } => {}
        other => panic!("expected structure-trusted patch, got {other:?}"),
    }
}

/// Whole-program gain, end to end (`docs/dev/TEMP-program-gain-plan.md`): a scan that measured B
/// 6 dB *quieter* than A must make every fill come up by that much, with no hole-local
/// normalization involved — and the JSON must say so twice, as the applied `normalize_gain` and as
/// the run-level `program_gain` that lets a reader factor out the local residual.
#[test]
fn patch_audio_applies_the_whole_program_gain_without_local_normalize() {
    let temp = tempfile::tempdir().expect("tempdir");
    // B is the same frequency but quieter than A's borders — the level difference this run corrects
    // is the *program* one, declared by the scan below, not one measured at the hole.
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 2_500.0);
    let mut report = make_report(
        fixture.path_a,
        fixture.path_b,
        stereo_identical_compat(SAMPLE_RATE),
    );
    report.program_levels = Some(program_levels_offset(-6.0, 0.3));

    // Local normalization OFF: the applied gain must be the program scalar exactly.
    let (_, _, _, summary) =
        patch_to_samples_and_summary(patch_request(report, false, 2.0, 0.35), 10);

    let expected = 10f32.powf(6.0 / 20.0);
    let applied = only_gap_normalize_gain(&summary);
    assert!(
        (applied - expected).abs() < 1e-4,
        "a -6 dB program offset must apply a {expected} boost to the fill, got {applied}"
    );

    let json = serde_json::to_value(&summary).expect("summary serializes");
    let patched = &json["gaps"][0]["status"]["patched"];
    let program_gain = patched["program_gain"]
        .as_f64()
        .expect("program_gain in JSON");
    let normalize_gain = patched["normalize_gain"]
        .as_f64()
        .expect("normalize_gain in JSON");
    assert!(
        (program_gain - f64::from(expected)).abs() < 1e-4,
        "per-gap program_gain must record the run scalar, got {program_gain}"
    );
    // The documented decomposition: g_local = normalize_gain / program_gain, and with local off
    // that residual is exactly 1.
    assert!(
        (normalize_gain / program_gain - 1.0).abs() < 1e-4,
        "local residual must be unity with normalize_fill off, got {}",
        normalize_gain / program_gain
    );

    // And the run-level record of *why*, which is what a human line and a roll-up both read.
    let run = &json["program_gain"];
    assert!(
        run["refused"].is_null(),
        "a 0.3 dB spread at -6 dB must not be refused: {run}"
    );
    assert!(
        (run["offset_db"].as_f64().expect("offset_db") + 6.0).abs() < 1e-9,
        "{run}"
    );
}

/// The same run with a spread past `program_gain_max_spread_db`: no single scalar describes the
/// pair, so the policy refuses rather than applying a number it has reason to disbelieve, and the
/// fill lands at B's own level (unity) — the pre-2026-08-26 behaviour, exactly.
#[test]
fn patch_audio_refuses_the_program_gain_on_a_wide_spread() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 2_500.0);
    let mut report = make_report(
        fixture.path_a,
        fixture.path_b,
        stereo_identical_compat(SAMPLE_RATE),
    );
    report.program_levels = Some(program_levels_offset(-6.0, 9.0));

    let (_, _, _, summary) =
        patch_to_samples_and_summary(patch_request(report, false, 2.0, 0.35), 10);
    assert_eq!(only_gap_normalize_gain(&summary), 1.0);

    let json = serde_json::to_value(&summary).expect("summary serializes");
    assert_eq!(json["program_gain"]["refused"], "spread");
    assert_eq!(json["gaps"][0]["status"]["patched"]["program_gain"], 1.0);
}

/// The hole-local half applies the **paired collar residual** (2026-08-31), so a report carrying a
/// collar moves the fill by exactly `10^(-offset_db/20)` relative to the same run with
/// `--no-normalize`. This replaced `patch_audio_normalizes_fill_loudness_to_a_border`, which
/// asserted the estimator that was deleted: that one boosted a fill toward A's *border RMS*, a
/// different quantity that only coincides with the paired offset on uniform material.
///
/// The gain is asserted against the unnormalized run rather than against A's border level, because
/// the collar is a *stamped measurement* here. That is the point: this test covers the wiring —
/// report → request → characterize → splice, and the same value reaching the record — while the
/// estimator's own numbers are unit-tested in `domain::local_gain` on data that is not a sine.
#[test]
fn patch_audio_applies_the_paired_collar_residual_to_the_fill() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    // B reads 2 dB hot in this hole's collar, so the fill must come DOWN by 2 dB. An attenuation is
    // deliberate: it is the direction the boost-only rail forbade before this estimator landed.
    const COLLAR_DB: f64 = 2.0;
    let want = 10f32.powf(-(COLLAR_DB as f32) / 20.0);

    let (unnormalized, _, _) = patch_to_samples(
        patch_request(
            report_with_collar(
                fixture.path_a.clone(),
                fixture.path_b.clone(),
                stereo_identical_compat(SAMPLE_RATE),
                COLLAR_DB,
            ),
            false,
            2.0,
            0.35,
        ),
        10,
    );
    let (normalized, _, _, summary) = patch_to_samples_and_summary(
        patch_request(
            report_with_collar(
                fixture.path_a,
                fixture.path_b,
                stereo_identical_compat(SAMPLE_RATE),
                COLLAR_DB,
            ),
            true,
            2.0,
            0.35,
        ),
        10,
    );

    // Measure the gap interior, away from the crossfade seams.
    const GAP_INNER_START: f64 = 3.5;
    const GAP_INNER_END: f64 = 5.5;
    let gap_unnorm = rms_region(
        &unnormalized,
        SAMPLE_RATE,
        CHANNELS,
        GAP_INNER_START,
        GAP_INNER_END,
    );
    let gap_norm = rms_region(
        &normalized,
        SAMPLE_RATE,
        CHANNELS,
        GAP_INNER_START,
        GAP_INNER_END,
    );
    assert!(gap_unnorm > 0.0, "the fill must not be silent");
    let ratio = gap_norm / gap_unnorm;
    assert!(
        (ratio - want).abs() < 0.01,
        "fill should scale by the collar residual (want {want}, got {ratio}; \
         unnorm={gap_unnorm}, norm={gap_norm})"
    );

    // The same number reaches the record, with the reason that separates "applied" from the three
    // ways this can be 1.0. `normalize_gain` is the product; program gain is unity on this report
    // (no `program_levels`), so the two coincide here.
    let (local_gain, reason) = only_gap_local_gain(&summary);
    assert!(
        (local_gain - want).abs() < 1e-4,
        "recorded local_gain should be the applied residual, got {local_gain}"
    );
    assert_eq!(reason, clip_sync_repair::domain::LocalGainReason::Applied);
    assert!(
        (only_gap_normalize_gain(&summary) - want).abs() < 1e-4,
        "normalize_gain is the product, and the program half is unity on this report"
    );
}

/// Two ways the local half declines, both unity, both **bit-identical** to `--no-normalize`, and
/// distinguishable only by `local_gain_reason` — which is the entire reason that field exists.
///
/// This replaced `patch_audio_never_attenuates_a_fill_louder_than_a_border`. That test asserted the
/// boost-only floor, which went away with the RMS estimator that needed it (whole-fill RMS let one
/// loud mid-gap event drag the whole patch toward talking-level shoulders; a paired-shoulder scalar
/// cannot). Left as it was it would still have passed — a report with no collar abstains to unity —
/// while claiming to prove a rail that no longer exists.
#[test]
fn patch_audio_local_half_declines_to_unity_and_says_why() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let (unnormalized, _, _) = patch_to_samples(
        patch_request(
            make_report(
                fixture.path_a.clone(),
                fixture.path_b.clone(),
                stereo_identical_compat(SAMPLE_RATE),
            ),
            false,
            2.0,
            0.35,
        ),
        10,
    );

    // (a) No collar on the report at all — nothing paired, or the scan predates the field.
    let (no_collar, _, _, no_collar_summary) = patch_to_samples_and_summary(
        patch_request(
            make_report(
                fixture.path_a.clone(),
                fixture.path_b.clone(),
                stereo_identical_compat(SAMPLE_RATE),
            ),
            true,
            2.0,
            0.35,
        ),
        10,
    );
    assert_eq!(
        only_gap_local_gain(&no_collar_summary),
        (1.0, clip_sync_repair::domain::LocalGainReason::AbstainedN)
    );
    assert_eq!(
        no_collar, unnormalized,
        "an abstaining local half must be sample-identical to --no-normalize"
    );

    // (b) A collar past MAX_LOCAL_RESIDUAL_DB (3.0). It **refuses** rather than clamping to the
    // bound: a reading this far out says the collar is reaching material that is not this hole's
    // neighbourhood, and applying 3 dB of a number you disbelieve is worse than applying none.
    // Note this is also why no gap can pin at a rail the way the old 12 dB clamp made seven of them.
    let (refused, _, _, refused_summary) = patch_to_samples_and_summary(
        patch_request(
            report_with_collar(
                fixture.path_a,
                fixture.path_b,
                stereo_identical_compat(SAMPLE_RATE),
                9.0,
            ),
            true,
            2.0,
            0.35,
        ),
        10,
    );
    assert_eq!(
        only_gap_local_gain(&refused_summary),
        (
            1.0,
            clip_sync_repair::domain::LocalGainReason::RefusedResidual
        )
    );
    assert_eq!(
        refused, unnormalized,
        "a refused local half must be sample-identical to --no-normalize"
    );
}

#[test]
fn patch_audio_resamples_b_when_sample_rates_differ() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, RATE_B, 440.0, 16_000.0);

    let request = patch_request(
        make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_compatible_diff_rate(),
        ),
        false,
        5.0,
        // Resampler phase at the seam can depress correlation slightly; isolate resample behaviour.
        -1.0,
    );

    let (samples, sample_rate, channels) = patch_to_samples(request, 10);
    assert_eq!(
        sample_rate, SAMPLE_RATE,
        "output should be at A's native rate"
    );
    assert_eq!(channels, CHANNELS);

    let gap_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "gap should be filled after resampling B from {RATE_B} Hz, got rms={gap_rms}"
    );
}

#[test]
fn patch_audio_aligns_shifted_b_fill_to_a_borders() {
    const SHIFT_SECS: f64 = 0.05;

    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let mut gap = default_gap();
    // Simulate a coarse alignment error: mapped B window is 50 ms late.
    gap.video_b_start_secs = Some(GAP_START + SHIFT_SECS);
    gap.video_b_end_secs = Some(GAP_END + SHIFT_SECS);

    let mut report = make_report(
        fixture.path_a,
        fixture.path_b,
        stereo_identical_compat(SAMPLE_RATE),
    );
    report.gaps = vec![gap];

    let request = patch_request(report, false, 2.0, 0.35);

    let (samples, _, _) = patch_to_samples(request, 10);

    let gap_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "aligned fill should recover shifted B mapping, got rms={gap_rms}"
    );

    // Seam should stay continuous across the gap (no audible dip at boundaries).
    let pre_last = rms_region(&samples, SAMPLE_RATE, CHANNELS, 2.9, 3.0);
    let gap_open = rms_region(&samples, SAMPLE_RATE, CHANNELS, 3.0, 3.1);
    let gap_close = rms_region(&samples, SAMPLE_RATE, CHANNELS, 5.9, 6.0);
    let post_first = rms_region(&samples, SAMPLE_RATE, CHANNELS, 6.0, 6.1);

    assert!(
        pre_last > 100.0 / 32767.0,
        "pre-gap should stay loud, got rms={pre_last}"
    );
    assert!(
        gap_open > pre_last * 0.5,
        "gap opening should not dip to silence (pre={pre_last}, open={gap_open})"
    );
    assert!(
        gap_close > pre_last * 0.5,
        "gap closing should not dip to silence (pre={pre_last}, close={gap_close})"
    );
    assert!(
        post_first > 100.0 / 32767.0,
        "post-gap should stay loud, got rms={post_first}"
    );
}

#[test]
fn decoded_gap_frames_are_silent_in_sine_fixture() {
    use std::time::Duration;

    use clip_sync::{ClipLabel, ClipWindow, SymphoniaMediaReader};

    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let media_reader = SymphoniaMediaReader;
    let mut session = media_reader
        .open(&clip_sync::MediaSource::new(fixture.path_a.clone()))
        .expect("open A");
    let tracks = session.list_tracks().expect("tracks");
    let track = &tracks[0];
    let duration = track.duration.expect("duration");
    let window = ClipWindow::new(Duration::ZERO, duration, ClipLabel::Interior);
    let pcm = session
        .extract_interleaved(track, &window, &FakeProgressReporter, "probe-a")
        .expect("extract A");

    let gap_frame = (GAP_START * SAMPLE_RATE as f64) as usize;
    let pre_frame = gap_frame.saturating_sub(1);
    assert!(
        !policies::is_silent_frame(&pcm.samples, CHANNELS as usize, pre_frame, 0.01, 0.0),
        "frame before gap should not be silent"
    );
    assert!(
        policies::is_silent_frame(&pcm.samples, CHANNELS as usize, gap_frame, 0.01, 0.0),
        "first gap frame should decode as silent"
    );
}

/// Scanner block quantization can report a gap that starts slightly early and ends slightly
/// early. Patch should refine edges and still splice B with passing seam correlation.
#[test]
fn patch_audio_fills_gap_with_imprecise_scan_boundaries() {
    const BOUNDARY_SLACK_SECS: f64 = 0.25;

    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let imprecise_gap = Gap {
        video_a_start_secs: GAP_START - BOUNDARY_SLACK_SECS,
        video_a_end_secs: GAP_END - BOUNDARY_SLACK_SECS,
        video_b_start_secs: Some(GAP_START - BOUNDARY_SLACK_SECS),
        video_b_end_secs: Some(GAP_END - BOUNDARY_SLACK_SECS),
        b_has_energy: true,
        collar_level: None,
    };

    let mut report = make_report(
        fixture.path_a,
        fixture.path_b,
        stereo_identical_compat(SAMPLE_RATE),
    );
    report.gaps = vec![imprecise_gap];

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let request = patch_request(report, false, 5.0, 0.35);
    let result = PatchAudio::new(&media_reader, &progress)
        .execute(request, 10)
        .expect("patch should succeed");

    assert_eq!(
        result.summary.patched_count, 1,
        "expected patch to succeed, got {:?}",
        result.summary.gaps
    );

    let pcm = expect_pcm(&result);
    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "imprecise scan boundaries should still be filled, got rms={gap_rms}, status={:?}",
        result.summary.gaps[0].status
    );
}

#[test]
fn scan_then_patch_fills_detected_gap() {
    use std::time::Duration;

    use clip_sync::ClipConfig;
    use clip_sync::{AlignConfig, SymphoniaMediaReader};
    use clip_sync_repair::application::{ScanGaps, ScanGapsRequest};

    const SCAN_TOTAL_SECS: u32 = 120;
    const SCAN_GAP_START: u32 = 40;
    const SCAN_GAP_END: u32 = 43;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        SCAN_TOTAL_SECS,
        SCAN_GAP_START,
        SCAN_GAP_END,
        440.0,
        16_000.0,
    );
    write_stereo_sine_wav(&path_b, SAMPLE_RATE, SCAN_TOTAL_SECS, 440.0, 16_000.0);

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let outcome = ScanGaps::new(&media_reader, &progress, &aligner)
        .execute(ScanGapsRequest {
            video_a: path_a.clone(),
            video_b: path_b.clone(),
            align: AlignConfig {
                clip: ClipConfig {
                    clip_length: Duration::from_secs(60),
                    num_clips: 1,
                    target_sample_rate: Some(SAMPLE_RATE),
                    ..ClipConfig::default()
                },
                ..Default::default()
            },
            decode_chunk_secs: 2,
            recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
            scan_both: false,
            gap_offset_tolerance_secs: 0.5,
            stall_min_secs: 10.0,
            limit_fill_to_mapped_region: true,
            apply_donor_registration: true,
            local_pair_collar_secs: 2.0,
        })
        .expect("scan should succeed");

    assert!(
        outcome.fillable_count() >= 1,
        "scan should detect a fillable gap"
    );

    let patch_request = patch_request(outcome.report.clone(), false, 5.0, 0.35);
    let result = PatchAudio::new(&media_reader, &progress)
        .execute(patch_request, 10)
        .expect("patch should succeed");

    assert!(
        result.summary.patched_count >= 1,
        "expected at least one patched gap, got {:?}",
        result.summary.gaps
    );

    let pcm = expect_pcm(&result);
    let gap_rms = rms_region(
        &pcm.samples,
        SAMPLE_RATE,
        CHANNELS,
        f64::from(SCAN_GAP_START),
        f64::from(SCAN_GAP_END),
    );
    assert!(
        gap_rms > 100.0 / 32767.0,
        "scan-detected gap should be filled, got rms={gap_rms}"
    );
}

#[test]
fn patch_audio_skips_when_structure_trust_disabled_and_waveform_disagrees() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 2200.0, 16_000.0);

    let options = PatchTestOptions {
        disable_structure_trust: true,
        ..Default::default()
    };

    let result = run_patch(
        patch_request_with_options(
            make_report(
                fixture.path_a,
                fixture.path_b,
                stereo_identical_compat(SAMPLE_RATE),
            ),
            false,
            5.0,
            0.35,
            options,
        ),
        10,
    );

    assert_eq!(result.summary.patched_count, 0);
    assert_eq!(result.summary.skipped_count, 1);
    match &result.summary.gaps[0].status {
        GapPatchStatus::Skipped {
            reason: GapPatchSkipReason::CorrelationBelowThreshold { .. },
        } => {}
        other => panic!("expected waveform skip with --no-structure-trust, got {other:?}"),
    }
}

/// 6b.3d coverage: the dual-fit rescue path routes through `execute_region_spec`'s SilenceSplice arm. Same
/// inverted-post-border fixture as the one-strong-seam test, but dual-fit ON (production default) — the
/// bracket search is exhausted (the post seam collapses at lag 0), yet dual-fit recovers the post shoulder at
/// its own lag and rescues the gap. Pins that the rescued patch is produced with `dual_fit_used: true` — the
/// fast-tier gate for the SilenceSplice executor arm (previously covered only by the validation-tier
/// `validate_dual_fit_oracle`).
#[test]
fn patch_audio_dual_fit_rescues_inverted_post_border() {
    const SHORT_GAP_END: f64 = 4.0;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        SHORT_GAP_END as u32,
        440.0,
        16_000.0,
    );
    write_stereo_sine_with_inverted_region(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        SHORT_GAP_END - 0.05,
        SHORT_GAP_END + 0.50,
        440.0,
        16_000.0,
    );

    let gap = default_gap();
    let gap = Gap {
        video_a_end_secs: SHORT_GAP_END,
        video_b_end_secs: Some(SHORT_GAP_END),
        ..gap
    };
    let report = make_report(path_a, path_b, stereo_identical_compat(SAMPLE_RATE));
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![gap],
        ..report
    };

    // Bracket search exhausted (post seam collapses at lag 0), all fallback mechanisms off — only dual-fit
    // can rescue. `dual_fit` defaults to the production value (on).
    let opts = PatchTestOptions {
        short_gap_one_strong_seam_fallback: false,
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        disable_structure_trust: true,
        partial_structure_waveform_soften: 1.0,
        ..Default::default()
    };

    let patched = run_patch(
        patch_request_with_options(report, false, 2.0, 0.12, opts),
        10,
    );
    assert_eq!(
        patched.summary.patched_count, 1,
        "dual-fit should rescue the inverted post border, got {:?}",
        patched.summary.gaps
    );
    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            dual_fit_used: true,
            ..
        } => {}
        other => panic!("expected a dual-fit-rescued patch (dual_fit_used), got {other:?}"),
    }
    let pcm = expect_pcm(&patched);
    let gap_rms = rms_region(
        &pcm.samples,
        SAMPLE_RATE,
        CHANNELS,
        GAP_START,
        SHORT_GAP_END,
    );
    assert!(
        gap_rms > 100.0 / 32767.0,
        "dual-fit-rescued gap should contain audio, rms={gap_rms}"
    );
}

/// seam-lock rigid-fill (docs/dev/archive/TEMP-seam-lock-rigid-fill-plan.md § 3.1) coverage: both shoulders are a
/// *constant*-shift copy of B relative to A (same lag pre and post — no step), placed far enough outside
/// the bracket search's radius that keep-path finds nothing at lag 0, but well inside dual-fit's fixed
/// per-shoulder search. Both shoulder locks land on the same lag, so the rescue is rigid: `dual_fit_used:
/// true, dual_fit_step_real: Some(false)`, trim-0 seam-lock fill — not the length-edit ("real step") arm
/// covered by `patch_audio_dual_fit_rescues_inverted_post_border`.
#[test]
fn patch_audio_dual_fit_rescues_constant_shift_as_rigid_seam_lock() {
    const SHIFT_SECS: f64 = 0.35;
    // Cover the whole timeline: a matching sine tone outside the band (as the earlier revision used)
    // gives the correlation gate a "free" match at lag 0 regardless of the noise band's shift, since the
    // gate's correlation window extends well past the 1s shoulders. Only the file's first/last
    // `SHIFT_SECS` fall outside the noise band (B's band starts late by the shift) — far from the gap.
    const REGION_START: f64 = 0.0;
    const REGION_END: f64 = TOTAL_SECS as f64;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");

    write_stereo_with_shifted_noise_band(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        REGION_START,
        REGION_END,
        0.0,
        7,
        16_000.0,
        Some((GAP_START, GAP_END)),
        440.0,
        8_000.0,
    );
    write_stereo_with_shifted_noise_band(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        REGION_START,
        REGION_END,
        SHIFT_SECS,
        7,
        16_000.0,
        None,
        440.0,
        8_000.0,
    );

    let report = make_report(path_a, path_b, stereo_identical_compat(SAMPLE_RATE));

    // Bracket search radius far below SHIFT_SECS: keep-path cannot reach the shared lock lag. Dual-fit's
    // per-shoulder search (fixed ±600ms, independent of this option) still can.
    let opts = PatchTestOptions {
        fill_border_search_secs: 0.05,
        short_gap_one_strong_seam_fallback: false,
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        disable_structure_trust: true,
        partial_structure_waveform_soften: 1.0,
        ..Default::default()
    };

    let patched = run_patch(
        patch_request_with_options(report, false, 2.0, 0.12, opts),
        10,
    );
    assert_eq!(
        patched.summary.patched_count, 1,
        "dual-fit should rescue the constant-shift gap as a rigid seam-lock, got {:?}",
        patched.summary.gaps
    );
    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            dual_fit_used: true,
            dual_fit_step_real: Some(false),
            ..
        } => {}
        other => panic!(
            "expected a rigid dual-fit rescue (dual_fit_used, dual_fit_step_real: Some(false)), got {other:?}"
        ),
    }
    let pcm = expect_pcm(&patched);
    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "rigid-rescued gap should contain audio, rms={gap_rms}"
    );
}

/// Registration routing (docs/dev/TEMP-dual-fit-registration-routing-plan.md § 5) is step-kind only:
/// a common-mode miss (equal shoulders) never routes, whatever the bound. A noise fixture whose B is an
/// unshifted copy of A, so the single-lag fill passes the gate at lag 0 and both shoulders read the same
/// 10 ms crossfade-width error in the written frame — common-mode, even under a 1 ms bound.
/// The positive step case needs a B with different pre/post lags (synthetic step fixture, still to build).
#[test]
fn registration_routing_leaves_a_common_mode_fill_alone_at_any_bound() {
    const SHIFT_SECS: f64 = 0.0;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_with_shifted_noise_band(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        0.0,
        TOTAL_SECS as f64,
        0.0,
        7,
        16_000.0,
        Some((GAP_START, GAP_END)),
        440.0,
        8_000.0,
    );
    write_stereo_with_shifted_noise_band(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        0.0,
        TOTAL_SECS as f64,
        SHIFT_SECS,
        7,
        16_000.0,
        None,
        440.0,
        8_000.0,
    );

    let run = |bound: Option<f64>| {
        let opts = PatchTestOptions {
            short_gap_one_strong_seam_fallback: false,
            gap_end_extend_on_post_seam_fail: false,
            gap_start_extend_on_pre_seam_fail: false,
            disable_structure_trust: true,
            partial_structure_waveform_soften: 1.0,
            dual_fit_registration_bound_ms: bound,
            ..Default::default()
        };
        let report = make_report(
            path_a.clone(),
            path_b.clone(),
            stereo_identical_compat(SAMPLE_RATE),
        );
        run_patch(
            patch_request_with_options(report, false, 2.0, 0.12, opts),
            10,
        )
    };
    let dual_fit_used =
        |r: &clip_sync_repair::application::PatchAudioResult| match &r.summary.gaps[0].status {
            GapPatchStatus::Patched { dual_fit_used, .. } => *dual_fit_used,
            other => panic!("routing must never turn a patch into a skip, got {other:?}"),
        };

    let off = run(None);
    assert!(
        !dual_fit_used(&off),
        "premise: the wide bracket search should pass the gate on its own"
    );
    assert!(
        !dual_fit_used(&run(Some(50.0))),
        "a bound above the written-frame error must leave the single-lag fill alone"
    );
    assert!(
        !dual_fit_used(&run(Some(1.0))),
        "a common-mode miss must not route, even far past the bound"
    );
}

/// Step-kind routing: B's lag steps 40 ms at the gap midpoint, so no single lag registers both shoulders.
fn step_fixture(dir: &Path) -> (PathBuf, PathBuf) {
    step_fixture_with_dropout(dir, None)
}

fn step_fixture_with_dropout(dir: &Path, b_dropout_secs: Option<(f64, f64)>) -> (PathBuf, PathBuf) {
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    clip_sync_repair_fixtures::registration_step::write_registration_step_pair(
        &clip_sync_repair_fixtures::registration_step::RegistrationStepSpec {
            sample_rate: SAMPLE_RATE,
            total_secs: f64::from(TOTAL_SECS),
            gap_start_secs: GAP_START,
            gap_end_secs: GAP_END,
            pre_shift_secs: 0.0,
            post_shift_secs: 0.040,
            seed: 7,
            amplitude: 0.5,
            periodic_hz: Some(100.0),
            b_dropout_secs,
        },
        &a,
        &b,
    );
    (a, b)
}

fn run_step_fixture(
    a: &Path,
    b: &Path,
    bound: Option<f64>,
) -> clip_sync_repair::application::PatchAudioResult {
    let opts = PatchTestOptions {
        short_gap_one_strong_seam_fallback: false,
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        disable_structure_trust: true,
        partial_structure_waveform_soften: 1.0,
        dual_fit_registration_bound_ms: bound,
        fill_mode: FillMode::Fit,
        ..Default::default()
    };
    let report = make_report(
        a.to_path_buf(),
        b.to_path_buf(),
        stereo_identical_compat(SAMPLE_RATE),
    );
    run_patch(
        patch_request_with_options(report, false, 2.0, 0.12, opts),
        10,
    )
}

/// Step-kind registration routing replaces a gate-passing single-lag fill with dual-fit only when the bound
/// is set below the step's far-seam error (written frame: 40 ms step + 10 ms crossfade = 50 ms), and never
/// turns the patch into a skip. The 40 ms step is four periods of the 100 Hz tone, so the waveform seams
/// still pass at the wrong lag and the gate accepts the single-lag fill on its own.
#[test]
fn registration_routing_replaces_a_gate_passing_step_fill_only_past_the_bound() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (a, b) = step_fixture(temp.path());
    let dual_fit_used =
        |bound: Option<f64>| match &run_step_fixture(&a, &b, bound).summary.gaps[0].status {
            GapPatchStatus::Patched { dual_fit_used, .. } => *dual_fit_used,
            other => panic!("routing must never turn a patch into a skip, got {other:?}"),
        };
    assert!(
        !dual_fit_used(None),
        "premise: the gate passes the single-lag fill on its own"
    );
    assert!(
        !dual_fit_used(Some(75.0)),
        "a bound above the 50 ms written-frame error must not route"
    );
    assert!(
        dual_fit_used(Some(30.0)),
        "a bound below the 50 ms written-frame error must route"
    );
    let route = |bound| {
        run_step_fixture(&a, &b, bound).summary.gaps[0]
            .tags
            .dual_fit_route
    };
    assert_eq!(route(None), None, "no dual-fit, no route");
    assert_eq!(route(Some(30.0)), Some(DualFitRoute::RegistrationStep));
}

/// Plan § 2 invariant 1: when routing fires but dual-fit declines (here the donor bridge has an 800 ms
/// dropout, so the aligned span is not continuous), the single-lag fill is kept — the patch is neither
/// skipped nor tagged as dual-fit, and is byte-identical to the routing-off result.
#[test]
fn registration_routing_decline_keeps_the_single_lag_fill() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (a, b) = step_fixture_with_dropout(temp.path(), Some((4.1, 4.9)));
    let off = run_step_fixture(&a, &b, None);
    let on = run_step_fixture(&a, &b, Some(30.0));
    for (label, result) in [("off", &off), ("on", &on)] {
        match &result.summary.gaps[0].status {
            GapPatchStatus::Patched { dual_fit_used, .. } => assert!(
                !dual_fit_used,
                "{label}: a declined route must keep the single-lag fill"
            ),
            other => panic!("{label}: routing must never turn a patch into a skip, got {other:?}"),
        }
    }
    assert_eq!(on.summary.gaps[0].tags.dual_fit_route, None);
    assert_eq!(
        on.pcm.as_ref().map(|p| &p.samples),
        off.pcm.as_ref().map(|p| &p.samples),
        "a declined route must leave the output bit-identical to routing off"
    );
}

#[test]
fn patch_audio_short_gap_one_strong_seam_fallback_enables_patch() {
    const SHORT_GAP_END: f64 = 4.0;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        SHORT_GAP_END as u32,
        440.0,
        16_000.0,
    );
    // Invert B's post-gap border so Pearson post correlation collapses while pre stays high.
    write_stereo_sine_with_inverted_region(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        SHORT_GAP_END - 0.05,
        SHORT_GAP_END + 0.50,
        440.0,
        16_000.0,
    );

    let gap = default_gap();
    let gap = Gap {
        video_a_end_secs: SHORT_GAP_END,
        video_b_end_secs: Some(SHORT_GAP_END),
        ..gap
    };

    let report = make_report(path_a, path_b, stereo_identical_compat(SAMPLE_RATE));
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![gap],
        ..report
    };

    let strict = PatchTestOptions {
        short_gap_one_strong_seam_fallback: false,
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        disable_structure_trust: true,
        partial_structure_waveform_soften: 1.0,
        // Isolate the one-strong-seam mechanism: dual-fit (on by default post-A3b–A7) otherwise rescues the
        // inverted-post-border fixture at the post shoulder's own lag, masking what this test measures.
        dual_fit: false,
        ..Default::default()
    };

    let skipped = run_patch(
        patch_request_with_options(report.clone(), false, 2.0, 0.12, strict),
        10,
    );
    assert_eq!(
        skipped.summary.patched_count, 0,
        "mean seam score should fail without one-strong-seam fallback, got {:?}",
        skipped.summary.gaps
    );

    // One-strong-seam fallback is disabled when `--no-structure-trust` is on (require both seams).
    let no_trust_with_fallback = PatchTestOptions {
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        disable_structure_trust: true,
        partial_structure_waveform_soften: 1.0,
        dual_fit: false, // isolate one-strong-seam (see `strict` above)
        ..Default::default()
    };
    let still_skipped = run_patch(
        patch_request_with_options(report.clone(), false, 2.0, 0.12, no_trust_with_fallback),
        10,
    );
    assert_eq!(
        still_skipped.summary.patched_count, 0,
        "one-strong-seam fallback must not apply with disable_structure_trust, got {:?}",
        still_skipped.summary.gaps
    );

    let relaxed = PatchTestOptions {
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        disable_structure_trust: false,
        // Above fixture structure scores (~1.0) so the waveform gate runs and one-strong-seam applies.
        strong_structure_trust: 1.01,
        partial_structure_waveform_soften: 1.0,
        dual_fit: false, // isolate one-strong-seam: the patch must come from the fallback, not dual-fit
        ..Default::default()
    };

    let patched = run_patch(
        patch_request_with_options(report, false, 2.0, 0.12, relaxed),
        10,
    );
    assert_eq!(
        patched.summary.patched_count, 1,
        "one-strong-seam fallback should patch when a single seam passes, got {:?}",
        patched.summary.gaps
    );

    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            pre_correlation,
            post_correlation,
            structure_trusted: false,
            ..
        } => {
            assert!(
                *pre_correlation >= 0.12,
                "pre seam should pass threshold, got {pre_correlation}"
            );
            assert!(
                *post_correlation < 0.12,
                "post seam should fail threshold, got {post_correlation}"
            );
        }
        other => panic!("expected patched gap, got {other:?}"),
    }

    let pcm = expect_pcm(&patched);
    let gap_rms = rms_region(
        &pcm.samples,
        SAMPLE_RATE,
        CHANNELS,
        GAP_START,
        SHORT_GAP_END,
    );
    assert!(
        gap_rms > 100.0 / 32767.0,
        "gap should contain audio after one-strong-seam patch, rms={gap_rms}"
    );
}

#[test]
fn patch_audio_gap_end_extension_retries_failed_post_seam() {
    const EARLY_GAP_END: f64 = 5.85;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    write_stereo_sine_with_inverted_region(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_END - 0.20,
        GAP_END + 0.20,
        440.0,
        16_000.0,
    );

    let gap = Gap {
        video_a_start_secs: GAP_START,
        video_a_end_secs: EARLY_GAP_END,
        video_b_start_secs: Some(GAP_START),
        video_b_end_secs: Some(EARLY_GAP_END),
        b_has_energy: true,
        collar_level: None,
    };

    let report = make_report(path_a, path_b, stereo_identical_compat(SAMPLE_RATE));
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![gap],
        ..report
    };

    let no_extend = PatchTestOptions {
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        short_gap_one_strong_seam_fallback: false,
        disable_structure_trust: true,
        partial_structure_waveform_soften: 1.0,
        // Isolate gap-end extension: dual-fit (on by default post-A3b–A7) otherwise rescues the inverted
        // post-border fixture at the post shoulder's own lag, masking the extension mechanism under test.
        dual_fit: false,
        ..Default::default()
    };

    let skipped = run_patch(
        patch_request_with_options(report.clone(), false, 5.0, 0.35, no_extend),
        10,
    );
    assert_eq!(
        skipped.summary.patched_count, 0,
        "inverted post border should fail without extension, got {:?}",
        skipped.summary.gaps
    );

    let with_extend = PatchTestOptions {
        short_gap_one_strong_seam_fallback: false,
        disable_structure_trust: true,
        partial_structure_waveform_soften: 1.0,
        dual_fit: false, // isolate extension: the patch must come from extension, not dual-fit
        ..Default::default()
    };

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, with_extend),
        10,
    );
    assert_eq!(
        patched.summary.patched_count, 1,
        "gap-end extension should recover weak post seam, got {:?}",
        patched.summary.gaps
    );

    let pcm = expect_pcm(&patched);
    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "extended gap should be filled, rms={gap_rms}"
    );
}

#[test]
fn patch_audio_fit_mode_joint_gap_end_extension_patches_weak_post_seam() {
    const EARLY_GAP_END: f64 = 5.85;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    // Post seam at the early gap end sits inside this inverted band; extending the gap end
    // moves the seam onto clean B audio (B slide is capped by fast_fit_patch_options).
    write_stereo_sine_with_inverted_region(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        EARLY_GAP_END - 0.05,
        GAP_END + 0.10,
        440.0,
        16_000.0,
    );

    let gap = Gap {
        video_a_start_secs: GAP_START,
        video_a_end_secs: EARLY_GAP_END,
        video_b_start_secs: Some(GAP_START),
        video_b_end_secs: Some(EARLY_GAP_END),
        b_has_energy: true,
        collar_level: None,
    };

    let report = make_report(
        path_a.clone(),
        path_b.clone(),
        stereo_identical_compat(SAMPLE_RATE),
    );
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![gap],
        ..report
    };

    let mut no_extend = fast_fit_patch_options();
    no_extend.gap_end_extend_on_post_seam_fail = false;
    no_extend.gap_start_extend_on_pre_seam_fail = false;
    // Isolate joint gap-end extension: dual-fit (on by default) otherwise rescues the inverted post border.
    no_extend.dual_fit = false;

    let skipped = run_patch(
        patch_request_with_options(report.clone(), false, 5.0, 0.35, no_extend),
        10,
    );
    assert_eq!(
        skipped.summary.patched_count, 0,
        "without boundary search the weak post seam should skip, got {:?}",
        skipped.summary.gaps
    );

    let mut patched_opts = fast_fit_patch_options();
    patched_opts.dual_fit = false; // isolate: the patch must come from joint extension, not dual-fit
    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, patched_opts),
        10,
    );
    assert_eq!(
        patched.summary.patched_count, 1,
        "fit joint boundary search should recover weak post seam, got {:?}",
        patched.summary.gaps
    );

    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            gap_end_adjust_frames,
            ..
        } => {
            assert!(
                *gap_end_adjust_frames > 0,
                "expected gap end extension via joint search, got {gap_end_adjust_frames}"
            );
        }
        other => panic!("expected patched gap, got {other:?}"),
    }

    let pcm = expect_pcm(&patched);
    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "extended gap should be filled, rms={gap_rms}"
    );
}

#[test]
fn patch_audio_full_profile_runs_boundary_grid_when_baseline_insufficient() {
    const EARLY_GAP_END: f64 = 5.85;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    write_stereo_sine_with_inverted_region(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        EARLY_GAP_END - 0.05,
        GAP_END + 0.10,
        440.0,
        16_000.0,
    );

    let gap = Gap {
        video_a_start_secs: GAP_START,
        video_a_end_secs: GAP_END,
        video_b_start_secs: Some(GAP_START),
        video_b_end_secs: Some(GAP_END),
        b_has_energy: true,
        collar_level: None,
    };
    let report = make_report(
        path_a.clone(),
        path_b.clone(),
        stereo_identical_compat(SAMPLE_RATE),
    );
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![gap],
        ..report
    };

    let mut full_profile = fast_fit_patch_options();
    full_profile.profile = RepairProfile::Full;
    full_profile.fit_boundary_search = FitBoundarySearch::FullGrid;

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, full_profile),
        10,
    );
    assert_eq!(patched.summary.patched_count, 1);
    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            gap_end_adjust_frames,
            ..
        } => assert!(
            *gap_end_adjust_frames > 0,
            "full profile should shift gap end via boundary grid"
        ),
        other => panic!("expected patched gap under full profile, got {other:?}"),
    }
}

#[test]
fn patch_audio_fit_mode_marginal_tier_patches_with_warn_confidence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    // Distort B's post-gap border (the waveform post template window starts at gap end).
    write_stereo_sine_with_seam_distortion(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        SeamDistortionWindow {
            start_secs: GAP_END,
            end_secs: GAP_END + 0.25,
            mix_inverted: 0.49,
        },
        SineTone {
            freq: 440.0,
            amplitude: 16_000.0,
        },
    );

    let report = make_report(path_a, path_b, stereo_identical_compat(SAMPLE_RATE));
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![default_gap()],
        ..report
    };

    let mut marginal_opts = fast_fit_patch_options();
    marginal_opts.gap_end_extend_on_post_seam_fail = false;
    marginal_opts.gap_start_extend_on_pre_seam_fail = false;

    // Threshold just above measured post seam (~0.99) so the patch lands in the warn tier.
    const MARGINAL_THRESHOLD: f32 = 0.999;

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, MARGINAL_THRESHOLD, marginal_opts),
        10,
    );
    assert_eq!(
        patched.summary.patched_count, 1,
        "marginal seam scores should still patch, got {:?}",
        patched.summary.gaps
    );
    assert_eq!(
        patched.summary.patched_marginal_count, 1,
        "expected one marginal patch, got {:?}",
        patched.summary
    );

    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            pre_correlation,
            post_correlation,
            confidence: FillConfidence::Marginal,
            structure_trusted: false,
            ..
        } => {
            let min_score = pre_correlation.min(*post_correlation);
            assert!(
                min_score < f64::from(MARGINAL_THRESHOLD)
                    && min_score >= f64::from(MARGINAL_THRESHOLD - 0.08),
                "expected marginal band scores, got pre={pre_correlation} post={post_correlation}"
            );
        }
        other => panic!("expected marginal patched gap, got {other:?}"),
    }

    let pcm = expect_pcm(&patched);
    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0 / 32767.0,
        "marginal patch should still splice audio, rms={gap_rms}"
    );
}

#[test]
fn patch_audio_fit_mode_high_confidence_on_clean_fixture() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    // B is continuous — a real dropout has donor content at the mapped span (G5/D11).
    write_stereo_sine_wav(&path_b, SAMPLE_RATE, TOTAL_SECS, 440.0, 16_000.0);

    let report = make_report(path_a, path_b, stereo_identical_compat(SAMPLE_RATE));
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![default_gap()],
        ..report
    };

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, fast_fit_patch_options()),
        10,
    );
    assert_eq!(patched.summary.patched_count, 1);
    assert_eq!(patched.summary.patched_marginal_count, 0);

    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            confidence: FillConfidence::High,
            gap_start_adjust_frames,
            gap_end_adjust_frames,
            ..
        } => {
            assert_eq!(*gap_start_adjust_frames, 0);
            assert_eq!(*gap_end_adjust_frames, 0);
        }
        other => panic!("expected high-confidence patched gap, got {other:?}"),
    }
}

#[test]
fn patch_audio_interpolated_offset_maps_late_gap_with_drift() {
    const TIMELINE_SECS: u32 = 120;
    const LATE_GAP_START: f64 = 90.0;
    const LATE_GAP_END: f64 = 93.0;
    const START_OFFSET: f64 = 0.0;
    const END_OFFSET: f64 = 1.0;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");

    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TIMELINE_SECS,
        LATE_GAP_START as u32,
        LATE_GAP_END as u32,
        440.0,
        16_000.0,
    );
    // B matches end-clip drift: occupant noise in (91–94) instead of silence (G5-safe).
    write_stereo_sine_wav(&path_b, SAMPLE_RATE, TIMELINE_SECS, 440.0, 16_000.0);
    patch_wav_noise_region(
        &path_b,
        SAMPLE_RATE,
        LATE_GAP_START + END_OFFSET,
        LATE_GAP_END + END_OFFSET,
        0xCA7E_6A90,
        500.0,
    );

    let gap = Gap {
        video_a_start_secs: LATE_GAP_START,
        video_a_end_secs: LATE_GAP_END,
        video_b_start_secs: Some(LATE_GAP_START + START_OFFSET),
        video_b_end_secs: Some(LATE_GAP_END + START_OFFSET),
        b_has_energy: true,
        collar_level: None,
    };

    let alignment = make_drift_alignment(START_OFFSET, END_OFFSET, f64::from(TIMELINE_SECS));
    let report = make_report_with_alignment(
        path_a,
        path_b,
        stereo_identical_compat(SAMPLE_RATE),
        alignment,
        vec![gap],
    );

    let recommended_opts = PatchTestOptions {
        short_gap_one_strong_seam_fallback: false,
        short_gap_mean_correlation_secs: 0.5,
        disable_structure_trust: true,
        ..Default::default()
    };

    let recommended = run_patch(
        patch_request_with_options(report.clone(), false, 5.0, 0.35, recommended_opts),
        10,
    );
    assert_eq!(
        recommended.summary.patched_count, 0,
        "start-clip offset should not bracket late gap on B, got {:?}",
        recommended.summary.gaps
    );

    let interpolated = PatchTestOptions {
        fill_offset_mode: FillOffsetMode::Interpolated,
        short_gap_one_strong_seam_fallback: false,
        short_gap_mean_correlation_secs: 0.5,
        ..Default::default()
    };

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, interpolated),
        10,
    );
    assert_eq!(
        patched.summary.patched_count, 1,
        "interpolated offset should patch late gap under drift, got {:?}",
        patched.summary.gaps
    );

    let pcm = expect_pcm(&patched);
    let gap_rms = rms_region(
        &pcm.samples,
        SAMPLE_RATE,
        CHANNELS,
        LATE_GAP_START,
        LATE_GAP_END,
    );
    assert!(
        gap_rms > 100.0 / 32767.0,
        "late gap should be filled with interpolated offset, rms={gap_rms}"
    );
}

#[test]
fn patch_audio_fit_default_repeat_penalty_patches_clean_fixture() {
    let opts = fast_fit_patch_options();
    assert!(
        (opts.fill_repeat_penalty_weight - 0.4).abs() < 1e-9,
        "fit integration should exercise shipped repeat-penalty default"
    );

    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![default_gap()],
        ..make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        )
    };

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, opts),
        10,
    );
    assert_eq!(patched.summary.patched_count, 1);
    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            confidence: FillConfidence::High,
            ..
        } => {}
        other => {
            panic!("expected high-confidence patch with default repeat penalty, got {other:?}")
        }
    }
}

/// Regression guard for the gap-index axis defect: `PatchOffsetAnchor::source_gap_index` must be the
/// donor's position in `GapReport.gaps`, **not** its ordinal within `GapFillPlan.regions`. The two
/// coincide only when every gap is planned — which is why the sibling anchor assertions passed while
/// the value was wrong. Making gap #1 unfillable puts one plan-time skip ahead of every donor, so a
/// region ordinal would land one gap low and point at the skipped gap itself.
#[test]
fn patch_anchor_source_gap_index_is_report_index_after_plan_time_skip() {
    let mut fixture = build_drift_anchor_retry_fixture();
    // Plan-time skip on the first gap (no B energy → `NotFillable`): `regions` now starts at report 1.
    fixture.report.gaps[0].b_has_energy = false;
    let report_gaps = fixture.report.gaps.clone();

    // Match the sibling anchored-retry tests: dual-fit off so pass-1 outcomes stay predictable.
    let mut opts = anchored_retry_drift_patch_options();
    opts.dual_fit = false;
    let patched = run_patch(
        patch_request_with_options(
            fixture.report,
            false,
            5.0,
            DRIFT_ANCHOR_MIN_CORRELATION,
            opts,
        ),
        10,
    );

    assert!(
        matches!(
            patched.summary.gaps[0].status,
            GapPatchStatus::NotPlanned { .. }
        ),
        "fixture premise: gap #1 must be skipped at plan time, got {:?}",
        patched.summary.gaps[0].status
    );

    let anchors = patched
        .summary
        .patch_anchors_used
        .as_ref()
        .expect("expected pass-1 anchors to be exported");
    assert!(
        !anchors.is_empty(),
        "fixture premise: pass 1 must produce at least one anchor"
    );

    for anchor in anchors {
        // The rendered label is `gap #{source_gap_index + 1}`; it must name a gap that was actually
        // attempted, never the unfillable one the same run reports as `not planned`.
        assert_ne!(
            anchor.source_gap_index, 0,
            "anchor names the plan-skipped gap #1, so it is carrying a region ordinal: {anchors:?}"
        );
        let donor = &patched.summary.gaps[anchor.source_gap_index];
        assert!(
            matches!(donor.status, GapPatchStatus::Patched { .. }),
            "anchor source gap #{} must be a patched gap, got {:?}",
            anchor.source_gap_index + 1,
            donor.status
        );
        // Axis check independent of status: the anchor sits at its own gap's midpoint on A.
        let gap = &report_gaps[anchor.source_gap_index];
        assert!(
            anchor.a_secs >= gap.video_a_start_secs && anchor.a_secs <= gap.video_a_end_secs,
            "anchor at {:.3}s is outside report gap #{} ({:.3}–{:.3}s)",
            anchor.a_secs,
            anchor.source_gap_index + 1,
            gap.video_a_start_secs,
            gap.video_a_end_secs
        );
    }
}

#[test]
fn patch_audio_anchored_retry_passes_on_clean_single_gap() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![default_gap()],
        ..make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        )
    };

    let opts = PatchTestOptions {
        fill_offset_mode: FillOffsetMode::AnchoredRetry,
        ..fast_fit_patch_options()
    };
    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, opts),
        10,
    );
    assert_eq!(patched.summary.patched_count, 1);
    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            confidence: FillConfidence::High,
            ..
        } => {}
        other => panic!("expected anchored_retry to patch clean gap, got {other:?}"),
    }
}

// Slowest row in this binary (~10–15 min debug solo; use `--test-threads=1`).
#[test]
fn patch_audio_anchored_retry_pass2_recovers_hard_gap_using_easy_anchors() {
    let fixture = build_drift_anchor_retry_fixture();
    // Isolate anchored-retry: dual-fit (on by default post-A3b–A7) otherwise rescues the hard tail in pass 1,
    // breaking the "hard gap stays skipped so pass-2 recovers it" premise this test gates on.
    let mut opts = anchored_retry_drift_patch_options();
    opts.dual_fit = false;
    let interpolated_opts = PatchTestOptions {
        fill_offset_mode: FillOffsetMode::Interpolated,
        fill_mode: FillMode::Fit,
        fill_border_search_secs: opts.fill_border_search_secs,
        fill_align_margin_secs: opts.fill_align_margin_secs,
        gap_signature_context_secs: opts.gap_signature_context_secs,
        fill_length_slack_secs: opts.fill_length_slack_secs,
        fill_extract_tail_slack_secs: opts.fill_extract_tail_slack_secs,
        gap_end_extend_max_ms: opts.gap_end_extend_max_ms,
        gap_end_extend_step_ms: opts.gap_end_extend_step_ms,
        gap_end_extend_on_post_seam_fail: opts.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: opts.gap_start_extend_on_pre_seam_fail,
        short_gap_one_strong_seam_fallback: false,
        short_gap_mean_correlation_secs: opts.short_gap_mean_correlation_secs,
        fill_absolute_floor: opts.fill_absolute_floor,
        dual_fit: false,
        ..Default::default()
    };

    const INTERPOLATED_HARD_GAP_MIN_CORRELATION: f32 = 0.78;

    let interpolated_only = run_patch(
        patch_request_with_options(
            fixture.report.clone(),
            false,
            5.0,
            INTERPOLATED_HARD_GAP_MIN_CORRELATION,
            interpolated_opts,
        ),
        10,
    );
    assert_fit_interpolated_only_patches_bridge_gaps(&interpolated_only.summary);

    let anchored = run_patch(
        patch_request_with_options(
            fixture.report,
            false,
            5.0,
            DRIFT_ANCHOR_MIN_CORRELATION,
            opts,
        ),
        10,
    );
    assert_eq!(
        anchored.summary.patched_count, 4,
        "fit pass 1 should patch bridge gaps; hard tail stays skipped (pass-2 recovery: gate test), got {:?}",
        anchored.summary.gaps
    );
    let anchors = anchored
        .summary
        .patch_anchors_used
        .as_ref()
        .expect("expected pass-1 anchors to be exported");
    assert!(
        anchors.len() >= 2,
        "expected pass-1 anchors from gaps that patch inside the search window, got {anchors:?}"
    );

    match &anchored.summary.gaps[4].status {
        GapPatchStatus::Skipped { reason } => assert!(
            matches!(
                reason,
                GapPatchSkipReason::CorrelationBelowThreshold { .. }
                    | GapPatchSkipReason::BoundaryAlignmentFailed
                    | GapPatchSkipReason::ProgramQuiet
            ),
            "hard gap should stay skipped in fit mode under G5-safe fixture, got {reason:?}"
        ),
        GapPatchStatus::Patched {
            confidence: FillConfidence::High,
            ..
        } => {}
        other => panic!("unexpected hard gap outcome: {other:?}"),
    }
}

#[test]
fn patch_audio_anchored_retry_pass2_recovers_hard_gap_gate_mode() {
    let fixture = build_gate_anchor_retry_fixture();
    let opts = anchored_retry_drift_gate_patch_options();
    let interpolated_opts = PatchTestOptions {
        fill_offset_mode: FillOffsetMode::Interpolated,
        fill_mode: FillMode::Gate,
        fill_border_search_secs: 0.32,
        fill_align_margin_secs: opts.fill_align_margin_secs,
        gap_signature_context_secs: opts.gap_signature_context_secs,
        fill_length_slack_secs: opts.fill_length_slack_secs,
        fill_extract_tail_slack_secs: opts.fill_extract_tail_slack_secs,
        gap_end_extend_max_ms: 0,
        gap_end_extend_step_ms: opts.gap_end_extend_step_ms,
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        short_gap_one_strong_seam_fallback: false,
        short_gap_mean_correlation_secs: opts.short_gap_mean_correlation_secs,
        // Force strict waveform gate so partial soften cannot pass the hard tail on clip offset alone.
        partial_structure_waveform_soften: 2.0,
        disable_structure_trust: true,
        ..Default::default()
    };

    let interpolated_only = run_patch(
        patch_request_with_options(fixture.report.clone(), false, 5.0, 0.35, interpolated_opts),
        10,
    );
    assert!(
        interpolated_only.summary.patched_count >= 1,
        "expected at least one interior gap to patch under interpolated clip offset: {:?}",
        interpolated_only.summary.gaps
    );
    assert!(
        interpolated_only.summary.patched_count < fixture.report.gaps.len(),
        "interpolated-only should leave at least one gap unpatched: {:?}",
        interpolated_only.summary.gaps
    );
    assert_hard_gap_skipped_interpolated_only(&interpolated_only.summary);

    let anchored = run_patch(
        patch_request_with_options(fixture.report.clone(), false, 5.0, 0.35, opts.clone()),
        10,
    );
    assert!(
        anchored.summary.patched_count >= interpolated_only.summary.patched_count,
        "gate + anchored_retry should not regress interpolated-only: interpolated={:?}, anchored={:?}",
        interpolated_only.summary.gaps,
        anchored.summary.gaps
    );

    let anchors = anchored
        .summary
        .patch_anchors_used
        .as_ref()
        .expect("expected pass-1 anchors from interior gaps");
    assert!(
        !anchors.is_empty(),
        "expected pass-1 waveform anchors from interior gaps, got {anchors:?}"
    );

    match &anchored.summary.gaps[4].status {
        GapPatchStatus::Patched {
            structure_trusted: false,
            ..
        } => {
            let pcm = expect_pcm(&anchored);
            let gap_rms = rms_region(
                &pcm.samples,
                SAMPLE_RATE,
                CHANNELS,
                fixture.hard_gap_span.0,
                fixture.hard_gap_span.1,
            );
            assert!(
                gap_rms > 100.0 / 32767.0,
                "hard gap should be filled after gate anchored retry, rms={gap_rms}"
            );
        }
        GapPatchStatus::Skipped { reason } => assert!(
            matches!(
                reason,
                GapPatchSkipReason::CorrelationBelowThreshold { .. }
                    | GapPatchSkipReason::BoundaryAlignmentFailed
                    | GapPatchSkipReason::ProgramQuiet
            ),
            "hard gap may stay skipped under G5-safe fixture, got {reason:?}"
        ),
        other => panic!("unexpected hard gap outcome via gate anchored retry, got {other:?}"),
    }

    // Default structure trust: easy interior gaps structure-trust; they must not become anchors.
    let mut exclusion_opts = anchored_retry_drift_gate_patch_options();
    exclusion_opts.strong_structure_trust = 0.90;
    let exclusion = run_patch(
        patch_request_with_options(fixture.report, false, 5.0, 0.35, exclusion_opts),
        10,
    );
    assert!(
        exclusion.summary.gaps.iter().any(|outcome| {
            matches!(
                outcome.status,
                GapPatchStatus::Patched {
                    structure_trusted: true,
                    ..
                }
            )
        }),
        "expected structure-trusted interior patch under default gate trust, got {:?}",
        exclusion.summary.gaps
    );
    assert_patch_anchors_exclude_structure_trusted(&exclusion.summary);
}

#[test]
fn patch_audio_anchored_retry_with_marginal_flag_recovers_hard_gap() {
    let fixture = build_drift_anchor_retry_fixture();
    let mut opts = anchored_retry_drift_patch_options();
    opts.fill_anchor_retry_marginal = true;
    // Isolate the marginal anchored-retry flag: dual-fit (on by default) otherwise rescues the hard tail,
    // adding a 5th patch and breaking the "must not regress pass-1 bridge patches" count.
    opts.dual_fit = false;

    let anchored = run_patch(
        patch_request_with_options(
            fixture.report,
            false,
            5.0,
            DRIFT_ANCHOR_MIN_CORRELATION,
            opts,
        ),
        10,
    );
    assert_eq!(
        anchored.summary.patched_count, 4,
        "marginal retry flag must not regress pass-1 bridge patches, got {:?}",
        anchored.summary.gaps
    );
    assert_eq!(anchored.summary.patched_marginal_count, 0);
}

#[test]
fn patch_audio_anchored_retry_marginal_flag_without_anchors_skips_pass2() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    write_stereo_sine_with_seam_distortion(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        SeamDistortionWindow {
            start_secs: GAP_END,
            end_secs: GAP_END + 0.25,
            mix_inverted: 0.49,
        },
        SineTone {
            freq: 440.0,
            amplitude: 16_000.0,
        },
    );

    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![default_gap()],
        ..make_report(path_a, path_b, stereo_identical_compat(SAMPLE_RATE))
    };

    let mut opts = fast_fit_patch_options();
    opts.fill_offset_mode = FillOffsetMode::AnchoredRetry;
    opts.fill_anchor_retry_marginal = true;
    opts.gap_end_extend_on_post_seam_fail = false;
    opts.gap_start_extend_on_pre_seam_fail = false;

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.999, opts),
        10,
    );
    assert_eq!(patched.summary.patched_count, 1);
    assert_eq!(patched.summary.patched_marginal_count, 1);
    assert!(
        patched.summary.patch_anchors_used.is_none(),
        "marginal pass-1 patch must not build anchors or run pass 2 even with marginal retry flag"
    );
}

#[test]
fn patch_audio_anchored_retry_skips_pass2_when_no_anchors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    write_stereo_sine_with_seam_distortion(
        &path_b,
        SAMPLE_RATE,
        TOTAL_SECS,
        SeamDistortionWindow {
            start_secs: GAP_END,
            end_secs: GAP_END + 0.25,
            mix_inverted: 0.49,
        },
        SineTone {
            freq: 440.0,
            amplitude: 16_000.0,
        },
    );

    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![default_gap()],
        ..make_report(path_a, path_b, stereo_identical_compat(SAMPLE_RATE))
    };

    // Marginal tier is excluded from the anchor table.
    let mut opts = fast_fit_patch_options();
    opts.fill_offset_mode = FillOffsetMode::AnchoredRetry;
    opts.gap_end_extend_on_post_seam_fail = false;
    opts.gap_start_extend_on_pre_seam_fail = false;

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.999, opts),
        10,
    );
    assert_eq!(patched.summary.patched_count, 1);
    assert_eq!(patched.summary.patched_marginal_count, 1);
    assert!(
        patched.summary.patch_anchors_used.is_none(),
        "marginal pass-1 patch must not build anchors or run pass 2"
    );
}

#[test]
fn patch_audio_anchored_retry_skips_pass2_when_all_gaps_patch_in_pass1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);
    let report = GapReport {
        local_pair_collar_secs: None,
        gaps: vec![default_gap()],
        ..make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        )
    };

    let mut opts = fast_fit_patch_options();
    opts.fill_offset_mode = FillOffsetMode::AnchoredRetry;
    opts.fill_border_search_secs = 5.0;

    let patched = run_patch(
        patch_request_with_options(report, false, 5.0, 0.35, opts),
        10,
    );
    assert_eq!(patched.summary.patched_count, 1);
    match &patched.summary.gaps[0].status {
        GapPatchStatus::Patched {
            confidence: FillConfidence::High,
            ..
        } => {}
        other => panic!("expected high-confidence pass-1 patch, got {other:?}"),
    }
    let anchors = patched
        .summary
        .patch_anchors_used
        .as_ref()
        .expect("pass-1 anchor table should export even when pass 2 has nothing to retry");
    assert_eq!(anchors.len(), 1);
}

// --- Energy signature acceptance (I4, `docs/dev/archive/TEMP-energy-signature-plan.md`) ---

const ENERGY_SIG_RATE: u32 = 48_000;

#[test]
fn i4_f3_auto_matches_bool_outcome() {
    use clip_sync_repair::domain::gap_signature::{build_gap_signature, GapSignature};

    let fixture = build_f3_drone_integration(ENERGY_SIG_RATE, CHANNELS as usize);
    let auto_sig = build_gap_signature(
        &fixture.a_samples,
        fixture.channels,
        fixture.gap_start,
        fixture.gap_end,
        fixture.context_frames,
        &fixture.structure_params,
        GapSignatureMode::Auto,
    );
    assert!(
        matches!(auto_sig, GapSignature::Bool(_)),
        "I4: auto on drone should resolve to bool"
    );

    let auto_domain = fixture.unified_match(GapSignatureMode::Auto, structure_heavy_weights());
    let bool_domain = fixture.unified_match(GapSignatureMode::Bool, structure_heavy_weights());
    assert_eq!(
        auto_domain.map(|m| m.alignment.start_frame),
        bool_domain.map(|m| m.alignment.start_frame),
        "I4: auto and bool domain placement should match"
    );
}

/// Thin selection v1: `--only-gaps 2` on a 3-gap run patches only gap #2; gaps 1 and 3 keep A's silence.
#[test]
fn only_gaps_2_leaves_other_gaps_on_a() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    let gaps = [(3.0, 4.0), (5.0, 6.0), (7.0, 8.0)];
    write_stereo_sine_with_gaps(&path_a, SAMPLE_RATE, 10, &gaps, 440.0, 16_000.0);
    write_stereo_sine_wav(&path_b, SAMPLE_RATE, 10, 440.0, 16_000.0);

    let report = make_report_with_alignment(
        path_a,
        path_b,
        stereo_identical_compat(SAMPLE_RATE),
        make_alignment(0.0),
        gaps.iter()
            .map(|&(s, e)| make_gap_on_a(s, e, 0.0))
            .collect(),
    );

    let request = patch_request_with_options(
        report,
        false,
        5.0,
        0.35,
        PatchTestOptions {
            gap_selection: GapSelectionMode::Only(vec!["2".into()]),
            ..PatchTestOptions::default()
        },
    );

    let result = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter)
        .execute(request, 10)
        .expect("patch");
    let pcm = expect_pcm(&result);

    assert_eq!(result.summary.patched_count, 1);
    assert_eq!(result.summary.not_planned_count, 2);
    match &result.summary.gaps[0].status {
        GapPatchStatus::NotPlanned { reason } => {
            assert_eq!(*reason, GapFillSkipReason::GapNotSelected);
        }
        other => panic!("gap 1 expected not selected, got {other:?}"),
    }
    assert!(matches!(
        result.summary.gaps[1].status,
        GapPatchStatus::Patched { .. }
    ));
    match &result.summary.gaps[2].status {
        GapPatchStatus::NotPlanned { reason } => {
            assert_eq!(*reason, GapFillSkipReason::GapNotSelected);
        }
        other => panic!("gap 3 expected not selected, got {other:?}"),
    }

    let gap1_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 3.0, 4.0);
    let gap2_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 5.0, 6.0);
    let gap3_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 7.0, 8.0);
    assert!(
        gap1_rms < 1e-3,
        "unselected gap 1 must stay silent on A, rms={gap1_rms}"
    );
    assert!(
        gap2_rms > 100.0 / 32767.0,
        "selected gap 2 must be filled, rms={gap2_rms}"
    );
    assert!(
        gap3_rms < 1e-3,
        "unselected gap 3 must stay silent on A, rms={gap3_rms}"
    );
}
