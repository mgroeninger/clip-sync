//! Dump integrity / health check for `--gap-fingerprints` corpora.
//!
//! This is **not** the prevalence analyzer ([`super::analyze_dirs`]). It asserts writer invariants
//! (placement ↔ gate Ok, outcome ↔ brackets, library packaging, fill slack). Point
//! `gap-fingerprint-stats --check` at a corpus root after a bulk dump.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::analysis::{is_pair_dir, pair_dirs, pair_label};

/// Default production `fill_length_slack_secs` (+ small float margin).
pub const DEFAULT_FILL_SLACK_SECS: f64 = 5.1;

#[derive(Debug, Clone)]
pub struct HealthCheckOptions {
    /// Max |fill_frames/sr − geometry gap duration| allowed for placed brackets.
    /// Loose sanity bound only: includes anchor widening. Phase B slack use is |fill − span|.
    pub fill_slack_secs: f64,
}

impl Default for HealthCheckOptions {
    fn default() -> Self {
        Self {
            fill_slack_secs: fill_slack_from_env().unwrap_or(DEFAULT_FILL_SLACK_SECS),
        }
    }
}

/// Optional override: `GAP_FP_FILL_SLACK_SECS` (seconds).
pub fn fill_slack_from_env() -> Option<f64> {
    std::env::var("GAP_FP_FILL_SLACK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Dump is inconsistent — `--check` exits non-zero.
    Error,
    /// Incomplete / soft anomaly — printed, does not fail the check.
    Warn,
}

#[derive(Debug, Clone)]
pub struct HealthIssue {
    pub severity: IssueSeverity,
    pub pair: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct HealthCheckReport {
    pub pairs_checked: usize,
    pub gaps: usize,
    pub brackets: usize,
    pub gate_ok: usize,
    pub placed: usize,
    pub patches: usize,
    pub skips: usize,
    pub issues: Vec<HealthIssue>,
}

impl HealthCheckReport {
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .count()
    }

    pub fn warn_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warn)
            .count()
    }

    pub fn ok(&self) -> bool {
        self.error_count() == 0
    }

    pub fn summary_text(&self) -> String {
        let mut s = String::new();
        s.push_str("=== fingerprint dump health check ===\n");
        s.push_str(&format!(
            "pairs={pairs} gaps={gaps} brackets={brackets} gate_ok={ok} placed={placed} patch={patch} skip={skip}\n",
            pairs = self.pairs_checked,
            gaps = self.gaps,
            brackets = self.brackets,
            ok = self.gate_ok,
            placed = self.placed,
            patch = self.patches,
            skip = self.skips,
        ));
        s.push_str(&format!(
            "result: {status}  (errors={err} warnings={warn})\n",
            status = if self.ok() { "PASS" } else { "FAIL" },
            err = self.error_count(),
            warn = self.warn_count(),
        ));
        if !self.issues.is_empty() {
            s.push('\n');
            for issue in &self.issues {
                let tag = match issue.severity {
                    IssueSeverity::Error => "ERROR",
                    IssueSeverity::Warn => "WARN ",
                };
                s.push_str(&format!(
                    "{tag}  {pair}: {msg}\n",
                    pair = issue.pair,
                    msg = issue.message
                ));
            }
        }
        s
    }
}

/// Walk `roots` the same way as [`super::analyze_dirs`], plus warn on empty sibling dirs.
pub fn check_dirs(roots: &[PathBuf], opts: &HealthCheckOptions) -> HealthCheckReport {
    let mut report = HealthCheckReport::default();
    for root in roots {
        warn_incomplete_siblings(root, &mut report);
        for pair_dir in pair_dirs(root) {
            let label = pair_label(root, &pair_dir);
            check_pair(&label, &pair_dir, opts, &mut report);
        }
    }
    report
}

fn warn_incomplete_siblings(root: &Path, report: &mut HealthCheckReport) {
    let Ok(rd) = fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        if is_pair_dir(&p) {
            continue;
        }
        // Empty or partial dir left by a bulk harness before/during a failed pair.
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Warn,
            pair: name,
            message: format!(
                "directory has no corpus.json / per-gap library (incomplete dump?): {}",
                p.display()
            ),
        });
    }
}

fn check_pair(label: &str, dir: &Path, opts: &HealthCheckOptions, report: &mut HealthCheckReport) {
    let corpus_path = dir.join("corpus.json");
    let corpus = match read_json::<CorpusFile>(&corpus_path) {
        Some(c) => c,
        None => match read_merged_library(dir) {
            Some(c) => c,
            None => {
                report.issues.push(HealthIssue {
                    severity: IssueSeverity::Error,
                    pair: label.to_string(),
                    message: format!("unreadable corpus in {}", dir.display()),
                });
                return;
            }
        },
    };
    report.pairs_checked += 1;
    check_corpus(label, dir, &corpus, opts, report);
}

