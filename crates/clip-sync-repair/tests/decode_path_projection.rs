//! **C5 — live from-decode dump self-guard (always-on, no media).**
//!
//! Tier: **pr-repair** (synthetic A/B fixture; media-free). Runs the live from-decode dump pipeline
//! (`characterize_gaps_from_decode`, via the `synth_ab_from_decode_corpus` fixture) and asserts
//! its output is well-formed and survives a projection round-trip (`GapFingerprint → GapRepairTags →
//! GapFingerprint`) with every `golden_baseline` decision axis preserved — in both lean and diagnostics modes.
//!
//! History: this replaced the transitional old-vs-new decode differentials (`characterize_gaps_with_gate` vs
//! from-decode), which were **migration scaffolding**. The migration is complete and the oracle was deleted at
//! 8g.6, so we no longer check "new matches old" — only that the from-decode dump stays internally consistent.

/// Both modes: the live from-decode dump produces gaps, and each gap's decision axes survive the spec
/// projection round-trip unchanged (diagnostics-on additionally exercises the `b_levels` X-set carry).
#[test]
fn from_decode_dump_projection_preserves_golden_baseline() {
    for diagnostics in [false, true] {
        let corpus =
            clip_sync_repair_fixtures::fingerprint_corpus_fixtures::synth_ab_from_decode_corpus(
                diagnostics,
            );
        assert!(
            !corpus.gaps.is_empty(),
            "synthetic from-decode dump produced no gaps (diagnostics={diagnostics})"
        );
        let diffs = clip_sync_repair_harness::corpus_projection::projection_diff(&corpus);
        assert!(
            diffs.is_empty(),
            "projection changed {} decision-axis field(s) on the from-decode dump (diagnostics={diagnostics}):\n{}",
            diffs.len(),
            diffs.join("\n"),
        );
    }
}

/// `--fingerprint-keep-path-shift` is record-only: the unshifted bracket list is identical to a
/// dump with the flag off, and the shifted arms sit beside `outcome` rather than rewriting it.
#[test]
fn keep_path_shift_probe_preserves_unshifted_brackets_and_omits_when_off() {
    use clip_sync_repair::application::gap_fingerprint::{
        DumpOptions, KeepPathShiftMode, KeepPathShiftSource, KeepPathShiftUnavailable,
    };
    use clip_sync_repair_fixtures::fingerprint_corpus_fixtures::synth_ab_from_decode_corpus_with_dump;

    let off = synth_ab_from_decode_corpus_with_dump(DumpOptions::default());
    let on = synth_ab_from_decode_corpus_with_dump(DumpOptions {
        keep_path_shift: KeepPathShiftMode::Both,
        ..DumpOptions::default()
    });
    let fp_off = off.gaps.first().expect("one gap (off)");
    let fp_on = on.gaps.first().expect("one gap (on)");
    assert!(
        fp_off.keep_path_shift.is_none(),
        "probe off: keep_path_shift must be omitted"
    );
    let arms = fp_on
        .keep_path_shift
        .as_ref()
        .expect("probe on: keep_path_shift must be present");
    assert_eq!(arms.len(), 2, "both arms");
    assert_eq!(arms[0].source, KeepPathShiftSource::PreLock);
    assert_eq!(arms[1].source, KeepPathShiftSource::MidLag);
    assert_eq!(
        fp_off.outcome.as_ref().map(|o| o.tier.as_str()),
        fp_on.outcome.as_ref().map(|o| o.tier.as_str()),
        "unshifted outcome.tier must not move"
    );
    assert_eq!(
        fp_off.brackets.len(),
        fp_on.brackets.len(),
        "unshifted bracket count"
    );
    for (a, b) in fp_off.brackets.iter().zip(fp_on.brackets.iter()) {
        assert_eq!(a.move_frames, b.move_frames);
        assert_eq!(a.failure_stage, b.failure_stage);
        assert_eq!(a.seam_pre, b.seam_pre);
        assert_eq!(a.seam_post, b.seam_post);
        assert_eq!(a.structure_pre, b.structure_pre);
        assert_eq!(a.structure_post, b.structure_post);
        assert_eq!(a.start_frame, b.start_frame);
        assert_eq!(a.fill_frames, b.fill_frames);
    }
    assert!(
        off.source.probe_recipe.as_ref().unwrap().keep_path_shift.is_none(),
        "probe off: probe_recipe.keep_path_shift must be omitted"
    );
    assert_eq!(
        on.source.probe_recipe.as_ref().unwrap().keep_path_shift,
        Some(KeepPathShiftMode::Both),
        "probe on: recipe must stamp the arms that ran"
    );
    for arm in arms {
        if let Some(unavailable) = arm.unavailable {
            assert_eq!(arm.n_brackets, 0);
            assert!(
                arm.skip_reason.is_none(),
                "unavailable is not a FailureStage skip_reason"
            );
            match unavailable {
                KeepPathShiftUnavailable::Locks | KeepPathShiftUnavailable::LagDecision => {
                    assert!(arm.shift_ms.is_none(), "unknown lag omits shift_ms");
                }
                _ => {
                    assert!(
                        arm.shift_ms.is_some(),
                        "known lag kept when extract/map failed"
                    );
                }
            }
            continue;
        }
        assert_eq!(
            arm.n_brackets, fp_on.brackets.len(),
            "shifted arm scores the same A-side list"
        );
        assert!(
            arm.skip_reason
                .as_deref()
                .is_none_or(|s| !s.contains("unavailable") && s != "extract_empty"),
            "skip_reason is FailureStage only, got {:?}",
            arm.skip_reason
        );
        assert!(
            arm.b_extract_start_secs.is_some(),
            "scored arm stamps the shifted extract origin"
        );
        if arm.any_ok {
            assert!(arm.skip_reason.is_none());
        }
        assert_eq!(
            arm.any_ok,
            arm.n_ok > 0,
            "any_ok is overlay structure+waveform, not residual"
        );
    }
}

