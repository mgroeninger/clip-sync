//! Pure `GapRepairSpec` ↔ `GapFingerprint` projection (Fingerprint-unification 8e/8f).
//!
//! Depends only on [`super::schema`] (types) + domain tag structs — **no PCM measurement**. The
//! from-decode `tags_from_measurements` sibling stays in the `measure` slice (it consumes
//! `RegionMeasurements`); both funnel through the shared `tags_from_fields` core here so the oracle
//! (8f) and from-decode (8g.3b) paths read specs through one mapping. See the module-split plan.

use super::schema::*;
use crate::domain::gap_repair_spec::{GapRepairSpec, GapRepairVerdict, LevelTags};

// ---------------------------------------------------------------------------------------------
// GapRepairSpec → GapFingerprint projection (Fingerprint-unification 8e)
// ---------------------------------------------------------------------------------------------

/// Diagnostic X-set attached to a projected fingerprint — the fields production characterize does not carry
/// (§1.2 label X). `None`/empty on the production path; populated only when `fingerprint_diagnostics` is on
/// (8g). Kept out of the D/R projection so a decision-only fingerprint is `spec_to_fingerprint_summary(.., None)`.
#[derive(Debug, Clone, Default)]
pub struct FingerprintXSet {
    pub seam_probe: Option<SeamProbeFingerprint>,
    pub wide_envelope: Option<WideEnvelopeFingerprint>,
    pub b_levels: Option<LevelProfile>,
    pub lag_editorial: Option<LagFingerprint>,
}

/// Per-gap detail the **from-decode** path measured and the spec cannot carry.
///
/// The spec stores decision *scalars*; anything richer (the per-bracket rows, the full lag sweep) is
/// reconstructed by this module when it is absent, and reconstruction is lossy in ways that read as
/// measurements — see [`projected_lag_entry`] and [`synth_brackets`]. A caller that actually ran the
/// measurement hands it over here instead of letting the projection invent a stand-in, and the dump
/// then carries the real thing. `Default` (both `None`) is the oracle/spec-only path, which has
/// nothing to hand over.
///
/// Fabricated `lag_decision` shoulders (when this is `None`) are what
/// [`crate::application::gap_fingerprint::PROJECTED_LAG_DECISION_FIELDS`] declares. Envelope /
/// silence / contour / anchors / seam_shape are omitted outright (`None`) rather than declared.
#[derive(Debug, Clone, Default)]
pub struct MeasuredDetail {
    /// Real per-bracket rows (8g.4b), else [`synth_brackets`]' reconstruction from the stored counts.
    pub brackets: Option<Vec<BracketInfo>>,
    /// The real ±`max_lag_ms` sweep at `b_mapped` (`lag_at_placement`), else [`projected_lag_entry`]'s
    /// four-scalar stand-in.
    pub lag_decision: Option<LagFingerprint>,
}

