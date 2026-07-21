//! Real-content anchor-prominence calibration (plan §8 Q1 follow-up).
//!
//! Tier: **validation** (`validation-tests` feature; needs `ffmpeg` on PATH and
//! `.\scripts\fetch_corpus_sources.ps1`). Measures the energy-peak anchor-prominence distribution on
//! the three corpus archetypes (orchestral / restaurant ambience / interview) so a production default
//! for `anchor_seam_min_prominence` (today `0.0`) can be chosen with evidence.
//!
//! This is a **calibration probe, not a pass/fail test**: it has no assertions — it decodes corpus
//! media, injects gaps, measures prominences, and prints a CSV for a human to read. The real logic
//! lives in `clip_sync_repair_harness::anchor_prominence`.
//!
//! **Status: finding settled — kept for re-runs, not pending work.** This probe already ran and
//! *refuted* raising the default above `0.0` (real anchors measured ≤0.073; a 0.10 floor would disable
//! anchor rescue), so `anchor_seam_min_prominence` stays `0.0`. It is retained (gated + `#[ignore]`, so
//! it never runs in a normal build) only to re-measure **if anchor detection itself changes**. Do not
//! mistake it for queued work, and do not promote it to a bin — the `validation-tests` tier is the
//! right home for an on-demand, corpus-dependent calibration probe.
//!
//! PR: **no**.
//! Run: `cargo test -p clip-sync-repair --features validation-tests --test calibrate_anchor_prominence -- --ignored --nocapture`

use clip_sync::testing::corpus_sources::{
    find_source, load_sources, prepare_source_master_wav, source_cache_path, source_ready,
};
use clip_sync_repair_harness::anchor_prominence::{
    fraction_kept, measure_anchor_prominences, ProminenceStats,
};
use clip_sync_repair_harness::floor_oracle::{read_mono_wav, require_validation_env};

const SAMPLE_RATE: u32 = 48_000;
const MASTER_SECS: u32 = 60;
const GAP_SECS: f64 = 1.0;

/// (archetype label, corpus source id) — the three sources in `tests/corpus/sources.toml`.
const SOURCES: &[(&str, &str)] = &[
    ("orchestral", "musopen_grieg_mountain_king"),
    ("restaurant", "wikimedia_schiphol_restaurant_ambience"),
    ("interview", "wikimedia_martin_amis_interview_1990"),
];

#[test]
#[ignore = "validation: needs ffmpeg + fetch_corpus_sources.ps1 — run via test-tier validation"]
fn calibrate_anchor_prominence_csv() {
    require_validation_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let thetas = [0.05f32, 0.10, 0.15, 0.20, 0.30];

    println!(
        "archetype,source_id,count,min,median,p90,p99,max,keep@0.05,keep@0.10,keep@0.15,keep@0.20,keep@0.30"
    );
    for &(label, source_id) in SOURCES {
        if !source_ready(source_id) {
            eprintln!(
                "SKIP {label} ({source_id}): not fetched — run scripts/fetch_corpus_sources.ps1"
            );
            continue;
        }
        let sources = load_sources();
        let source = find_source(&sources, source_id);
        let src_path = source_cache_path(source);
        let master = temp.path().join(format!("{source_id}.wav"));
        if !prepare_source_master_wav(&src_path, &master, SAMPLE_RATE, Some(MASTER_SECS)) {
            eprintln!("SKIP {label}: ffmpeg decode failed for {}", src_path.display());
            continue;
        }
        let (rate, samples) = read_mono_wav(&master);
        let mono: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32768.0).collect();
        let dur = mono.len() as f64 / f64::from(rate);

        // Aggregate energy-peak anchor prominences across several injected-gap positions.
        let mut all = Vec::new();
        for frac in [0.2, 0.35, 0.5, 0.65, 0.8] {
            let pos = dur * frac;
            if pos + GAP_SECS < dur {
                all.extend(measure_anchor_prominences(&mono, rate, pos, GAP_SECS));
            }
        }

        let keeps: Vec<String> = thetas
            .iter()
            .map(|&t| format!("{:.3}", fraction_kept(&all, t)))
            .collect();
        match ProminenceStats::from_values(all) {
            Some(s) => {
                println!(
                    "{label},{source_id},{},{:.4},{:.4},{:.4},{:.4},{:.4},{}",
                    s.count,
                    s.min,
                    s.median,
                    s.p90,
                    s.p99,
                    s.max,
                    keeps.join(","),
                );
                eprintln!(
                    "{label}: count={} min={:.4} median={:.4} p90={:.4} p99={:.4} max={:.4}",
                    s.count, s.min, s.median, s.p90, s.p99, s.max
                );
                eprintln!(
                    "  keep fraction @ [0.05,0.10,0.15,0.20,0.30] = {}",
                    keeps.join(", ")
                );
            }
            None => eprintln!("{label}: no energy-peak anchors detected"),
        }
    }
}