/// Delayed B puts content later in the file. Unshifted keep path searches ±20 ms around the old
/// map and must skip; the `mid_lag` arm applies a **positive** shift (look later in B) and must
/// convert. A flipped lag sign would search earlier and still skip.
#[test]
fn keep_path_shift_mid_lag_converts_a_delayed_b() {
    use clip_sync_repair::application::gap_fingerprint::{
        DumpOptions, KeepPathShiftMode, KeepPathShiftSource,
    };
    use clip_sync_repair_fixtures::fingerprint_corpus_fixtures::synth_ab_from_decode_corpus_with_dump_and_b_delay;

    const DELAY_SECS: f64 = 0.10;
    let corpus = synth_ab_from_decode_corpus_with_dump_and_b_delay(
        DumpOptions {
            keep_path_shift: KeepPathShiftMode::Both,
            ..DumpOptions::default()
        },
        DELAY_SECS,
    );
    let fp = corpus.gaps.first().expect("one gap");
    assert_eq!(
        fp.outcome.as_ref().map(|o| o.tier.as_str()),
        Some("skip"),
        "unshifted overlay must miss a 100 ms delay outside the 20 ms search"
    );
    let mid = fp
        .keep_path_shift
        .as_ref()
        .expect("probe on")
        .iter()
        .find(|a| a.source == KeepPathShiftSource::MidLag)
        .expect("mid_lag arm");
    assert!(
        mid.n_brackets > 0,
        "keep-path-shift fixture must score the throat"
    );
    let shift_ms = mid.shift_ms.expect("1 s lag known on delayed B");
    assert!(
        shift_ms > 50.0,
        "positive delay must produce a positive mid_lag beyond the search radius (flipped sign would be negative): {shift_ms}"
    );
    assert!(
        (shift_ms - DELAY_SECS * 1000.0).abs() < 25.0,
        "mid_lag should recover the delay, got {shift_ms} ms"
    );
    assert!(
        mid.any_ok,
        "registering by the measured lag must convert; a flipped sign would still skip"
    );
    assert!(mid.unavailable.is_none());
    let residual = mid
        .throat
        .residual
        .as_ref()
        .expect("shifted residual is a throat column");
    assert!(
        residual.headroom_ok.is_none(),
        "this fixture's residual abstains; any_ok must still be true (not residual-gated)"
    );
}