/// Project a characterized [`GapRepairSpec`] into the licensing-safe [`GapFingerprint`] export schema
/// (Fingerprint-unification 8e). **Pure** — reads the spec's stored D/R tags + verdict and measures nothing
/// (the A7 single-source rule): every seam/lag/donor scalar is the value characterize already computed. X-set
/// fields are attached only when supplied.
///
/// Lossy **by design** on non-decision fields: `silence`/`contour`/`anchors`/`seam_shape` and the
/// levels envelope are omitted (`None` / skip-empty) when the spec cannot carry them — absence, not
/// zeros. `outcome.tier` is `patch`/`skip` (matching the scan path, `gap_fingerprint.rs` tier logic);
/// uniqueness validators (`*_seam_prom`/`*_seam_z`, `peak_z`) are `None` on the production path
/// (Tier-3, tolerated by the golden diff). `brackets` and `lag_decision` are the **real** measurements
/// when [`MeasuredDetail`] supplies them (the from-decode dump, 8g.4b), else reconstructed from the
/// stored scalars — round-tripping the counts/best/closest and the four registration scalars
/// respectively, not the original detail. See §2.5.2a / 8e.
pub fn spec_to_fingerprint_summary(
    spec: &GapRepairSpec,
    sample_rate: u32,
    channels: u16,
    x: Option<FingerprintXSet>,
    measured: MeasuredDetail,
) -> GapFingerprint {
    let tags = &spec.tags_ctx;
    let rate = f64::from(sample_rate.max(1));
    let refined_start_secs = spec.refined.start_frame as f64 / rate;
    let refined_end_secs = spec.refined.end_frame as f64 / rate;
    let x = x.unwrap_or_default();

    let gate = &tags.gate;
    let MeasuredDetail {
        brackets: measured_brackets,
        lag_decision: measured_lag_decision,
    } = measured;
    // Real per-bracket rows when characterize supplied them (from-decode dump, 8g.4b); else synthesize just
    // enough structure to round-trip the stored counts/best/closest (the corpus-projection path, which can't
    // recover per-bracket detail from stored tags).
    let brackets = measured_brackets.unwrap_or_else(|| {
        synth_brackets(
            gate.brackets_total,
            gate.brackets_passing,
            gate.best_bracket_seam,
            gate.closest_failure_stage.as_deref(),
        )
    });

    // Fingerprint-native skip label: closest failing bracket's `FailureStage` — not production
    // `GapPatchSkipReason`. Prefer the brackets we are about to emit; fall back to the stored gate
    // tag when synthesis could not recover a stage (empty / all-passing synth).
    let (tier, skip_reason) = match &spec.verdict {
        GapRepairVerdict::Patch(_) => ("patch".to_string(), None),
        GapRepairVerdict::Skip { .. } => (
            "skip".to_string(),
            closest_bracket_failure_stage(&brackets)
                .map(|s| s.as_str().to_string())
                .or_else(|| gate.closest_failure_stage.clone()),
        ),
    };

    let reg = &tags.registration;
    let splice = match (reg.step_ms, reg.pre_peak_r, reg.post_peak_r) {
        (Some(step_ms), Some(pre_peak_r), Some(post_peak_r)) => Some(SpliceSummary {
            step_ms,
            pre_peak_r,
            post_peak_r,
            pre_peak_z: reg.pre_peak_z,
            post_peak_z: reg.post_peak_z,
            edge_pinned: reg.edge_pinned,
        }),
        _ => None,
    };
    // The measured sweep when the caller ran one; otherwise the four-scalar stand-in, whose fabricated
    // half `source.not_measured` disowns.
    let lag_decision = measured_lag_decision.or_else(|| {
        if reg.pre_peak_r.is_some() || reg.post_peak_r.is_some() {
            Some(LagFingerprint {
                pre_anchor: projected_lag_entry(
                    reg.pre_peak_r,
                    reg.pre_frac_lag_ms,
                    reg.pre_peak_z,
                    reg.pre_prominence,
                ),
                post_anchor: projected_lag_entry(
                    reg.post_peak_r,
                    reg.post_frac_lag_ms,
                    reg.post_peak_z,
                    reg.post_prominence,
                ),
            })
        } else {
            None
        }
    });

    let structure = gate.structure_min.map(|m| StructureScores {
        baseline_pre: m,
        baseline_post: m,
    });
    let seams = gate.seam_min.map(|m| SeamScores {
        baseline_pre: m,
        baseline_post: m,
        selected_channels: Vec::new(),
        per_channel: Vec::new(),
        mono_pre: m,
        mono_post: m,
    });
    let residual = gate.residual.map(|r| ResidualInfo {
        chosen_pre_db: residual_db_opt(r.chosen_pre_db),
        chosen_post_db: residual_db_opt(r.chosen_post_db),
        floor_pre_db: residual_db_opt(r.floor_pre_db),
        floor_post_db: residual_db_opt(r.floor_post_db),
        // Always `Some` — the verdict carries a source for both sides, and it is what tells an absent
        // `floor_*_db` ("no reference window found") apart from a suppressed one ("measured, then
        // −120/non-finite"). Only the wire type is optional, for pre-2026-08-03 dumps.
        floor_source_pre: Some(r.floor_source_pre),
        floor_source_post: Some(r.floor_source_post),
        informative: r.informative,
        // Recorded, never recomputed on replay: `floor_above_ok_db` is relative to the run's
        // `residual_floor_ok_db` (echoed in `CorpusGateRecipe`), so re-deriving it later would answer
        // with the reader's threshold instead of the writer's.
        uninformative_pre: r.uninformative_pre,
        uninformative_post: r.uninformative_post,
        // Always `Some` on a dump written by this path — production knows both, and without them a
        // replayed verdict answers `beyond_lag_reach()` as if the placement never slid.
        placement_slide_frames: Some(r.placement_slide_frames),
        max_lag_frames: Some(r.max_lag_frames),
    });

    let gap_frames = spec
        .refined
        .end_frame
        .saturating_sub(spec.refined.start_frame);
    let splice_dualfit = tags.seam_local.as_ref().map(|sl| SpliceDualfit {
        pre_seam_r: sl.pre_seam_r,
        post_seam_r: sl.post_seam_r,
        gap_frames,
        bridge_frames: gap_frames as i64 + sl.trim_frames,
        trim_frames: sl.trim_frames,
        gate_pass: sl.gate_pass,
        post_seam_global_r: sl.post_seam_global_r,
        pre_seam_prom: sl.pre_seam_prom,
        post_seam_prom: sl.post_seam_prom,
        pre_seam_z: sl.pre_seam_z,
        post_seam_z: sl.post_seam_z,
    });
    // F14: same predicate the from-decode dump applies, over the projection's own brackets (real when
    // characterize supplied them, else `synth_brackets`' reconstruction — see `brackets_dual_fit_eligible`
    // for why the synthesized shape still answers the question correctly).
    let dual_fit_rescue = dual_fit_rescue_flag(&DualFitRescueInput {
        patched: tier == "patch",
        brackets: &brackets,
        splice_dualfit: splice_dualfit.as_ref(),
        donor_aligned: tags.donor_aligned.as_ref(),
        donor_nominal: tags.donor_nominal.as_ref(),
    });

    GapFingerprint {
        index: spec.gap_index,
        tier: DetailTier::Full,
        sample_rate,
        channels,
        geometry: GapGeometry {
            a_start_secs: spec.a_start_secs,
            a_end_secs: spec.a_end_secs,
            a_refined_start_secs: refined_start_secs,
            a_refined_end_secs: refined_end_secs,
            duration_secs: (refined_end_secs - refined_start_secs).max(0.0),
            b_mapped_start_secs: Some(refined_start_secs + spec.gap_offset_secs),
            b_mapped_end_secs: Some(refined_end_secs + spec.gap_offset_secs),
            fill_offset_secs: Some(spec.gap_offset_secs),
        },
        levels: projected_level_profile(tags.levels.as_ref()),
        silence: None,
        contour: None,
        anchors: None,
        brackets,
        structure,
        seams,
        lag_editorial: x.lag_editorial,
        lag_decision,
        residual,
        seam_probe: x.seam_probe,
        donor_interior: tags.donor_aligned,
        donor_interior_nominal: tags.donor_nominal,
        b_levels: x.b_levels,
        splice,
        wide_envelope: x.wide_envelope,
        splice_dualfit,
        outcome: Some(GateOutcome {
            plan_kind: "fillable".into(),
            dual_fit_rescue,
            tier,
            seam_shape: None,
            fit_path: None,
            signature_mode: None,
            skip_reason,
        }),
        // Equivalence is a from-decode-loop overlay (not a spec projection); the projection leaves it None.
        equivalence_diagnostic: None,
        equivalence_production: None,
    }
}

