//! Integration energy smokes (scan→patch tripwire + lib scan/domain rows).

use clip_sync::SymphoniaMediaReader;
use clip_sync::testing::fakes::FakeProgressReporter;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::GapSignatureMode;
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_production, build_f1_production_at, gap_report_times, structure_heavy_weights,
};
use clip_sync_repair::test_support::energy_signature_production::{
    patch_request_from_repair, production_repair_config, scan_gaps_for_fixture,
};

#[test]
fn corpus_scan_patch_smoke() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_production_at(16_000, 1, 32.0, 3.0);
    let repair = production_repair_config(GapSignatureMode::Energy, 3.0);

    let report = scan_gaps_for_fixture(&fixture, temp.path());
    assert_eq!(
        report.gaps.iter().filter(|g| g.is_fillable()).count(),
        1,
        "smoke: scan should find one fillable gap: {:#?}",
        report.gaps,
    );

    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let result = patch
        .execute(
            patch_request_from_repair(report, &repair),
            RepairConfig::default().crossfade_ms,
        )
        .expect("smoke scan→patch");
    assert_eq!(
        result.summary.patched_count, 1,
        "corpus smoke should patch one gap: {:?}",
        result.summary.gaps,
    );
}

#[test]
fn scan_detects_f1_production_gap() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_production(48_000, 2, 3.0);
    let (expected_start, expected_end, _, _, _) = gap_report_times(&fixture);
    let report = scan_gaps_for_fixture(&fixture, temp.path());
    let fillable: Vec<_> = report.gaps.iter().filter(|g| g.is_fillable()).collect();
    assert_eq!(fillable.len(), 1, "expected one fillable gap: {:#?}", report.gaps);
    let gap = fillable[0];
    const TOL: f64 = 0.35;
    assert!(
        (gap.video_a_start_secs - expected_start).abs() <= TOL,
        "scan start {:.3} expected {:.3}",
        gap.video_a_start_secs,
        expected_start,
    );
    assert!(
        (gap.video_a_end_secs - expected_end).abs() <= TOL,
        "scan end {:.3} expected {:.3}",
        gap.video_a_end_secs,
        expected_end,
    );
}

#[test]
fn f1_production_scan_and_domain_smoke() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_production(48_000, 2, 3.0);
    let report = scan_gaps_for_fixture(&fixture, temp.path());
    assert_eq!(
        report.gaps.iter().filter(|g| g.is_fillable()).count(),
        1,
        "scan should find one fillable gap: {:#?}",
        report.gaps,
    );

    let matched = fixture
        .unified_match(GapSignatureMode::Auto, structure_heavy_weights())
        .expect("F1-long auto domain on full B");
    assert!(
        fixture.within_bin_tolerance(matched.alignment.start_frame, fixture.true_fill_start),
        "auto domain start {} true {}",
        matched.alignment.start_frame,
        fixture.true_fill_start,
    );
}
