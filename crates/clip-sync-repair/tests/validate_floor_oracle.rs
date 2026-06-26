//! Floor oracle validation (ffmpeg + corpus).

use clip_sync_repair::domain::ResidualGateMode;
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair_harness::floor_oracle::{
    build_floor_oracle_pair, format_label, load_manifest, require_case_sources,
    require_validation_env, BuiltFloorOracle, FloorOracleCase, FloorOracleManifest, OracleVariant,
};
use clip_sync_repair_harness::residual_gate::{
    assert_deadzone_punch_inert, assert_floor_expectations,
    assert_production_fit_gate_no_worse_than_off_baseline,
    assert_production_fit_pearson_deadzone,
    assert_production_fit_veto_rescue_matches_veto_baseline, assert_truth_patches,
    assert_veto_rescue_matches_veto_on_truth, check_floor_expectations, production_fit_repair_config,
    rescue_trigger, run_built_floor_oracle, run_built_floor_oracle_cfg,
    run_built_floor_oracle_production_fit, run_pearson_min, FloorOracleRun,
};

fn variant_label(variant: OracleVariant) -> &'static str {
    match variant {
        OracleVariant::SameMaster => "same_master",
        OracleVariant::TwoMic => "two_mic",
    }
}

fn manifest_case<'a>(manifest: &'a FloorOracleManifest, id: &str) -> &'a FloorOracleCase {
    manifest
        .case
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("missing floor oracle case {id}"))
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

#[test]
fn source_gap_oracle_floor_csv() {
    require_validation_env();

    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let temp = tempfile::tempdir().expect("tempdir");

    print_csv_header();

    let mut ran = 0usize;
    let mut mismatches: Vec<String> = Vec::new();
    for case in &manifest.case {
        if case.ignore {
            continue;
        }
        require_case_sources(case);

        let case_dir = temp.path().join(&case.id);
        let built = build_floor_oracle_pair(&case_dir, case, &manifest.defaults);
        let run = run_built_floor_oracle(&built, ResidualGateMode::Off);
        print_csv_row(&built, &run);
        if let Err(e) = check_floor_expectations(&case.id, &built, &run) {
            mismatches.push(e);
        }
        ran += 1;
    }

    assert!(
        mismatches.is_empty(),
        "floor expectation mismatches ({} — update manifest `expect_informative_floor`):\n{}",
        mismatches.len(),
        mismatches.join("\n"),
    );

    assert!(
        ran > 0,
        "validation tier: no floor oracle cases ran — manifest has no runnable rows or all cases are `ignore = true`"
    );
}

#[test]
fn floor_oracle_vorbis_64k_veto_no_false_veto() {
    require_validation_env();
    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let case = manifest_case(&manifest, "cc_speech_gap_oracle_vorbis_64k");
    require_case_sources(case);
    let temp = tempfile::tempdir().expect("tempdir");
    let built = build_floor_oracle_pair(&temp.path().join(&case.id), case, &manifest.defaults);
    let off = run_built_floor_oracle(&built, ResidualGateMode::Off);
    assert_eq!(off.status, "patched", "vorbis_64k should patch (skip={})", off.skip_reason);
    let veto = run_built_floor_oracle(&built, ResidualGateMode::Veto);
    assert_eq!(
        veto.status, "patched",
        "M5: veto must not false-veto when slide > reach (abstain); skip={}",
        veto.skip_reason
    );
}

