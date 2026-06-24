//! Real-media floor-oracle calibration (FLOOR_OK / M2–G3).
//!
//! Run: `scripts/fetch_corpus_sources.ps1`
//!      `cargo test -p clip-sync-repair source_gap_oracle_floor_csv -- --ignored --nocapture`

mod common;

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::testing::ffmpeg_util;
use clip_sync::SymphoniaMediaReader;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::policies::DEFAULT_RESIDUAL_FLOOR_OK_DB;
use clip_sync_repair::domain::{GapPatchSkipReason, GapPatchStatus, GapSignatureMode};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_floor_oracle, patch_request_from_repair, production_repair_config,
};

use common::floor_oracle_fixtures::{
    build_floor_oracle_pair, case_sources_ready, decode_to_mono_wav_at, format_label, load_manifest,
    read_mono_wav, BuiltFloorOracle, OracleVariant,
};

struct FloorOracleRun {
    status: &'static str,
    skip_reason: String,
    structure_ok: bool,
    residual: Option<clip_sync_repair::domain::policies::SeamResidualVerdict>,
    align_adjustment_secs: f64,
}

fn floor_oracle_repair_config() -> RepairConfig {
    production_repair_config(GapSignatureMode::Energy, 3.0)
}

fn gap_status_label(status: &GapPatchStatus) -> (&'static str, String) {
    match status {
        GapPatchStatus::Patched { .. } => ("patched", String::new()),
        GapPatchStatus::Skipped { reason } => ("skipped", skip_reason_label(reason)),
        GapPatchStatus::NotPlanned { reason } => ("not_planned", format!("{reason:?}")),
    }
}

fn skip_reason_label(reason: &GapPatchSkipReason) -> String {
    match reason {
        GapPatchSkipReason::CorrelationBelowThreshold { .. } => "correlation_below".into(),
        GapPatchSkipReason::ResidualHeadroomExceeded { .. } => "residual_headroom".into(),
        GapPatchSkipReason::BExtractFailed => "b_extract_failed".into(),
        GapPatchSkipReason::BoundaryAlignmentFailed => "boundary_alignment_failed".into(),
        GapPatchSkipReason::AlignedSegmentOutOfRange => "aligned_segment_out_of_range".into(),
        GapPatchSkipReason::ZeroLengthGap => "zero_length_gap".into(),
    }
}

fn run_built_floor_oracle(built: &BuiltFloorOracle) -> FloorOracleRun {
    let dir = built.path_a.parent().expect("path_a parent");
    let decoded_a = dir.join("patch_a.wav");
    assert!(decode_to_mono_wav_at(
        &built.path_a,
        &decoded_a,
        built.meta.sample_rate,
        None,
    ));
    let (_, decoded_a_mono) = read_mono_wav(&decoded_a);

    let report = gap_report_from_floor_oracle(
        &built.path_a,
        &built.path_b,
        &decoded_a_mono,
        built.meta.sample_rate,
        built.meta.gap_start_frame,
        built.meta.gap_end_frame,
    );

    let repair = floor_oracle_repair_config();
    let mut request = patch_request_from_repair(report, &repair);
    request.measure_residual = true;

    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let result = patch
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("floor oracle patch");

    let gap = result.summary.gaps.first().expect("one gap");
    let (status, skip_reason) = gap_status_label(&gap.status);
    let structure_ok = status == "patched" || !skip_reason.contains("structure");
    let align_adjustment_secs = match &gap.status {
        GapPatchStatus::Patched {
            align_adjustment_secs,
            ..
        } => *align_adjustment_secs,
        _ => f64::NAN,
    };

    FloorOracleRun {
        status,
        skip_reason,
        structure_ok,
        residual: gap.residual,
        align_adjustment_secs,
    }
}

fn variant_label(variant: OracleVariant) -> &'static str {
    match variant {
        OracleVariant::SameMaster => "same_master",
        OracleVariant::TwoMic => "two_mic",
    }
}

fn print_csv_header() {
    println!(
        "case_id,variant,source_id,donor_source_id,format_a,format_b,\
         status,skip_reason,structure_ok,informative,floor_pre_db,floor_post_db,\
         headroom_db,align_adjustment_secs"
    );
}

