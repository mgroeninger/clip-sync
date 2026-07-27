//! Ad-hoc diagnostic: a 1.5 s silent gap embedded in broadband noise, flanked by speech.
//!
//! Reproduces the production geometry behind a `boundary correlation below threshold
//! (pre=0.03 post=0.03)` skip, then exercises the anchor-seam machinery directly at the
//! **domain** level (no alignment / decode plumbing) to answer one question:
//!
//!   Given speech peaks ~1 s outside a noise-bracketed gap, does the anchor path
//!   (a) find the speech anchors, (b) form a feasible bracket, and (c) score a HIGH seam
//!   on B where the bare throat scores ~0?
//!
//! Geometry (48 kHz, mono):
//!   0.00–0.50  silence lead
//!   0.50–0.85  speech1  (tone, ~-24 dBFS, Hann-enveloped → single energy peak)
//!   0.85–1.85  background noise1 (broadband, ~-45 dBFS, DECORRELATED A vs B)
//!   1.85–3.35  GAP (A: silence; B: reference content)            [1.5 s]
//!   3.35–4.35  background noise2 (broadband, ~-45 dBFS, DECORRELATED A vs B)
//!   4.35–4.70  speech2  (tone, ~-22 dBFS, Hann-enveloped)
//!   4.70–5.20  silence trail
//!
//! Speech is byte-identical in A and B (same-master content → correlates ~1).
//! Background is decorrelated (different seed → correlates ~0). This is what collapses the
//! throat seam while leaving the speech-anchored seam intact.
//!
//! Run: cargo test -p clip-sync-repair --test diag_anchor_quiet_gap -- --nocapture

use clip_sync_repair::domain::gap_anchor_seam::{
    anchor_bracket_both_matchable, list_anchor_candidates_a, list_feasible_anchor_brackets,
    AnchorMatchabilityParams, AnchorSeamMode, AnchorSeamParams, AnchorSource,
};
use clip_sync_repair::domain::gap_structure::StructureMatchParams;
use clip_sync_repair::domain::policies::{
    border_templates_for_gap, fill_seam_correlations, GapBorderSpec, RefinedGapFrames,
    SeamPlacement, SeamTemplates,
};
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    EnergySignatureFixture, ProductionScenarioSpec,
};
use clip_sync_repair_fixtures::energy_signature_production::w5_anchor_rescue_repair;
use clip_sync_repair_fixtures::w5_anchor_rescue_diag::{score_w5_fixture, W5JointWinner};

const RATE: usize = 48_000;

fn secs(s: f64) -> usize {
    (s * RATE as f64).round() as usize
}