/// One mono `LagSummary` from the stored registration scalars (empty when the shoulder wasn't measured).
fn projected_lag_entry(
    peak_r: Option<f64>,
    frac_lag_ms: Option<f64>,
    peak_z: Option<f64>,
    prominence: Option<f64>,
) -> Vec<LagSummary> {
    match peak_r {
        Some(pr) => vec![LagSummary {
            window_ms: 0,
            max_lag_ms: 0,
            channel: LagChannel::Mono,
            lag0_r: pr,
            peak_r: pr,
            second_peak_r: None,
            peak_z,
            prominence,
            top2_spacing_ms: None,
            peak_lag_samples: 0,
            frac_lag_samples: 0.0,
            frac_lag_ms: frac_lag_ms.unwrap_or(0.0),
            edge_pinned: None,
            verdict: LagVerdict::TimingOffset,
        }],
        None => Vec::new(),
    }
}

/// Synthesize a bracket list that round-trips the stored gate summary through the corpus reader's derivations
/// (`brackets_total = len`, `brackets_passing = count(no failure_stage)`, `best_bracket_seam = max min-seam`,
/// `closest_failure_stage = failing bracket with the highest min-seam`). Not the original per-bracket detail —
/// only enough structure to reproduce those four reads. Requires a closest stage whenever a bracket fails.
///
/// **Limitation (`best = None`):** when no bracket reached seam scoring (all failed pre-seam ⇒ every synthetic
/// seam is `None`), the reader's `closest_failure_stage` is an arbitrary tie-break over equal (`NEG_INFINITY`)
/// min-seams — it may report a filler stage rather than the stored one. `closest_failure_stage`/
/// `best_bracket_seam` are **not** decision axes (`golden_baseline` omits them), so this does not affect the 8f
/// differential or C4; a diagnostics consumer that reads them from a projected corpus should carry the real
/// `Vec<BracketInfo>` (8g full-fidelity) rather than rely on the synthesis in this edge.
fn synth_brackets(
    total: usize,
    passing: usize,
    best: Option<f64>,
    closest: Option<&str>,
) -> Vec<BracketInfo> {
    let closest_stage = closest.and_then(failure_stage_from_tag);
    let failing = total.saturating_sub(passing);
    debug_assert!(
        failing == 0 || closest_stage.is_some(),
        "a failing bracket needs a closest_failure_stage to round-trip"
    );
    let mk = |score: Option<f64>, failure_stage: Option<FailureStage>| {
        let structure_floor = matches!(failure_stage, Some(FailureStage::StructureFloor));
        BracketInfo {
            pre_time_secs: 0.0,
            post_time_secs: 0.0,
            span_secs: 0.0,
            move_frames: 0,
            structure_pre: if structure_floor { score } else { None },
            structure_post: if structure_floor { score } else { None },
            // Projection is a synthetic reconstruction from roll-up counts — it has no placement.
            // Structure-floor failures carry scores on `structure_*`, not overloaded onto `seam_*`.
            seam_pre: if structure_floor { None } else { score },
            seam_post: if structure_floor { None } else { score },
            start_frame: None,
            fill_frames: None,
            failure_stage,
            residual_margin_db: None,
        }
    };
    (0..total)
        .map(|i| {
            if i < passing {
                // First passing bracket carries `best` so the reader's max-min derives it; rest carry None.
                mk(if i == 0 { best } else { None }, None)
            } else if i == passing {
                // First failing bracket is the "closest": a seam so the reader selects it, plus the stage.
                // When there are no passing brackets it also carries `best` (the reader's max sees it here).
                let seam = if passing == 0 {
                    best
                } else {
                    best.map(|b| b - 0.01)
                };
                mk(seam, closest_stage)
            } else {
                mk(None, Some(FailureStage::StructureAlign))
            }
        })
        .collect()
}

