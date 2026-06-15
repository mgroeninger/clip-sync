use std::process::ExitCode;

use clip_sync::{
    AlignmentReport, AlignmentResult, AppError, ClipLabel, ClipMatch, ClipRepetitionReport,
    ConfigError, DomainError, FingerprintError, HighRateRefinement, MediaError,
    OffsetVerification, RepetitionFinding, TimelineOverlap,
};
use clip_sync::testing::alignment_fixtures::{minimal_alignment_result, start_clip_match};
use clip_sync_cli::infrastructure::cli::exit_code::exit_code_for;
use clip_sync_cli::infrastructure::cli::output::{format_human_output, format_json_output};
use clip_sync_cli::infrastructure::config::{OutputConfig, OutputFormat};

// --- helpers ---

fn aligned_result(offset: f64) -> AlignmentResult {
    minimal_alignment_result(Some(offset))
        .with_clips(vec![start_clip_match(Some(offset), 900.0, 0.9)])
        .build()
}

fn unaligned_result() -> AlignmentResult {
    let mut result = minimal_alignment_result(None)
        .with_clips(vec![start_clip_match(None, 900.0, 0.1)])
        .build();
    result.offsets_consistent = false;
    result
}

fn result_with_repetition(a_lag: Option<f64>, b_lag: Option<f64>) -> AlignmentResult {
    let rep_a = a_lag.map(|lag_secs| RepetitionFinding {
        lag_secs,
        confidence: 0.72,
        items_count: 48,
    });
    let rep_b = b_lag.map(|lag_secs| RepetitionFinding {
        lag_secs,
        confidence: 0.65,
        items_count: 40,
    });
    let mut clip = start_clip_match(Some(3.0), 900.0, 0.85);
    clip.repetition = Some(ClipRepetitionReport { a: rep_a, b: rep_b });
    minimal_alignment_result(Some(3.0))
        .with_clips(vec![clip])
        .build()
}

/// Every optional top-level field and clip-level `repetition` populated for JSON contract goldens.
fn full_surface_alignment_result() -> AlignmentResult {
    AlignmentResult {
        clips: vec![
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: 900.0,
                aligned: false,
                offset_secs: None,
                confidence: 0.42,
                video_a_decode_skips: 1,
                video_b_decode_skips: 2,
                repetition: Some(ClipRepetitionReport {
                    a: Some(RepetitionFinding {
                        lag_secs: 30.5,
                        confidence: 0.72,
                        items_count: 48,
                    }),
                    b: None,
                }),
            },
            ClipMatch {
                label: ClipLabel::End,
                window_start_secs: 1800.0,
                window_end_secs: 2700.0,
                aligned: true,
                offset_secs: Some(12.355),
                confidence: 0.91,
                video_a_decode_skips: 0,
                video_b_decode_skips: 3,
                repetition: None,
            },
        ],
        start_aligned: false,
        end_aligned: Some(true),
        recommended_offset_secs: Some(12.34),
        offsets_consistent: false,
        offset_drift_secs: Some(0.015),
        start_overlap: Some(TimelineOverlap {
            video_a_start_secs: 10.956,
            video_a_end_secs: 600.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 589.044,
            shared_length_secs: 589.044,
        }),
        high_rate_refinement: Some(HighRateRefinement {
            segment_start_secs: 120.0,
            segment_length_secs: 3.0,
            adjustment_secs: 0.01,
            correlation_peak: 2_813_101_397.0,
            applied: true,
            skipped: false,
            skip_reason: None,
        }),
        offset_verification: Some(OffsetVerification {
            window_a_start_secs: 60.0,
            window_a_end_secs: 90.0,
            window_b_start_secs: 63.0,
            window_b_end_secs: 93.0,
            confidence: 0.85,
            verified: true,
            skipped: false,
            skip_reason: None,
            candidates_tried: 1,
            independent_offset_secs: None,
            parallel_recheck_delta_secs: None,
            verify_inconclusive: false,
        }),
        offset_ambiguous_mod_secs: Some(10.0),
    }
}

fn result_with_no_repetition_finding() -> AlignmentResult {
    let mut clip = start_clip_match(Some(3.0), 900.0, 0.85);
    clip.repetition = Some(ClipRepetitionReport { a: None, b: None });
    minimal_alignment_result(Some(3.0))
        .with_clips(vec![clip])
        .build()
}

