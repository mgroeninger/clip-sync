//! Step 1 of the residual-vs-seam value experiment (report-only, direct scoring).
//!
//! For each oracle fixture (F1, F2, F4-decoy) this scores the **seam Pearson**, the **residual**
//! cancellation, and the **measured floor** at the known placements (truth / decoy / nominal),
//! using the *same window geometry as `patch_region`* so the dB numbers transfer to the pipeline.
//! It emits one CSV row per `(fixture, variant, placement)` — the schema steps 2 (codec-noise
//! variants) and 3 (disagreement table) extend by adding rows, not columns.
//!
//! This measures **scoring discrimination** (does residual separate truth from decoy better than
//! Pearson?), not the end-to-end search+gate decision. The floor here is anchored at the *true*
//! alignment because these fixtures carry a deliberately wrong nominal map; production anchors the
//! floor at the alignment nominal (with the probe's wide lag + outward walk absorbing local drift).
//!
//! Run: `cargo test -p clip-sync-repair seam_residual_truth_decoy_csv -- --ignored --nocapture`

use clip_sync_repair::domain::gap_fill_fit::{
    apply_residual_to_confidence, FillConfidence, ResidualGateError,
};
use clip_sync_repair::domain::policies::{
    border_templates_for_gap, border_templates_per_channel_for_gap, fill_seam_correlations,
    interleaved_to_channels, interleaved_to_mono, seam_chosen_and_floor, GapBorderSpec,
    DEFAULT_RESIDUAL_FLOOR_OK_DB, SeamFloorParams, SeamPlacement, SeamSide, SeamTemplates,
};
use clip_sync_repair::domain::{
    residual_max_lag_frames, DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB, DEFAULT_RESIDUAL_LAG_SECS,
};
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_production, build_f2_production, build_f4_decoy_production, EnergySignatureFixture,
};

// Production defaults mirrored from infrastructure/config.rs + application/patch_audio.rs so the
// harness windows match `evaluate_seam_gate_fit_candidate` exactly.
const NORMALIZE_WINDOW_SECS: f64 = 5.0;
const MIN_BORDER_DISCOVERY_SECS: f64 = 2.0;
const FILL_SEAM_SEARCH_SECS: f64 = 0.25;
// Production border_standoff is 0.35 s; the harness uses 0 (see `geometry_for`).

fn correlate_frames_for_gap(gap_frames: usize, rate: u32) -> usize {
    let gap_secs = gap_frames as f64 / rate as f64;
    let window_secs = NORMALIZE_WINDOW_SECS
        .min(gap_secs * 0.45)
        .clamp(MIN_BORDER_DISCOVERY_SECS, 2.0)
        .max(0.25);
    ((window_secs * rate as f64) as usize).max(1)
}

struct Geometry {
    border_frames: usize,
    seam_gate_frames: usize,
    standoff_frames: usize,
}

fn geometry_for(gap_frames: usize, rate: u32) -> Geometry {
    let correlate = correlate_frames_for_gap(gap_frames, rate);
    Geometry {
        border_frames: ((NORMALIZE_WINDOW_SECS * rate as f64) as usize).min(correlate),
        seam_gate_frames: correlate
            .min((FILL_SEAM_SEARCH_SECS * rate as f64).round() as usize)
            .max(1),
        // Zero standoff in the harness: production's search slides B to absorb the standoff, but
        // direct scoring places B at the exact oracle frame, so the A template must not be trimmed
        // or the pre/post windows misalign by ~`border_standoff` and even the true fill won't cancel.
        standoff_frames: 0,
    }
}

struct Scored {
    seam_pre: f64,
    seam_post: f64,
    residual_pre_db: f64,
    residual_post_db: f64,
    floor_pre_db: f64,
    floor_post_db: f64,
    floor_src_pre: &'static str,
    floor_src_post: &'static str,
}

impl Scored {
    fn headroom_pre(&self) -> f64 {
        self.residual_pre_db - self.floor_pre_db
    }
    fn headroom_post(&self) -> f64 {
        self.residual_post_db - self.floor_post_db
    }
}

/// Best-lag seam Pearson per side over ±`max_lag`, mirroring the residual's lag search so Pearson
/// is a *fair* baseline: in production the unified search aligns the placement, so scoring without
/// any lag would unfairly penalize Pearson for the A template's standoff/low-energy-trim offsets.
fn best_lag_seam(templates: &SeamTemplates<'_>, placement: SeamPlacement, max_lag: i64) -> (f64, f64) {
    let mut best_pre = f64::NEG_INFINITY;
    let mut best_post = f64::NEG_INFINITY;
    for lag in -max_lag..=max_lag {
        let start = placement.start as i64 + lag;
        if start < 0 {
            continue;
        }
        let (pre, post) =
            fill_seam_correlations(templates, SeamPlacement { start: start as usize, ..placement });
        best_pre = best_pre.max(pre);
        best_post = best_post.max(post);
    }
    (best_pre, best_post)
}

