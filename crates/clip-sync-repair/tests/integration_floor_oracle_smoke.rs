//! Floor oracle integration smokes.

#[allow(dead_code)]
mod floor_oracle_fixtures {
    include!("common/floor_oracle_fixtures.rs");
}

use floor_oracle_fixtures::{gap_frames_for_case, load_manifest, FloorOracleCase, FloorOracleDefaults};

#[test]
fn floor_oracle_manifest_loads() {
    let manifest = load_manifest();
    assert!(manifest.version >= 1);
    assert!(
        manifest.case.len() >= 18,
        "expected speech+ambient+music wav/aac/vorbis matrix, dual encodes, and two_mic case"
    );
}

#[test]
fn floor_oracle_gap_frames_use_production_anchor() {
    let defaults = FloorOracleDefaults::default();
    let case = FloorOracleCase {
        id: "geom".into(),
        source_id: "x".into(),
        donor_source_id: None,
        oracle_variant: None,
        format_a: None,
        format_b: None,
        bitrate_a: None,
        bitrate_b: None,
        total_secs: Some(60),
        sample_rate: Some(48_000),
        gap_duration_secs: Some(1.0),
        gap_signature_context_secs: Some(3.0),
        gap_anchor_secs: None,
        gap_interior_peak_max: None,
        punch_after_encode: false,
        b_encode_delay_ms: None,
        expect_informative_floor: None,
        ignore: false,
    };
    let (start, end) = gap_frames_for_case(&case, &defaults);
    assert_eq!(start, 14 * 48_000);
    assert_eq!(end - start, 48_000);

    // gap_anchor_secs override (Run B): places the gap at the requested absolute position.
    let anchored = FloorOracleCase {
        gap_anchor_secs: Some(148.5),
        total_secs: Some(153),
        ..case
    };
    let (astart, aend) = gap_frames_for_case(&anchored, &defaults);
    assert_eq!(astart, (148.5 * 48_000.0) as usize);
    assert_eq!(aend - astart, 48_000);
}