fn db(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// Deterministic, decorrelated broadband noise in [-amp, amp]. Same `seed` ⇒ same waveform;
/// different seeds give independent streams (splitmix64 finalizer over seed:frame).
fn noise(seed: u32, frame: usize) -> f32 {
    let mut z =
        (((seed as u64) << 32) | (frame as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let unit = (z >> 40) as f32 / (1u32 << 24) as f32; // [0, 1)
    unit * 2.0 - 1.0
}

/// Hann-enveloped tone — a single clean energy peak (a flat plateau has no strict local max).
fn write_speech(buf: &mut [f32], start: usize, end: usize, freq: f64, amp: f32) {
    let n = (end - start) as f64;
    for f in start..end {
        let t = (f - start) as f64;
        let env = 0.5 - 0.5 * (std::f64::consts::TAU * t / n).cos();
        let s = (std::f64::consts::TAU * freq * t / RATE as f64).sin();
        buf[f] = (env * s) as f32 * amp;
    }
}

fn write_noise(buf: &mut [f32], start: usize, end: usize, seed: u32, amp: f32) {
    for f in start..end {
        buf[f] = noise(seed, f) * amp;
    }
}

struct Fixture {
    a: Vec<f32>,
    b: Vec<f32>,
    gap_start: usize,
    gap_end: usize,
}

fn build_fixture() -> Fixture {
    let total = secs(5.20);
    let sp1 = (secs(0.50), secs(0.85));
    let bg1 = (secs(0.85), secs(1.85));
    let gap = (secs(1.85), secs(3.35));
    let bg2 = (secs(3.35), secs(4.35));
    let sp2 = (secs(4.35), secs(4.70));

    let (a_speech, b_speech, bg) = (db(-24.0), db(-22.0), db(-45.0));

    let mut a = vec![0.0f32; total];
    let mut b = vec![0.0f32; total];

    // Speech: identical (same-master) in A and B.
    write_speech(&mut a, sp1.0, sp1.1, 330.0, a_speech);
    write_speech(&mut b, sp1.0, sp1.1, 330.0, a_speech);
    write_speech(&mut a, sp2.0, sp2.1, 440.0, b_speech);
    write_speech(&mut b, sp2.0, sp2.1, 440.0, b_speech);

    // Background: decorrelated (seed 1 on A, seed 2 on B).
    write_noise(&mut a, bg1.0, bg1.1, 1, bg);
    write_noise(&mut b, bg1.0, bg1.1, 2, bg);
    write_noise(&mut a, bg2.0, bg2.1, 3, bg);
    write_noise(&mut b, bg2.0, bg2.1, 4, bg);

    // Gap: A is a true dropout (silence); B carries the reference content that A is missing.
    write_noise(&mut b, gap.0, gap.1, 5, bg);
    write_speech(&mut b, gap.0, gap.1, 200.0, db(-30.0));

    Fixture {
        a,
        b,
        gap_start: gap.0,
        gap_end: gap.1,
    }
}

fn anchor_params(gap_frames: usize) -> AnchorSeamParams {
    let bin_frames = secs(0.050); // gap_signature_bin_ms = 50
    AnchorSeamParams {
        context_frames: secs(3.0), // gap_signature_context_secs = 3
        max_anchors_per_side: 5,
        max_bracket_frames: secs(5.0), // max_anchor_bracket_secs = 5
        min_prominence: 0.0,           // anchor_seam_min_prominence = 0.0
        structure: StructureMatchParams {
            gap_frames,
            bin_frames,
            search_radius_frames: secs(10.0),
            fill_length_slack_frames: bin_frames,
            max_fine_adjustment_frames: bin_frames,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        },
    }
}

/// Build (a_pre, a_post) border templates + a B-side `SeamTemplates`, then score the seam at a
/// placement. Mono path (b_ch len 1 ⇒ `use_channels` false).
fn seam_at(
    fx: &Fixture,
    refined: RefinedGapFrames,
    b_mono: &[f64],
    placement_start: usize,
    window: usize,
) -> (f64, f64, bool) {
    let spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: secs(0.60),
        border_standoff_frames: 0,
        silence_peak_fraction: 0.01,
        absolute_rms_floor: 0.0,
    };
    let (a_pre, a_post) = border_templates_for_gap(&fx.a, 1, &spec);
    let b_ch = [b_mono.to_vec()];
    let templates = SeamTemplates {
        a_pre: &a_pre,
        a_post: &a_post,
        a_pre_ch: &[],
        a_post_ch: &[],
        b_mono,
        b_ch: &b_ch,
    };
    let gap_frames = refined.end_frame - refined.start_frame;
    let placement = SeamPlacement {
        start: placement_start,
        gap_frames,
        pre_window: window,
        post_window: window,
    };
    let (pre, post) = fill_seam_correlations(&templates, placement);
    let matchable = anchor_bracket_both_matchable(
        &templates,
        placement,
        window,
        window,
        &AnchorMatchabilityParams::default(), // min_pearson 0.12
        None,
        0,
    );
    (pre, post, matchable)
}

#[test]
fn anchor_rescue_on_noise_bracketed_gap() {
    let fx = build_fixture();
    let b_mono: Vec<f64> = fx.b.iter().map(|&s| s as f64).collect();
    let gap_frames = fx.gap_end - fx.gap_start;
    let scan_hole = RefinedGapFrames {
        start_frame: fx.gap_start,
        end_frame: fx.gap_end,
    };
    let params = anchor_params(gap_frames);
    let window = secs(0.20);

    println!("\n=== geometry ===");
    println!(
        "gap {:.3}-{:.3}s ({} frames, {:.3}s)  context={:.1}s  max_bracket={:.1}s",
        fx.gap_start as f64 / RATE as f64,
        fx.gap_end as f64 / RATE as f64,
        gap_frames,
        gap_frames as f64 / RATE as f64,
        params.context_frames as f64 / RATE as f64,
        params.max_bracket_frames as f64 / RATE as f64,
    );

    // (a) anchors
    let set = list_anchor_candidates_a(&fx.a, 1, scan_hole, &params);
    let show =
        |label: &str, cands: &[clip_sync_repair::domain::gap_anchor_seam::AnchorCandidate]| {
            println!("\n=== {label} anchors ===");
            for c in cands {
                println!(
                    "  frame {:>7} ({:.3}s)  {:?}  prominence={:.4}",
                    c.frame,
                    c.frame as f64 / RATE as f64,
                    c.source,
                    c.prominence,
                );
            }
        };
    show("pre", &set.pre);
    show("post", &set.post);

    // (b) feasible brackets
    let brackets = list_feasible_anchor_brackets(&set, scan_hole, &params);
    println!("\n=== feasible brackets ({}) ===", brackets.len());
    for br in &brackets {
        println!(
            "  pre {:>7} ({:.3}s)  post {:>7} ({:.3}s)  span={:.3}s  move={} frames",
            br.pre.frame,
            br.pre.frame as f64 / RATE as f64,
            br.post.frame,
            br.post.frame as f64 / RATE as f64,
            (br.post.frame - br.pre.frame) as f64 / RATE as f64,
            br.move_frames,
        );
    }

    // (c) throat vs best-anchor seam Pearson on B (no global B shift ⇒ identity placement)
    println!("\n=== seam Pearson on B ===");
    let (tp, tq, tm) = seam_at(&fx, scan_hole, &b_mono, fx.gap_start, window);
    println!("  baseline throat   pre={tp:.3} post={tq:.3}  matchable(≥0.12)={tm}");

    // The genuine speech bracket: both anchors are energy peaks; pick the one whose weakest
    // anchor is the most prominent (noise energy-peaks have prominence ~0, speech ~0.008).
    let best = brackets
        .iter()
        .filter(|b| {
            b.pre.source == AnchorSource::EnergyPeak && b.post.source == AnchorSource::EnergyPeak
        })
        .max_by(|a, b| {
            a.pre
                .prominence
                .min(a.post.prominence)
                .partial_cmp(&b.pre.prominence.min(b.post.prominence))
                .unwrap()
        })
        .or_else(|| brackets.first());
    if let Some(br) = best {
        let refined = br.refined;
        let (ap, aq, am) = seam_at(&fx, refined, &b_mono, refined.start_frame, window);
        println!(
            "  speech anchor     pre={ap:.3} post={aq:.3}  matchable(≥0.12)={am}  (pre@{:.3}s post@{:.3}s)",
            br.pre.frame as f64 / RATE as f64,
            br.post.frame as f64 / RATE as f64,
        );

        assert!(tp < 0.12 && tq < 0.12, "throat should collapse in noise");
        assert!(
            am && ap.min(aq) >= 0.12,
            "speech-anchored bracket should clear matchability where the throat cannot"
        );
    } else {
        panic!("no feasible bracket — anchors did not reach across the noise collar");
    }
    println!();
}

// ----------------------------------------------------------------------------------------------
// Full gate path with a global B offset.
//
// Production-scale (40 s) version of the same noise-collar geometry, placed deep enough to satisfy
// the patch geometry's lead-in (context + border + margin). B's bursts are byte-identical to A's but
// translated by `shift`; collars are INDEPENDENT noise (throat stays weak wherever the search slides);
// the fill sits at the shifted dropout. Driven through `score_w5_fixture`, which runs the real
// structure-align → matchability → E1–E7 routing and reports each bracket's `failure_stage`.
// ----------------------------------------------------------------------------------------------

/// Build a production-scale noise-collar fixture whose B content is globally shifted by `shift_secs`
/// (the structure search must slide that far to find it). `shift_secs = 0` ⇒ no offset.
fn build_offset_fixture(shift_secs: f64) -> EnergySignatureFixture {
    let rate = RATE as u32;
    let ch = 1usize;
    let spec = ProductionScenarioSpec::production_standard(40.0, 3.0);
    let total = secs(40.0);
    let context = spec.context_frames(rate, total);

    // Gap ~14.35 s in (clears the 14 s lead); 1.5 s silent dropout, 1 s noise collars, 0.35 s speech.
    let gap_start = secs(14.35);
    let gap_end = secs(15.85);
    let sp1 = (secs(13.00), secs(13.35));
    let n1 = (secs(13.35), secs(14.35));
    let n2 = (secs(15.85), secs(16.85));
    let sp2 = (secs(16.85), secs(17.20));
    let (a_sp, b_sp, bg) = (db(-24.0), db(-22.0), db(-45.0));

    let mut a = vec![0.0f32; total];
    write_speech(&mut a, sp1.0, sp1.1, 330.0, a_sp);
    write_speech(&mut a, sp2.0, sp2.1, 440.0, b_sp);
    write_noise(&mut a, n1.0, n1.1, 1, bg);
    write_noise(&mut a, n2.0, n2.1, 3, bg);
    // gap stays silent (true dropout)

    let shift = secs(shift_secs);
    let mut b = vec![0.0f32; total];
    // Same-master bursts: identical waveform, translated by `shift` (anchors must match on B).
    write_speech(&mut b, sp1.0 + shift, sp1.1 + shift, 330.0, a_sp);
    write_speech(&mut b, sp2.0 + shift, sp2.1 + shift, 440.0, b_sp);
    // Independent collars (different seeds) → baseline throat decorrelates wherever it lands.
    write_noise(&mut b, n1.0 + shift, n1.1 + shift, 11, bg);
    write_noise(&mut b, n2.0 + shift, n2.1 + shift, 13, bg);
    // Reference content where A dropped out (so there is something to extract as fill).
    write_speech(&mut b, gap_start + shift, gap_end + shift, 200.0, db(-30.0));

    let structure_params =
        spec.structure_match_params(rate, gap_end - gap_start, spec.search_radius_frames(rate));

    EnergySignatureFixture {
        id: "noise_collar_b_offset",
        a_samples: a,
        b_samples: b,
        channels: ch,
        sample_rate: rate,
        gap_start,
        gap_end,
        context_frames: context,
        true_fill_start: gap_start + shift,
        true_fill_end: gap_end + shift,
        nominal_fill_start: gap_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: shift,
        structure_params,
    }
}

#[test]
fn anchor_rescue_under_global_b_offset() {
    // Gate search radius = 5 s (w5_anchor_rescue_repair's fill_border_search_secs).
    let search_radius_secs = 5.0;
    let repair = w5_anchor_rescue_repair(AnchorSeamMode::Auto, search_radius_secs);

    println!(
        "\n=== full gate path: anchor rescue vs global B offset (search radius {:.1}s) ===",
        search_radius_secs
    );
    println!(
        "{:>10}  {:>14}  {:>16}  {:>8}  {:>20}  {}",
        "shift", "would_run", "baseline(min)", "brackets", "winner", "best_pass / failures"
    );

    for &shift_secs in &[0.0, 2.0, 4.0, 6.0] {
        let fx = build_offset_fixture(shift_secs);
        let s = score_w5_fixture(&fx, &repair);

        let baseline = s
            .baseline
            .map(|(p, q)| format!("{:.3}", p.min(q)))
            .unwrap_or_else(|| "degenerate".into());
        let passing: Vec<_> = s.brackets.iter().filter(|b| b.passed_gate).collect();
        let best = passing
            .iter()
            .filter_map(|b| b.min_pearson.map(|m| (m, b.move_frames)))
            .fold(None, |acc: Option<(f64, usize)>, (m, mv)| {
                Some(acc.map_or((m, mv), |a| if m > a.0 { (m, mv) } else { a }))
            });

        // Histogram of failure stages among non-passing brackets.
        let mut stages = std::collections::BTreeMap::new();
        for b in s.brackets.iter().filter(|b| !b.passed_gate) {
            *stages
                .entry(b.failure_stage.unwrap_or("?"))
                .or_insert(0usize) += 1;
        }
        let detail = match best {
            Some((m, mv)) => format!("best min={:.3} move={} frames", m, mv),
            None => format!("no pass; failures={stages:?}"),
        };
        let winner = match s.joint_winner {
            W5JointWinner::Skip => "SKIP".to_string(),
            W5JointWinner::Baseline => "baseline".to_string(),
            W5JointWinner::Anchor { move_frames } => format!("ANCHOR(+{move_frames})"),
        };

        println!(
            "{:>9.1}s  {:>14}  {:>16}  {:>8}  {:>20}  {}",
            shift_secs,
            s.anchor_seam_would_run,
            baseline,
            s.brackets.len(),
            winner,
            detail,
        );
    }
    println!();
}