/// Build `patch_region`-matched templates + the true-alignment floor for `fixture`, then score the
/// seam (best-lag Pearson), residual, and floor at B frame `start`. The floor is anchored at the
/// true alignment and is independent of `start`; it is rebuilt per call for simplicity.
fn score_at(fixture: &EnergySignatureFixture, start: usize) -> Scored {
    let ch = fixture.channels.max(1);
    let rate = fixture.sample_rate;
    let gap_start = fixture.gap_start;
    let gap_end = fixture.gap_end;
    let gap_frames = gap_end - gap_start;
    let geom = geometry_for(gap_frames, rate);

    let border_spec = GapBorderSpec {
        gap_start_frame: gap_start,
        gap_end_frame: gap_end,
        border_frames: geom.border_frames,
        border_standoff_frames: geom.standoff_frames,
        silence_peak_fraction: fixture.structure_params.silence_peak_fraction,
        absolute_rms_floor: fixture.structure_params.absolute_silence_rms,
    };
    let (a_pre, a_post) = border_templates_for_gap(&fixture.a_samples, ch, &border_spec);
    let (a_pre_ch, a_post_ch) =
        border_templates_per_channel_for_gap(&fixture.a_samples, ch, &border_spec);
    let b_mono = interleaved_to_mono(&fixture.b_samples, ch);
    let b_ch = interleaved_to_channels(&fixture.b_samples, ch);

    let pre_window = geom.seam_gate_frames.min(a_pre.len().max(1));
    let post_window = geom.seam_gate_frames.min(a_post.len()).max(1);
    let templates = SeamTemplates {
        a_pre: &a_pre,
        a_post: &a_post,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        b_mono: &b_mono,
        b_ch: &b_ch,
    };
    let placement = SeamPlacement { start, gap_frames, pre_window, post_window };
    let max_lag = residual_max_lag_frames(rate, DEFAULT_RESIDUAL_LAG_SECS);
    let (seam_pre, seam_post) = best_lag_seam(&templates, placement, max_lag);

    // Unified model (matches the pipeline): chosen residual and floor share the same lag radius
    // (`residual_lag_secs` → frames). Floor anchors at the true alignment; chosen at `start`.
    let delta_true = fixture.true_fill_start as i64 - gap_start as i64;
    let chosen_delta = start as i64 - gap_start as i64;
    let floor_params = |window: usize| SeamFloorParams {
        a_samples: &fixture.a_samples,
        channels: ch,
        b_mono: &b_mono,
        window,
        standoff_frames: geom.standoff_frames,
        a_to_b_delta: delta_true,
        step_frames: window.max(1),
        max_walk_frames: rate as usize * 3,
        absolute_silence_rms: fixture.structure_params.absolute_silence_rms,
        max_lag_frames: max_lag,
    };
    let (chosen_pre, floor_pre) = seam_chosen_and_floor(
        &floor_params(pre_window),
        SeamSide::Pre,
        gap_start,
        gap_end,
        chosen_delta,
    );
    let (chosen_post, floor_post) = seam_chosen_and_floor(
        &floor_params(post_window),
        SeamSide::Post,
        gap_start,
        gap_end,
        chosen_delta,
    );

    Scored {
        seam_pre,
        seam_post,
        residual_pre_db: chosen_pre.residual_db,
        residual_post_db: chosen_post.residual_db,
        floor_pre_db: floor_pre.residual_db,
        floor_post_db: floor_post.residual_db,
        floor_src_pre: floor_pre.source_label(),
        floor_src_post: floor_post.source_label(),
    }
}

fn run_fixture(fixture: &EnergySignatureFixture, variant: &str) {
    let placements = [
        ("truth", fixture.true_fill_start),
        ("decoy", fixture.b_decoy_fill_start()),
        ("nominal", fixture.nominal_fill_start),
    ];
    for (label, start) in placements {
        let s = score_at(fixture, start);
        println!(
            "{},{},{},{},{:.3},{:.3},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{},{}",
            fixture.id,
            variant,
            label,
            start == fixture.true_fill_start,
            s.seam_pre,
            s.seam_post,
            s.residual_pre_db,
            s.residual_post_db,
            s.floor_pre_db,
            s.floor_post_db,
            s.headroom_pre(),
            s.headroom_post(),
            s.floor_src_pre,
            s.floor_src_post,
        );
    }
}

// ── Step 2: broadband same-master fixtures ──────────────────────────────────────────────────
//
// F1/F2 use piecewise-constant ramps, which are degenerate for any waveform-domain metric (Pearson
// reads 0 on a zero-variance window). Real soundtracks are broadband, so these fixtures synthesize
// a broadband master (inharmonic partials + shaped noise) and build a same-master A/B pair with a
// silenced gap, a true fill at the gap timestamp, and a different-content decoy elsewhere. Variants
// model two encodes of one master (independent requantization noise) and an inter-encode delay.

