//! Energy signature integration smokes — scan→patch tripwire + production scan/domain rows.
//!
//! Tier: **integration**. **EC01** e2e: `corpus_scan_patch_smoke` (16 kHz / 32 s F1 production
//! geometry). Also `scan_detects_f1_production_gap` and `f1_production_scan_and_domain_smoke`.
//!
//! PR: **yes** — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test integration_energy_smoke`

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::SymphoniaMediaReader;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::GapSignatureMode;
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    build_f1_production, build_f1_production_at, gap_report_times, structure_heavy_weights,
};
use clip_sync_repair_fixtures::energy_signature_production::{
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

/// The fill-level check reaches the outcome of a gap that was actually spliced, and honors its
/// switch. Record-only, so the assertions are about presence and sanity, not about a threshold —
/// there is deliberately no threshold to assert (§7.4 item 1).
#[test]
fn patched_gap_records_a_fill_level_unless_switched_off() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_production_at(16_000, 1, 32.0, 3.0);
    let mut repair = production_repair_config(GapSignatureMode::Energy, 3.0);

    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let report = scan_gaps_for_fixture(&fixture, temp.path());
    let result = patch
        .execute(
            patch_request_from_repair(report, &repair),
            RepairConfig::default().crossfade_ms,
        )
        .expect("scan→patch");
    let check = result.summary.gaps[0]
        .fill_level
        .expect("a spliced gap records its fill level");
    assert!(
        check.peak_delta_db.is_finite() && check.peak_bin_db.is_finite(),
        "{check:?}"
    );
    assert!(check.bins > 0, "{check:?}");
    assert!(
        check.peak_bin_index < check.bins,
        "the peak is one of the bins measured: {check:?}"
    );
    assert!(
        check.reference_db
            == check
                .pre_shoulder_db
                .into_iter()
                .chain(check.post_shoulder_db)
                .fold(f64::NEG_INFINITY, f64::max),
        "the reference is the louder shoulder: {check:?}"
    );

    repair.measure_fill_level = false;
    let report = scan_gaps_for_fixture(&fixture, temp.path());
    let off = patch
        .execute(
            patch_request_from_repair(report, &repair),
            RepairConfig::default().crossfade_ms,
        )
        .expect("scan→patch");
    assert!(
        off.summary.gaps[0].fill_level.is_none(),
        "--no-measure-fill-level skips the pass: {:?}",
        off.summary.gaps[0].fill_level
    );
}

#[test]
fn scan_detects_f1_production_gap() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_production(48_000, 2, 3.0);
    let (expected_start, expected_end, _, _, _) = gap_report_times(&fixture);
    let report = scan_gaps_for_fixture(&fixture, temp.path());
    let fillable: Vec<_> = report.gaps.iter().filter(|g| g.is_fillable()).collect();
    assert_eq!(
        fillable.len(),
        1,
        "expected one fillable gap: {:#?}",
        report.gaps
    );
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
