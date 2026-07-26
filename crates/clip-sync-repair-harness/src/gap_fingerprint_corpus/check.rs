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

    for gap in &corpus.gaps {
        check_gap(label, gap, opts, report);
    }

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
    for path in list_library_files(dir) {
        let file: CorpusFile = read_json(&path)?;
        gaps.extend(file.gaps);
    }
    if gaps.is_empty() {
        return None;
    }
    Some(CorpusFile { gaps })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

// ── minimal JSON projection (independent of the prevalence analyzer types) ───────

#[derive(Deserialize)]
struct CorpusFile {
    #[serde(default)]
    gaps: Vec<GapEntry>,
}

#[derive(Deserialize)]
struct GapEntry {
    index: usize,
    #[serde(default)]
    sample_rate: Option<u32>,
    #[serde(default)]
    geometry: Option<Geometry>,
    #[serde(default)]
    brackets: Vec<Bracket>,
    #[serde(default)]
    outcome: Option<Outcome>,
}

#[derive(Deserialize)]
struct Geometry {
    #[serde(default)]
    duration_secs: Option<f64>,
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
          "source":{"a_source":{"id":"aaaaaaaa","sample_rate":48000,"channels":2,"duration_secs":60},
                    "b_source":{"id":"bbbbbbbb","sample_rate":48000,"channels":2,"duration_secs":60},
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
             "outcome":{"plan_kind":"fillable","tier":"skip","seam_shape":"","skip_reason":"correlation_below_threshold"}}
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
        write(dir, "aaaaaaaa_bbbb_t00-00-00_g000_full_patch.json", &single_gap(&corpus, 0));
        write(dir, "aaaaaaaa_bbbb_t00-00-10_g001_full_skip.json", &single_gap(&corpus, 1));
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
        assert!(report.issues.iter().any(|i| i.message.contains("gate Ok but placement missing")));
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
    fn incomplete_sibling_is_warning_only() {
        let root = tempfile::tempdir().unwrap();
        write_healthy_pair(&root.path().join("1"));
        fs::create_dir_all(root.path().join("99")).unwrap();
        let report = check_dirs(&[root.path().to_path_buf()], &HealthCheckOptions::default());
        assert!(report.ok(), "{}", report.summary_text());
        assert_eq!(report.warn_count(), 1);
    }
}