fn print_csv_row(built: &BuiltFloorOracle, run: &FloorOracleRun) {
    let informative = run.residual.map(|v| v.informative).unwrap_or(false);
    let floor_pre = run.residual.map(|v| v.floor_pre_db).unwrap_or(f64::NAN);
    let floor_post = run.residual.map(|v| v.floor_post_db).unwrap_or(f64::NAN);
    let headroom = run
        .residual
        .map(|v| v.worst_headroom_db())
        .unwrap_or(f64::NAN);

    println!(
        "{},{},{},{},{},{},{},{},{},{informative},{floor_pre:.1},{floor_post:.1},{headroom:.1},{:.3}",
        built.meta.case_id,
        variant_label(built.meta.variant),
        built.meta.source_id,
        built.meta.donor_source_id.as_deref().unwrap_or(""),
        format_label(built.meta.format_a),
        format_label(built.meta.format_b),
        run.status,
        run.skip_reason,
        run.structure_ok,
        run.align_adjustment_secs,
    );
}

fn assert_floor_expectations(case_id: &str, built: &BuiltFloorOracle, run: &FloorOracleRun) {
    let Some(residual) = run.residual else {
        panic!(
            "{case_id}: no residual on outcome (status={}, skip={}) — \
             patch path blocked before floor measurement",
            run.status,
            run.skip_reason
        );
    };

    if built.meta.expect_informative_floor {
        assert!(
            residual.informative,
            "{case_id}: expected informative floor (pre={:.1} post={:.1}, FLOOR_OK={DEFAULT_RESIDUAL_FLOOR_OK_DB})",
            residual.floor_pre_db,
            residual.floor_post_db,
        );
    } else {
        assert!(
            !residual.informative,
            "{case_id}: two-mic should not be informative (pre={:.1} post={:.1})",
            residual.floor_pre_db,
            residual.floor_post_db,
        );
    }
}

#[test]
fn floor_oracle_manifest_loads() {
    let manifest = load_manifest();
    assert!(manifest.version >= 1);
    assert!(
        manifest.case.len() >= 7,
        "expected speech+ambient wav/aac matrix and two_mic case"
    );
}

#[test]
fn floor_oracle_gap_frames_use_production_anchor() {
    use common::floor_oracle_fixtures::{gap_frames_for_case, FloorOracleCase, FloorOracleDefaults};

    let defaults = FloorOracleDefaults::default();
    let case = FloorOracleCase {
        id: "geom".into(),
        source_id: "x".into(),
        donor_source_id: None,
        oracle_variant: None,
        format_a: None,
        format_b: None,
        aac_bitrate_a: None,
        aac_bitrate_b: None,
        total_secs: Some(60),
        sample_rate: Some(48_000),
        gap_duration_secs: Some(1.0),
        gap_signature_context_secs: Some(3.0),
        b_encode_delay_ms: None,
        expect_informative_floor: None,
        ignore: false,
    };
    let (start, end) = gap_frames_for_case(&case, &defaults);
    assert_eq!(start, 14 * 48_000);
    assert_eq!(end - start, 48_000);
}

#[test]
#[ignore = "needs fetch_corpus_sources + ffmpeg: cargo test -p clip-sync-repair source_gap_oracle_floor_csv -- --ignored --nocapture"]
fn source_gap_oracle_floor_csv() {
    if !ffmpeg_util::ffmpeg_available() {
        eprintln!("skipping source_gap_oracle_floor_csv: ffmpeg unavailable");
        return;
    }

    let manifest = load_manifest();
    let temp = tempfile::tempdir().expect("tempdir");

    print_csv_header();

    let mut ran = 0usize;
    for case in &manifest.case {
        if case.ignore {
            continue;
        }
        if !case_sources_ready(case) {
            eprintln!(
                "skip {}: run scripts/fetch_corpus_sources.ps1 (source cache missing)",
                case.id
            );
            continue;
        }

        let case_dir = temp.path().join(&case.id);
        let built = build_floor_oracle_pair(&case_dir, case, &manifest.defaults);
        let run = run_built_floor_oracle(&built);
        print_csv_row(&built, &run);
        assert_floor_expectations(&case.id, &built, &run);
        ran += 1;
    }

    if ran == 0 {
        eprintln!(
            "source_gap_oracle_floor_csv: no cases ran (fetch sources or check ffmpeg)"
        );
    }
}