fn check_corpus(
    label: &str,
    dir: &Path,
    corpus: &CorpusFile,
    opts: &HealthCheckOptions,
    report: &mut HealthCheckReport,
) {
    report.gaps += corpus.gaps.len();

    let lib_files = list_library_files(dir);
    if lib_files.len() != corpus.gaps.len() {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Error,
            pair: label.to_string(),
            message: format!(
                "library JSON count {} != corpus gaps {}",
                lib_files.len(),
                corpus.gaps.len()
            ),
        });
    }

    if let Some(manifest) = read_json::<ManifestFile>(&dir.join("manifest.json")) {
        if manifest.gap_count != corpus.gaps.len() || manifest.entries.len() != corpus.gaps.len() {
            report.issues.push(HealthIssue {
                severity: IssueSeverity::Error,
                pair: label.to_string(),
                message: format!(
                    "manifest gap_count/entries ({}/{}) != corpus gaps {}",
                    manifest.gap_count,
                    manifest.entries.len(),
                    corpus.gaps.len()
                ),
            });
        }
        for entry in &manifest.entries {
            let path = dir.join(&entry.file);
            if !path.is_file() {
                report.issues.push(HealthIssue {
                    severity: IssueSeverity::Error,
                    pair: label.to_string(),
                    message: format!("manifest lists missing file {}", entry.file),
                });
                continue;
            }
            if let Some(single) = read_json::<CorpusFile>(&path) {
                if single.gaps.len() != 1 {
                    report.issues.push(HealthIssue {
                        severity: IssueSeverity::Error,
                        pair: label.to_string(),
                        message: format!(
                            "{}: expected 1 gap, got {}",
                            entry.file,
                            single.gaps.len()
                        ),
                    });
                } else if single.gaps[0].index != entry.index {
                    report.issues.push(HealthIssue {
                        severity: IssueSeverity::Error,
                        pair: label.to_string(),
                        message: format!(
                            "{}: gap index {} != manifest index {}",
                            entry.file, single.gaps[0].index, entry.index
                        ),
                    });
                }
            } else {
                report.issues.push(HealthIssue {
                    severity: IssueSeverity::Error,
                    pair: label.to_string(),
                    message: format!("failed to parse library file {}", entry.file),
                });
            }
        }
    } else {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Warn,
            pair: label.to_string(),
            message: "manifest.json missing or unreadable".into(),
        });
    }

    // Source provenance (Track A): a corpus that cannot say what media it measured is still readable,
    // so this is a Warn, not an Error — the same class as "manifest.json missing" above. Every corpus
    // dumped before Track A trips it; re-dumping the pair is the fix.
    let missing_provenance = match corpus.source.as_ref() {
        None => true,
        Some(src) => !matches!(
            (src.a_source.as_ref(), src.b_source.as_ref()),
            (Some(a), Some(b)) if a.has_provenance() && b.has_provenance()
        ),
    };
    if missing_provenance {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Warn,
            pair: label.to_string(),
            message: concat!(
                "source.{a,b}_source carry no codec / native_sample_rate: ",
                "corpus predates source provenance, so codec-conditioned reads cannot be stratified"
            )
            .into(),
        });
    }

    // Channel-layout refuse / legacy mismatch: surface so empty dumps and silently-wrong pre-gate
    // dumps are not mistaken for "pair had no gaps".
    if let Some(src) = corpus.source.as_ref() {
        if let Some(reason) = src.incomparable.as_deref() {
            report.issues.push(HealthIssue {
                severity: IssueSeverity::Warn,
                pair: label.to_string(),
                message: format!(
                    "source.incomparable={reason}: pairwise fingerprint characterize was refused \
                     (gaps empty); do not treat this pair as a measured null"
                ),
            });
        } else if let (Some(a), Some(b)) = (src.a_source.as_ref(), src.b_source.as_ref()) {
            if let (Some(ac), Some(bc)) = (a.native_channels, b.native_channels) {
                if ac != bc && !corpus.gaps.is_empty() {
                    report.issues.push(HealthIssue {
                        severity: IssueSeverity::Warn,
                        pair: label.to_string(),
                        message: format!(
                            "native_channels disagree (A {ac} / B {bc}) but gaps are present: \
                             corpus predates the channel-layout refuse gate; B may have been \
                             indexed at A's channel count — re-dump the pair"
                        ),
                    });
                }
            }
        }
    }

    for gap in &corpus.gaps {
        check_gap(label, gap, opts, report);
    }
    check_not_measured(label, corpus, report);
    check_equivalence_production_coverage(label, corpus, report);
    check_donor_within_b(label, corpus, report);

    // Filename ↔ outcome when the library used patch/skip as the verdict suffix.
    for path in &lib_files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(single) = read_json::<CorpusFile>(path) else {
            continue;
        };
        let Some(gap) = single.gaps.first() else {
            continue;
        };
        let Some(tier) = gap.outcome.as_ref().map(|o| o.tier.as_str()) else {
            continue;
        };
        if name.ends_with("_patch.json") && tier != "patch" {
            report.issues.push(HealthIssue {
                severity: IssueSeverity::Error,
                pair: label.to_string(),
                message: format!("{name}: filename says patch but outcome.tier={tier}"),
            });
        }
        if name.ends_with("_skip.json") && tier != "skip" {
            report.issues.push(HealthIssue {
                severity: IssueSeverity::Error,
                pair: label.to_string(),
                message: format!("{name}: filename says skip but outcome.tier={tier}"),
            });
        }
    }
}