/// Corpus-reader `failure_stage` tag → [`FailureStage`] (serde snake_case, mirrors [`FailureStage`]'s repr).
fn failure_stage_from_tag(tag: &str) -> Option<FailureStage> {
    match tag {
        "structure_align" => Some(FailureStage::StructureAlign),
        "structure_floor" => Some(FailureStage::StructureFloor),
        "waveform_floor" => Some(FailureStage::WaveformFloor),
        "residual" => Some(FailureStage::Residual),
        _ => None,
    }
}

/// A minimal [`LevelProfile`] carrying only the summary floors the corpus reader consumes (`gap_floor_db`,
/// `noise_floor_db`); the RMS envelope is omitted (`None` / empty), not written as `0` / `−120`.
/// `None` tags ⇒ silence-floored placeholder floors.
fn projected_level_profile(l: Option<&LevelTags>) -> LevelProfile {
    let (gap_floor_db, noise_floor_db) = match l {
        Some(lt) => (lt.a_gap_floor_db as f32, lt.a_noise_floor_db as f32),
        None => (SILENCE_FLOOR_DB, SILENCE_FLOOR_DB),
    };
    LevelProfile {
        bin_ms: None,
        profile_db: Vec::new(),
        floor_db: None,
        speech_peak_db: None,
        noise_floor_db,
        gap_floor_db,
    }
}

// ---------------------------------------------------------------------------------------------
// Inverse: GapFingerprint → GapRepairTags / GapRepairSpec (Fingerprint-unification 8f)
// ---------------------------------------------------------------------------------------------
//
// The 8f overlay populates the full D/R payload for the export path. It is validated by an in-process
// differential (harness `gap_repair_spec_diff`): extract tags from an oracle-produced `GapFingerprint`, project
// them back, and assert the corpus reader's decision axes (`golden_baseline`) are unchanged. Reads only the
// decision/repair fields the reader consumes — the same set `spec_to_fingerprint_summary` re-emits — so the
// round-trip is identity on `GoldenRecord`. `tags_from_fingerprint` mirrors `gap_fingerprint_corpus::gap_row`
// (lag_decision-preferred registration, per-side donor, brackets → counts/best/closest).

fn mono_lag(v: &[LagSummary]) -> Option<&LagSummary> {
    v.iter()
        .find(|e| e.channel == LagChannel::Mono)
        .or_else(|| v.first())
}

