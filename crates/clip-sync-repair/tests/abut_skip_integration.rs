//! **Early shoulder locks → `shoulders_abut` skip** on the production `PatchAudio` path.
//!
//! Tier: **integration**. Synthetic noise WAV fixtures (no ffmpeg, no licensed media).
//!
//! PR: **no** — `.\scripts\test-tier.ps1 -Tier pr-repair-extended` (or the full `integration` tier).
//!
//! Run: `cargo test -p clip-sync-repair --test abut_skip_integration`
//!
//! The fixture is an **introduced-silence** pair: B is continuous noise, and A is that same noise
//! with a silent span *inserted* (so A is longer than B by exactly the gap). Both of A's seam
//! borders therefore match the SAME point in B — the pre shoulder at lag ~0, the post shoulder at
//! lag ~`-gap_frames` — so the donor bridge between the two locks has zero length and there is
//! nothing to fill. That is the whole population the abut skip exists for.
//!
//! Contrast fixture: A's silent span *replaces* B content (both files the same length). The
//! shoulders then bridge a real span of B, the abut test must not fire, and the gap goes on to the
//! ordinary gate search.

use std::path::{Path, PathBuf};

use clip_sync::{AlignmentResult, ClipLabel, ClipMatch};
use clip_sync_repair::application::align_bridge::scan_alignment_from_result;
use clip_sync_repair::domain::gap::{Gap, GapReport};
use clip_sync_repair::domain::{
    CompatibilityVerdict, GapPatchSkipReason, GapPatchStatus, TrackCompatibility,
};
use clip_sync_repair_harness::patch_audio::{
    patch_request_with_options, run_patch, run_patch_preview, PatchTestOptions,
};
use hound::{SampleFormat, WavSpec, WavWriter};

const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const TOTAL_SECS: f64 = 12.0;
const GAP_START: f64 = 6.0;
/// Short enough that the post shoulder's true lag (`-GAP_SECS`) is inside the ±600 ms seam-local
/// search — the abut has to be findable by the fine sweep alone, with no envelope rescue.
const GAP_SECS: f64 = 0.4;

/// Deterministic broadband noise — the correlation surface a sine fixture cannot provide (a periodic
/// signal locks equally well at every period, which is exactly the ambiguity the locks must not face
/// here).
fn noise(len: usize) -> Vec<i16> {
    let mut state: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 16) as i16 / 4).saturating_mul(3)
        })
        .collect()
}

fn write_stereo(path: &Path, samples: &[i16]) {
    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for s in samples {
        writer.write_sample(*s).expect("write L");
        writer.write_sample(*s).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

struct Fixture {
    _temp: tempfile::TempDir,
    path_a: PathBuf,
    path_b: PathBuf,
}

/// `inserted = true` → A is B with a silent span spliced IN (abut). `false` → A is B with the same
/// span silenced in place (a real dropout: B still holds the missing content).
fn build_fixture(inserted: bool) -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");

    let b_frames = (TOTAL_SECS * f64::from(SAMPLE_RATE)) as usize;
    let gap_frames = (GAP_SECS * f64::from(SAMPLE_RATE)) as usize;
    let cut = (GAP_START * f64::from(SAMPLE_RATE)) as usize;
    let b = noise(b_frames);

    let mut a = Vec::with_capacity(b_frames + gap_frames);
    a.extend_from_slice(&b[..cut]);
    a.extend(std::iter::repeat_n(0i16, gap_frames));
    a.extend_from_slice(&b[if inserted { cut } else { cut + gap_frames }..]);

    write_stereo(&path_a, &a);
    write_stereo(&path_b, &b);
    Fixture {
        _temp: temp,
        path_a,
        path_b,
    }
}

fn alignment() -> AlignmentResult {
    alignment_for(TOTAL_SECS)
}

fn alignment_for(total_secs: f64) -> AlignmentResult {
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: total_secs,
            aligned: true,
            offset_secs: Some(0.0),
            confidence: 0.9,
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
            repetition: None,
            video_b_window_start_secs: None,
            video_b_window_end_secs: None,
        }],
        start_aligned: true,
        end_aligned: None,
        recommended_offset_secs: Some(0.0),
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