fn check_gap(
    label: &str,
    gap: &GapEntry,
    opts: &HealthCheckOptions,
    report: &mut HealthCheckReport,
) {
    let tier = gap.outcome.as_ref().map(|o| o.tier.as_str()).unwrap_or("");
    match tier {
        "patch" => report.patches += 1,
        "skip" => report.skips += 1,
        _ => {}
    }

    let mut ok_count = 0usize;
    let duration = gap.geometry.as_ref().and_then(|g| g.duration_secs);
    let sr = gap.sample_rate.unwrap_or(0);

    for (bi, b) in gap.brackets.iter().enumerate() {
        report.brackets += 1;
        let gate_ok = b.failure_stage.is_none();
        let has_start = b.start_frame.is_some();
        let has_fill = b.fill_frames.is_some();
        let placed = has_start && has_fill;

        if gate_ok {
            report.gate_ok += 1;
            ok_count += 1;
        }
        if placed {
            report.placed += 1;
        }

        if has_start ^ has_fill {
            report.issues.push(HealthIssue {
                severity: IssueSeverity::Error,
                pair: label.to_string(),
                message: format!(
                    "g{} bracket[{bi}]: partial placement (start_frame={has_start} fill_frames={has_fill})",
                    gap.index
                ),
            });
        }
        if gate_ok && !placed {
            report.issues.push(HealthIssue {
                severity: IssueSeverity::Error,
                pair: label.to_string(),
                message: format!(
                    "g{} bracket[{bi}]: gate Ok but placement missing (Phase A projection broken?)",
                    gap.index
                ),
            });
        }
        if !gate_ok && placed {
            report.issues.push(HealthIssue {
                severity: IssueSeverity::Error,
                pair: label.to_string(),
                message: format!(
                    "g{} bracket[{bi}]: placement present on failed gate ({})",
                    gap.index,
                    b.failure_stage.as_deref().unwrap_or("?")
                ),
            });
        }
        if placed {
            if let Some(ff) = b.fill_frames {
                if ff == 0 {
                    report.issues.push(HealthIssue {
                        severity: IssueSeverity::Error,
                        pair: label.to_string(),
                        message: format!("g{} bracket[{bi}]: fill_frames == 0", gap.index),
                    });
                }
            }
            if let (Some(dur), Some(ff)) = (duration, b.fill_frames) {
                if sr > 0 {
                    let fill_secs = ff as f64 / sr as f64;
                    let delta = (fill_secs - dur).abs();
                    if delta > opts.fill_slack_secs {
                        report.issues.push(HealthIssue {
                            severity: IssueSeverity::Error,
                            pair: label.to_string(),
                            message: format!(
                                "g{} bracket[{bi}]: |fill−gap| = {delta:.3}s exceeds slack {:.3}s (fill_frames={ff}, gap={dur:.3}s, sr={sr})",
                                gap.index, opts.fill_slack_secs
                            ),
                        });
                    }
                }
            }
        }
    }

    if tier == "patch" && ok_count == 0 {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Error,
            pair: label.to_string(),
            message: format!(
                "g{}: outcome.tier=patch but 0 gate-Ok brackets (brackets={})",
                gap.index,
                gap.brackets.len()
            ),
        });
    }
    if tier == "skip" && ok_count > 0 {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Error,
            pair: label.to_string(),
            message: format!(
                "g{}: outcome.tier=skip but {ok_count} gate-Ok bracket(s)",
                gap.index
            ),
        });
    }
}

/// The fields a production `--gap-fingerprints` dump leaves at structural defaults, as dotted paths.
///
/// Mirrors `clip_sync_repair::application::gap_fingerprint::NOT_MEASURED_BY_PROJECTION`. Deliberately
/// **re-stated** rather than imported: that const lives behind the `calibration` feature on another
/// crate, and this module is a from-JSON reader that must keep working on corpora written by any
/// binary. The two lists are kept honest from both ends — the emitter has a unit test asserting it
/// declares this set, and [`check_not_measured`] below fails a corpus whose declaration disagrees with
/// what the file actually contains.
///
/// The union of the emitter's two lists, deliberately: the `baseline_lag.*` tail is
/// Paths whose **presence at a projection default** still means "unmeasured" on legacy dumps that
/// wrote zeros / `−120` / `""` instead of omitting the field. New dumps omit these via `Option` /
/// skip-empty and no longer declare them in `source.not_measured`.
///
/// Also includes `PROJECTED_BASELINE_LAG_FIELDS`, which a dump declares only when it projected the
/// row rather than measuring it. This reader must recognize the paths either way — a corpus that
/// declares them is a projected one (fine), and a corpus that carries a *real* sweep while declaring
/// them is the false declaration [`check_not_measured`] exists to catch.
const KNOWN_UNMEASURED: &[&str] = &[
    "levels.bin_ms",
    "levels.profile_db",
    "levels.floor_db",
    "levels.speech_peak_db",
    "silence.collar_rms_peak_ratio",
    "silence.collar_above_relative_floor",
    "silence.silence_peak_fraction",
    "contour.has_anchor_seam_contour",
    "contour.pre_flatness",
    "contour.post_flatness",
    "anchors.pre",
    "anchors.post",
    "outcome.seam_shape",
    "baseline_lag.window_ms",
    "baseline_lag.max_lag_ms",
    "baseline_lag.peak_lag_samples",
    "baseline_lag.frac_lag_samples",
    "baseline_lag.lag0_r",
    "baseline_lag.verdict",
];

/// The `floor_db` / `speech_peak_db` constant older `projected_level_profile` writes (`SILENCE_FLOOR_DB`).
const PROJECTED_FLOOR_DB: f64 = -120.0;

/// The `verdict` `projected_lag_entry` hardcodes onto every synthesized shoulder row.
const PROJECTED_LAG_VERDICT: &str = "timing_offset";

