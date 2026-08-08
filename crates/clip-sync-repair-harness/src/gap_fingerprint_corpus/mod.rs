//! Cross-corpus aggregation of gap-fingerprint output (the **P0 prevalence scan**, see
//! `docs/dev/archive/TEMP-w5-timing-offset-rescue-plan.md` §5 P0).
//!
//! `--gap-fingerprints DIR` writes one `corpus.json` per A/B pair (all that pair's gaps) plus per-gap
//! library files. Point this at the parent of several such runs (e.g. `gap-files/`, holding `1/`..`6/`)
//! and it tallies, **across every pair**, the lag verdicts and gate outcomes — the numbers P0 needs:
//! how many gaps are `timing_offset` (a recoverable seam the gate skipped), split **constant** vs
//! **drift**, vs genuinely `decorrelated` skips.
//!
//! **`--check` mode** ([`check_dirs`]) asserts dump writer invariants (placement ↔ gate, outcome ↔
//! brackets, library packaging) — a post-dump health check, not a prevalence scan.
//!
//! Parses a **minimal** projection of the schema (ids, index, geometry duration, lag, outcome) so it is
//! robust to unrelated `GapCorpus` schema drift. Prefers each pair dir's `corpus.json` (authoritative,
//! all gaps, no per-gap-file accumulation); falls back to globbing per-gap `*.json` and de-duping by
//! gap index when `corpus.json` is absent.

mod analysis;
mod check;
mod report;
mod schema;

