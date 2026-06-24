//! P1 step 2 (in-memory tier): injected-gap **same-master** oracle through the real patch pipeline.
//!
//! Builds a broadband master, derives A (master with a silenced gap + independent codec-like noise)
//! and B (the full master + independent noise) — a same-master, different-"encode" pair where the
//! true fill sits at the gap timestamp (nominal == truth). Runs the actual `PatchAudio::execute`
//! with `measure_residual = true` and reads `GapPatchOutcome.residual`, validating the JSON
//! plumbing end-to-end and emitting a labeled headroom row at a known-correct fill (the
//! false-positive / FLOOR_OK calibration substrate). Real-codec tier:
//! `tests/floor_oracle/manifest.toml` + `source_gap_oracle_floor_csv` (Wikimedia sources).
//!
//! Run: `cargo test -p clip-sync-repair seam_residual_oracle_csv -- --ignored --nocapture`
//! H2-B rescue: `cargo test -p clip-sync-repair broadband_oracle_veto_rescue_patches_marginal -- --ignored --nocapture`

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::SymphoniaMediaReader;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::gap_structure::StructureMatchParams;
use clip_sync_repair::domain::{
    FillConfidence, FitBoundarySearch, GapPatchSkipReason, GapPatchStatus, GapSignatureMode,
    ResidualGateMode, DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    gap_anchor_secs, structure_slide_secs, EnergySignatureFixture, ProductionScenarioSpec,
};
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair, production_repair_config,
};

const PI: f64 = std::f64::consts::PI;

fn lcg(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((*state >> 33) as f64 / (1u64 << 31) as f64) - 1.0
}

/// Broadband, non-stationary master (three chirps + shaped noise) — see the corpus harness.
fn broadband_master(total: usize, rate: u32) -> Vec<f64> {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    (0..total)
        .map(|i| {
            let t = i as f64 / rate as f64;
            let c1 = (2.0 * PI * (150.0 * t + 0.5 * 40.0 * t * t)).sin() * 3000.0;
            let c2 = (2.0 * PI * (400.0 * t - 0.5 * 15.0 * t * t)).sin() * 2000.0;
            let c3 = (2.0 * PI * (900.0 * t + 0.5 * 25.0 * t * t)).sin() * 1200.0;
            c1 + c2 + c3 + lcg(&mut seed) * 1500.0
        })
        .collect()
}