/// Which of [`KNOWN_UNMEASURED`] this gap carries a **real** value for.
///
/// "Real" is the negation of the exact constant the projection path writes, so a measured value that
/// happens to equal the default reads as unmeasured. That direction is the safe one: it under-reports
/// measurement, so it can only suppress a warning, never invent one.
fn measured_paths(gap: &GapEntry) -> Vec<&'static str> {
    let mut out = Vec::new();
    if let Some(l) = gap.levels.as_ref() {
        if l.bin_ms.unwrap_or(0) != 0 {
            out.push("levels.bin_ms");
        }
        if !l.profile_db.is_empty() {
            out.push("levels.profile_db");
        }
        if l.floor_db.is_some_and(|v| v != PROJECTED_FLOOR_DB) {
            out.push("levels.floor_db");
        }
        if l.speech_peak_db.is_some_and(|v| v != PROJECTED_FLOOR_DB) {
            out.push("levels.speech_peak_db");
        }
    }
    if let Some(s) = gap.silence.as_ref() {
        if s.collar_rms_peak_ratio.unwrap_or(0.0) != 0.0 {
            out.push("silence.collar_rms_peak_ratio");
        }
        if s.collar_above_relative_floor.unwrap_or(false) {
            out.push("silence.collar_above_relative_floor");
        }
        if s.silence_peak_fraction.unwrap_or(0.0) != 0.0 {
            out.push("silence.silence_peak_fraction");
        }
    }
    if let Some(c) = gap.contour.as_ref() {
        if c.has_anchor_seam_contour.unwrap_or(false) {
            out.push("contour.has_anchor_seam_contour");
        }
        if c.pre_flatness.unwrap_or(0.0) != 0.0 {
            out.push("contour.pre_flatness");
        }
        if c.post_flatness.unwrap_or(0.0) != 0.0 {
            out.push("contour.post_flatness");
        }
    }
    if let Some(a) = gap.anchors.as_ref() {
        if !a.pre.is_empty() {
            out.push("anchors.pre");
        }
        if !a.post.is_empty() {
            out.push("anchors.post");
        }
    }
    if gap
        .outcome
        .as_ref()
        .and_then(|o| o.seam_shape.as_deref())
        .is_some_and(|s| !s.is_empty())
    {
        out.push("outcome.seam_shape");
    }
    // A shoulder row is "measured" per-field: the projection zeroes the search parameters and the
    // integer lag, copies `peak_r` into `lag0_r`, and hardcodes the verdict. Any shoulder departing
    // from one of those is enough — a real sweep cannot systematically reproduce them.
    if let Some(bl) = gap.baseline_lag.as_ref() {
        for e in bl.pre_anchor.iter().chain(bl.post_anchor.iter()) {
            if e.window_ms.unwrap_or(0) != 0 {
                push_once(&mut out, "baseline_lag.window_ms");
            }
            if e.max_lag_ms.unwrap_or(0) != 0 {
                push_once(&mut out, "baseline_lag.max_lag_ms");
            }
            if e.peak_lag_samples.unwrap_or(0) != 0 {
                push_once(&mut out, "baseline_lag.peak_lag_samples");
            }
            if e.frac_lag_samples.unwrap_or(0.0) != 0.0 {
                push_once(&mut out, "baseline_lag.frac_lag_samples");
            }
            // The tell: projected rows peak exactly at lag 0 because `lag0_r` *is* `peak_r`.
            if let (Some(l0), Some(pk)) = (e.lag0_r, e.peak_r) {
                if l0 != pk {
                    push_once(&mut out, "baseline_lag.lag0_r");
                }
            }
            if e.verdict
                .as_deref()
                .is_some_and(|v| v != PROJECTED_LAG_VERDICT)
            {
                push_once(&mut out, "baseline_lag.verdict");
            }
        }
    }
    out
}

fn push_once(out: &mut Vec<&'static str>, path: &'static str) {
    if !out.contains(&path) {
        out.push(path);
    }
}

/// `source.not_measured` must be present when it is true, and true when it is present.
///
/// The 2026-07-31 corpus carried twelve of these fields at a constant on 802/802 full-tier gaps with
/// nothing in the file saying so; a reader had no way to tell a hardcoded `0.0` from a measured one,
/// and at least one downstream read took the constants at face value. Both directions matter: an
/// absent declaration invites that misread, and a **wrong** declaration is worse, because it licenses
/// discarding a field that was in fact measured.
fn check_not_measured(label: &str, corpus: &CorpusFile, report: &mut HealthCheckReport) {
    let full: Vec<&GapEntry> = corpus
        .gaps
        .iter()
        .filter(|g| g.tier.as_deref() == Some("full"))
        .collect();
    if full.is_empty() {
        return;
    }
    let declared: &[String] = corpus
        .source
        .as_ref()
        .map(|s| s.not_measured.as_slice())
        .unwrap_or(&[]);

    // Any listed field that some gap actually measured: the declaration is false.
    let mut falsely_declared: Vec<&str> = Vec::new();
    for gap in &full {
        for path in measured_paths(gap) {
            if declared.iter().any(|d| d == path) && !falsely_declared.contains(&path) {
                falsely_declared.push(path);
            }
        }
    }
    if !falsely_declared.is_empty() {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Error,
            pair: label.to_string(),
            message: format!(
                "source.not_measured claims {} but full-tier gaps carry real values there: {}",
                if falsely_declared.len() == 1 {
                    "a field"
                } else {
                    "fields"
                },
                falsely_declared.join(", ")
            ),
        });
    }

    if !declared.is_empty() {
        return;
    }
    // No declaration: is this the projection shape? Only claim so if *every* full-tier gap is at the
    // constant for every known path — one gap with real anchors means the dump is a different animal.
    let all_defaulted = full.iter().all(|g| measured_paths(g).is_empty());
    if all_defaulted {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Warn,
            pair: label.to_string(),
            message: format!(
                "{} full-tier gap(s) carry all {} legacy projection-default fields with no \
                 source.not_measured declaration: corpus predates honest omission (or the \
                 declaration), so levels/silence/contour/anchors/seam_shape zeros here are \
                 structural, not measurements",
                full.len(),
                KNOWN_UNMEASURED.len()
            ),
        });
    }
}

/// `equivalence_production` is either on every gap or on none — never on some.
///
/// The scan pushes one verdict per gap unconditionally, so a corpus missing it on a subset was written
/// by a binary that dropped the copy on an early `continue` (head gaps, before 2026-08-01). Partial
/// coverage is the signature; total absence just means scan did not classify. Also flags a verdict with
/// no `a_span_secs`, which predates span provenance — with it absent the diagnostic path silently binds
/// the nominal span while still printing `a_span: core`.
fn check_equivalence_production_coverage(
    label: &str,
    corpus: &CorpusFile,
    report: &mut HealthCheckReport,
) {
    let with: Vec<&GapEntry> = corpus
        .gaps
        .iter()
        .filter(|g| g.equivalence_production.is_some())
        .collect();
    if with.is_empty() {
        return;
    }
    let missing = corpus.gaps.len() - with.len();
    if missing > 0 {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Warn,
            pair: label.to_string(),
            message: format!(
                "{missing} of {} gap(s) carry no equivalence_production while {} do: the authoritative verdict \
                 was dropped for a subset (pre-2026-08-01 dumps lost it on head gaps), so those gaps have \
                 only the diagnostic reading",
                corpus.gaps.len(),
                with.len()
            ),
        });
    }
    let no_span = with
        .iter()
        .filter(|g| {
            g.equivalence_production
                .as_ref()
                .is_some_and(|v| v.a_span_secs.is_none())
        })
        .count();
    if no_span > 0 {
        report.issues.push(HealthIssue {
            severity: IssueSeverity::Warn,
            pair: label.to_string(),
            message: format!(
                "{no_span} equivalence_production verdict(s) carry no a_span_secs: corpus predates span \
                 provenance, so the `equivalence_diagnostic` block measured the nominal gap span while \
                 reporting a_span=core — the two readings are not comparable"
            ),
        });
    }
}