/// Real-codec residual gate on Wikimedia floor oracles: truth gaps patch under `off`/`veto`/
/// `veto_rescue`; unrelated two-mic must not be rescued into a patch.
#[test]
fn floor_oracle_residual_gate_real_codec() {
    require_validation_env();

    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let ambient = manifest_case(&manifest, "cc_ambient_gap_oracle_aac_same");
    let speech = manifest_case(&manifest, "cc_speech_gap_oracle_aac_same");
    let speech_vorbis = manifest_case(&manifest, "cc_speech_gap_oracle_vorbis_128k");
    let ambient_vorbis = manifest_case(&manifest, "cc_ambient_gap_oracle_vorbis_128k");
    let two_mic = manifest_case(&manifest, "cc_speech_ambient_two_mic");

    for case in [ambient, speech, speech_vorbis, ambient_vorbis, two_mic] {
        require_case_sources(case);
    }

    let temp = tempfile::tempdir().expect("tempdir");

    let ambient_built =
        build_floor_oracle_pair(&temp.path().join(&ambient.id), ambient, &manifest.defaults);
    for gate in [
        ResidualGateMode::Off,
        ResidualGateMode::Veto,
        ResidualGateMode::VetoRescue,
    ] {
        let run = run_built_floor_oracle(&ambient_built, gate);
        assert_truth_patches(&ambient.id, &run, gate);
    }

    let speech_built =
        build_floor_oracle_pair(&temp.path().join(&speech.id), speech, &manifest.defaults);
    for gate in [ResidualGateMode::Off, ResidualGateMode::Veto] {
        let run = run_built_floor_oracle(&speech_built, gate);
        assert_truth_patches(&speech.id, &run, gate);
    }
    let speech_rescue = run_built_floor_oracle(&speech_built, ResidualGateMode::VetoRescue);
    assert_truth_patches(&speech.id, &speech_rescue, ResidualGateMode::VetoRescue);

    let speech_vorbis_built = build_floor_oracle_pair(
        &temp.path().join(&speech_vorbis.id),
        speech_vorbis,
        &manifest.defaults,
    );
    for gate in [
        ResidualGateMode::Off,
        ResidualGateMode::Veto,
        ResidualGateMode::VetoRescue,
    ] {
        let run = run_built_floor_oracle(&speech_vorbis_built, gate);
        assert_truth_patches(&speech_vorbis.id, &run, gate);
    }

    let ambient_vorbis_built = build_floor_oracle_pair(
        &temp.path().join(&ambient_vorbis.id),
        ambient_vorbis,
        &manifest.defaults,
    );
    for gate in [
        ResidualGateMode::Off,
        ResidualGateMode::Veto,
        ResidualGateMode::VetoRescue,
    ] {
        let run = run_built_floor_oracle(&ambient_vorbis_built, gate);
        assert_truth_patches(&ambient_vorbis.id, &run, gate);
    }

    let two_mic_built =
        build_floor_oracle_pair(&temp.path().join(&two_mic.id), two_mic, &manifest.defaults);
    let off = run_built_floor_oracle(&two_mic_built, ResidualGateMode::Off);
    let rescue = run_built_floor_oracle(&two_mic_built, ResidualGateMode::VetoRescue);
    assert_floor_expectations(&two_mic.id, &two_mic_built, &off);
    assert_floor_expectations(&two_mic.id, &two_mic_built, &rescue);
    assert!(
        !off.residual.is_some_and(|v| v.informative),
        "two_mic floor must be uninformative (gate off)"
    );
    assert!(
        !rescue.residual.is_some_and(|v| v.informative),
        "two_mic floor must stay uninformative under veto_rescue"
    );
    if off.status != "patched" {
        assert_ne!(
            rescue.status, "patched",
            "veto_rescue must not rescue unrelated two-mic content (off skip={})",
            off.skip_reason
        );
    }
    if off.status == "patched" {
        let veto = run_built_floor_oracle(&two_mic_built, ResidualGateMode::Veto);
        assert_eq!(
            veto.status, "patched",
            "veto must not skip when gate off patches unrelated two-mic (skip={})",
            veto.skip_reason
        );
    }
}

/// Real-codec `veto_rescue` on broadband (music) and dual-bitrate Vorbis truth gaps.
///
/// Real Wikimedia/Musopen masters do not hit the synthetic H2-B Pearson dead zone at truth
/// placement — Pearson passes and `veto_rescue` must match `veto` (no extra marginal patches).
/// Dead-zone rescue proof stays on `broadband_oracle_veto_rescue_patches_marginal`
/// (`seam_residual_oracle.rs`). This test ties rescue safety to the same corpus as FLOOR_OK.
#[test]
fn floor_oracle_veto_rescue_real_broadband_codec() {
    require_validation_env();

    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let cases = [
        manifest_case(&manifest, "cc_music_gap_oracle_vorbis_128k"),
        manifest_case(&manifest, "cc_music_gap_oracle_vorbis_dual"),
        manifest_case(&manifest, "cc_speech_gap_oracle_vorbis_dual"),
        manifest_case(&manifest, "cc_ambient_gap_oracle_vorbis_dual"),
    ];

    for case in cases {
        require_case_sources(case);
    }

    let temp = tempfile::tempdir().expect("tempdir");
    for case in cases {
        let built =
            build_floor_oracle_pair(&temp.path().join(&case.id), case, &manifest.defaults);
        assert_veto_rescue_matches_veto_on_truth(&case.id, &built);
        let rescue = run_built_floor_oracle(&built, ResidualGateMode::VetoRescue);
        assert_floor_expectations(&case.id, &built, &rescue);
    }
}