fn exit_code_u8(error: &AppError) -> u8 {
    // ExitCode has no public u8 accessor; compare against known values by
    // pattern-matching the display string or reconstructing from ExitCode::from.
    // Simplest: round-trip through ExitCode::from on the expected value.
    let code = exit_code_for(error);
    let candidates: &[u8] = &[2, 3, 4, 5, 6];
    for &n in candidates {
        if code == ExitCode::from(n) {
            return n;
        }
    }
    panic!("exit_code_for returned an unexpected code for {error:?}");
}

// --- JSON output shape ---

/// Regenerate `tests/fixtures/full_surface_alignment.json` after an intentional contract change:
/// `cargo test -p clip-sync-cli write_full_surface_alignment_golden -- --ignored --nocapture`
#[test]
#[ignore = "fixture generator — run manually when the JSON contract is revised"]
fn write_full_surface_alignment_golden() {
    let json = format_json_output(&full_surface_alignment_result());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/full_surface_alignment.json");
    std::fs::create_dir_all(path.parent().expect("fixture parent dir")).expect("create fixtures dir");
    std::fs::write(&path, json).expect("write golden fixture");
}

/// Plan 1 Phase 0: byte-identical guard for the analyzer JSON contract (pre-DTO split).
#[test]
fn full_surface_alignment_json_golden() {
    let json = format_json_output(&full_surface_alignment_result());
    assert_eq!(
        json,
        include_str!("fixtures/full_surface_alignment.json"),
        "analyzer JSON contract changed — update the golden only with an explicit contract revision"
    );
}

