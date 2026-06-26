//! Seam residual score harness.

use clip_sync_repair::domain::gap_fill_fit::{
    apply_residual_to_confidence, classify_fill_waveform_confidence, FillConfidence,
    ResidualGateError,
};
use clip_sync_repair::domain::policies::{
    border_templates_for_gap, border_templates_per_channel_for_gap, fill_seam_correlations,
    interleaved_to_channels, interleaved_to_mono, seam_chosen_and_floor,
    seam_chosen_and_floor_multichannel, selected_seam_channels, GapBorderSpec,
    DEFAULT_RESIDUAL_FLOOR_OK_DB, SeamChannelResidual, SeamFloorParams, SeamFloorProbe, SeamPlacement,
    SeamResidualVerdict, SeamSide, SeamTemplates,
};
use clip_sync_repair::domain::{
    residual_max_lag_frames, DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB, DEFAULT_RESIDUAL_LAG_SECS,
};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::EnergySignatureFixture;

// Production defaults mirrored from infrastructure/config.rs + application/patch_audio.rs so the
// harness windows match `evaluate_seam_gate_fit_candidate` exactly.
const NORMALIZE_WINDOW_SECS: f64 = 5.0;
const MIN_BORDER_DISCOVERY_SECS: f64 = 2.0;
const FILL_SEAM_SEARCH_SECS: f64 = 0.25;
// Production border_standoff is 0.35 s; the harness uses 0 (see `geometry_for`).

pub fn correlate_frames_for_gap(gap_frames: usize, rate: u32) -> usize {
    let gap_secs = gap_frames as f64 / rate as f64;
    let window_secs = NORMALIZE_WINDOW_SECS
        .min(gap_secs * 0.45)
        .clamp(MIN_BORDER_DISCOVERY_SECS, 2.0)
        .max(0.25);
    ((window_secs * rate as f64) as usize).max(1)
}

pub struct Geometry {
    pub border_frames: usize,
    pub seam_gate_frames: usize,
    pub standoff_frames: usize,
}

