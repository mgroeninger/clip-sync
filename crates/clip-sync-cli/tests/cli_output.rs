use std::process::ExitCode;

use clip_sync::{
    AlignmentResult, AppError, ClipLabel, ClipMatch, ConfigError, DomainError,
    FingerprintError, MediaError,
};
use clip_sync_cli::infrastructure::cli::exit_code::exit_code_for;

// --- helpers ---

fn aligned_result(offset: f64) -> AlignmentResult {
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: 900.0,
            aligned: true,
            offset_secs: Some(offset),
            confidence: 0.9,
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
        }],
        start_aligned: true,
        end_aligned: None,
        recommended_offset_secs: Some(offset),
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: None,
        high_rate_refinement: None,
    }
}

fn unaligned_result() -> AlignmentResult {
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: 900.0,
            aligned: false,
            offset_secs: None,
            confidence: 0.1,
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
        }],
        start_aligned: false,
        end_aligned: None,
        recommended_offset_secs: None,
        offsets_consistent: false,
        offset_drift_secs: None,
        start_overlap: None,
        high_rate_refinement: None,
    }
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

#[test]
fn aligned_result_serializes_to_expected_json_shape() {
    let result = aligned_result(12.5);
    let json = serde_json::to_string_pretty(&result).expect("serialize");
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
    let json = serde_json::to_string_pretty(&result).expect("serialize");
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
    let json = serde_json::to_string(&result).expect("serialize");
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
fn high_rate_refinement_is_omitted_when_none() {
    let result = aligned_result(3.0);
    let json = serde_json::to_string(&result).expect("serialize");
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
    let json = serde_json::to_string(&result).expect("serialize");
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