/// A donor window that runs past B's end is not a measurement of quiet.
///
/// **This reports media geometry, not a defect, since 2026-08-01.** Scan has always failed closed here
/// (`not_evaluated`); the diagnostic path used to clamp to the samples that exist and score the truncated
/// remainder, which came back 99.3–100 % silent on all 20 such gaps of the 2026-07-31 corpus — a drop
/// verdict manufactured out of absent audio. That path now refuses the window too (`donor_span: None`),
/// so both sides agree by refusal and the overrun no longer produces a divergence.
///
/// Kept as a Warn anyway: the overrun is a real property of the pair's mapping worth seeing (worst
/// observed +118 s), and it is the condition under which the equivalence gate declines to answer at all.
/// A dump where these stop appearing on known-tail-gap media would mean the mapping changed, not that
/// something got fixed.
fn check_donor_within_b(label: &str, corpus: &CorpusFile, report: &mut HealthCheckReport) {
    let Some(b_dur) = corpus
        .source
        .as_ref()
        .and_then(|s| s.b_source.as_ref())
        .and_then(|b| b.duration_secs)
    else {
        return;
    };
    // One frame of slack at 48 kHz: an end exactly at EOF is fine, and edges are rounded.
    let slack = 1.0 / 48_000.0;
    let mut overruns: Vec<(usize, f64)> = Vec::new();
    for gap in &corpus.gaps {
        let Some(end) = gap.geometry.as_ref().and_then(|g| g.b_mapped_end_secs) else {
            continue;
        };
        if end > b_dur + slack {
            overruns.push((gap.index, end - b_dur));
        }
    }
    if overruns.is_empty() {
        return;
    }
    let worst = overruns
        .iter()
        .map(|(_, o)| *o)
        .fold(f64::NEG_INFINITY, f64::max);
    let sample: Vec<String> = overruns
        .iter()
        .take(5)
        .map(|(i, o)| format!("g{i}(+{o:.2}s)"))
        .collect();
    report.issues.push(HealthIssue {
        severity: IssueSeverity::Warn,
        pair: label.to_string(),
        message: format!(
            "{} gap(s) map a donor window past B's end ({b_dur:.2}s), worst +{worst:.2}s: {}{} — both \
             equivalence paths refuse these (not_evaluated / no donor measured), so the gaps are kept, \
             not dropped; reported as pair geometry. Dumps written before 2026-08-01 instead scored the \
             clamped remainder and can carry a drop verdict there resting on audio that does not exist",
            overruns.len(),
            sample.join(" "),
            if overruns.len() > sample.len() {
                " …"
            } else {
                ""
            }
        ),
    });
}

fn list_library_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()) == Some("json")
                && p.file_name().and_then(|n| n.to_str()) != Some("manifest.json")
                && p.file_name().and_then(|n| n.to_str()) != Some("corpus.json")
        })
        .collect();
    files.sort();
    files
}