#[test]
fn aligned_result_serializes_to_expected_json_shape() {
    let result = aligned_result(12.5);
    let json = serde_json::to_string_pretty(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    assert!(value["start_aligned"].is_boolean(), "start_aligned must be boolean");
    assert!(value["clips"].is_array(), "clips must be array");
    assert!(!value["clips"].as_array().unwrap().is_empty(), "clips must be non-empty");
    assert!(
        value["recommended_offset_secs"].is_number(),
        "recommended_offset_secs must be a number when aligned"
    );
    assert_eq!(value["start_aligned"], true);
    assert_eq!(value["recommended_offset_secs"], 12.5);
    assert_eq!(value["offsets_consistent"], true);
}

#[test]
fn unaligned_result_serializes_with_null_offset() {
    let result = unaligned_result();
    let json = serde_json::to_string_pretty(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    assert_eq!(value["start_aligned"], false);
    assert!(
        value["recommended_offset_secs"].is_null(),
        "recommended_offset_secs must be null when unaligned"
    );
}

#[test]
fn clip_match_json_has_required_fields() {
    let result = aligned_result(3.0);
    let json = serde_json::to_string(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    let clip = &value["clips"][0];
    assert!(clip["label"].is_string(), "label must be a string");
    assert!(clip["window_start_secs"].is_number());
    assert!(clip["window_end_secs"].is_number());
    assert!(clip["aligned"].is_boolean());
    assert!(clip["confidence"].is_number());
    assert!(clip["video_a_decode_skips"].is_number());
    assert!(clip["video_b_decode_skips"].is_number());
}

#[test]
fn compact_human_output_omits_redundant_single_clip_lines() {
    let mut result = aligned_result(-10.956);
    result.start_overlap = Some(TimelineOverlap {
        video_a_start_secs: 10.956,
        video_a_end_secs: 600.0,
        video_b_start_secs: 0.0,
        video_b_end_secs: 589.044,
        shared_length_secs: 589.044,
    });

    let output = format_human_output(false, &result);

    assert!(
        output.starts_with("Alignment: offset -10.956s  confidence 0.90\n"),
        "expected compact header: {output}"
    );
    assert!(
        output.contains("Overlap:   A [0:11–10:00]   B [0:00–9:49]   (9:49 shared)\n"),
        "expected compact overlap line: {output}"
    );
    assert!(
        !output.contains("Alignment report"),
        "legacy report header must not appear: {output}"
    );
    assert!(
        !output.contains("Start clip aligned"),
        "redundant start-aligned line must not appear: {output}"
    );
    assert!(
        !output.contains("Recommended offset"),
        "redundant recommended-offset line must not appear: {output}"
    );
}

#[test]
fn compact_human_output_shows_per_clip_offsets_when_multiple_clips() {
    let result = AlignmentResult {
        clips: vec![
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: 900.0,
                aligned: true,
                offset_secs: Some(12.34),
                confidence: 0.94,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
            },
            ClipMatch {
                label: ClipLabel::End,
                window_start_secs: 1800.0,
                window_end_secs: 2700.0,
                aligned: true,
                offset_secs: Some(12.355),
                confidence: 0.91,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
            },
        ],
        start_aligned: true,
        end_aligned: Some(true),
        recommended_offset_secs: Some(12.34),
        offsets_consistent: true,
        offset_drift_secs: Some(0.015),
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
    };

    let output = format_human_output(false, &result);

    assert!(output.contains("  Start clip: +12.340s  (confidence 0.94)\n"));
    assert!(output.contains("  End clip: +12.355s  (confidence 0.91)\n"));
}

#[test]
fn compact_human_output_headline_uses_start_clip_not_first_clip() {
    let result = AlignmentResult {
        clips: vec![
            ClipMatch {
                label: ClipLabel::End,
                window_start_secs: 1800.0,
                window_end_secs: 2700.0,
                aligned: true,
                offset_secs: Some(12.355),
                confidence: 0.91,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
            },
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: 900.0,
                aligned: true,
                offset_secs: Some(12.34),
                confidence: 0.94,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
            },
        ],
        start_aligned: true,
        end_aligned: Some(true),
        recommended_offset_secs: Some(12.34),
        offsets_consistent: true,
        offset_drift_secs: Some(0.015),
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
    };

    let output = format_human_output(false, &result);

    assert!(
        output.starts_with("Alignment: offset +12.340s  confidence 0.94\n"),
        "headline must use start-clip confidence, not clips[0]: {output}"
    );
}

#[test]
fn high_rate_refinement_omits_peak_by_default() {
    let result = minimal_alignment_result(Some(3.0))
        .with_clips(vec![start_clip_match(Some(3.0), 900.0, 0.9)])
        .with_high_rate_refinement(Some(HighRateRefinement {
            segment_start_secs: 0.0,
            segment_length_secs: 3.0,
            adjustment_secs: 0.01,
            correlation_peak: 2_813_101_397.0,
            applied: true,
            skipped: false,
            skip_reason: None,
        }))
        .build();

    let output = format_human_output(false, &result);
    assert!(
        output.contains("High-rate: +0.010s refinement applied\n"),
        "expected adjustment without peak: {output}"
    );
    assert!(
        !output.contains("peak"),
        "correlation peak must not appear in default human output: {output}"
    );
}

#[test]
fn high_rate_refinement_shows_peak_with_diagnostics() {
    let result = minimal_alignment_result(Some(3.0))
        .with_clips(vec![start_clip_match(Some(3.0), 900.0, 0.9)])
        .with_high_rate_refinement(Some(HighRateRefinement {
            segment_start_secs: 0.0,
            segment_length_secs: 3.0,
            adjustment_secs: 0.01,
            correlation_peak: 2_813_101_397.0,
            applied: true,
            skipped: false,
            skip_reason: None,
        }))
        .build();

    let output = format_human_output(true, &result);
    assert!(
        output.contains("High-rate: +0.010s refinement applied (peak 2813101397.00)"),
        "expected peak in verbose output: {output}"
    );
}

#[test]
fn high_rate_refinement_is_omitted_when_none() {
    let result = aligned_result(3.0);
    let json = serde_json::to_string(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    // `#[serde(skip_serializing_if = "Option::is_none")]` on high_rate_refinement
    assert!(
        value.get("high_rate_refinement").is_none(),
        "high_rate_refinement must be omitted (not null) when None"
    );
}

#[test]
fn aligned_result_roundtrips_through_json() {
    let result = aligned_result(7.25);
    let json = serde_json::to_string(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    // Re-serialize the Value and re-parse; both parsed Values must be equal.
    // (Key order differs between struct serialization and Value re-serialization
    // because serde_json::Value::Object uses BTreeMap, but semantic equality holds.)
    let json2 = serde_json::to_string(&value).expect("re-serialize");
    let value2: serde_json::Value = serde_json::from_str(&json2).expect("re-parse");
    assert_eq!(value, value2, "JSON content must be stable across re-serialization");
}

// --- Exit code mapping ---

#[test]
fn config_error_maps_to_exit_2() {
    let err = AppError::Config(ConfigError::InvalidValue {
        field: "clip_length".into(),
        reason: "too short".into(),
    });
    assert_eq!(exit_code_u8(&err), 2);
}

#[test]
fn no_audio_tracks_maps_to_exit_3() {
    let err = AppError::Domain(DomainError::NoAudioTracks);
    assert_eq!(exit_code_u8(&err), 3);
}

#[test]
fn domain_errors_map_to_exit_3() {
    let err = AppError::Domain(DomainError::InvalidDuration);
    assert_eq!(exit_code_u8(&err), 3);
}

#[test]
fn media_error_maps_to_exit_4() {
    let err = AppError::Media(MediaError::FileNotFound("x.mp4".into()));
    assert_eq!(exit_code_u8(&err), 4);
}

#[test]
fn fingerprint_error_maps_to_exit_5() {
    let err = AppError::Fingerprint(FingerprintError::InvalidPcm("empty".into()));
    assert_eq!(exit_code_u8(&err), 5);
}

#[test]
fn alignment_error_maps_to_exit_6() {
    let err = AppError::Alignment(clip_sync::AlignmentError::EngineFailed(
        "no match".into(),
    ));
    assert_eq!(exit_code_u8(&err), 6);
}

// --- repetition JSON ---

#[test]
fn align_json_repetition_object_when_flag_on() {
    let result = result_with_repetition(Some(30.0), None);
    let json = serde_json::to_string_pretty(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    let clip = &value["clips"][0];
    assert!(clip["repetition"].is_object(), "repetition must be an object when flag on");

    let rep = &clip["repetition"];
    assert!(rep["a"].is_object(), "a must be an object when finding present");
    assert!((rep["a"]["lag_secs"].as_f64().unwrap() - 30.0).abs() < 0.01);
    assert!(rep["b"].is_null(), "b must be null when no finding");
}

#[test]
fn align_json_no_repetition_key_when_flag_off() {
    let result = aligned_result(3.0);
    let json = serde_json::to_string(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    let clip = &value["clips"][0];
    assert!(
        clip.get("repetition").is_none(),
        "repetition key must be absent when flag off"
    );
}

// --- repetition human output ---

#[test]
fn align_human_shows_repeat_line() {
    let result = result_with_repetition(Some(30.0), None);
    let config = OutputConfig { format: OutputFormat::Human, show_diagnostics: false };
    let output = format_human_output(config.show_diagnostics, &result);

    assert!(
        output.contains("video A: internal repeat ~30.0s"),
        "expected repeat line in output: {output}"
    );
    assert!(
        !output.contains("video B"),
        "video B should be absent when no B finding and not verbose: {output}"
    );
}

#[test]
fn align_human_verbose_shows_none() {
    let result = result_with_no_repetition_finding();
    let output = format_human_output(true, &result);

    assert!(
        output.contains("video A: no internal repeat detected"),
        "expected no-repeat line for A: {output}"
    );
    assert!(
        output.contains("video B: no internal repeat detected"),
        "expected no-repeat line for B: {output}"
    );
}

#[test]
fn align_human_no_repetition_lines_when_repetition_field_absent() {
    let result = aligned_result(3.0);
    let output = format_human_output(true, &result);

    assert!(
        !output.contains("internal repeat") && !output.contains("no internal repeat"),
        "no repetition lines expected when field absent: {output}"
    );
}

// --- offset verification human output ---

fn make_verification(
    verified: bool,
    confidence: f32,
    skipped: bool,
    skip_reason: Option<&str>,
) -> OffsetVerification {
    OffsetVerification {
        window_a_start_secs: 60.0,
        window_a_end_secs: 90.0,
        window_b_start_secs: 63.0,
        window_b_end_secs: 93.0,
        confidence,
        verified,
        skipped,
        skip_reason: skip_reason.map(String::from),
        candidates_tried: if skipped { 0 } else { 1 },
        independent_offset_secs: None,
        parallel_recheck_delta_secs: None,
        verify_inconclusive: false,
    }
}

fn make_inconclusive_verification(confidence: f32, independent_offset_secs: f64) -> OffsetVerification {
    OffsetVerification {
        independent_offset_secs: Some(independent_offset_secs),
        parallel_recheck_delta_secs: Some(10.0),
        verify_inconclusive: true,
        verified: false,
        confidence,
        skipped: false,
        skip_reason: None,
        candidates_tried: 1,
        window_a_start_secs: 60.0,
        window_a_end_secs: 90.0,
        window_b_start_secs: 73.0,
        window_b_end_secs: 103.0,
    }
}

#[test]
fn verify_human_shows_warning_when_not_verified() {
    let result = minimal_alignment_result(Some(3.0))
        .with_clips(vec![start_clip_match(Some(3.0), 900.0, 0.9)])
        .with_verification(Some(make_verification(false, 0.32, false, None)))
        .build();

    let output = format_human_output(false, &result);
    assert!(
        output.contains("Verify:    offset not independently verified (hold-out confidence 0.32)"),
        "expected unverified warning: {output}"
    );
}

#[test]
fn verify_human_shows_nothing_when_verified_non_verbose() {
    let result = minimal_alignment_result(Some(3.0))
        .with_clips(vec![start_clip_match(Some(3.0), 900.0, 0.9)])
        .with_verification(Some(make_verification(true, 0.85, false, None)))
        .build();

    let output = format_human_output(false, &result);
    assert!(
        !output.contains("Verify:"),
        "no Verify line expected for confirmed result in non-verbose: {output}"
    );
}

#[test]
fn verify_human_verbose_shows_confirmation_when_verified() {
    let result = minimal_alignment_result(Some(3.0))
        .with_clips(vec![start_clip_match(Some(3.0), 900.0, 0.9)])
        .with_verification(Some(make_verification(true, 0.85, false, None)))
        .build();

    let output = format_human_output(true, &result);
    assert!(
        output.contains("Verify:    offset confirmed at hold-out window (confidence 0.85)"),
        "expected confirmation line in verbose mode: {output}"
    );
}

#[test]
fn verify_human_skip_silent_when_non_verbose() {
    let result = minimal_alignment_result(Some(3.0))
        .with_clips(vec![start_clip_match(Some(3.0), 900.0, 0.9)])
        .with_verification(Some(make_verification(
            false,
            0.0,
            true,
            Some("hold-out window unavailable"),
        )))
        .build();

    let output = format_human_output(false, &result);
    assert!(
        !output.contains("Verify:"),
        "skip must be silent in non-verbose mode: {output}"
    );
}

#[test]
fn verify_human_verbose_shows_skip_reason() {
    let result = minimal_alignment_result(Some(3.0))
        .with_clips(vec![start_clip_match(Some(3.0), 900.0, 0.9)])
        .with_verification(Some(make_verification(
            false,
            0.0,
            true,
            Some("hold-out window unavailable"),
        )))
        .build();

    let output = format_human_output(true, &result);
    assert!(
        output.contains("Verify:    skipped (hold-out window unavailable)"),
        "expected skip reason in verbose mode: {output}"
    );
}

#[test]
fn periodic_ambiguity_human_shows_warning_line() {
    let result = full_surface_alignment_result();
    let output = format_human_output(false, &result);
    assert!(
        output.contains("Warning:   offset ambiguous (repeats every ~10 s)"),
        "expected periodic ambiguity warning: {output}"
    );
}

#[test]
fn verify_human_shows_inconclusive_periodic_message() {
    let result = minimal_alignment_result(Some(13.0))
        .with_clips(vec![start_clip_match(Some(13.0), 900.0, 0.9)])
        .with_verification(Some(make_inconclusive_verification(0.85, 3.0)))
        .build();

    let output = format_human_output(false, &result);
    assert!(
        output.contains(
            "Verify:    offset not independently verified (periodic content; hold-out confidence 0.85)"
        ),
        "expected inconclusive verify line: {output}"
    );
}

#[test]
fn verify_human_verbose_shows_parallel_recheck_on_inconclusive() {
    let result = minimal_alignment_result(Some(13.0))
        .with_clips(vec![start_clip_match(Some(13.0), 900.0, 0.9)])
        .with_verification(Some(make_inconclusive_verification(0.85, 3.0)))
        .build();

    let output = format_human_output(true, &result);
    assert!(
        output.contains("parallel recheck offset +3.000s"),
        "expected parallel recheck diagnostic: {output}"
    );
}

// --- offset verification JSON ---

#[test]
fn verify_json_absent_when_none() {
    let result = aligned_result(3.0);
    let json = serde_json::to_string(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    assert!(
        value.get("offset_verification").is_none(),
        "offset_verification key must be absent when None"
    );
}

#[test]
fn verify_json_present_when_some() {
    let mut result = aligned_result(3.0);
    result.offset_verification = Some(make_verification(true, 0.85, false, None));

    let json = serde_json::to_string_pretty(&AlignmentReport::from(&result)).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    let verify = &value["offset_verification"];
    assert!(verify.is_object(), "offset_verification must be an object when Some");
    assert_eq!(verify["verified"], true);
    assert!((verify["confidence"].as_f64().unwrap() - 0.85).abs() < 0.01);
    assert!(
        verify.get("skip_reason").is_none(),
        "skip_reason must be absent when None"
    );
}