/// Extract the D/R payload (`GapRepairTags`) an oracle-produced [`GapFingerprint`] carries — the inverse of
/// [`spec_to_fingerprint_summary`]'s tag mapping, mirroring `gap_row`'s reads so the projection round-trips the
/// `golden_baseline` axes. Registration prefers `lag_decision` (falls back to `lag_editorial`), matching
/// the reader.
/// Shared core of [`tags_from_fingerprint`] and [`tags_from_measurements`] — build the D/R tag payload from the
/// individual overlay fields, so the oracle-fingerprint path (8f) and the from-decode path (8g.3b) read specs
/// through ONE mapping. `structure`/`seams` are the summary throat scores (`None` under `skip_baseline`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn tags_from_fields(
    lag_decision: Option<&LagFingerprint>,
    diag_lag: Option<&LagFingerprint>,
    splice: Option<&SpliceSummary>,
    splice_dualfit: Option<SpliceDualfit>,
    brackets: &[BracketInfo],
    structure: Option<&StructureScores>,
    seams: Option<&SeamScores>,
    residual: Option<ResidualInfo>,
    donor_interior: Option<DonorInterior>,
    donor_interior_nominal: Option<DonorInterior>,
    levels: Option<crate::domain::gap_repair_spec::LevelTags>,
) -> crate::domain::gap_repair_spec::GapRepairTags {
    use crate::domain::gap_repair_spec::{GateTags, RegistrationTags, SeamLocalTags};

    let lag = lag_decision.or(diag_lag);
    let pre = lag.and_then(|l| mono_lag(&l.pre_anchor));
    let post = lag.and_then(|l| mono_lag(&l.post_anchor));

    let pre_peak_r = splice
        .map(|s| s.pre_peak_r)
        .or_else(|| pre.map(|p| p.peak_r));
    let post_peak_r = splice
        .map(|s| s.post_peak_r)
        .or_else(|| post.map(|p| p.peak_r));
    let pre_frac_lag_ms = pre.map(|p| p.frac_lag_ms);
    let post_frac_lag_ms = post.map(|p| p.frac_lag_ms);
    let step_ms = splice
        .map(|s| s.step_ms)
        .or(match (post_frac_lag_ms, pre_frac_lag_ms) {
            (Some(a), Some(b)) => Some(a - b),
            _ => None,
        });
    let registration = RegistrationTags {
        pre_peak_r,
        post_peak_r,
        pre_frac_lag_ms,
        post_frac_lag_ms,
        pre_peak_z: splice
            .and_then(|s| s.pre_peak_z)
            .or_else(|| pre.and_then(|p| p.peak_z)),
        post_peak_z: splice
            .and_then(|s| s.post_peak_z)
            .or_else(|| post.and_then(|p| p.peak_z)),
        pre_prominence: pre.and_then(|p| p.prominence),
        post_prominence: post.and_then(|p| p.prominence),
        step_ms,
        edge_pinned: splice.and_then(|s| s.edge_pinned),
    };

    let seam_local = splice_dualfit.map(|d| SeamLocalTags {
        pre_seam_r: d.pre_seam_r,
        post_seam_r: d.post_seam_r,
        post_seam_global_r: d.post_seam_global_r,
        trim_frames: d.trim_frames,
        gate_pass: d.gate_pass,
        pre_lag: 0, // not read by the reader; the spec's lags live on the SilenceSplice strategy
        post_lag: 0,
        pre_seam_prom: d.pre_seam_prom,
        post_seam_prom: d.post_seam_prom,
        pre_seam_z: d.pre_seam_z,
        post_seam_z: d.post_seam_z,
    });

    let min_seam = |b: &BracketInfo| match (b.seam_pre, b.seam_post) {
        (Some(a), Some(c)) => Some(a.min(c)),
        _ => None,
    };
    let best_bracket_seam = brackets
        .iter()
        .filter_map(min_seam)
        .fold(None, |acc, v| Some(acc.map_or(v, |a: f64| a.max(v))));
    let closest_failure_stage =
        closest_bracket_failure_stage(brackets).map(|s| s.as_str().to_string());
    let residual = residual.map(|r| crate::domain::policies::SeamResidualVerdict {
        chosen_pre_db: r.chosen_pre_db.unwrap_or(f64::NAN),
        chosen_post_db: r.chosen_post_db.unwrap_or(f64::NAN),
        floor_pre_db: r.floor_pre_db.unwrap_or(f64::NAN),
        floor_post_db: r.floor_post_db.unwrap_or(f64::NAN),
        // Read back from the dump now that `ResidualInfo` carries it. `None` here means the dump
        // predates the field (or genuinely found no reference window) — not "we didn't bother",
        // which is what the old unconditional `SeamFloorSource::None` asserted on every gap.
        floor_source_pre: r
            .floor_source_pre
            .unwrap_or(crate::domain::policies::SeamFloorSource::None),
        floor_source_post: r
            .floor_source_post
            .unwrap_or(crate::domain::policies::SeamFloorSource::None),
        informative: r.informative,
        // Read back from the dump, never recomputed — see the write mapping.
        uninformative_pre: r.uninformative_pre,
        uninformative_post: r.uninformative_post,
        // `unwrap_or(0)` only for dumps written before these fields existed, which reproduces the old
        // structurally-`false` `beyond_lag_reach()`; a current dump replays production's own reach.
        placement_slide_frames: r.placement_slide_frames.unwrap_or(0),
        max_lag_frames: r.max_lag_frames.unwrap_or(0),
    });
    let gate = GateTags {
        brackets_total: brackets.len(),
        brackets_passing: brackets
            .iter()
            .filter(|b| b.failure_stage.is_none())
            .count(),
        closest_failure_stage,
        structure_min: structure.map(|s| s.baseline_pre.min(s.baseline_post)),
        seam_min: seams.map(|s| s.baseline_pre.min(s.baseline_post)),
        best_bracket_seam,
        residual,
    };

    crate::domain::gap_repair_spec::GapRepairTags {
        registration,
        seam_local,
        donor_nominal: donor_interior_nominal,
        donor_aligned: donor_interior,
        gate,
        levels,
    }
}