/// Same-master broadband oracle at production geometry: A = master with a silenced gap + noise,
/// B = full master + independent noise. `nominal == truth == gap_start`.
fn build_broadband_oracle(rate: u32, channels: usize, noise_amp: f64) -> EnergySignatureFixture {
    let ch = channels.max(1);
    let spec = ProductionScenarioSpec::production_standard(60.0, 3.0);
    let total_frames = (60.0 * rate as f64) as usize;
    let bin = spec.bin_frames(rate);
    let context = spec.context_frames(rate, total_frames);
    let gap = spec.min_gap_frames(rate).max(bin * 2);
    let anchor = (gap_anchor_secs(&spec) * rate as f64) as usize;
    let gap_start = anchor.max(context + bin);
    let gap_end = gap_start + gap;

    let master = broadband_master(total_frames, rate);
    let mut seed_a = 0x1111_2222_3333_4444u64;
    let mut seed_b = 0x5555_6666_7777_8888u64;
    let mut a = vec![0i16; total_frames * ch];
    let mut b = vec![0i16; total_frames * ch];
    for (f, &m) in master.iter().enumerate() {
        let in_gap = (gap_start..gap_end).contains(&f);
        for c in 0..ch {
            let idx = f * ch + c;
            let a_val = if in_gap { 0.0 } else { m + lcg(&mut seed_a) * noise_amp };
            let b_val = m + lcg(&mut seed_b) * noise_amp;
            a[idx] = a_val.round().clamp(-32768.0, 32767.0) as i16;
            b[idx] = b_val.round().clamp(-32768.0, 32767.0) as i16;
        }
    }

    let structure_params: StructureMatchParams =
        spec.structure_match_params(rate, gap, spec.search_radius_frames(rate));

    EnergySignatureFixture {
        id: "broadband_oracle",
        a_samples: a,
        b_samples: b,
        channels: ch,
        sample_rate: rate,
        gap_start,
        gap_end,
        context_frames: context,
        true_fill_start: gap_start,
        true_fill_end: gap_end,
        nominal_fill_start: gap_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

/// Production-like repair defaults for H2-B: shipped fit weights and gate floors, energy signature.
fn production_like_broadband_repair(residual_gate: ResidualGateMode) -> RepairConfig {
    RepairConfig {
        gap_signature_mode: GapSignatureMode::Energy,
        gap_signature_context_secs: 3.0,
        residual_gate,
        ..RepairConfig::default()
    }
}

/// H2-B: broadband same-master oracle patches via residual **rescue** when Pearson is ~0.
///
/// At the true fill, residual headroom ≈ 0 but `fill_seam_correlations` cannot score broadband
/// borders (Pearson below `fill_absolute_floor`). `residual_gate = veto_rescue` must upgrade the
/// dead zone to `FillConfidence::Marginal`. With the gate off, the same placement path skips.
/// The rescued arm also asserts `align_adjustment_secs` is within ~one energy bin of the true
/// slide so rescue is not masking a wrong placement.
#[test]
#[ignore = "slow (~100s): cargo test -p clip-sync-repair broadband_oracle_veto_rescue_patches_marginal -- --ignored --nocapture"]
fn broadband_oracle_veto_rescue_patches_marginal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_broadband_oracle(48_000, 1, 40.0);
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let crossfade_ms = RepairConfig::default().crossfade_ms;

    let off_repair = production_like_broadband_repair(ResidualGateMode::Off);
    let off_report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let off_result = patch
        .execute(
            patch_request_from_repair(off_report, &off_repair),
            crossfade_ms,
        )
        .expect("broadband oracle patch (gate off)");
    assert_eq!(
        off_result.summary.patched_count, 0,
        "gate off: Pearson dead zone should skip broadband oracle: {:?}",
        off_result.summary.gaps,
    );
    match &off_result.summary.gaps[0].status {
        GapPatchStatus::Skipped {
            reason: GapPatchSkipReason::CorrelationBelowThreshold { .. },
        } => {}
        other => panic!("gate off: expected waveform/structure skip, got {other:?}"),
    }

    let rescue_repair = production_like_broadband_repair(ResidualGateMode::VetoRescue);
    let rescue_report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let mut rescue_request = patch_request_from_repair(rescue_report, &rescue_repair);
    rescue_request.measure_residual = true;
    let rescue_result = patch
        .execute(rescue_request, crossfade_ms)
        .expect("broadband oracle patch (veto_rescue)");
    assert_eq!(
        rescue_result.summary.patched_count, 1,
        "veto_rescue: should patch via residual rescue: {:?}",
        rescue_result.summary.gaps,
    );
    assert_eq!(
        rescue_result.summary.patched_marginal_count, 1,
        "rescued patch should be marginal tier: {:?}",
        rescue_result.summary.gaps,
    );

    let gap = &rescue_result.summary.gaps[0];
    let truth_slide = structure_slide_secs(&fixture, fixture.true_fill_start);
    let bin_secs = fixture.bin_frames() as f64 / fixture.sample_rate as f64;
    let slide_tol = (bin_secs * 1.1).max(0.1);
    match &gap.status {
        GapPatchStatus::Patched {
            pre_correlation,
            post_correlation,
            confidence,
            align_adjustment_secs,
            ..
        } => {
            assert_eq!(*confidence, FillConfidence::Marginal);
            let pearson_min = pre_correlation.min(*post_correlation);
            assert!(
                pearson_min < f64::from(RepairConfig::default().fill_absolute_floor),
                "broadband Pearson should be below absolute floor (got min {pearson_min:.3})",
            );
            assert!(
                (align_adjustment_secs - truth_slide).abs() <= slide_tol,
                "rescued patch should land at truth slide {truth_slide:.3}s (got {align_adjustment_secs:.3}s, tol {slide_tol:.3}s)",
            );
        }
        other => panic!("veto_rescue: expected patched, got {other:?}"),
    }

    let verdict = gap
        .residual
        .expect("residual on patched gap when measure_residual + veto_rescue");
    assert!(
        verdict.informative,
        "same-master oracle floor should be informative: floor pre={:.1} post={:.1}",
        verdict.floor_pre_db,
        verdict.floor_post_db,
    );
    assert!(
        verdict.worst_headroom_db() <= DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        "headroom {:.1} dB should be within rescue margin {:.1} dB",
        verdict.worst_headroom_db(),
        DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
    );
}

/// Run one config on the broadband oracle, return `(status_label, pearson_pre, pearson_post,
/// headroom_db_or_nan)`. `measure` enables residual measurement (skip it for the costly full grid).
fn run_oracle_config(
    temp: &std::path::Path,
    fixture: &EnergySignatureFixture,
    repair: &RepairConfig,
    measure: bool,
) -> (String, f64, f64, f64) {
    let report = gap_report_from_energy_fixture(temp, fixture);
    let mut request = patch_request_from_repair(report, repair);
    request.measure_residual = measure;
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let result = patch
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("oracle patch");
    let gap = result.summary.gaps.first().expect("one gap");
    let headroom = gap.residual.map_or(f64::NAN, |v| v.worst_headroom_db());
    match &gap.status {
        GapPatchStatus::Patched { pre_correlation, post_correlation, .. } => {
            ("patched".into(), *pre_correlation, *post_correlation, headroom)
        }
        GapPatchStatus::Skipped { reason } => match reason {
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation,
                post_correlation,
                ..
            } => ("skipped_corr".into(), *pre_correlation, *post_correlation, headroom),
            other => (format!("skipped:{other:?}"), f64::NAN, f64::NAN, headroom),
        },
        GapPatchStatus::NotPlanned { reason } => {
            (format!("not_planned:{reason:?}"), f64::NAN, f64::NAN, headroom)
        }
    }
}