/// Real-codec residual gate under **production_fit** (shipped floors + waveform weighting).
///
/// Mirrors the calibration [`floor_oracle_residual_gate_real_codec`] corpus but with
/// production-fit expectations: shaped speech/ambient gaps may skip (Pearson tier) — the gate must
/// not rescue-patch when `off` skips; music mid-content control must patch (C3 positive, Run B
/// baseline); broadband finale gaps skip in the Pearson dead zone with uninformative floor; two-mic
/// must abstain (C2).
#[test]
fn gate_real_codec_production_fit() {
    require_validation_env();

    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let shaped_speech_ambient = [
        manifest_case(&manifest, "cc_ambient_gap_oracle_aac_same"),
        manifest_case(&manifest, "cc_speech_gap_oracle_aac_same"),
        manifest_case(&manifest, "cc_speech_gap_oracle_vorbis_128k"),
        manifest_case(&manifest, "cc_ambient_gap_oracle_vorbis_128k"),
    ];
    let music_control = manifest_case(&manifest, "cc_music_gap_oracle_aac_128k");
    let two_mic = manifest_case(&manifest, "cc_speech_ambient_two_mic");
    let finale_deadzone = manifest_case(&manifest, "cc_music_transient_aac_same");

    for case in shaped_speech_ambient
        .into_iter()
        .chain([music_control, two_mic, finale_deadzone])
    {
        require_case_sources(case);
    }

    let temp = tempfile::tempdir().expect("tempdir");

    for case in shaped_speech_ambient {
        eprintln!("gate_real_codec_production_fit: {}", case.id);
        let built =
            build_floor_oracle_pair(&temp.path().join(&case.id), case, &manifest.defaults);
        let off = run_built_floor_oracle_production_fit(&built, ResidualGateMode::Off);
        let veto = run_built_floor_oracle_production_fit(&built, ResidualGateMode::Veto);
        let rescue = run_built_floor_oracle_production_fit(&built, ResidualGateMode::VetoRescue);
        for (gate, run) in [
            (ResidualGateMode::Veto, &veto),
            (ResidualGateMode::VetoRescue, &rescue),
        ] {
            assert_production_fit_gate_no_worse_than_off_baseline(&case.id, &off, run, gate);
        }
        if off.status == "patched" {
            assert_production_fit_veto_rescue_matches_veto_baseline(&case.id, &veto, &rescue);
        }
    }

    eprintln!("gate_real_codec_production_fit: {}", music_control.id);
    let control_built = build_floor_oracle_pair(
        &temp.path().join(&music_control.id),
        music_control,
        &manifest.defaults,
    );
    let control_off =
        run_built_floor_oracle_production_fit(&control_built, ResidualGateMode::Off);
    let control_veto =
        run_built_floor_oracle_production_fit(&control_built, ResidualGateMode::Veto);
    let control_rescue =
        run_built_floor_oracle_production_fit(&control_built, ResidualGateMode::VetoRescue);
    assert_eq!(
        control_off.status, "patched",
        "{}: music mid-content control must patch under production_fit (Run B baseline)",
        music_control.id
    );
    for (gate, run) in [
        (ResidualGateMode::Off, &control_off),
        (ResidualGateMode::Veto, &control_veto),
        (ResidualGateMode::VetoRescue, &control_rescue),
    ] {
        assert_truth_patches(&music_control.id, run, gate);
        if gate != ResidualGateMode::Off {
            assert_production_fit_gate_no_worse_than_off_baseline(
                &music_control.id,
                &control_off,
                run,
                gate,
            );
        }
    }
    assert_production_fit_veto_rescue_matches_veto_baseline(
        &music_control.id,
        &control_veto,
        &control_rescue,
    );

    eprintln!("gate_real_codec_production_fit: {}", two_mic.id);
    let two_mic_built =
        build_floor_oracle_pair(&temp.path().join(&two_mic.id), two_mic, &manifest.defaults);
    let off = run_built_floor_oracle_production_fit(&two_mic_built, ResidualGateMode::Off);
    let veto = run_built_floor_oracle_production_fit(&two_mic_built, ResidualGateMode::Veto);
    let rescue = run_built_floor_oracle_production_fit(&two_mic_built, ResidualGateMode::VetoRescue);
    for (gate, run) in [
        (ResidualGateMode::Veto, &veto),
        (ResidualGateMode::VetoRescue, &rescue),
    ] {
        assert_production_fit_gate_no_worse_than_off_baseline(&two_mic.id, &off, run, gate);
    }
    assert_floor_expectations(&two_mic.id, &two_mic_built, &off);
    assert_floor_expectations(&two_mic.id, &two_mic_built, &rescue);
    assert!(
        !off.residual.is_some_and(|v| v.informative),
        "two_mic floor must be uninformative under production_fit (gate off)"
    );
    assert!(
        !rescue.residual.is_some_and(|v| v.informative),
        "two_mic floor must stay uninformative under veto_rescue + production_fit"
    );
    if off.status != "patched" {
        assert_ne!(
            rescue.status, "patched",
            "veto_rescue must not rescue unrelated two-mic content (off skip={})",
            off.skip_reason
        );
    }

    eprintln!("gate_real_codec_production_fit: {}", finale_deadzone.id);
    let finale_built = build_floor_oracle_pair(
        &temp.path().join(&finale_deadzone.id),
        finale_deadzone,
        &manifest.defaults,
    );
    for gate in [
        ResidualGateMode::Off,
        ResidualGateMode::Veto,
        ResidualGateMode::VetoRescue,
    ] {
        let gate_repair = production_fit_repair_config(gate);
        let run = run_built_floor_oracle_cfg(&finale_built, &gate_repair);
        assert_production_fit_pearson_deadzone(&finale_deadzone.id, &run, gate, &gate_repair);
    }
}