const PI: f64 = std::f64::consts::PI;

#[derive(Clone, Copy)]
enum Variant {
    Clean,
    /// Independent low-level broadband noise on A and B (two encodes of one master).
    CodecNoise,
    /// Codec noise plus a 3.4-sample inter-encode delay on B (tests lag recovery).
    CodecNoiseShift,
}

impl Variant {
    fn label(self) -> &'static str {
        match self {
            Variant::Clean => "broadband_clean",
            Variant::CodecNoise => "broadband_codec_noise",
            Variant::CodecNoiseShift => "broadband_codec_noise_shift",
        }
    }
}

fn lcg(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((*state >> 33) as f64 / (1u64 << 31) as f64) - 1.0 // ~[-1, 1)
}

/// Broadband, **non-stationary** master: three chirps (frequencies sweep over the timeline so each
/// timestamp is spectrally distinct) plus shaped noise. Non-stationarity is what makes truth and
/// decoy distinguishable — a stationary tone looks identical at every placement.
fn broadband_master(total: usize, rate: u32) -> Vec<f64> {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    (0..total)
        .map(|i| {
            let t = i as f64 / rate as f64;
            // phase = 2π·(f0·t + 0.5·rate·t²) for a linear chirp of slope `rate` Hz/s.
            let c1 = (2.0 * PI * (150.0 * t + 0.5 * 40.0 * t * t)).sin() * 3000.0;
            let c2 = (2.0 * PI * (400.0 * t - 0.5 * 15.0 * t * t)).sin() * 2000.0;
            let c3 = (2.0 * PI * (900.0 * t + 0.5 * 25.0 * t * t)).sin() * 1200.0;
            c1 + c2 + c3 + lcg(&mut seed) * 1500.0
        })
        .collect()
}

/// Linear-interpolated sample of `master` at fractional index `x` (0 outside range).
fn interp(master: &[f64], x: f64) -> f64 {
    if x < 0.0 {
        return 0.0;
    }
    let i = x.floor() as usize;
    if i + 1 >= master.len() {
        return master.get(i).copied().unwrap_or(0.0);
    }
    let f = x - i as f64;
    master[i] * (1.0 - f) + master[i + 1] * f
}

fn build_broadband(rate: u32, variant: Variant) -> EnergySignatureFixture {
    let (noise_amp, b_shift) = match variant {
        Variant::Clean => (0.0, 0.0),
        Variant::CodecNoise => (40.0, 0.0),
        Variant::CodecNoiseShift => (40.0, 3.4), // inter-encode delay (samples)
    };
    build_broadband_with(rate, noise_amp, b_shift)
}