/// H2 Part B experiment: does the broadband same-master gap actually *patch* under realistic
/// configs, or does the coarse search leave the waveform seam too low to pass the Pearson gate?
/// Compares default fit weights, `--full` (full grid + boundary extend), and structure isolation.
#[test]
#[ignore = "diagnostic: cargo test -p clip-sync-repair seam_residual_h2_placement_experiment -- --ignored --nocapture"]
fn seam_residual_h2_placement_experiment() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_broadband_oracle(48_000, 1, 40.0);

    let default_fit = RepairConfig {
        gap_signature_mode: GapSignatureMode::Energy,
        gap_signature_context_secs: 3.0,
        ..RepairConfig::default()
    };
    // Bounded full grid: a real `--full` here uses fill_border_search_secs=10 + a ~13×13 grid and
    // costs minutes-to-30min/gap. We shrink the B search radius and the extend span to a tractable
    // proxy — the COST is intentionally not representative; we only need the patch/skip OUTCOME
    // (does the grid/extend mechanism rescue the broadband gap?).
    let full_grid = RepairConfig {
        fit_boundary_search: FitBoundarySearch::FullGrid,
        gap_end_extend_on_post_seam_fail: true,
        gap_start_extend_on_pre_seam_fail: true,
        fill_border_search_secs: 2.0,
        gap_end_extend_max_ms: 200,
        gap_end_extend_step_ms: 40,
        ..default_fit.clone()
    };
    let structure_iso = production_repair_config(GapSignatureMode::Energy, 3.0);

    println!("config,status,pearson_pre,pearson_post,headroom_db");
    for (label, repair, measure) in [
        ("default_fit", &default_fit, true),
        ("full_grid", &full_grid, false), // per-cell residual would be costly; only need patch/skip
        ("structure_iso", &structure_iso, true),
    ] {
        let (status, pre, post, headroom) = run_oracle_config(temp.path(), &fixture, repair, measure);
        println!("{label},{status},{pre:.3},{post:.3},{headroom:.1}");
    }
}

#[test]
#[ignore = "diagnostic: cargo test -p clip-sync-repair seam_residual_oracle_csv -- --ignored --nocapture"]
fn seam_residual_oracle_csv() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_broadband_oracle(48_000, 1, 40.0);
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    // Structure-isolated config so the gap reliably patches (relaxed gate floors). The chosen
    // residual and the floor are measured on the same raw reference window, so even though the
    // structure-chosen placement isn't waveform-refined, both map to the (correct, same-master)
    // content and cancel — headroom collapses to ~0 at the true fill.
    let repair = production_repair_config(GapSignatureMode::Energy, 3.0);
    let mut request = patch_request_from_repair(report, &repair);
    request.measure_residual = true;

    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let result = patch
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("oracle patch");

    let gap = result.summary.gaps.first().expect("one gap");
    assert!(
        matches!(gap.status, GapPatchStatus::Patched { .. }),
        "same-master oracle gap should patch: {:?}",
        gap.status
    );
    let v = gap
        .residual
        .expect("residual present (measure_residual on + patched) — validates JSON plumbing");

    println!(
        "fixture,variant,placement,chosen_pre_db,chosen_post_db,\
         floor_pre_db,floor_post_db,headroom_db,floor_source_pre,floor_source_post"
    );
    println!(
        "broadband_oracle,codec_noise,true_fill,{:.1},{:.1},{:.1},{:.1},{:.1},{:?},{:?}",
        v.chosen_pre_db,
        v.chosen_post_db,
        v.floor_pre_db,
        v.floor_post_db,
        v.worst_headroom_db(),
        v.floor_source_pre,
        v.floor_source_post,
    );

    // Floor establishes (same-master cancels) → regime informative; and headroom at the true fill
    // is small now that chosen and floor share the raw reference window (H1 fix).
    assert!(
        v.floor_pre_db < -20.0 && v.floor_post_db < -20.0,
        "floor should establish (same-master cancels): pre={:.1} post={:.1}",
        v.floor_pre_db,
        v.floor_post_db,
    );
    assert!(v.informative, "same-master oracle should have informative floor");
    assert!(
        v.worst_headroom_db() < 6.0,
        "headroom at the true fill should be small after the raw-window fix, got {:.1} dB",
        v.worst_headroom_db()
    );
}