fn report(fixture: &Fixture) -> GapReport {
    GapReport {
        local_pair_collar_secs: None,
        video_a: fixture.path_a.clone(),
        video_b: fixture.path_b.clone(),
        track_compatibility: Some(TrackCompatibility {
            a_channels: CHANNELS,
            b_channels: CHANNELS,
            a_sample_rate: SAMPLE_RATE,
            b_sample_rate: SAMPLE_RATE,
            channels_match: true,
            rate_match: true,
            verdict: CompatibilityVerdict::Identical,
        }),
        alignment: scan_alignment_from_result(&alignment()),
        gaps: vec![Gap {
            video_a_start_secs: GAP_START,
            video_a_end_secs: GAP_START + GAP_SECS,
            video_b_start_secs: Some(GAP_START),
            video_b_end_secs: Some(GAP_START + GAP_SECS),
            b_has_energy: true,
            collar_level: None,
        }],
        gap_equivalence: Vec::new(),
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
        limit_fill_to_mapped_region: true,
        b_scanned_end_secs: None,
        b_scan_truncated: false,
        audio_timeline_skew: None,
        program_levels: None,
    }
}

fn run(fixture: &Fixture, dual_fit: bool) -> clip_sync_repair::domain::PatchSummary {
    let request = patch_request_with_options(
        report(fixture),
        false,
        5.0,
        0.35,
        PatchTestOptions {
            dual_fit,
            ..Default::default()
        },
    );
    run_patch(request, 10).summary
}

fn skip_reason(summary: &clip_sync_repair::domain::PatchSummary) -> GapPatchSkipReason {
    match &summary.gaps[0].status {
        GapPatchStatus::Skipped { reason, .. } => reason.clone(),
        other => panic!("expected a skip, got {other:?}"),
    }
}

/// The headline: an introduced-silence gap is refused as `ShouldersAbut`, with the geometry that
/// justifies the refusal (a non-positive bridge) on the payload.
#[test]
fn introduced_silence_skips_as_shoulders_abut() {
    let fixture = build_fixture(true);
    let summary = run(&fixture, true);

    assert_eq!(summary.patched_count, 0, "abut gaps must never be filled");
    let reason = skip_reason(&summary);
    let GapPatchSkipReason::ShouldersAbut(locks) = &reason else {
        panic!("expected shoulders_abut, got {reason:?}");
    };
    let (bridge_frames, gap_frames, post_lag_ms, program_quiet) = (
        &locks.bridge_frames,
        &locks.gap_frames,
        &locks.post_lag_ms,
        &locks.program_quiet,
    );
    assert!(
        *bridge_frames <= 0,
        "the skip's own premise: no donor bridge between the locks, got {bridge_frames}"
    );
    assert_eq!(*gap_frames, (GAP_SECS * f64::from(SAMPLE_RATE)) as usize);
    // The post shoulder had to walk back by the whole inserted span to find its content.
    assert!(
        (*post_lag_ms + GAP_SECS * 1000.0).abs() < 25.0,
        "post shoulder should lock ~{} ms back, got {post_lag_ms}",
        -GAP_SECS * 1000.0
    );
    // B is broadband noise throughout — the lookalike tag must not fire here.
    assert!(!*program_quiet, "B's nominal span is not quiet");
}

/// The skip is **not** a dual-fit feature: `--no-dual-fit` must reach the same verdict. This is the
/// test that fails if the locks (or the buffers they read) drift back inside the `request.dual_fit`
/// gate they were hoisted out of.
#[test]
fn shoulders_abut_skip_survives_no_dual_fit() {
    let fixture = build_fixture(true);
    let summary = run(&fixture, false);

    assert_eq!(summary.patched_count, 0);
    assert!(
        matches!(skip_reason(&summary), GapPatchSkipReason::ShouldersAbut(_)),
        "the abut skip must not depend on --dual-fit"
    );
}

/// The guard against over-firing: when A's silence *replaces* B content, the two shoulders bridge a
/// real span, so `abut()` is false and the gap proceeds to the ordinary gate search — which, on a
/// clean synthetic pair, fills it.
#[test]
fn a_real_dropout_is_not_abut_and_still_reaches_the_gate() {
    let fixture = build_fixture(false);
    let summary = run(&fixture, true);

    assert!(
        !matches!(
            &summary.gaps[0].status,
            GapPatchStatus::Skipped {
                reason: GapPatchSkipReason::ShouldersAbut(_),
                ..
            }
        ),
        "a real dropout must not be refused as an abut: {:?}",
        summary.gaps[0].status
    );
    assert_eq!(
        summary.patched_count, 1,
        "the dropout should still be repaired by the ordinary path: {:?}",
        summary.gaps[0].status
    );
}