pub fn geometry_for(gap_frames: usize, rate: u32) -> Geometry {
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

pub struct Scored {
    /// Best-lag Pearson (fair baseline for discrimination CSVs).
    pub seam_pre: f64,
    pub seam_post: f64,
    /// Production Pearson at the chosen placement (no lag) — matches `patch_region`.
    pub pearson_pre: f64,
    pub pearson_post: f64,
    pub residual_pre_db: f64,
    pub residual_post_db: f64,
    pub floor_pre_db: f64,
    pub floor_post_db: f64,
    pub floor_src_pre: &'static str,
    pub floor_src_post: &'static str,
}

impl Scored {
    pub fn headroom_pre(&self) -> f64 {
        self.residual_pre_db - self.floor_pre_db
    }
    pub fn headroom_post(&self) -> f64 {
        self.residual_post_db - self.floor_post_db
    }
}

pub struct ScoredPlacement {
    pub scored: Scored,
    pub chosen_pre: SeamFloorProbe,
    pub chosen_post: SeamFloorProbe,
    pub floor_pre: SeamFloorProbe,
    pub floor_post: SeamFloorProbe,
    pub placement_slide_frames: u64,
    pub max_lag_frames: i64,
}

impl ScoredPlacement {
    pub fn verdict(&self) -> SeamResidualVerdict {
        SeamResidualVerdict::from_parts_with_placement(
            &self.chosen_pre,
            &self.chosen_post,
            &self.floor_pre,
            &self.floor_post,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
            self.placement_slide_frames,
            self.max_lag_frames,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PearsonTierLabel {
    High,
    Marginal,
    DeadZone,
}

impl PearsonTierLabel {
    fn from_confidence(c: FillConfidence) -> Self {
        match c {
            FillConfidence::High => Self::High,
            FillConfidence::Marginal => Self::Marginal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Marginal => "marginal",
            Self::DeadZone => "dead_zone",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcomeLabel {
    Pass,
    Rescue,
    Veto,
    Abstain,
    SkipPearson,
}

impl GateOutcomeLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Rescue => "rescue",
            Self::Veto => "veto",
            Self::Abstain => "abstain",
            Self::SkipPearson => "skip_pearson",
        }
    }
}

pub struct DisagreementRow {
    pub pearson_min: f64,
    pub pearson_tier: PearsonTierLabel,
    pub pearson_patches: bool,
    pub informative: bool,
    pub headroom_db: f64,
    pub veto_outcome: GateOutcomeLabel,
    pub rescue_outcome: GateOutcomeLabel,
    pub veto_patches: bool,
    pub rescue_patches: bool,
}

pub fn production_pearson(
    pearson_pre: f64,
    pearson_post: f64,
) -> Result<FillConfidence, f64> {
    let repair = RepairConfig::default();
    classify_fill_waveform_confidence(
        pearson_pre,
        pearson_post,
        repair.min_fill_correlation,
        repair.fill_marginal_margin,
        repair.fill_absolute_floor,
    )
}

pub fn gate_outcome(
    pearson: Result<FillConfidence, f64>,
    verdict: &SeamResidualVerdict,
    rescue_enabled: bool,
) -> GateOutcomeLabel {
    if !verdict.informative || verdict.beyond_lag_reach() {
        return match pearson {
            Ok(_) => GateOutcomeLabel::Abstain,
            Err(_) => GateOutcomeLabel::SkipPearson,
        };
    }
    let pearson_was_err = pearson.is_err();
    match apply_residual_to_confidence(
        pearson,
        verdict,
        DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        rescue_enabled,
    ) {
        Ok(FillConfidence::Marginal) if rescue_enabled && pearson_was_err => {
            GateOutcomeLabel::Rescue
        }
        Ok(_) => GateOutcomeLabel::Pass,
        Err(ResidualGateError::HeadroomExceeded { .. }) => GateOutcomeLabel::Veto,
        Err(ResidualGateError::PearsonBelowFloor(_)) => GateOutcomeLabel::SkipPearson,
    }
}

pub fn gate_patches(outcome: GateOutcomeLabel) -> bool {
    matches!(
        outcome,
        GateOutcomeLabel::Pass | GateOutcomeLabel::Rescue | GateOutcomeLabel::Abstain
    )
}

pub fn disagreement_row(placement: &ScoredPlacement) -> DisagreementRow {
    let s = &placement.scored;
    let verdict = placement.verdict();
    let pearson = production_pearson(s.pearson_pre, s.pearson_post);
    let pearson_tier = match pearson {
        Ok(c) => PearsonTierLabel::from_confidence(c),
        Err(_) => PearsonTierLabel::DeadZone,
    };
    let pearson_patches = pearson.is_ok();
    let veto_outcome = gate_outcome(pearson, &verdict, false);
    let rescue_outcome = gate_outcome(pearson, &verdict, true);
    DisagreementRow {
        pearson_min: s.pearson_pre.min(s.pearson_post),
        pearson_tier,
        pearson_patches,
        informative: verdict.informative,
        headroom_db: verdict.worst_headroom_db(),
        veto_outcome,
        rescue_outcome,
        veto_patches: gate_patches(veto_outcome),
        rescue_patches: gate_patches(rescue_outcome),
    }
}

/// Best-lag seam Pearson per side over ±`max_lag`, mirroring the residual's lag search so Pearson
/// is a *fair* baseline: in production the unified search aligns the placement, so scoring without
/// any lag would unfairly penalize Pearson for the A template's standoff/low-energy-trim offsets.
pub fn best_lag_seam(templates: &SeamTemplates<'_>, placement: SeamPlacement, max_lag: i64) -> (f64, f64) {
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

/// Score seam (best-lag Pearson), residual, and floor at B frame `start`.
///
/// Floor anchors at the **true** alignment; chosen residual is at `start`.
pub fn score_placement(fixture: &EnergySignatureFixture, start: usize) -> ScoredPlacement {
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
    let (pearson_pre, pearson_post) = fill_seam_correlations(&templates, placement);
    let (seam_pre, seam_post) = best_lag_seam(&templates, placement, max_lag);

    // Unified model (matches the pipeline): chosen residual and floor share the same lag radius
    // (`residual_lag_secs` → frames). Floor anchors at the true alignment; chosen at `start`.
    let delta_true = fixture.true_fill_start as i64 - gap_start as i64;
    let nominal_delta = fixture.nominal_fill_start as i64 - gap_start as i64;
    let chosen_delta = start as i64 - gap_start as i64;
    // Production anchors the floor at nominal; this harness often anchors at truth for
    // discrimination. Only apply reach abstention when the floor anchor matches production.
    let production_floor_anchor = delta_true == nominal_delta;
    let placement_slide = if production_floor_anchor {
        (chosen_delta - nominal_delta).unsigned_abs()
    } else {
        0
    };
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

    ScoredPlacement {
        scored: Scored {
            seam_pre,
            seam_post,
            pearson_pre,
            pearson_post,
            residual_pre_db: chosen_pre.residual_db,
            residual_post_db: chosen_post.residual_db,
            floor_pre_db: floor_pre.residual_db,
            floor_post_db: floor_post.residual_db,
            floor_src_pre: floor_pre.source_label(),
            floor_src_post: floor_post.source_label(),
        },
        chosen_pre,
        chosen_post,
        floor_pre,
        floor_post,
        placement_slide_frames: placement_slide,
        max_lag_frames: max_lag,
    }
}

/// Multichannel result of [`score_placement_multichannel`]: the production verdict (built from the
/// energy-selected channels), the channels selected, and the per-side per-channel residuals for CSV.
pub struct ScoredPlacementMultichannel {
    pub pearson_pre: f64,
    pub pearson_post: f64,
    /// Energy-selected channels (`selected_seam_channels`); empty ⇒ mono-downmix fallback was used.
    pub selected_channels: Vec<usize>,
    pub verdict: SeamResidualVerdict,
    pub pre: Vec<SeamChannelResidual>,
    pub post: Vec<SeamChannelResidual>,
}

/// Score a placement through the **per-channel** residual path — the production routing
/// ([`seam_chosen_and_floor_multichannel`] + [`SeamResidualVerdict::from_channel_residuals`]) used by
/// `measure_fit_residual_verdict`. Separate from [`score_placement`] (Option A) so the existing mono /
/// stereo corpus rows keep scoring on the mono path unchanged. Empty selection falls back to the mono
/// path with `from_parts_with_placement`, matching production.
pub fn score_placement_multichannel(
    fixture: &EnergySignatureFixture,
    start: usize,
) -> ScoredPlacementMultichannel {
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
    let (pearson_pre, pearson_post) = fill_seam_correlations(&templates, placement);

    let delta_true = fixture.true_fill_start as i64 - gap_start as i64;
    let nominal_delta = fixture.nominal_fill_start as i64 - gap_start as i64;
    let chosen_delta = start as i64 - gap_start as i64;
    let production_floor_anchor = delta_true == nominal_delta;
    let placement_slide = if production_floor_anchor {
        (chosen_delta - nominal_delta).unsigned_abs()
    } else {
        0
    };
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

    // Same selection production recomputes (`selected_seam_channels`), filtered to channels B has.
    let selected: Vec<usize> = selected_seam_channels(&fixture.a_samples, ch, &border_spec)
        .into_iter()
        .filter(|&c| c < b_ch.len())
        .collect();

    let (pre, post, verdict) = if selected.is_empty() {
        let (chosen_pre, floor_pre) =
            seam_chosen_and_floor(&floor_params(pre_window), SeamSide::Pre, gap_start, gap_end, chosen_delta);
        let (chosen_post, floor_post) =
            seam_chosen_and_floor(&floor_params(post_window), SeamSide::Post, gap_start, gap_end, chosen_delta);
        let verdict = SeamResidualVerdict::from_parts_with_placement(
            &chosen_pre, &chosen_post, &floor_pre, &floor_post,
            DEFAULT_RESIDUAL_FLOOR_OK_DB, placement_slide, max_lag,
        );
        (Vec::new(), Vec::new(), verdict)
    } else {
        let pre = seam_chosen_and_floor_multichannel(
            &floor_params(pre_window), &b_ch, &selected, SeamSide::Pre, gap_start, gap_end, chosen_delta,
        );
        let post = seam_chosen_and_floor_multichannel(
            &floor_params(post_window), &b_ch, &selected, SeamSide::Post, gap_start, gap_end, chosen_delta,
        );
        let verdict = SeamResidualVerdict::from_channel_residuals(
            &pre, &post, DEFAULT_RESIDUAL_FLOOR_OK_DB, placement_slide, max_lag,
        );
        (pre, post, verdict)
    };

    ScoredPlacementMultichannel { pearson_pre, pearson_post, selected_channels: selected, verdict, pre, post }
}

pub fn score_at(fixture: &EnergySignatureFixture, start: usize) -> Scored {
    score_placement(fixture, start).scored
}

impl DisagreementRow {
    pub fn veto_flipped(&self) -> bool {
        self.pearson_patches != self.veto_patches
    }

    pub fn rescue_flipped(&self) -> bool {
        self.pearson_patches != self.rescue_patches
    }
}

pub fn run_disagreement_fixture(fixture: &EnergySignatureFixture, variant: &str) {
    let placements = [
        ("truth", fixture.true_fill_start),
        ("decoy", fixture.b_decoy_fill_start()),
        ("nominal", fixture.nominal_fill_start),
    ];
    for (label, start) in placements {
        let row = disagreement_row(&score_placement(fixture, start));
        let oracle_correct = start == fixture.true_fill_start;
        println!(
            "{},{},{},{},{:.3},{},{},{},{:.1},{},{},{},{}",
            fixture.id,
            variant,
            label,
            oracle_correct,
            row.pearson_min,
            row.pearson_tier.as_str(),
            row.pearson_patches,
            row.informative,
            row.headroom_db,
            row.veto_outcome.as_str(),
            row.rescue_outcome.as_str(),
            row.veto_flipped(),
            row.rescue_flipped(),
        );
    }
}

pub fn disagreement_at(fixture: &EnergySignatureFixture, placement: &str) -> DisagreementRow {
    let start = match placement {
        "truth" => fixture.true_fill_start,
        "decoy" => fixture.b_decoy_fill_start(),
        "nominal" => fixture.nominal_fill_start,
        other => panic!("unknown placement {other}"),
    };
    disagreement_row(&score_placement(fixture, start))
}

pub fn run_fixture(fixture: &EnergySignatureFixture, variant: &str) {
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
pub enum Variant {
    Clean,
    /// Independent low-level broadband noise on A and B (two encodes of one master).
    CodecNoise,
    /// Codec noise plus a 3.4-sample inter-encode delay on B (tests lag recovery).
    CodecNoiseShift,
}

impl Variant {
    pub fn label(self) -> &'static str {
        match self {
            Variant::Clean => "broadband_clean",
            Variant::CodecNoise => "broadband_codec_noise",
            Variant::CodecNoiseShift => "broadband_codec_noise_shift",
        }
    }
}

pub fn lcg(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((*state >> 33) as f64 / (1u64 << 31) as f64) - 1.0 // ~[-1, 1)
}

/// Broadband, **non-stationary** master: three chirps (frequencies sweep over the timeline so each
/// timestamp is spectrally distinct) plus shaped noise. Non-stationarity is what makes truth and
/// decoy distinguishable — a stationary tone looks identical at every placement.
pub fn broadband_master(total: usize, rate: u32) -> Vec<f64> {
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
pub fn interp(master: &[f64], x: f64) -> f64 {
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

pub fn build_broadband(rate: u32, variant: Variant) -> EnergySignatureFixture {
    let (noise_amp, b_shift) = match variant {
        Variant::Clean => (0.0, 0.0),
        Variant::CodecNoise => (40.0, 0.0),
        Variant::CodecNoiseShift => (40.0, 3.4), // inter-encode delay (samples)
    };
    build_broadband_with(rate, noise_amp, b_shift)
}

/// Broadband same-master fixture with explicit codec-noise amplitude and B inter-encode delay
/// (samples, may be fractional). `b_shift` models imperfect alignment between two encodes.
pub fn build_broadband_with(rate: u32, noise_amp: f64, b_shift: f64) -> EnergySignatureFixture {
    let total = (rate as usize) * 20; // 20 s
    let gap_start = (rate as usize) * 10; // gap at 10 s
    let gap_frames = rate as usize; // 1 s gap
    let gap_end = gap_start + gap_frames;
    let decoy_start = (rate as usize) * 6; // different-content decoy at 6 s

    let master = broadband_master(total, rate);
    let mut seed_a = 0x1111_2222_3333_4444u64;
    let mut seed_b = 0x5555_6666_7777_8888u64;

    let mut a = vec![0.0f32; total];
    let mut b = vec![0.0f32; total];
    for f in 0..total {
        let in_gap = (gap_start..gap_end).contains(&f);
        let a_val = if in_gap { 0.0 } else { master[f] + lcg(&mut seed_a) * noise_amp };
        let b_val = interp(&master, f as f64 - b_shift) + lcg(&mut seed_b) * noise_amp;
        a[f] = (a_val / 32767.0).clamp(-1.0, 1.0) as f32;
        b[f] = (b_val / 32767.0).clamp(-1.0, 1.0) as f32;
    }

    let bin_frames = ((0.05 * rate as f64).round() as usize).max(1);
    let structure_params = clip_sync_repair::domain::gap_structure::StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: rate as usize, // unused by direct scoring
        fill_length_slack_frames: 0,
        max_fine_adjustment_frames: 0,
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 33.0 / 32767.0,
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