/// Broadband same-master fixture with explicit codec-noise amplitude and B inter-encode delay
/// (samples, may be fractional). `b_shift` models imperfect alignment between two encodes.
fn build_broadband_with(rate: u32, noise_amp: f64, b_shift: f64) -> EnergySignatureFixture {
    let total = (rate as usize) * 20; // 20 s
    let gap_start = (rate as usize) * 10; // gap at 10 s
    let gap_frames = rate as usize; // 1 s gap
    let gap_end = gap_start + gap_frames;
    let decoy_start = (rate as usize) * 6; // different-content decoy at 6 s

    let master = broadband_master(total, rate);
    let mut seed_a = 0x1111_2222_3333_4444u64;
    let mut seed_b = 0x5555_6666_7777_8888u64;

    let mut a = vec![0i16; total];
    let mut b = vec![0i16; total];
    for f in 0..total {
        let in_gap = (gap_start..gap_end).contains(&f);
        let a_val = if in_gap { 0.0 } else { master[f] + lcg(&mut seed_a) * noise_amp };
        let b_val = interp(&master, f as f64 - b_shift) + lcg(&mut seed_b) * noise_amp;
        a[f] = a_val.round().clamp(-32768.0, 32767.0) as i16;
        b[f] = b_val.round().clamp(-32768.0, 32767.0) as i16;
    }

    let bin_frames = ((0.05 * rate as f64).round() as usize).max(1);
    let structure_params = clip_sync_repair::domain::gap_structure::StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: rate as usize, // unused by direct scoring
        fill_length_slack_frames: 0,
        max_fine_adjustment_frames: 0,
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 33.0,
    };

    EnergySignatureFixture {
        id: "broadband",
        a_samples: a,
        b_samples: b,
        channels: 1,
        sample_rate: rate,
        gap_start,
        gap_end,
        context_frames: (rate as usize) * 3,
        true_fill_start: gap_start,
        true_fill_end: gap_end,
        nominal_fill_start: decoy_start,
        nominal_fill_end: decoy_start + gap_frames,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

#[test]
#[ignore = "diagnostic: cargo test -p clip-sync-repair seam_residual_broadband_csv -- --ignored --nocapture"]
fn seam_residual_broadband_csv() {
    println!(
        "fixture,variant,placement,oracle_correct,seam_pre,seam_post,\
         residual_pre_db,residual_post_db,floor_pre_db,floor_post_db,\
         headroom_pre_db,headroom_post_db,floor_source_pre,floor_source_post"
    );
    for variant in [Variant::Clean, Variant::CodecNoise, Variant::CodecNoiseShift] {
        run_fixture(&build_broadband(16_000, variant), variant.label());
    }
}

/// Placement-offset sweep (unified model): floor anchored at the true fill, chosen placement moved
/// `offset` frames off it (B itself is aligned). With seam and floor sharing one lag radius
/// (`residual_lag_secs`), a true fill offset within that radius recovers → headroom ≈ 0; beyond it
/// headroom grows (correct reject). Codec noise on → realistic floor.
#[test]
#[ignore = "diagnostic: cargo test -p clip-sync-repair seam_residual_alignment_sweep_csv -- --ignored --nocapture"]
fn seam_residual_alignment_sweep_csv() {
    println!(
        "rate,offset_samples,seam_pre,seam_post,residual_pre_db,residual_post_db,\
         floor_pre_db,floor_post_db,headroom_pre_db,headroom_post_db"
    );
    let rate = 16_000u32;
    let fixture = build_broadband_with(rate, 40.0, 0.0); // B aligned; vary the *placement* instead
    let offsets = [0i64, 16, 32, 64, 100, 200, 400, 512, 600, 1000];
    for offset in offsets {
        let start = (fixture.true_fill_start as i64 + offset) as usize;
        let s = score_at(&fixture, start);
        println!(
            "{},{},{:.3},{:.3},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}",
            rate,
            offset,
            s.seam_pre,
            s.seam_post,
            s.residual_pre_db,
            s.residual_post_db,
            s.floor_pre_db,
            s.floor_post_db,
            s.headroom_pre(),
            s.headroom_post(),
        );
    }
}

/// F4 decoy at the bool nominal: Pearson accepts (~0.84) but residual headroom is huge — the EC-6
/// veto case. Fast score-level check (no full patch search).
#[test]
fn f4_decoy_placement_informative_with_high_headroom() {
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let decoy = fixture.b_decoy_fill_start();
    let s = score_at(&fixture, decoy);

    let pearson_min = s.seam_pre.min(s.seam_post);
    assert!(
        pearson_min >= 0.35,
        "F4 decoy Pearson {pearson_min:.3} should pass min_fill_correlation",
    );

    let headroom = s.headroom_pre().max(s.headroom_post());
    assert!(
        headroom > DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        "F4 decoy headroom {headroom:.1} dB should exceed veto margin {}",
        DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
    );
    assert!(
        s.floor_pre_db <= DEFAULT_RESIDUAL_FLOOR_OK_DB
            && s.floor_post_db <= DEFAULT_RESIDUAL_FLOOR_OK_DB,
        "F4 decoy floor pre={:.1} post={:.1} should be informative (≤ {DEFAULT_RESIDUAL_FLOOR_OK_DB})",
        s.floor_pre_db,
        s.floor_post_db,
    );

    let verdict = clip_sync_repair::domain::policies::SeamResidualVerdict {
        chosen_pre_db: s.residual_pre_db,
        chosen_post_db: s.residual_post_db,
        floor_pre_db: s.floor_pre_db,
        floor_post_db: s.floor_post_db,
        floor_source_pre: clip_sync_repair::domain::policies::SeamFloorSource::Border,
        floor_source_post: clip_sync_repair::domain::policies::SeamFloorSource::Border,
        informative: true,
    };
    let err = apply_residual_to_confidence(
        Ok(FillConfidence::High),
        &verdict,
        DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        false,
    )
    .unwrap_err();
    assert!(matches!(err, ResidualGateError::HeadroomExceeded { .. }));
}

#[test]
#[ignore = "diagnostic: cargo test -p clip-sync-repair seam_residual_truth_decoy_csv -- --ignored --nocapture"]
fn seam_residual_truth_decoy_csv() {
    println!(
        "fixture,variant,placement,oracle_correct,seam_pre,seam_post,\
         residual_pre_db,residual_post_db,floor_pre_db,floor_post_db,\
         headroom_pre_db,headroom_post_db,floor_source_pre,floor_source_post"
    );

    run_fixture(&build_f1_production(48_000, 2, 3.0), "clean");
    run_fixture(&build_f2_production(48_000, 2, 90.0, 3.0), "clean");
    run_fixture(&build_f4_decoy_production(48_000, 2, 90.0, 3.0), "clean");
}