fn read_merged_library(dir: &Path) -> Option<CorpusFile> {
    let mut gaps = Vec::new();
    let mut source: Option<SourceMeta> = None;
    for path in list_library_files(dir) {
        let file: CorpusFile = read_json(&path)?;
        // Every per-gap file repeats the same `source`; keep the first and ignore the rest.
        if source.is_none() {
            source = file.source;
        }
        gaps.extend(file.gaps);
    }
    if gaps.is_empty() {
        return None;
    }
    Some(CorpusFile { source, gaps })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

// ── minimal JSON projection (independent of the prevalence analyzer types) ───────

#[derive(Deserialize)]
struct CorpusFile {
    #[serde(default)]
    source: Option<SourceMeta>,
    #[serde(default)]
    gaps: Vec<GapEntry>,
}

#[derive(Deserialize)]
struct SourceMeta {
    #[serde(default)]
    a_source: Option<FileSourceProj>,
    #[serde(default)]
    b_source: Option<FileSourceProj>,
    /// Wire token from `IncomparableReason` (e.g. `channel_layout_mismatch`).
    #[serde(default)]
    incomparable: Option<String>,
    /// Dotted paths the dumping binary declares it did not measure. Absent on pre-2026-08-01 corpora.
    #[serde(default)]
    not_measured: Vec<String>,
}

/// Just enough of `FileSource` to tell "this corpus records what it measured" from "it does not".
#[derive(Deserialize)]
struct FileSourceProj {
    #[serde(default)]
    codec: Option<String>,
    #[serde(default)]
    native_sample_rate: Option<u32>,
    #[serde(default)]
    native_channels: Option<u16>,
    #[serde(default)]
    duration_secs: Option<f64>,
}

impl FileSourceProj {
    fn has_provenance(&self) -> bool {
        self.codec.is_some() || self.native_sample_rate.is_some()
    }
}

#[derive(Deserialize)]
struct GapEntry {
    index: usize,
    /// `summary` or `full` — the detail tier this gap was dumped at.
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    sample_rate: Option<u32>,
    #[serde(default)]
    geometry: Option<Geometry>,
    #[serde(default)]
    brackets: Vec<Bracket>,
    #[serde(default)]
    outcome: Option<Outcome>,
    #[serde(default)]
    levels: Option<Levels>,
    #[serde(default)]
    silence: Option<Silence>,
    #[serde(default)]
    contour: Option<Contour>,
    #[serde(default)]
    anchors: Option<Anchors>,
    // Wire key was `scan_equivalence` before 2026-08-07; the alias keeps old corpora readable.
    #[serde(default, alias = "scan_equivalence")]
    equivalence_production: Option<ScanEquivalence>,
    #[serde(default)]
    baseline_lag: Option<BaselineLag>,
}

#[derive(Deserialize)]
struct BaselineLag {
    #[serde(default)]
    pre_anchor: Vec<LagRow>,
    #[serde(default)]
    post_anchor: Vec<LagRow>,
}

/// Only the fields [`KNOWN_UNMEASURED`] covers, plus `peak_r` — needed because `lag0_r`'s default is
/// not a constant but "whatever `peak_r` is".
#[derive(Deserialize)]
struct LagRow {
    #[serde(default)]
    window_ms: Option<u32>,
    #[serde(default)]
    max_lag_ms: Option<u32>,
    #[serde(default)]
    peak_lag_samples: Option<i64>,
    #[serde(default)]
    frac_lag_samples: Option<f64>,
    #[serde(default)]
    lag0_r: Option<f64>,
    #[serde(default)]
    peak_r: Option<f64>,
    #[serde(default)]
    verdict: Option<String>,
}

#[derive(Deserialize)]
struct Geometry {
    #[serde(default)]
    duration_secs: Option<f64>,
    #[serde(default)]
    b_mapped_end_secs: Option<f64>,
}

// The five blocks the projection path leaves at defaults. Every field is `Option` here purely so a
// corpus that omits one is readable — absence and the default are treated alike by `measured_paths`.

#[derive(Deserialize)]
struct Levels {
    #[serde(default)]
    bin_ms: Option<u32>,
    #[serde(default)]
    profile_db: Vec<f64>,
    #[serde(default)]
    floor_db: Option<f64>,
    #[serde(default)]
    speech_peak_db: Option<f64>,
}

#[derive(Deserialize)]
struct Silence {
    #[serde(default)]
    collar_rms_peak_ratio: Option<f64>,
    #[serde(default)]
    collar_above_relative_floor: Option<bool>,
    #[serde(default)]
    silence_peak_fraction: Option<f64>,
}

#[derive(Deserialize)]
struct Contour {
    #[serde(default)]
    has_anchor_seam_contour: Option<bool>,
    #[serde(default)]
    pre_flatness: Option<f64>,
    #[serde(default)]
    post_flatness: Option<f64>,
}

#[derive(Deserialize)]
struct Anchors {
    #[serde(default)]
    pre: Vec<serde_json::Value>,
    #[serde(default)]
    post: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ScanEquivalence {
    /// The A-side window scan actually measured. Absent on pre-2026-08-01 dumps.
    #[serde(default)]
    a_span_secs: Option<(f64, f64)>,
}

#[derive(Deserialize)]
struct Bracket {
    #[serde(default)]
    start_frame: Option<usize>,
    #[serde(default)]
    fill_frames: Option<usize>,
    #[serde(default)]
    failure_stage: Option<String>,
}

#[derive(Deserialize)]
struct Outcome {
    tier: String,
    #[serde(default)]
    seam_shape: Option<String>,
}

#[derive(Deserialize)]
struct ManifestFile {
    #[serde(default)]
    gap_count: usize,
    #[serde(default)]
    entries: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    file: String,
    index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, name: &str, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), body).unwrap();
    }

    fn healthy_corpus() -> String {
        r#"{
          "source":{"a_source":{"id":"aaaaaaaa","codec":"flac","bit_depth":"s24","native_sample_rate":48000,
                                "native_channels":2,"sample_rate":48000,"channels":2,"duration_secs":60},
                    "b_source":{"id":"bbbbbbbb","codec":"aac","native_sample_rate":44100,"native_channels":2,
                                "source_audio_bitrate_bps":192000,"sample_rate":48000,"channels":2,"duration_secs":60},
                    "scan_recipe":{"min_gap_ms":500,"silence_peak_fraction":0.01,"absolute_silence_rms":0.0,"scan_block_ms":100},
                    "gap_count":2},
          "gaps":[
            {"index":0,"tier":"full","sample_rate":48000,"channels":2,
             "geometry":{"a_start_secs":0,"a_end_secs":1,"a_refined_start_secs":0,"a_refined_end_secs":1,"duration_secs":1.0},
             "levels":{"bin_ms":0,"profile_db":[],"floor_db":-120,"speech_peak_db":-40,"noise_floor_db":-50,"gap_floor_db":-90},
             "silence":{"collar_rms_peak_ratio":0,"collar_above_relative_floor":false,"silence_peak_fraction":0},
             "contour":{"has_anchor_seam_contour":false,"pre_flatness":0,"post_flatness":0},
             "anchors":{"pre":[],"post":[]},
             "brackets":[
               {"pre_time_secs":0,"post_time_secs":1,"span_secs":1,"move_frames":0,
                "seam_pre":0.9,"seam_post":0.9,"start_frame":100,"fill_frames":48000},
               {"pre_time_secs":0,"post_time_secs":1.1,"span_secs":1.1,"move_frames":4800,
                "seam_pre":0.1,"seam_post":0.1,"failure_stage":"waveform_floor"}
             ],
             "outcome":{"plan_kind":"fillable","tier":"patch","seam_shape":""}},
            {"index":1,"tier":"full","sample_rate":48000,"channels":2,
             "geometry":{"a_start_secs":10,"a_end_secs":11,"a_refined_start_secs":10,"a_refined_end_secs":11,"duration_secs":1.0},
             "levels":{"bin_ms":0,"profile_db":[],"floor_db":-120,"speech_peak_db":-40,"noise_floor_db":-50,"gap_floor_db":-90},
             "silence":{"collar_rms_peak_ratio":0,"collar_above_relative_floor":false,"silence_peak_fraction":0},
             "contour":{"has_anchor_seam_contour":false,"pre_flatness":0,"post_flatness":0},
             "anchors":{"pre":[],"post":[]},
             "brackets":[
               {"pre_time_secs":10,"post_time_secs":11,"span_secs":1,"move_frames":0,
                "seam_pre":0.2,"seam_post":0.2,"failure_stage":"waveform_floor"}
             ],
             "outcome":{"plan_kind":"fillable","tier":"skip","seam_shape":"","skip_reason":"waveform_floor"}}
          ]
        }"#
        .into()
    }

    fn single_gap(corpus: &str, index: usize) -> String {
        let mut v: serde_json::Value = serde_json::from_str(corpus).unwrap();
        let gaps = v["gaps"].as_array().unwrap().clone();
        let gap = gaps.into_iter().find(|g| g["index"] == index).unwrap();
        v["gaps"] = serde_json::json!([gap]);
        v["source"]["gap_count"] = serde_json::json!(1);
        v.to_string()
    }

    fn write_healthy_pair(dir: &Path) {
        let corpus = healthy_corpus();
        write(dir, "corpus.json", &corpus);
        write(
            dir,
            "aaaaaaaa_bbbb_t00-00-00_g000_full_patch.json",
            &single_gap(&corpus, 0),
        );
        write(
            dir,
            "aaaaaaaa_bbbb_t00-00-10_g001_full_skip.json",
            &single_gap(&corpus, 1),
        );
        write(
            dir,
            "manifest.json",
            r#"{
              "a_id":"aaaaaaaa","b_id":"bbbbbbbb",
              "scan_recipe":{"min_gap_ms":500,"silence_peak_fraction":0.01,"absolute_silence_rms":0.0,"scan_block_ms":100},
              "gap_count":2,
              "entries":[
                {"file":"aaaaaaaa_bbbb_t00-00-00_g000_full_patch.json","index":0,"a_start_secs":0,"tier":"full","outcome":"patch"},
                {"file":"aaaaaaaa_bbbb_t00-00-10_g001_full_skip.json","index":1,"a_start_secs":10,"tier":"full","outcome":"skip"}
              ]
            }"#,
        );
    }

    #[test]
    fn healthy_corpus_passes() {
        let root = tempfile::tempdir().unwrap();
        write_healthy_pair(&root.path().join("1"));
        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.pairs_checked, 1);
        assert_eq!(report.gaps, 2);
        assert_eq!(report.gate_ok, 1);
        assert_eq!(report.placed, 1);
        assert_eq!(report.patches, 1);
        assert_eq!(report.skips, 1);
    }

    #[test]
    fn gate_ok_without_placement_fails() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        // Corrupt: remove placement from the Ok bracket.
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        corpus["gaps"][0]["brackets"][0]
            .as_object_mut()
            .unwrap()
            .remove("start_frame");
        corpus["gaps"][0]["brackets"][0]
            .as_object_mut()
            .unwrap()
            .remove("fill_frames");
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(!report.ok());
        assert!(report
            .issues
            .iter()
            .any(|i| i.message.contains("gate Ok but placement missing")));
    }

    #[test]
    fn skip_with_ok_bracket_fails() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        // Make skip gap's only bracket pass with placement.
        let br = corpus["gaps"][1]["brackets"][0].as_object_mut().unwrap();
        br.remove("failure_stage");
        br.insert("start_frame".into(), serde_json::json!(1));
        br.insert("fill_frames".into(), serde_json::json!(48000));
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(!report.ok());
        assert!(report
            .issues
            .iter()
            .any(|i| i.message.contains("outcome.tier=skip but")));
    }

    #[test]
    fn corpus_without_source_provenance_warns_only() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        // Strip the Track-A provenance fields: the shape every pre-Track-A corpus has.
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        for side in ["a_source", "b_source"] {
            let obj = corpus["source"][side].as_object_mut().unwrap();
            for field in [
                "codec",
                "bit_depth",
                "native_sample_rate",
                "native_channels",
            ] {
                obj.remove(field);
            }
        }
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.warn_count(), 1);
        // Assert the whole sentence, not a fragment: a `\`-continuation in the source once left a run of
        // indentation inside this message, and a fragment match sailed straight past it.
        let expected = concat!(
            "source.{a,b}_source carry no codec / native_sample_rate: ",
            "corpus predates source provenance, so codec-conditioned reads cannot be stratified"
        );
        assert!(report.issues.iter().any(|i| i.message == expected));
    }

    #[test]
    fn incomparable_channel_layout_warns_only() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        // Provenance-only refuse dump: empty gaps, matching empty manifest, no per-gap library files.
        write(
            &dir,
            "corpus.json",
            r#"{
              "source":{"a_source":{"id":"aaaaaaaa","codec":"flac","native_sample_rate":48000,
                                    "native_channels":2,"sample_rate":48000,"channels":2,"duration_secs":60},
                        "b_source":{"id":"bbbbbbbb","codec":"aac","native_sample_rate":48000,
                                    "native_channels":1,"sample_rate":48000,"channels":1,"duration_secs":60},
                        "scan_recipe":{"silence_peak_fraction":0.01},"gap_count":1,
                        "incomparable":"channel_layout_mismatch"},
              "gaps":[]
            }"#,
        );
        write(
            &dir,
            "manifest.json",
            r#"{"a_id":"aaaaaaaa","b_id":"bbbbbbbb","gap_count":0,"entries":[]}"#,
        );

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.warn_count(), 1);
        let expected = concat!(
            "source.incomparable=channel_layout_mismatch: pairwise fingerprint characterize was refused ",
            "(gaps empty); do not treat this pair as a measured null"
        );
        assert!(
            report.issues.iter().any(|i| i.message == expected),
            "{:?}",
            report.issues
        );
    }

    #[test]
    fn legacy_native_channels_mismatch_with_gaps_warns_only() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        corpus["source"]["b_source"]["native_channels"] = serde_json::json!(1);
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.warn_count(), 1);
        let expected = concat!(
            "native_channels disagree (A 2 / B 1) but gaps are present: ",
            "corpus predates the channel-layout refuse gate; B may have been ",
            "indexed at A's channel count — re-dump the pair"
        );
        assert!(
            report.issues.iter().any(|i| i.message == expected),
            "{:?}",
            report.issues
        );
    }

    /// Strip every field the projection path defaults, reproducing the 2026-07-31 corpus shape.
    fn defaults_only(gap: &mut serde_json::Value) {
        gap["levels"]["speech_peak_db"] = serde_json::json!(-120.0);
        gap["levels"]["floor_db"] = serde_json::json!(-120.0);
        gap["levels"]["bin_ms"] = serde_json::json!(0);
        gap["levels"]["profile_db"] = serde_json::json!([]);
    }

    #[test]
    fn projection_defaults_without_declaration_warn() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        for g in corpus["gaps"].as_array_mut().unwrap() {
            defaults_only(g);
        }
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("no source.not_measured declaration")),
            "{}",
            report.summary_text()
        );
    }

    #[test]
    fn declaring_not_measured_silences_the_warning() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        for g in corpus["gaps"].as_array_mut().unwrap() {
            defaults_only(g);
        }
        corpus["source"]["not_measured"] = serde_json::json!(KNOWN_UNMEASURED);
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.warn_count(), 0, "{}", report.summary_text());
    }

    /// A declaration that is wrong is worse than none: it licenses discarding real data.
    #[test]
    fn declaring_a_field_that_was_measured_fails() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        // The fixture measures speech_peak_db (−40, not the −120 constant); declare it anyway.
        corpus["source"]["not_measured"] = serde_json::json!(["levels.speech_peak_db"]);
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(!report.ok());
        assert!(report
            .issues
            .iter()
            .any(|i| i.message.contains("levels.speech_peak_db")
                && i.message.contains("carry real values")));
    }

    /// A real sweep is distinguishable from `projected_lag_entry`'s fabrication, and declaring it
    /// unmeasured then fails.
    ///
    /// The two tells are independent: a nonzero `max_lag_ms` (the projection has no search to report)
    /// and `lag0_r != peak_r` (the projection copies one into the other, so every projected shoulder
    /// reads as peaking at zero lag). Either alone must be enough.
    #[test]
    fn declaring_a_measured_baseline_lag_fails() {
        for (field, row) in [
            (
                "baseline_lag.max_lag_ms",
                serde_json::json!({"peak_r":0.9,"lag0_r":0.9,"max_lag_ms":600,"verdict":"timing_offset"}),
            ),
            (
                "baseline_lag.lag0_r",
                serde_json::json!({"peak_r":0.9,"lag0_r":0.1,"verdict":"timing_offset"}),
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("1");
            write_healthy_pair(&dir);
            let mut corpus: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap())
                    .unwrap();
            for g in corpus["gaps"].as_array_mut().unwrap() {
                defaults_only(g);
                g["baseline_lag"] = serde_json::json!({"pre_anchor":[row], "post_anchor":[]});
            }
            corpus["source"]["not_measured"] = serde_json::json!(KNOWN_UNMEASURED);
            fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

            let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
            assert!(
                report
                    .issues
                    .iter()
                    .any(|i| i.message.contains(field) && i.message.contains("carry real values")),
                "{field}: {}",
                report.summary_text()
            );
        }
    }

    /// The projected shape — zeroed search, `lag0_r == peak_r`, constant verdict — stays silent.
    #[test]
    fn declaring_a_projected_baseline_lag_is_clean() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        for g in corpus["gaps"].as_array_mut().unwrap() {
            defaults_only(g);
            g["baseline_lag"] = serde_json::json!({
                "pre_anchor":[{"window_ms":0,"max_lag_ms":0,"lag0_r":0.9,"peak_r":0.9,
                               "peak_lag_samples":0,"frac_lag_samples":0.0,"frac_lag_ms":-3.0,
                               "verdict":"timing_offset"}],
                "post_anchor":[]
            });
        }
        corpus["source"]["not_measured"] = serde_json::json!(KNOWN_UNMEASURED);
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.warn_count(), 0, "{}", report.summary_text());
    }

    #[test]
    fn partial_equivalence_production_coverage_warns() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        // Gap 0 classified, gap 1 not: the head-gap `continue` signature.
        corpus["gaps"][0]["equivalence_production"] =
            serde_json::json!({"class":"repairable_dropout","a_span_secs":[0.0,1.0]});
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("carry no equivalence_production while")),
            "{}",
            report.summary_text()
        );
    }

    #[test]
    fn equivalence_production_without_span_provenance_warns() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        for g in corpus["gaps"].as_array_mut().unwrap() {
            g["equivalence_production"] = serde_json::json!({"class":"repairable_dropout"});
        }
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("carry no a_span_secs")),
            "{}",
            report.summary_text()
        );
    }

    /// Pre-2026-08-07 corpora wrote `scan_equivalence`. The serde alias must keep them readable, so
    /// the same defect (no `a_span_secs`) is still detected under the legacy key — otherwise old
    /// dumps would silently read as "no production verdict at all".
    #[test]
    fn legacy_scan_equivalence_key_still_parses() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        for g in corpus["gaps"].as_array_mut().unwrap() {
            g["scan_equivalence"] = serde_json::json!({"class":"repairable_dropout"});
        }
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("carry no a_span_secs")),
            "legacy key must deserialize into equivalence_production: {}",
            report.summary_text()
        );
    }

    #[test]
    fn donor_window_past_b_end_warns() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        // b_source.duration_secs is 60 in the fixture.
        corpus["gaps"][1]["geometry"]["b_mapped_end_secs"] = serde_json::json!(63.5);
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.message.contains("map a donor window past B's end")
                    && i.message.contains("g1(+3.50s)")),
            "{}",
            report.summary_text()
        );
    }

    /// A donor window that ends exactly at B's last sample is not an overrun.
    #[test]
    fn donor_window_at_b_end_is_clean() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        write_healthy_pair(&dir);
        let mut corpus: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();
        corpus["gaps"][1]["geometry"]["b_mapped_end_secs"] = serde_json::json!(60.0);
        fs::write(dir.join("corpus.json"), corpus.to_string()).unwrap();

        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.warn_count(), 0, "{}", report.summary_text());
    }

    #[test]
    fn incomplete_sibling_is_warning_only() {
        let root = tempfile::tempdir().unwrap();
        write_healthy_pair(&root.path().join("1"));
        fs::create_dir_all(root.path().join("99")).unwrap();
        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.warn_count(), 1);
    }
}