/// **Capture + post-splice visit** (plan §4). A run with *both* kinds of gap: one real dropout that
/// the gate fills, and one introduced silence refused as `shoulders_abut`. What it pins:
///
/// * the refusal is republished at run level on `summary.shoulders_abut`, with the gap's identity
///   and its **frames** on A — the successor collapse Change's input, and the reason the row is not
///   re-derivable from `gaps` (the per-gap status carries seconds, no frames);
/// * the visit ran after the splice and did **not** shorten A — the patched PCM is exactly as long
///   as the decoded A, so the abut frames still name the same silence they did at characterize;
/// * the neighbouring fill happened anyway. An abut refusal is a per-gap decision; a run that
///   quietly stopped filling its other gaps would satisfy every assertion above.
#[test]
fn abut_capture_survives_splice_and_reaches_the_summary() {
    const TOTAL: f64 = 20.0;
    const DROPOUT_START: f64 = 5.0;
    const INSERT_START: f64 = 13.0;
    const SPAN: f64 = 0.4;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");

    let b_frames = (TOTAL * f64::from(SAMPLE_RATE)) as usize;
    let span_frames = (SPAN * f64::from(SAMPLE_RATE)) as usize;
    let dropout_at = (DROPOUT_START * f64::from(SAMPLE_RATE)) as usize;
    let insert_at = (INSERT_START * f64::from(SAMPLE_RATE)) as usize;
    let b = noise(b_frames);

    // A = B with the first span silenced **in place** (B still holds that content: fillable) and the
    // second span **inserted** (B holds nothing there: the abut). The dropout comes first so that
    // everything up to the insertion is still frame-aligned with B at offset 0.
    let mut a = b.clone();
    a[dropout_at..dropout_at + span_frames].fill(0);
    a.splice(insert_at..insert_at, std::iter::repeat_n(0i16, span_frames));

    write_stereo(&path_a, &a);
    write_stereo(&path_b, &b);
    let fixture = Fixture {
        _temp: temp,
        path_a,
        path_b,
    };

    let gap = |a_start: f64, b_start: f64| Gap {
        video_a_start_secs: a_start,
        video_a_end_secs: a_start + SPAN,
        video_b_start_secs: Some(b_start),
        video_b_end_secs: Some(b_start + SPAN),
        b_has_energy: true,
        collar_level: None,
    };
    let mut report = report(&fixture);
    report.alignment = scan_alignment_from_result(&alignment_for(TOTAL));
    report.gaps = vec![
        gap(DROPOUT_START, DROPOUT_START),
        gap(INSERT_START, INSERT_START),
    ];

    let request = patch_request_with_options(report, false, 5.0, 0.35, PatchTestOptions::default());
    let result = run_patch(request, 10);
    let summary = &result.summary;

    let abut = summary
        .shoulders_abut
        .as_ref()
        .expect("the abut gap must be republished at run level");
    assert_eq!(abut.len(), 1, "one abut, not the dropout too: {abut:?}");
    assert_eq!(abut[0].gap_index, 1, "the inserted-silence gap");
    assert!(
        abut[0].locks.bridge_frames <= 0,
        "the row carries the geometry that justified the refusal: {:?}",
        abut[0].locks
    );

    // The frames are A's refined throat, so they land inside the inserted span — which the splice
    // left alone, and which the refine step may have trimmed to the audible silence.
    let (start, end) = (abut[0].a_start_frame, abut[0].a_end_frame);
    assert!(
        start < end && end <= insert_at + span_frames && start >= insert_at.saturating_sub(4_410),
        "abut frames {start}..{end} should name the inserted span at ~{insert_at}"
    );

    let pcm = result.pcm.as_ref().expect("write mode returns patched PCM");
    // ±1 frame is the decoder's edge, not the splice: `splice_into_a` is length-preserving, and the
    // drain this visit will one day perform would remove the whole inserted span (~17 640 frames).
    assert!(
        pcm.frames().abs_diff(a.len()) <= 1,
        "the visit is record-only in this Change: A must not be shortened ({} vs {})",
        pcm.frames(),
        a.len()
    );
    assert!(
        end <= pcm.frames(),
        "the abut span must still be in range after the splice"
    );
    assert!(
        pcm.samples[start * usize::from(CHANNELS)..end * usize::from(CHANNELS)]
            .iter()
            .all(|s| s.abs() < 1e-4),
        "the refused gap is still silent — nothing was filled into it"
    );

    assert_eq!(
        summary.patched_count, 1,
        "the real dropout is still filled: {:?}",
        summary.gaps
    );
}

/// Preview stamps the same list and never visits (there is no spliced A to visit).
#[test]
fn preview_reports_the_abut_without_pcm() {
    let fixture = build_fixture(true);
    let request = patch_request_with_options(
        report(&fixture),
        false,
        5.0,
        0.35,
        PatchTestOptions::default(),
    );
    let result = run_patch_preview(request, 10);

    assert!(result.preview && result.pcm.is_none());
    let abut = result
        .summary
        .shoulders_abut
        .as_ref()
        .expect("preview must report what it would refuse");
    assert_eq!(abut.len(), 1);
    assert_eq!(abut[0].gap_index, 0);
}