pub use analysis::{
    analyze_dirs, curated_gap_cell_projected_rows, curated_gap_cell_rows, drift_eps_from_env,
    gap_rows_from_corpus_json, tail_secs_from_env,
};
pub use check::{
    check_dirs, fill_slack_from_env, HealthCheckOptions, HealthCheckReport, HealthIssue,
    IssueSeverity, DEFAULT_FILL_SLACK_SECS,
};
pub use report::CorpusReport;
pub use schema::{GapKind, GapRow, SeamDiag, SkewClass, SpliceDiag};

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn write_corpus(dir: &Path, a_id: &str, b_id: &str, gaps_json: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let json = format!(
            r#"{{"source":{{"a_source":{{"id":"{a_id}","sample_rate":48000,"channels":2,"duration_secs":60.0}},
            "b_source":{{"id":"{b_id}","sample_rate":48000,"channels":2,"duration_secs":60.0}},
            "scan_recipe":{{"silence_peak_fraction":0.01}},"gap_count":1}},"gaps":{gaps_json}}}"#
        );
        std::fs::write(dir.join("corpus.json"), json).unwrap();
    }

    fn gap(index: usize, verdict: &str, tier: &str, pre_ms: f64, post_ms: f64) -> String {
        format!(
            r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{{"a_start_secs":0,"a_end_secs":0,"a_refined_start_secs":0,"a_refined_end_secs":0,"duration_secs":1.8}},
            "levels":{{"bin_ms":50,"profile_db":[],"floor_db":-120,"speech_peak_db":-40,"noise_floor_db":-50,"gap_floor_db":-90}},
            "silence":{{"collar_rms_peak_ratio":0.1,"collar_above_relative_floor":true,"silence_peak_fraction":0.01}},
            "contour":{{"has_anchor_seam_contour":true,"pre_flatness":0,"post_flatness":0}},
            "anchors":{{"pre":[],"post":[]}},
            "lag":{{"pre_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"second_peak_r":0.20,"peak_lag_samples":-100,"frac_lag_samples":-100,"frac_lag_ms":{pre_ms},"verdict":"{verdict}"}}],
                    "post_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"second_peak_r":0.20,"peak_lag_samples":-50,"frac_lag_samples":-50,"frac_lag_ms":{post_ms},"verdict":"{verdict}"}}]}},
            "lag_decision":{{"pre_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"second_peak_r":0.20,"peak_lag_samples":-100,"frac_lag_samples":-100,"frac_lag_ms":{pre_ms},"verdict":"{verdict}"}}],
                    "post_anchor":[{{"window_ms":250,"max_lag_ms":200,"channel":"mono","lag0_r":-0.1,"peak_r":0.95,"second_peak_r":0.20,"peak_lag_samples":-50,"frac_lag_samples":-50,"frac_lag_ms":{post_ms},"verdict":"{verdict}"}}]}},
            "residual":{{"chosen_pre_db":-42.0,"chosen_post_db":-41.0,"floor_pre_db":-40.0,"floor_post_db":-39.0,"informative":true}},
            "outcome":{{"plan_kind":"fillable","tier":"{tier}","seam_shape":""}}}}"#
        )
    }

    /// A gap with **no** lag block, at a given duration — exercises the `NoLag` / `Tail` kinds.
    fn gap_nolag(index: usize, duration_secs: f64, tier: &str) -> String {
        format!(
            r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{{"a_start_secs":0,"a_end_secs":0,"a_refined_start_secs":0,"a_refined_end_secs":0,"duration_secs":{duration_secs}}},
            "levels":{{"bin_ms":50,"profile_db":[],"floor_db":-120,"speech_peak_db":-40,"noise_floor_db":-50,"gap_floor_db":-90}},
            "silence":{{"collar_rms_peak_ratio":0.1,"collar_above_relative_floor":true,"silence_peak_fraction":0.01}},
            "contour":{{"has_anchor_seam_contour":true,"pre_flatness":0,"post_flatness":0}},
            "anchors":{{"pre":[],"post":[]}},
            "outcome":{{"plan_kind":"fillable","tier":"{tier}","seam_shape":""}}}}"#
        )
    }

    #[test]
    fn aggregates_verdicts_and_skew_across_pairs() {
        let root = tempfile::tempdir().unwrap();
        // pair 1: drifting timing_offset skip (−16 vs −8), decorrelated skip, and a 200 s tail.
        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!(
                "[{},{},{}]",
                gap(0, "timing_offset", "skip", -16.0, -8.0),
                gap(1, "decorrelated", "skip", 2.0, 3.0),
                gap_nolag(2, 200.0, "skip"),
            ),
        );
        // pair 2: constant timing_offset skip (−5 vs −5.2), a patched gap, and a short no-lag skip.
        write_corpus(
            &root.path().join("2"),
            "cccc",
            "dddd",
            &format!(
                "[{},{},{}]",
                gap(0, "timing_offset", "skip", -5.0, -5.2),
                gap(1, "timing_offset", "patch", -5.0, -5.0),
                gap_nolag(2, 3.0, "skip"),
            ),
        );

        let report = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0);
        assert_eq!(report.pairs.len(), 2, "two pair dirs");
        assert_eq!(report.total_gaps(), 6);

        // Region kinds: 4 matched (the lag-bearing gaps), 1 tail (200 s), 1 no-lag (3 s).
        assert_eq!(
            report.matched().len(),
            4,
            "four lag-bearing gaps are matched"
        );
        assert_eq!(
            report
                .rows
                .iter()
                .filter(|r| r.kind == GapKind::Tail)
                .count(),
            1
        );
        assert_eq!(
            report
                .rows
                .iter()
                .filter(|r| r.kind == GapKind::NoLag)
                .count(),
            1
        );

        // Addressable = matched + timing_offset + skipped (the patched and decorrelated excluded).
        let matched = report.matched();
        let addr: Vec<&&GapRow> = matched
            .iter()
            .filter(|r| r.verdict.as_deref() == Some("timing_offset") && !r.patched())
            .collect();
        assert_eq!(addr.len(), 2, "two timing_offset skips");
        assert_eq!(
            addr.iter().filter(|r| r.skew == SkewClass::Drift).count(),
            1,
            "−16/−8 is drift"
        );
        assert_eq!(
            addr.iter()
                .filter(|r| r.skew == SkewClass::Constant)
                .count(),
            1,
            "−5/−5.2 is constant"
        );

        // Uniqueness margin = peak_r − second_peak_r = 0.95 − 0.20 = 0.75 on the lag-bearing gaps.
        assert!(
            matched.iter().all(|r| r
                .uniqueness_margin
                .is_some_and(|mgn| (mgn - 0.75).abs() < 1e-6)),
            "matched gaps carry a 0.75 uniqueness margin"
        );
        // Residual headroom = worst of (−42−(−40), −41−(−39)) = −2 dB; informative ⇒ same-source.
        assert!(
            matched.iter().all(|r| {
                r.residual_headroom_db
                    .is_some_and(|h| (h - (-2.0)).abs() < 1e-6)
                    && r.residual_informative == Some(true)
            }),
            "matched gaps carry a −2 dB informative residual headroom"
        );

        // Plan kind surfaced; summary + CSV render and carry the new columns.
        assert!(report
            .rows
            .iter()
            .all(|r| r.plan_kind.as_deref() == Some("fillable")));
        // Registration decomposition: the −16/−8 drift gap → step +8, mid −12.
        let drift_gap = matched
            .iter()
            .find(|r| r.frac_lag_pre_ms == Some(-16.0))
            .expect("the −16/−8 gap");
        assert!(drift_gap
            .seam_step_ms()
            .is_some_and(|v| (v - 8.0).abs() < 1e-6));
        assert!(drift_gap
            .seam_mid_ms()
            .is_some_and(|v| (v - (-12.0)).abs() < 1e-6));

        // Silence-splice view: both lag-bearing skips have peak_r 0.95 ≥ 0.85 and margin 0.75 ≥ 0.30 ⇒
        // `splice` (both-sides-recoverable). None are one-sided-dead.
        assert!(
            addr.iter()
                .all(|r| r.splice_diag() == Some(SpliceDiag::Splice) && r.both_sides_recoverable()),
            "clean high-peak unique skips classify as recoverable splices"
        );
        let splice = report.splice_text();
        assert!(splice.contains("both-sides-recoverable"));
        assert!(splice.contains("one-sided-dead (a shoulder aligns at NO lag"));

        let summary = report.summary_text();
        assert!(summary.contains("plan_kind: fillable"));
        assert!(summary.contains("gap kind:"));
        assert!(summary.contains("uniqueness:"));
        assert!(summary.contains("residual:"));
        assert!(summary.contains("registration:"));
        let header = report.csv().lines().next().unwrap().to_string();
        assert!(header.contains("seam_mid_ms,seam_step_ms"));
        assert!(header.contains("uniqueness_margin") && header.contains("residual_headroom_db"));
        assert_eq!(report.csv().lines().count(), 7); // header + 6 gaps
    }

    #[test]
    fn csv_quotes_commas_in_pair_and_ids() {
        let root = tempfile::tempdir().unwrap();
        let pair_dir = root.path().join("pair,name");
        let gap_json = gap(0, "timing_offset", "skip", -16.0, -8.0);
        write_corpus(&pair_dir, "a,id", "b,id", &format!("[{gap_json}]"));
        let report = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0);
        let csv = report.csv();
        let mut rdr = csv::Reader::from_reader(csv.as_bytes());
        let rec = rdr.records().next().expect("row").expect("parse");
        let pair = rec.get(0).expect("pair");
        assert!(
            pair.ends_with("pair,name") || pair.contains("/pair,name"),
            "pair field should round-trip the comma: {pair:?}"
        );
        assert_eq!(rec.get(1), Some("a,id"));
        assert_eq!(rec.get(2), Some("b,id"));
        assert!(
            csv.contains("\"a,id\"") && csv.contains("\"b,id\""),
            "RFC 4180 should quote comma-bearing ids:\n{csv}"
        );
    }

    #[test]
    fn splice_diag_uses_peak_z_when_present() {
        let root = tempfile::tempdir().unwrap();
        let gap_json = r#"{"index":0,"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{"duration_secs":1.8,"a_refined_start_secs":0},
            "lag_decision":{"pre_anchor":[{"peak_r":0.95,"peak_z":8.0,"prominence":0.6,"frac_lag_ms":-10.0,"verdict":"timing_offset"}],
                    "post_anchor":[{"peak_r":0.95,"peak_z":15.0,"prominence":0.6,"frac_lag_ms":-5.0,"verdict":"timing_offset"}]},
            "outcome":{"tier":"skip"}}"#.to_string();
        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!("[{gap_json}]"),
        );
        let row = &analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0).rows[0];
        assert_eq!(row.splice_diag(), Some(SpliceDiag::AliasSuspect));
        assert!(!row.both_sides_recoverable());
    }

    #[test]
    fn program_quiet_skip_leaves_addressable_denominator() {
        // Two skips with the identical dual-fit *shape* (bracket-exhausted, both shoulders recoverable, not
        // edge-pinned); the only difference is donor occupancy at the nominal program time. D11: the one
        // where B is also silent is program-quiet — nothing to fill — and must drop out of the repair set.
        let gap = |index: usize, nominal_silence: f64| {
            format!(
                r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
                "geometry":{{"duration_secs":1.8,"a_refined_start_secs":0}},
                "lag_decision":{{"pre_anchor":[{{"peak_r":0.95,"peak_z":16.0,"prominence":0.6,"frac_lag_ms":-16.0,"verdict":"timing_offset"}}],
                        "post_anchor":[{{"peak_r":0.95,"peak_z":15.0,"prominence":0.6,"frac_lag_ms":-8.0,"verdict":"timing_offset"}}]}},
                "splice":{{"step_ms":8.0,"pre_peak_r":0.95,"post_peak_r":0.95,"pre_peak_z":16.0,"post_peak_z":15.0,"edge_pinned":false}},
                "donor_interior_nominal":{{"rms_db":-80.0,"silence_fraction":{nominal_silence},"continuous":false}},
                "brackets":[{{"failure_stage":"waveform_floor"}}],
                "outcome":{{"tier":"skip"}}}}"#
            )
        };
        let root = tempfile::tempdir().unwrap();
        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!("[{},{}]", gap(0, 0.95), gap(1, 0.02)),
        );
        let rows = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0).rows;
        let (quiet, dropout) = (&rows[0], &rows[1]);

        // Same shape — both would be dual-fit candidates on the pre-D11 predicate.
        assert!(quiet.bracket_exhausted() && quiet.both_sides_recoverable());
        assert!(dropout.bracket_exhausted() && dropout.both_sides_recoverable());

        // D11 classification: B-silent ⇒ program-quiet, out of the addressable set and not a repair target.
        assert!(
            quiet.program_quiet_skip(),
            "B silent at program time ⇒ program-quiet"
        );
        assert!(!quiet.addressable_dropout());
        assert!(
            !quiet.dualfit_candidate(),
            "program-quiet must not be a dual-fit target"
        );

        // The real dropout (B occupied) keeps its place in the denominator and stays a candidate.
        assert!(!dropout.program_quiet_skip());
        assert!(dropout.addressable_dropout());
        assert!(
            dropout.dualfit_candidate(),
            "occupied-donor dropout is a candidate"
        );
    }

    #[test]
    fn analyzer_hygiene_two_sided_metrics_and_legacy_flag() {
        let root = tempfile::tempdir().unwrap();

        // g0: shoulders DISAGREE (pre timing_offset, post decorrelated). C-harness-2: skew must be
        // NotApplicable (not Drift) — the one-sided verdict must not drive it. C-harness-1: only the pre
        // shoulder carries `peak_z`, so the two-sided robust uniqueness is `None`, not the pre value.
        let disagree = r#"{"index":0,"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{"duration_secs":1.8,"a_refined_start_secs":0},
            "lag_decision":{"pre_anchor":[{"peak_r":0.95,"peak_z":16.0,"prominence":0.6,"frac_lag_ms":-16.0,"verdict":"timing_offset"}],
                    "post_anchor":[{"peak_r":0.40,"frac_lag_ms":-8.0,"verdict":"decorrelated"}]},
            "outcome":{"tier":"skip"}}"#;

        // g1: pre-A2 fingerprint — only the diagnostic `lag` block, no decision lag. Deliberately keeps the
        // pre-2026-08-07 `lag` spelling: it is a legacy artifact, so it also exercises the serde alias.
        // C-harness-3: the
        // legacy fallback must be flagged and the summary must warn about the schema mix.
        let legacy = r#"{"index":1,"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{"duration_secs":1.8,"a_refined_start_secs":0},
            "lag":{"pre_anchor":[{"peak_r":0.95,"frac_lag_ms":-10.0,"verdict":"timing_offset"}],
                    "post_anchor":[{"peak_r":0.95,"frac_lag_ms":-9.5,"verdict":"timing_offset"}]},
            "outcome":{"tier":"skip"}}"#;

        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!("[{disagree},{legacy}]"),
        );
        let report = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0);
        let (g0, g1) = (&report.rows[0], &report.rows[1]);

        // C-harness-2 + C-harness-1.
        assert_eq!(
            g0.skew,
            SkewClass::NotApplicable,
            "disagreeing shoulders are not a timing skew"
        );
        assert_eq!(
            g0.uniqueness_z, None,
            "robust uniqueness needs both shoulders' peak_z"
        );

        // C-harness-3.
        assert!(!g0.registration_from_legacy_lag, "g0 has lag_decision");
        assert!(
            g1.registration_from_legacy_lag,
            "g1 fell back to legacy `lag`"
        );
        assert!(
            report.summary_text().contains("registration schema mix"),
            "mixed-schema corpus must warn"
        );
    }

    #[test]
    fn dualfit_target_scopes_on_gate_pass_and_donor_not_uniqueness() {
        // The A3 predicate: gate_pass ∧ step-real (post_own − post@pre ≥ margin) ∧ donor-continuous ∧
        // ¬program-quiet. Each non-target row flips exactly one condition, holding the others at pass.
        // (post_seam_r = 0.95, so step-spurious needs post_global ≳ 0.80 to make Δ < 0.15.)
        let gap = |index: usize,
                   gate_pass: bool,
                   post_global: f64,
                   cont: bool,
                   nominal_sil: f64| {
            format!(
                r#"{{"index":{index},"tier":"full","sample_rate":48000,"channels":2,
                "geometry":{{"duration_secs":1.8,"a_refined_start_secs":0}},
                "lag_decision":{{"pre_anchor":[{{"peak_r":0.95,"frac_lag_ms":-16.0,"verdict":"timing_offset"}}],
                        "post_anchor":[{{"peak_r":0.95,"frac_lag_ms":-8.0,"verdict":"timing_offset"}}]}},
                "donor_interior_aligned":{{"rms_db":-40.0,"silence_fraction":0.02,"continuous":{cont}}},
                "donor_interior_nominal":{{"rms_db":-40.0,"silence_fraction":{nominal_sil},"continuous":{cont}}},
                "splice_dualfit":{{"pre_seam_r":0.99,"post_seam_r":0.95,"trim_frames":10,"gate_pass":{gate_pass},"post_seam_global_r":{post_global}}},
                "brackets":[{{"failure_stage":"waveform_floor"}}],
                "outcome":{{"tier":"skip"}}}}"#
            )
        };
        // A gap that satisfies every dual-fit condition but ALREADY PATCHES (a passing bracket) must be
        // excluded — dual-fit never runs on patched gaps (B1/B11).
        let patched = r#"{"index":5,"tier":"full","sample_rate":48000,"channels":2,
            "geometry":{"duration_secs":1.8,"a_refined_start_secs":0},
            "lag_decision":{"pre_anchor":[{"peak_r":0.95,"frac_lag_ms":-16.0,"verdict":"timing_offset"}],
                    "post_anchor":[{"peak_r":0.95,"frac_lag_ms":-8.0,"verdict":"timing_offset"}]},
            "donor_interior_aligned":{"rms_db":-40.0,"silence_fraction":0.02,"continuous":true},
            "donor_interior_nominal":{"rms_db":-40.0,"silence_fraction":0.02,"continuous":true},
            "splice_dualfit":{"pre_seam_r":0.99,"post_seam_r":0.95,"trim_frames":10,"gate_pass":true,"post_seam_global_r":0.05},
            "brackets":[{"seam_pre":0.9,"seam_post":0.9}],
            "outcome":{"tier":"full"}}"#;
        let root = tempfile::tempdir().unwrap();
        write_corpus(
            &root.path().join("1"),
            "aaaa",
            "bbbb",
            &format!(
                "[{},{},{},{},{},{patched}]",
                gap(0, true, 0.05, true, 0.02),  // clean target
                gap(1, true, 0.90, true, 0.02), // step spurious: post@pre 0.90 vs post_own 0.95 (Δ 0.05) → not a target
                gap(2, true, 0.05, false, 0.02), // donor BROKEN (nothing to bridge) → not a target
                gap(3, true, 0.05, true, 0.95), // program-quiet (nothing to fill) → not a target
                gap(4, false, 0.05, true, 0.02), // gate FAIL → not a target
            ),
        );
        let rows = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0).rows;
        assert!(
            rows[0].dualfit_target(),
            "gate_pass + real step + continuous donor + occupied"
        );
        assert!(!rows[1].dualfit_target(), "spurious step excluded");
        assert!(!rows[2].dualfit_target(), "broken donor excluded");
        assert!(!rows[3].dualfit_target(), "program-quiet excluded");
        assert!(!rows[4].dualfit_target(), "gate fail excluded");
        assert!(
            !rows[5].dualfit_target(),
            "already-patched gap excluded (bracket-exhausted skips only)"
        );
    }
}