/// Extract the D/R tag payload from an oracle [`GapFingerprint`] (8f). Thin wrapper over [`tags_from_fields`].
pub fn tags_from_fingerprint(fp: &GapFingerprint) -> crate::domain::gap_repair_spec::GapRepairTags {
    tags_from_fields(
        fp.lag_decision.as_ref(),
        fp.lag_editorial.as_ref(),
        fp.splice.as_ref(),
        fp.splice_dualfit,
        &fp.brackets,
        fp.structure.as_ref(),
        fp.seams.as_ref(),
        fp.residual,
        fp.donor_interior,
        fp.donor_interior_nominal,
        Some(crate::domain::gap_repair_spec::LevelTags {
            a_gap_floor_db: f64::from(fp.levels.gap_floor_db),
            a_noise_floor_db: f64::from(fp.levels.noise_floor_db),
        }),
    )
}

/// Rebuild a decision-equivalent [`GapRepairSpec`] from an oracle [`GapFingerprint`] (8f differential). The
/// verdict carries only the `patch`/`skip` distinction the reader's `tier` axis needs (a placeholder strategy
/// / reason — cell and skip-reason strings are not read by `golden_baseline`); the D/R payload comes from
/// [`tags_from_fingerprint`].
pub fn fingerprint_to_spec(fp: &GapFingerprint) -> crate::domain::gap_repair_spec::GapRepairSpec {
    use crate::domain::gap_fill_fit::FillConfidence;
    use crate::domain::gap_repair_spec::{
        BExtractWindow, GapRepairSpec, GapRepairStrategy, GapRepairVerdict,
    };
    use crate::domain::patch_result::GapPatchSkipReason;
    use crate::domain::policies::RefinedGapFrames;

    let is_skip = fp
        .outcome
        .as_ref()
        .map(|o| o.tier == "skip")
        .unwrap_or(false);
    let verdict = if is_skip {
        // Placeholder for tier/cell only — dump skip_reason is FailureStage, not this enum.
        GapRepairVerdict::skip(GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.0,
            post_correlation: 0.0,
            min_correlation: 0.0,
            best_attempt: None,
        })
    } else {
        GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice {
            fill: Vec::new(),
            pre_seam_r: 0.0,
            post_seam_r: 0.0,
            pre_lag: 0,
            post_lag: 0,
            trim_frames: 0,
            residual: None,
            confidence: FillConfidence::High,
        })
    };

    GapRepairSpec {
        gap_index: fp.index,
        a_start_secs: fp.geometry.a_start_secs,
        a_end_secs: fp.geometry.a_end_secs,
        gap_offset_secs: fp.geometry.fill_offset_secs.unwrap_or(0.0),
        refined: RefinedGapFrames {
            start_frame: (fp.geometry.a_refined_start_secs * f64::from(fp.sample_rate)).round()
                as usize,
            end_frame: (fp.geometry.a_refined_end_secs * f64::from(fp.sample_rate)).round()
                as usize,
        },
        b_extract: BExtractWindow {
            start_frame: 0,
            end_frame: 0,
            b_mapped_start_frame: 0,
        },
        crossfade_secs: 0.0,
        verdict,
        tags_ctx: tags_from_fingerprint(fp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap_repair_spec::{
        BExtractWindow, GapRepairCell, GapRepairSpec, GapRepairTags, GapRepairVerdict, GateTags,
        SeamLocalTags,
    };
    use crate::domain::patch_result::GapPatchSkipReason;
    use crate::domain::policies::RefinedGapFrames;

    fn replayed_residual(r: ResidualInfo) -> crate::domain::policies::SeamResidualVerdict {
        tags_from_fields(
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            Some(r),
            None,
            None,
            None,
        )
        .gate
        .residual
        .expect("residual survives the replay mapping")
    }

    /// A dump whose production verdict abstained (placement slid past the lag radius) must replay as
    /// beyond-reach, and its per-side reason must be the stored one.
    ///
    /// Before the reach was carried, replay rebuilt every verdict with `0`/`0`, which makes
    /// `beyond_lag_reach()` structurally `false` — so a dump could never show the gate's own abstention,
    /// and the replayed band disagreed with production on exactly those gaps.
    #[test]
    fn replayed_residual_carries_the_reach_and_the_stored_reason() {
        let beyond = replayed_residual(ResidualInfo {
            chosen_pre_db: Some(-42.0),
            chosen_post_db: Some(-41.0),
            floor_pre_db: Some(-40.0),
            floor_post_db: Some(-40.0),
            floor_source_pre: Some(crate::domain::policies::SeamFloorSource::Border),
            floor_source_post: Some(crate::domain::policies::SeamFloorSource::Border),
            informative: true,
            uninformative_pre: None,
            uninformative_post: None,
            placement_slide_frames: Some(9_000),
            max_lag_frames: Some(4_410),
        });
        assert!(beyond.beyond_lag_reach());
        assert_eq!(
            beyond.uninformative_reason(),
            Some(crate::domain::policies::ResidualUninformative::BeyondLagReach)
        );

        let measured = replayed_residual(ResidualInfo {
            chosen_pre_db: Some(-42.0),
            chosen_post_db: Some(-30.0),
            floor_pre_db: Some(-40.0),
            floor_post_db: Some(-10.0),
            floor_source_pre: Some(crate::domain::policies::SeamFloorSource::Border),
            floor_source_post: Some(crate::domain::policies::SeamFloorSource::Border),
            informative: false,
            uninformative_pre: None,
            uninformative_post: Some(
                crate::domain::policies::ResidualUninformative::FloorAboveOkDb,
            ),
            placement_slide_frames: Some(10),
            max_lag_frames: Some(4_410),
        });
        assert!(!measured.beyond_lag_reach());
        // Stored, not recomputed: the reader has no access to the writer's `residual_floor_ok_db`.
        assert_eq!(
            measured.uninformative_reason(),
            Some(crate::domain::policies::ResidualUninformative::FloorAboveOkDb)
        );
    }

    /// A dump written before the reach existed keeps today's behaviour rather than inventing one.
    #[test]
    fn replayed_residual_from_a_pre_field_dump_lands_on_zero() {
        let old = replayed_residual(
            serde_json::from_str(
                r#"{"chosen_pre_db":-42.0,"chosen_post_db":-41.0,"floor_pre_db":-40.0,"floor_post_db":-40.0,"informative":true}"#,
            )
            .expect("parse"),
        );
        assert_eq!(old.placement_slide_frames, 0);
        assert_eq!(old.max_lag_frames, 0);
        assert!(!old.beyond_lag_reach());
        assert_eq!(old.uninformative_reason(), None);
    }

    /// 8e projection: a bracket-exhausted **silence-splice skip** (dual-fit declined) projects to a
    /// fingerprint whose corpus-read fields equal the spec's stored tags — no re-measurement (A7). Exercises
    /// the seam_local → splice_dualfit, donor, gate-count → bracket-synthesis, and outcome mappings.
    #[test]
    fn spec_to_fingerprint_projects_silence_splice_skip_axes() {
        let tags = GapRepairTags {
            seam_local: Some(SeamLocalTags {
                pre_seam_r: 0.97,
                post_seam_r: 0.95,
                post_seam_global_r: 0.40,
                trim_frames: 480,
                gate_pass: true,
                pre_lag: 12,
                post_lag: -8,
                pre_seam_prom: None,
                post_seam_prom: None,
                pre_seam_z: None,
                post_seam_z: None,
            }),
            donor_aligned: Some(crate::domain::donor::DonorInterior {
                rms_db: -22.0,
                silence_fraction: 0.03,
                longest_silence_ms: 0.0,
                continuous: true,
                basis: None,
            }),
            donor_nominal: Some(crate::domain::donor::DonorInterior {
                rms_db: -25.0,
                silence_fraction: 0.10,
                longest_silence_ms: 0.0,
                continuous: true,
                basis: None,
            }),
            gate: GateTags {
                brackets_total: 4,
                brackets_passing: 0,
                closest_failure_stage: Some("waveform_floor".into()),
                best_bracket_seam: Some(0.6),
                ..GateTags::default()
            },
            ..GapRepairTags::default()
        };
        let spec = GapRepairSpec {
            gap_index: 3,
            a_start_secs: 10.0,
            a_end_secs: 10.5,
            gap_offset_secs: 0.25,
            refined: RefinedGapFrames {
                start_frame: 480_000,
                end_frame: 504_000,
            },
            b_extract: BExtractWindow {
                start_frame: 0,
                end_frame: 0,
                b_mapped_start_frame: 0,
            },
            crossfade_secs: 0.01,
            verdict: GapRepairVerdict::Skip {
                cell: GapRepairCell::SilenceSplice,
                reason: GapPatchSkipReason::CorrelationBelowThreshold {
                    pre_correlation: 0.97,
                    post_correlation: 0.95,
                    min_correlation: 0.5,
                    best_attempt: None,
                },
            },
            tags_ctx: tags,
        };

        let fp = spec_to_fingerprint_summary(&spec, 48_000, 2, None, MeasuredDetail::default());

        // outcome: skip tier; skip_reason is the closest FailureStage (here from gate tags → synth).
        let o = fp.outcome.as_ref().unwrap();
        assert_eq!(o.tier, "skip");
        assert_eq!(o.skip_reason.as_deref(), Some("waveform_floor"));

        // splice_dualfit — single-source copies of seam_local (A7), gate_pass + step-real inputs preserved.
        let df = fp.splice_dualfit.expect("splice_dualfit projected");
        assert_eq!(df.pre_seam_r, 0.97);
        assert_eq!(df.post_seam_r, 0.95);
        assert_eq!(df.post_seam_global_r, 0.40);
        assert!(df.gate_pass);
        assert_eq!(df.gap_frames, 24_000);
        assert_eq!(df.trim_frames, 480);

        // donor blocks round-trip whole.
        assert_eq!(fp.donor_interior.unwrap().silence_fraction, 0.03);
        assert_eq!(fp.donor_interior_nominal.unwrap().silence_fraction, 0.10);

        // brackets: synthesized to read back total=4, passing=0 (bracket-exhausted).
        assert_eq!(fp.brackets.len(), 4);
        assert_eq!(
            fp.brackets
                .iter()
                .filter(|b| b.failure_stage.is_none())
                .count(),
            0
        );
    }

    /// Real brackets win over a production-shaped placeholder verdict: dump `skip_reason` is the
    /// closest `FailureStage`, not `correlation_below_threshold`.
    #[test]
    fn skip_reason_follows_closest_measured_failure_stage() {
        let tags = GapRepairTags {
            gate: GateTags {
                brackets_total: 1,
                brackets_passing: 0,
                closest_failure_stage: Some("waveform_floor".into()),
                ..GateTags::default()
            },
            ..GapRepairTags::default()
        };
        let spec = GapRepairSpec {
            gap_index: 0,
            a_start_secs: 1.0,
            a_end_secs: 1.5,
            gap_offset_secs: 0.0,
            refined: RefinedGapFrames {
                start_frame: 48_000,
                end_frame: 72_000,
            },
            b_extract: BExtractWindow {
                start_frame: 0,
                end_frame: 0,
                b_mapped_start_frame: 0,
            },
            crossfade_secs: 0.01,
            verdict: GapRepairVerdict::skip(GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: 0.0,
                post_correlation: 0.0,
                min_correlation: 0.0,
                best_attempt: None,
            }),
            tags_ctx: tags,
        };
        let brackets = vec![
            BracketInfo {
                pre_time_secs: 0.0,
                post_time_secs: 0.5,
                span_secs: 0.5,
                move_frames: 0,
                structure_pre: None,
                structure_post: None,
                seam_pre: Some(0.4),
                seam_post: Some(0.5),
                start_frame: None,
                fill_frames: None,
                failure_stage: Some(FailureStage::WaveformFloor),
                residual_margin_db: None,
            },
            BracketInfo {
                pre_time_secs: 0.0,
                post_time_secs: 0.5,
                span_secs: 0.5,
                move_frames: 10,
                structure_pre: None,
                structure_post: None,
                seam_pre: Some(0.55),
                seam_post: Some(0.6),
                start_frame: None,
                fill_frames: None,
                failure_stage: Some(FailureStage::Residual),
                residual_margin_db: Some(6.0),
            },
        ];
        let fp = spec_to_fingerprint_summary(
            &spec,
            48_000,
            2,
            None,
            MeasuredDetail {
                brackets: Some(brackets),
                lag_decision: None,
            },
        );
        assert_eq!(fp.outcome.as_ref().unwrap().tier, "skip");
        assert_eq!(
            fp.outcome.as_ref().unwrap().skip_reason.as_deref(),
            Some("residual"),
            "higher seam progress on residual bracket wins"
        );
        assert_eq!(
            closest_bracket_failure_stage(&fp.brackets),
            Some(FailureStage::Residual)
        );
    }
}