/// G5 assert — punch-after-encode finale gaps skip under all gates with uninformative floor.
///
/// Locks the Run B finding that `veto_rescue` is inert on real codec broadband noise when native
/// A borders remove the inject-then-encode confound. Baseline:
/// `tests/residual_gate_catalog/baseline_run_b_transient.csv`.
#[test]
fn deadzone_punch_assert() {
    require_validation_env();

    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let ids = [
        "cc_music_punch_finale_aac_same",
        "cc_music_punch_finale_aac_dual",
    ];

    let temp = tempfile::tempdir().expect("tempdir");
    for id in ids {
        let case = manifest_case(&manifest, id);
        require_case_sources(case);
        let built = build_floor_oracle_pair(&temp.path().join(id), case, &manifest.defaults);
        for gate in [
            ResidualGateMode::Off,
            ResidualGateMode::Veto,
            ResidualGateMode::VetoRescue,
        ] {
            let gate_repair = production_fit_repair_config(gate);
            let run = run_built_floor_oracle_cfg(&built, &gate_repair);
            assert_deadzone_punch_inert(id, &run, gate, &gate_repair);
        }
    }
}

/// Run B — transient/broadband dead-zone probe (exploratory, no assertions).
///
/// Anchors the gap on the Grieg fff finale (loudest broadband orchestral tutti in the corpus) and
/// re-encodes A/B same-master, then prints, for each gate, the seam Pearson at truth, the floor
/// verdict, and the headroom. The open question (does `veto_rescue` ever fire usefully on real
/// media?) is answered by the `rescue_trigger` column: TRUE iff Pearson is in the dead zone
/// (`min(pre,post) < min_fill_correlation`) AND the floor is informative AND `headroom ≤ margin`.
/// The first transient case is preceded by the benign mid-content `cc_music_gap_oracle_aac_128k`
/// control (where Pearson is known to pass), so the matrix shows both regimes side by side.
#[test]
fn source_gap_oracle_transient_csv() {
    require_validation_env();

    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let ids = [
        "cc_music_gap_oracle_aac_128k", // benign mid-content control
        "cc_music_transient_aac_same",
        "cc_music_transient_aac_dual",
        "cc_music_transient_vorbis_same",
        "cc_music_transient2_aac_dual",
        // Punch-after-encode (G5 confirmation): native A borders, clean gap.
        "cc_music_punch_finale_aac_same",
        "cc_music_punch_finale_aac_dual",
    ];

    let defaults_repair = RepairConfig::default();

    println!(
        "case_id,format_a,format_b,gate,status,skip_reason,seam_pre,seam_post,pearson_min,\
         informative,floor_pre_db,floor_post_db,headroom_db,align_secs,rescue_trigger"
    );

    let temp = tempfile::tempdir().expect("tempdir");
    for id in ids {
        let case = manifest_case(&manifest, id);
        if case.ignore {
            continue;
        }
        require_case_sources(case);
        let built = build_floor_oracle_pair(&temp.path().join(id), case, &manifest.defaults);
        for gate in [
            ResidualGateMode::Off,
            ResidualGateMode::Veto,
            ResidualGateMode::VetoRescue,
        ] {
            let run = run_built_floor_oracle_cfg(&built, &production_fit_repair_config(gate));
            let pearson_min = run_pearson_min(&run);
            let informative = run.residual.map(|v| v.informative).unwrap_or(false);
            let floor_pre = run.residual.map(|v| v.floor_pre_db).unwrap_or(f64::NAN);
            let floor_post = run.residual.map(|v| v.floor_post_db).unwrap_or(f64::NAN);
            let headroom = run
                .residual
                .map(|v| v.worst_headroom_db())
                .unwrap_or(f64::NAN);
            let rescue_trigger = rescue_trigger(&run, &defaults_repair);
            println!(
                "{},{},{},{:?},{},{},{:.3},{:.3},{:.3},{informative},{floor_pre:.1},{floor_post:.1},{headroom:.1},{:.4},{rescue_trigger}",
                built.meta.case_id,
                format_label(built.meta.format_a),
                format_label(built.meta.format_b),
                gate,
                run.status,
                run.skip_reason,
                run.seam_pre,
                run.seam_post,
                pearson_min,
                run.align_adjustment_secs,
            );
        }
    }
}

