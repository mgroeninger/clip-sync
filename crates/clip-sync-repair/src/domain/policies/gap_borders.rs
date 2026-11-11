//! Gap border refinement, templates, and seam-channel selection.
//!
//! Turns a raw gap into the A-side templates the seam gate scores against: refined `[start, end)`
//! frames, per-channel border templates, and the channel subset carrying border energy. Builds on
//! `silence::is_silent_frame`, `seam_splice`'s low-energy trim, and `seam_scoring`'s energy-based
//! channel selection.
use crate::domain::pcm::{interleaved_to_channels, interleaved_to_mono};

use super::seam_scoring::{seam_score_channel_indices, template_mean_square};
use super::seam_splice::{trim_low_energy_prefix, trim_low_energy_suffix};
use super::silence::is_silent_frame;

/// Result of bracketing a B fill between matched pre/post borders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillAlignment {
    /// Frame index into the extended B buffer where the fill should start.
    pub start_frame: usize,
    /// B-derived fill length in frames (`post` border starts at `start_frame + fill_frames`).
    pub fill_frames: usize,
    pub pre_correlation: f64,
    pub post_correlation: f64,
}

/// Refined gap boundaries on A's PCM timeline (frame indices, `[start, end)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefinedGapFrames {
    pub start_frame: usize,
    pub end_frame: usize,
}

/// The signed move [`refine_gap_frames`] made to each reported edge, in A frames: refined − reported.
/// Negative on the lead = retracted (earlier), positive on the trail = extended (later).
///
/// The first step of the edge chain floor → refine → snap: measured before the edge snap and before
/// any gate boundary extension, so a refine move is never mistaken for a snap or Fit move (83/g7: a
/// 90.5 ms lead retraction that no other field recorded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeRefineFrames {
    pub start_frames: i64,
    pub end_frames: i64,
}

impl EdgeRefineFrames {
    pub fn between(
        reported_start_frame: usize,
        reported_end_frame: usize,
        refined: RefinedGapFrames,
    ) -> Self {
        Self {
            start_frames: refined.start_frame as i64 - reported_start_frame as i64,
            end_frames: refined.end_frame as i64 - reported_end_frame as i64,
        }
    }
}

/// How far [`refine_gap_frames`] may walk a reported edge against A's decoded PCM (seconds).
///
/// One budget for every caller: production `PatchAudio`, fingerprint
/// `FingerprintConfig::from_request` / `Default`, and the fixture oracles that mirror the
/// patch path. Not a CLI knob.
pub const GAP_EDGE_REFINE_SECS: f64 = 0.75;

fn silent_run(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    run_frames: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> bool {
    (start_frame..start_frame + run_frames).all(|frame| {
        is_silent_frame(
            samples,
            channels,
            frame,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    })
}

/// Tighten a reported gap against A's decoded PCM.
///
/// - Retracts `start` through leading silence before the reported boundary (scanner block
///   quantization can start the run slightly late).
/// - Advances `start` past leading non-silent frames (scanner started the run too early).
/// - Extends `end` through trailing silence (scanner closed the run too early).
pub fn refine_gap_frames(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
    max_refine_frames: usize,
) -> RefinedGapFrames {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let mut start = start_frame.min(total_frames);
    let mut end = end_frame.min(total_frames);
    if start >= end {
        return RefinedGapFrames {
            start_frame: start,
            end_frame: end,
        };
    }

    let mut budget = max_refine_frames;
    while start > 0
        && budget > 0
        && is_silent_frame(
            samples,
            channels,
            start - 1,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        start -= 1;
        budget -= 1;
    }

    // Peel at most `max_refine_frames` of leading non-silent audio before the reported gap.
    // Cap at `start_frame + max_refine` so a noisy dropout interior cannot push `start` all the
    // way to `end_frame` (block-quantized gaps are typically misaligned by <250 ms).
    let max_start = (start_frame + max_refine_frames).min(end_frame);
    let confirm_frames = (max_refine_frames / 15).clamp(4, 4096);
    while start + confirm_frames <= max_start && budget > 0 {
        if silent_run(
            samples,
            channels,
            start,
            confirm_frames,
            silence_peak_fraction,
            absolute_rms_floor,
        ) {
            break;
        }
        start += 1;
        budget -= 1;
    }

    budget = max_refine_frames;
    while end < total_frames
        && budget > 0
        && is_silent_frame(
            samples,
            channels,
            end,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        end += 1;
        budget -= 1;
    }

    RefinedGapFrames {
        start_frame: start,
        end_frame: end.max(start),
    }
}

/// Level-bin width for [`snap_gap_edges_to_onset`] (ms).
const EDGE_SNAP_BIN_MS: f64 = 2.0;
/// A fall into the dropout (rise out of it) at least this large, sustained to the edge, is its onset.
const EDGE_SNAP_FALL_DB: f64 = 15.0;
/// The fall is measured from the loudest of this many bins before it, so a fall straddling a bin edge
/// still reads as one step. Onsets this many bins apart or closer are one fall.
const EDGE_SNAP_REF_BINS: usize = 2;
/// Level of an all-zero bin, so digital silence orders below everything without `-inf`.
const EDGE_SNAP_ZERO_DB: f64 = -140.0;
/// "Digital zero" for the snap's dropout test: every channel within 2.5 LSB at 16 bits (about
/// −82 dBFS). AAC dropouts decode to ±1 LSB at 16 bits; quiet programme sits far above this.
pub const EDGE_SNAP_ZERO_AMP: f32 = 2.5 / 32768.0;
/// The digital-zero run must start within this far into the gap. On the corpus the soft band's
/// codec tail reaches zero at most 29 ms past the refined edge.
pub const EDGE_SNAP_ZERO_SEARCH_MS: f64 = 85.0;
/// Shortest run of digital-zero frames that counts as the dropout.
pub const EDGE_SNAP_ZERO_RUN_MS: f64 = 5.0;
/// How far outward of the refined edge the onset search looks (the census window). Wider than any
/// cap, so a band longer than the cap still shows its onset and reads as a cap refusal, not as a
/// hard edge. The cap limits only the move.
pub const EDGE_SNAP_OBSERVE_MS: f64 = 200.0;
/// Default edge snap cap (ms). On since 2026-11-11 (blind listen: 7 snapped better / 0 current /
/// 11 same). The cap is load-bearing: 58/g2's 107.5 ms trail band must stay a cap refusal.
pub const EDGE_SNAP_DEFAULT_MAX_MS: f64 = 40.0;

/// How far [`snap_gap_edges_to_onset`] may move an edge past the silence floor, and the
/// digital-zero test it applies first.
///
/// `max_frames == 0` is off. The cap keeps quiet programme that runs up to a dropout (room tone
/// after a word) from being handed to B. The zero test cannot catch that case. See
/// `docs/dev/TEMP-soft-edge-census-plan.md` § 4 Phase 4 and § 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeSnap {
    pub max_frames: usize,
    pub bin_frames: usize,
    pub zero_search_frames: usize,
    pub zero_run_frames: usize,
    /// Onset search window outward of the refined edge ([`EDGE_SNAP_OBSERVE_MS`]).
    pub observe_frames: usize,
}

impl EdgeSnap {
    pub const OFF: Self = Self {
        max_frames: 0,
        bin_frames: 0,
        zero_search_frames: 0,
        zero_run_frames: 0,
        observe_frames: 0,
    };

    /// `max_ms <= 0` is [`EdgeSnap::OFF`].
    #[must_use]
    pub fn from_ms(max_ms: f64, sample_rate: f64) -> Self {
        if max_ms.is_nan() || max_ms <= 0.0 {
            return Self::OFF;
        }
        let frames = |ms: f64| (ms / 1000.0 * sample_rate).round() as usize;
        Self {
            max_frames: frames(max_ms),
            bin_frames: frames(EDGE_SNAP_BIN_MS).max(1),
            zero_search_frames: frames(EDGE_SNAP_ZERO_SEARCH_MS),
            zero_run_frames: frames(EDGE_SNAP_ZERO_RUN_MS).max(1),
            observe_frames: frames(EDGE_SNAP_OBSERVE_MS),
        }
    }

    #[must_use]
    pub fn is_off(self) -> bool {
        self.max_frames == 0
    }

    /// Frames the onset search spans outward of the edge: the observation window, or the cap plus
    /// the reference bins when a cap is set wider than the window.
    fn search_frames(self) -> usize {
        self.observe_frames
            .max(self.max_frames + EDGE_SNAP_REF_BINS * self.bin_frames.max(1))
    }
}

/// What the edge snap did at one edge (`docs/dev/TEMP-soft-edge-census-plan.md` step 4). The
/// refusals follow the code's order: cap first, then the zero test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeSnapOutcome {
    /// The edge moved to the onset.
    Snapped,
    /// No sustained 15 dB fall within the observation window: nothing to snap.
    Hard,
    /// An onset was found farther out than the cap.
    RefusedCap,
    /// An onset within the cap, but the gap does not reach digital zero beside the edge.
    RefusedNoZero,
    /// Lead only: refinement advanced the start past the reported edge, so the snap never ran.
    NotAttemptedAdvanced,
}

impl EdgeSnapOutcome {
    /// Every outcome, in report order.
    pub const ALL: [Self; 5] = [
        Self::Snapped,
        Self::Hard,
        Self::RefusedCap,
        Self::RefusedNoZero,
        Self::NotAttemptedAdvanced,
    ];

    /// Stable label for human output; matches the serde representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Snapped => "snapped",
            Self::Hard => "hard",
            Self::RefusedCap => "refused_cap",
            Self::RefusedNoZero => "refused_no_zero",
            Self::NotAttemptedAdvanced => "not_attempted_advanced",
        }
    }
}

/// The edge chain floor → refine → snap at one edge, in A frames. Record-only.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct EdgeSnapEdge {
    pub outcome: EdgeSnapOutcome,
    /// The signed refine step at this edge (refined − reported; negative on the lead = retracted
    /// earlier). The same value as `PatchPlacement::refine_*_frames`, carried here so a skipped gap
    /// records it too.
    pub refine_frames: i64,
    /// Onset distance outward of the refined (unsnapped) edge. Also on refusals; absent on `hard`
    /// and `not_attempted_advanced`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onset_frames: Option<usize>,
    /// How far the snap moved this edge outward; 0 unless `snapped`.
    pub moved_frames: usize,
    /// Whether the gap reaches digital zero beside this edge; present on every edge with an onset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reaches_zero: Option<bool>,
    /// Onset to the first frame of the digital-zero run; present when `reaches_zero`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onset_to_zero_frames: Option<usize>,
    /// Peak level (dBFS, loudest channel) of the band between the onset and the refined edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band_peak_dbfs: Option<f64>,
    /// The fall at the onset: the programme reference (loudest of the two 2 ms bins outward of it)
    /// minus `band_peak_dbfs`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_db: Option<f64>,
    /// Only on `refused_no_zero`: the quietest 2 ms bin (peak dBFS) inside the zero search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_floor_dbfs: Option<f64>,
}

impl EdgeSnapEdge {
    fn without_onset(outcome: EdgeSnapOutcome, refine_frames: i64) -> Self {
        Self {
            outcome,
            refine_frames,
            onset_frames: None,
            moved_frames: 0,
            reaches_zero: None,
            onset_to_zero_frames: None,
            band_peak_dbfs: None,
            step_db: None,
            gap_floor_dbfs: None,
        }
    }
}

/// Both edges' snap records for one gap ([`edge_snap_report`]).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct EdgeSnapReport {
    pub lead: EdgeSnapEdge,
    pub trail: EdgeSnapEdge,
}

impl EdgeSnapReport {
    /// `refined` with each edge moved by its `moved_frames`.
    #[must_use]
    pub fn apply(&self, refined: RefinedGapFrames) -> RefinedGapFrames {
        RefinedGapFrames {
            start_frame: refined.start_frame - self.lead.moved_frames,
            end_frame: refined.end_frame + self.trail.moved_frames,
        }
    }
}

fn frame_peak(samples: &[f32], channels: usize, frame: usize) -> f64 {
    samples[frame * channels..(frame + 1) * channels]
        .iter()
        .fold(0.0f64, |m, &s| m.max(f64::from(s.abs())))
}

fn range_peak_db(samples: &[f32], channels: usize, lo: usize, hi: usize) -> f64 {
    level_db(
        (lo..hi)
            .map(|f| frame_peak(samples, channels, f))
            .fold(0.0, f64::max),
    )
}

fn level_db(peak: f64) -> f64 {
    if peak <= 0.0 {
        EDGE_SNAP_ZERO_DB
    } else {
        (20.0 * peak.log10()).max(EDGE_SNAP_ZERO_DB)
    }
}

/// Frames into the gap `[start, end)` from the edge being snapped (`start` for the lead, `end` for
/// the trail) to the first frame of a run of `snap.zero_run_frames` digital-zero frames (every
/// channel within [`EDGE_SNAP_ZERO_AMP`]) that starts within `snap.zero_search_frames`. `None` when
/// there is no such run: the gap does not reach digital zero beside this edge.
fn zero_run_offset(
    samples: &[f32],
    channels: usize,
    (start, end): (usize, usize),
    lead: bool,
    snap: EdgeSnap,
) -> Option<usize> {
    let reach = zero_search_reach((start, end), snap);
    let mut run = 0;
    for k in 0..reach {
        let f = if lead { start + k } else { end - 1 - k };
        if samples[f * channels..(f + 1) * channels]
            .iter()
            .all(|s| s.abs() <= EDGE_SNAP_ZERO_AMP)
        {
            run += 1;
            if run >= snap.zero_run_frames {
                return Some(k + 1 - run);
            }
        } else {
            run = 0;
        }
    }
    None
}

fn zero_search_reach((start, end): (usize, usize), snap: EdgeSnap) -> usize {
    (snap.zero_search_frames + snap.zero_run_frames).min(end - start)
}

/// The quietest 2 ms bin (peak dBFS) the zero search walked, from the edge into the gap.
fn zero_search_floor_db(
    samples: &[f32],
    channels: usize,
    (start, end): (usize, usize),
    lead: bool,
    snap: EdgeSnap,
) -> f64 {
    let reach = zero_search_reach((start, end), snap);
    let bin = snap.bin_frames.max(1).min(reach.max(1));
    (0..reach.div_ceil(bin))
        .map(|k| {
            let near = k * bin;
            let far = (near + bin).min(reach);
            if lead {
                range_peak_db(samples, channels, start + near, start + far)
            } else {
                range_peak_db(samples, channels, end - far, end - near)
            }
        })
        .fold(f64::INFINITY, f64::min)
        .min(0.0)
}

/// Per-bin peak dBFS (max across channels), `nbins` chronological bins of `bin` frames from `lo`.
fn snap_bin_levels(
    samples: &[f32],
    channels: usize,
    lo: usize,
    nbins: usize,
    bin: usize,
) -> Vec<f64> {
    (0..nbins)
        .map(|k| {
            let s = lo + k * bin;
            range_peak_db(samples, channels, s, s + bin)
        })
        .collect()
}

/// A candidate onset: its distance outward of the edge, and the programme reference level the fall
/// was measured from.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Onset {
    frames: usize,
    reference_db: f64,
}

/// Collapse onsets closer than `merge` frames into one, represented by its outermost member — two
/// bins of one decay can both clear the fall by a fraction of a dB. Returns them ascending, nearest
/// first.
fn merge_onsets(mut outward: Vec<Onset>, merge: usize) -> Vec<Onset> {
    outward.sort_unstable_by_key(|o| o.frames);
    let mut out: Vec<Onset> = Vec::new();
    for o in outward {
        match out.last_mut() {
            Some(last) if o.frames - last.frames <= merge => *last = o,
            _ => out.push(o),
        }
    }
    out
}

/// The nearest merged onset.
fn nearest_onset(outward: Vec<Onset>, bin: usize) -> Option<Onset> {
    merge_onsets(outward, EDGE_SNAP_REF_BINS * bin)
        .into_iter()
        .next()
}

/// Lead onset: back from `edge` to where the sustained fall into the dropout begins.
fn lead_onset(samples: &[f32], channels: usize, edge: usize, snap: EdgeSnap) -> Option<Onset> {
    let bin = snap.bin_frames.max(1);
    let nbins = snap.search_frames().div_ceil(bin).min(edge / bin);
    let lo = edge - nbins * bin;
    let l = snap_bin_levels(samples, channels, lo, nbins, bin);
    let mut suffix_max = vec![f64::NEG_INFINITY; nbins + 1];
    for i in (0..nbins).rev() {
        suffix_max[i] = suffix_max[i + 1].max(l[i]);
    }
    let onsets = (1..nbins)
        .filter_map(|i| {
            let reference = l[i.saturating_sub(EDGE_SNAP_REF_BINS)..i]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            if suffix_max[i] > reference - EDGE_SNAP_FALL_DB {
                return None;
            }
            // Refine to the sample: the frame after the last one in the reference bins still above
            // the fall threshold. Every later frame up to the edge is below it by construction.
            let thr = 10f64.powf((reference - EDGE_SNAP_FALL_DB) / 20.0);
            let s = lo + i.saturating_sub(EDGE_SNAP_REF_BINS) * bin;
            let f = (s..lo + i * bin)
                .rev()
                .find(|&f| frame_peak(samples, channels, f) > thr)
                .map_or(s, |f| f + 1);
            Some(Onset {
                frames: edge - f,
                reference_db: reference,
            })
        })
        .collect();
    nearest_onset(onsets, bin)
}

/// Trail onset: forward from `edge` to where programme rises out of the dropout.
fn trail_onset(samples: &[f32], channels: usize, edge: usize, snap: EdgeSnap) -> Option<Onset> {
    let bin = snap.bin_frames.max(1);
    let total = samples.len() / channels;
    let nbins = snap.search_frames().div_ceil(bin).min((total - edge) / bin);
    let l = snap_bin_levels(samples, channels, edge, nbins, bin);
    let mut prefix_max = f64::NEG_INFINITY;
    let mut onsets = Vec::new();
    for j in 0..nbins {
        let level = l[j..(j + EDGE_SNAP_REF_BINS).min(nbins)]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        if j >= 1 && prefix_max <= level - EDGE_SNAP_FALL_DB {
            let thr = 10f64.powf((level - EDGE_SNAP_FALL_DB) / 20.0);
            let s = edge + j * bin;
            let f = (s..edge + (j + EDGE_SNAP_REF_BINS).min(nbins) * bin)
                .find(|&f| frame_peak(samples, channels, f) > thr)
                .unwrap_or(s);
            onsets.push(Onset {
                frames: f - edge,
                reference_db: level,
            });
        }
        prefix_max = prefix_max.max(l[j]);
    }
    nearest_onset(onsets, bin)
}

/// Read one edge: find its onset, then apply the cap and the zero test in that order.
fn snap_edge(
    samples: &[f32],
    channels: usize,
    gap: (usize, usize),
    lead: bool,
    snap: EdgeSnap,
    refine_frames: i64,
) -> EdgeSnapEdge {
    let edge = if lead { gap.0 } else { gap.1 };
    let onset = if lead {
        lead_onset(samples, channels, edge, snap)
    } else {
        trail_onset(samples, channels, edge, snap)
    };
    let Some(onset) = onset.filter(|o| o.frames > 0) else {
        return EdgeSnapEdge::without_onset(EdgeSnapOutcome::Hard, refine_frames);
    };
    let zero = zero_run_offset(samples, channels, gap, lead, snap);
    let band_peak = if lead {
        range_peak_db(samples, channels, edge - onset.frames, edge)
    } else {
        range_peak_db(samples, channels, edge, edge + onset.frames)
    };
    let outcome = if onset.frames > snap.max_frames {
        EdgeSnapOutcome::RefusedCap
    } else if zero.is_none() {
        EdgeSnapOutcome::RefusedNoZero
    } else {
        EdgeSnapOutcome::Snapped
    };
    EdgeSnapEdge {
        outcome,
        refine_frames,
        onset_frames: Some(onset.frames),
        moved_frames: if outcome == EdgeSnapOutcome::Snapped {
            onset.frames
        } else {
            0
        },
        reaches_zero: Some(zero.is_some()),
        onset_to_zero_frames: zero.map(|z| onset.frames + z),
        band_peak_dbfs: Some(band_peak),
        step_db: Some(onset.reference_db - band_peak),
        gap_floor_dbfs: (outcome == EdgeSnapOutcome::RefusedNoZero)
            .then(|| zero_search_floor_db(samples, channels, gap, lead, snap)),
    }
}

/// Read both edges of a refined gap for the edge snap, and say what the snap does at each.
/// `None` when the snap is off or the refined gap is empty or out of range.
///
/// A dropout can decay through a soft band above the silence floor before it reaches it (50/g7:
/// −22 → −30 → −45 dBFS over 26 ms). [`refine_gap_frames`] stops at the floor, so the band stays raw
/// A beside the fill. This continues the walk: in 2 ms peak bins, the **nearest** fall of at least
/// 15 dB below the loudest of the two bins before it, sustained into the gap, refined to the sample
/// (the trail edge mirrors it). The bins end at the edge, as the census instrument's end at the scan
/// boundary, so a band that ends in a hard step to silence still snaps. The search spans a fixed
/// observation window ([`EDGE_SNAP_OBSERVE_MS`]), so a band longer than the cap still reports its
/// onset (as a cap refusal).
///
/// Two guards, and the edge stays where refinement left it unless both pass:
/// - **The onset is within `snap.max_frames`.** The zero test cannot tell quiet programme that runs
///   up to the cut (58/g2 trail: 107 ms flat at −57 dBFS, then zero) from a soft band, so the cap
///   still guards it. The step test cannot either: room tone after a word starts with a step too.
/// - **The gap reaches digital zero** beside the edge ([`zero_run_offset`]). The band is then
///   codec noise smeared after a hard cut, and the onset is the cut (frame-grid census,
///   `docs/dev/TEMP-soft-edge-census-plan.md` § 5). A gap whose interior is quiet programme rather
///   than a dropout never snaps.
///
/// The lead edge is not snapped when refinement *advanced* it past `reported_start_frame`: the
/// scanner opened that run early, so the frames before it are programme, not a dropout's decay.
///
/// Its own rule, not the scanner's silence predicate — the census instrument (`edge-onset`) holds
/// the reference reading and shares no code with this.
#[must_use]
pub fn edge_snap_report(
    samples: &[f32],
    channels: usize,
    (reported_start_frame, reported_end_frame): (usize, usize),
    refined: RefinedGapFrames,
    snap: EdgeSnap,
) -> Option<EdgeSnapReport> {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    if snap.is_off() || refined.start_frame >= refined.end_frame || refined.end_frame > total_frames
    {
        return None;
    }
    let refine = EdgeRefineFrames::between(reported_start_frame, reported_end_frame, refined);
    let gap = (refined.start_frame, refined.end_frame);
    let lead = if refined.start_frame <= reported_start_frame {
        snap_edge(samples, channels, gap, true, snap, refine.start_frames)
    } else {
        EdgeSnapEdge::without_onset(EdgeSnapOutcome::NotAttemptedAdvanced, refine.start_frames)
    };
    let trail = snap_edge(samples, channels, gap, false, snap, refine.end_frames);
    Some(EdgeSnapReport { lead, trail })
}

/// Move refined gap edges outward from the silence floor to A's dropout onset: [`edge_snap_report`]
/// applied. `refined` unchanged when the snap is off.
#[must_use]
pub fn snap_gap_edges_to_onset(
    samples: &[f32],
    channels: usize,
    reported_start_frame: usize,
    refined: RefinedGapFrames,
    snap: EdgeSnap,
) -> RefinedGapFrames {
    // The trail record's refine step is not read here, so the reported end is immaterial.
    edge_snap_report(
        samples,
        channels,
        (reported_start_frame, refined.end_frame),
        refined,
        snap,
    )
    .map_or(refined, |report| report.apply(refined))
}

/// [`refine_gap_frames`], then [`snap_gap_edges_to_onset`]: the one edge walk every production caller
/// (patch characterize, injected-time detection, fingerprint measure) runs, so the patch and the
/// fingerprint geometry cannot drift apart.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn refine_and_snap_gap_frames(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
    max_refine_frames: usize,
    snap: EdgeSnap,
) -> RefinedGapFrames {
    refine_and_snap_gap_edges(
        samples,
        channels,
        start_frame,
        end_frame,
        silence_peak_fraction,
        absolute_rms_floor,
        max_refine_frames,
        snap,
    )
    .refined
}

/// Both ends of [`refine_and_snap_gap_frames`]'s walk: the refined edges before the snap, and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapEdgeChain {
    /// [`refine_gap_frames`] alone — where the abut verdict is read.
    pub unsnapped: RefinedGapFrames,
    /// After the edge snap — what every placement step measures from. Equal to `unsnapped` when the
    /// snap is off or moved nothing.
    pub refined: RefinedGapFrames,
}

impl GapEdgeChain {
    /// How far the snap moved the lead (earlier) and the trail (later), in frames.
    pub fn snap_moves(&self) -> (usize, usize) {
        (
            self.unsnapped
                .start_frame
                .saturating_sub(self.refined.start_frame),
            self.refined
                .end_frame
                .saturating_sub(self.unsnapped.end_frame),
        )
    }
}

/// [`refine_and_snap_gap_frames`], keeping the unsnapped edges too.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn refine_and_snap_gap_edges(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
    max_refine_frames: usize,
    snap: EdgeSnap,
) -> GapEdgeChain {
    let unsnapped = refine_gap_frames(
        samples,
        channels,
        start_frame,
        end_frame,
        silence_peak_fraction,
        absolute_rms_floor,
        max_refine_frames,
    );
    GapEdgeChain {
        unsnapped,
        refined: snap_gap_edges_to_onset(samples, channels, start_frame, unsnapped, snap),
    }
}

struct GapBorderFrameRange {
    pre_start: usize,
    pre_end: usize,
    post_start: usize,
    post_end: usize,
}

/// Gap bounds, template sizing, and silence thresholds shared by the border-template builders.
#[derive(Clone, Copy)]
pub struct GapBorderSpec {
    pub gap_start_frame: usize,
    pub gap_end_frame: usize,
    pub border_frames: usize,
    pub border_standoff_frames: usize,
    pub silence_peak_fraction: f32,
    pub absolute_rms_floor: f32,
}

fn gap_border_frame_range(
    samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> GapBorderFrameRange {
    let GapBorderSpec {
        gap_start_frame,
        gap_end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_rms_floor,
    } = *spec;
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;

    let pre_start = gap_start_frame.saturating_sub(border_frames);
    let mut pre_end = gap_start_frame.min(total_frames);
    while pre_end > pre_start
        && is_silent_frame(
            samples,
            channels,
            pre_end - 1,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        pre_end -= 1;
    }

    let post_end = (gap_end_frame + border_frames).min(total_frames);
    let mut post_start = gap_end_frame.min(total_frames);
    while post_start < post_end
        && is_silent_frame(
            samples,
            channels,
            post_start,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        post_start += 1;
    }

    // Skip audio immediately adjacent to the dropout (A-side templates only).
    if border_standoff_frames > 0 {
        pre_end = pre_end
            .saturating_sub(border_standoff_frames)
            .max(pre_start);
        post_start = (post_start + border_standoff_frames).min(post_end);
    }

    GapBorderFrameRange {
        pre_start,
        pre_end,
        post_start,
        post_end,
    }
}

/// Build mono border templates for seam correlation, skipping silence adjacent to the gap.
pub fn border_templates_for_gap(
    samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> (Vec<f64>, Vec<f64>) {
    let channels = channels.max(1);
    let range = gap_border_frame_range(samples, channels, spec);

    let mut pre_mono = if range.pre_end > range.pre_start {
        interleaved_to_mono(
            &samples[range.pre_start * channels..range.pre_end * channels],
            channels,
        )
    } else {
        Vec::new()
    };
    let mut post_mono = if range.post_end > range.post_start {
        interleaved_to_mono(
            &samples[range.post_start * channels..range.post_end * channels],
            channels,
        )
    } else {
        Vec::new()
    };

    pre_mono = trim_low_energy_suffix(&pre_mono);
    post_mono = trim_low_energy_prefix(&post_mono);

    (pre_mono, post_mono)
}

/// How far each template's **gap-facing edge** sits *away* from its gap edge, in frames: the silence
/// walk-off, plus `border_standoff_frames`, plus the 12 %-of-peak trim
/// [`border_templates_for_gap`] applied. Pass the lengths of the templates it returned (mono, or one
/// channel's — the trim is per-buffer, so the displacement is too).
///
/// Both values are **≥ 0** — every step moves the edge outward, never toward the dropout. That
/// matters to anything that correlates a template against a B window: `gap_fingerprint::measure::lag_pair`
/// shifts its B anchor by this same amount (map coordinates —
/// `docs/dev/TEMP-lag-decision-map-coordinates-plan.md` § Change) so a sweep anchored on the *gap*
/// edge instead of the *template* edge still reads `frac_lag_ms ≈ 0` at true registration. A caller
/// that anchors on the raw gap edge without this shift reads `−pre` on the pre shoulder and `+post`
/// on the post one. See `docs/dev/archive/TEMP-fill-placement-registration-plan.md` § "Step 5 result".
pub fn border_edge_displacement_frames(
    samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
    pre_template_len: usize,
    post_template_len: usize,
) -> (usize, usize) {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let range = gap_border_frame_range(samples, channels, spec);

    let pre_untrimmed = range.pre_end.saturating_sub(range.pre_start);
    let post_untrimmed = range.post_end.saturating_sub(range.post_start);
    let pre = spec
        .gap_start_frame
        .min(total_frames)
        .saturating_sub(range.pre_end)
        + pre_untrimmed.saturating_sub(pre_template_len);
    let post = range
        .post_start
        .saturating_sub(spec.gap_end_frame.min(total_frames))
        + post_untrimmed.saturating_sub(post_template_len);
    (pre, post)
}

/// Count of samples above 12% of border peak (matches trim_low_energy threshold).
pub(crate) fn border_active_extent_frames(border: &[f64]) -> usize {
    if border.is_empty() {
        return 0;
    }
    let peak = border.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return 0;
    }
    let floor = peak * 0.12;
    border.iter().filter(|s| s.abs() >= floor).count()
}

/// Adaptive seam window: cap at active extent, bounded by min/max seam frames.
pub(crate) fn adaptive_seam_window_frames(
    border_len: usize,
    min_frames: usize,
    max_frames: usize,
    active_extent_frames: usize,
) -> usize {
    if border_len == 0 {
        return 0;
    }
    let min_frames = min_frames.max(1);
    let max_frames = max_frames.max(min_frames);
    let target = active_extent_frames.max(min_frames).min(max_frames);
    target.min(border_len).max(1)
}

/// Per-channel border templates (same frame ranges as [`border_templates_for_gap`]).
pub fn border_templates_per_channel_for_gap(
    samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let channels = channels.max(1);
    let range = gap_border_frame_range(samples, channels, spec);

    let mut pre_ch = if range.pre_end > range.pre_start {
        interleaved_to_channels(
            &samples[range.pre_start * channels..range.pre_end * channels],
            channels,
        )
    } else {
        vec![Vec::new(); channels]
    };
    let mut post_ch = if range.post_end > range.post_start {
        interleaved_to_channels(
            &samples[range.post_start * channels..range.post_end * channels],
            channels,
        )
    } else {
        vec![Vec::new(); channels]
    };

    for ch in &mut pre_ch {
        *ch = trim_low_energy_suffix(ch);
    }
    for ch in &mut post_ch {
        *ch = trim_low_energy_prefix(ch);
    }

    (pre_ch, post_ch)
}

/// The **loudest** energy-passing seam channel — argmax of per-channel border energy (max of pre/post
/// mean-square), or `None` when every channel is near-silent. Use this for a single-channel
/// *representative* (e.g. the lag/probe diagnostic) instead of `selected_seam_channels().first()`, which
/// returns the **lowest-index** passing channel — index order, an arbitrary pick that lands on L over a
/// louder C in a center-dominant mix. The gate's multichannel decision still uses the full
/// `selected_seam_channels` set; this only changes which single channel a diagnostic follows.
pub fn loudest_seam_channel(
    a_samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> Option<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    let n = a_pre_ch.len().min(a_post_ch.len());
    (0..n)
        .map(|ch| {
            (
                ch,
                template_mean_square(&a_pre_ch[ch]).max(template_mean_square(&a_post_ch[ch])),
            )
        })
        .filter(|&(_, e)| e > f64::EPSILON)
        .max_by(|(_, x), (_, y)| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(ch, _)| ch)
}

/// Energy-selected seam channels for a gap, computed from the same per-channel border templates
/// Pearson scores — the single entry point so residual/floor measurement follows the *same*
/// channels as seam scoring (see `seam-scoring.md`). Returns empty when every channel is
/// near-silent, so the caller falls back to the mono downmix.
pub fn selected_seam_channels(
    a_samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> Vec<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    seam_score_channel_indices(&a_pre_ch, &a_post_ch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policies::{seam_channel_diagnostics, SeamPlacement, SeamTemplates};

    const SNAP_RATE: f64 = 48_000.0;
    /// −60 dBFS, the production floor (`absolute_silence_rms` default).
    const SNAP_FLOOR: f32 = 0.001;

    fn ms_frames(ms: f64) -> usize {
        (ms / 1000.0 * SNAP_RATE).round() as usize
    }

    fn dbfs(db: f64) -> f32 {
        10f64.powf(db / 20.0) as f32
    }

    /// `ms` of a constant stereo level; the right channel 6 dB under the left.
    fn level(out: &mut Vec<f32>, db: f64, ms: f64) {
        for _ in 0..ms_frames(ms) {
            out.extend([dbfs(db), dbfs(db - 6.0)]);
        }
    }

    /// A gradual decay (or rise, with `db_per_ms > 0`) from `from_db`: never a 15 dB step in two bins.
    fn ramp(out: &mut Vec<f32>, from_db: f64, db_per_ms: f64, ms: f64) {
        let n = ms_frames(ms);
        for k in 0..n {
            let db = from_db + db_per_ms * (k as f64 * 1000.0 / SNAP_RATE);
            out.extend([dbfs(db), dbfs(db - 6.0)]);
        }
    }

    fn silence(out: &mut Vec<f32>, ms: f64) {
        out.extend(std::iter::repeat_n(0.0, 2 * ms_frames(ms)));
    }

    fn snap_40() -> EdgeSnap {
        EdgeSnap::from_ms(40.0, SNAP_RATE)
    }

    fn refine_snap(samples: &[f32], start: usize, end: usize, snap: EdgeSnap) -> RefinedGapFrames {
        let max_refine = (GAP_EDGE_REFINE_SECS * SNAP_RATE).round() as usize;
        refine_and_snap_gap_frames(samples, 2, start, end, 0.01, SNAP_FLOOR, max_refine, snap)
    }

    fn refine_only(samples: &[f32], start: usize, end: usize) -> RefinedGapFrames {
        refine_snap(samples, start, end, EdgeSnap::OFF)
    }

    /// Programme, a soft band of `band_ms` at −35 dBFS, a gradual decay through the floor, silence;
    /// then the mirror image back into programme. Returns samples, band start, and band end (the
    /// trail band's last frame + 1), plus the scanner's (late) reported edges.
    fn soft_edged(band_ms: f64) -> (Vec<f32>, usize, usize, usize, usize) {
        let mut s = Vec::new();
        level(&mut s, -10.0, 300.0);
        let band_start = s.len() / 2;
        level(&mut s, -35.0, band_ms);
        ramp(&mut s, -35.0, -3.25, 20.0);
        silence(&mut s, 500.0);
        ramp(&mut s, -100.0, 3.25, 20.0);
        level(&mut s, -35.0, band_ms);
        let band_end = s.len() / 2;
        level(&mut s, -10.0, 300.0);
        // The scanner reports a block-quantized run: late on the lead, early on the trail.
        let reported_start = band_start + ms_frames(band_ms + 40.0);
        let reported_end = band_end - ms_frames(band_ms + 40.0);
        (s, band_start, band_end, reported_start, reported_end)
    }

    #[test]
    fn edge_snap_moves_both_soft_edges_to_the_onset() {
        let (s, band_start, band_end, rs, re) = soft_edged(20.0);
        let refined = refine_only(&s, rs, re);
        // Refinement stops at the floor, inside the decay: the band stays outside the gap.
        assert!(refined.start_frame > band_start + ms_frames(20.0));
        assert!(refined.end_frame < band_end - ms_frames(20.0));
        let snapped = refine_snap(&s, rs, re, snap_40());
        assert_eq!(
            snapped.start_frame, band_start,
            "lead snaps to the band's first frame"
        );
        assert_eq!(
            snapped.end_frame, band_end,
            "trail snaps past the band's last frame"
        );
    }

    #[test]
    fn edge_snap_refuses_a_band_beyond_its_cap() {
        // The gap is digital zero, so only the cap refuses this band (58/g2 trail's shape).
        let (s, _, _, rs, re) = soft_edged(60.0);
        assert_eq!(refine_snap(&s, rs, re, snap_40()), refine_only(&s, rs, re));
    }

    #[test]
    fn edge_snap_refuses_a_band_beside_a_gap_that_is_not_digital_zero() {
        // A quiet passage, not a dropout: the gap sits at −70 dBFS (below the floor, so the scanner
        // and refinement call it silent) and never reaches digital zero.
        let (mut s, _, _, rs, re) = soft_edged(20.0);
        let quiet = dbfs(-70.0);
        for x in &mut s {
            if x.abs() < quiet {
                *x = quiet;
            }
        }
        let refined = refine_only(&s, rs, re);
        assert!(refined.end_frame > refined.start_frame);
        assert_eq!(refine_snap(&s, rs, re, snap_40()), refined);
    }

    #[test]
    fn edge_snap_needs_the_zero_run_near_the_edge() {
        // Digital zero exists, but only beyond the search reach: the gap's first 200 ms hold
        // −70 dBFS, so neither edge sees zero within 85 ms.
        let (mut s, _, _, rs, re) = soft_edged(20.0);
        let refined = refine_only(&s, rs, re);
        let quiet = dbfs(-70.0);
        let reach = ms_frames(200.0);
        for f in (refined.start_frame..refined.start_frame + reach)
            .chain(refined.end_frame - reach..refined.end_frame)
        {
            for c in 0..2 {
                if s[2 * f + c].abs() < quiet {
                    s[2 * f + c] = quiet;
                }
            }
        }
        assert_eq!(refine_snap(&s, rs, re, snap_40()), refine_only(&s, rs, re));
    }

    #[test]
    fn edge_snap_is_a_no_op_on_a_hard_edge() {
        let mut s = Vec::new();
        level(&mut s, -10.0, 300.0);
        let start = s.len() / 2;
        silence(&mut s, 500.0);
        let end = s.len() / 2;
        level(&mut s, -10.0, 300.0);
        let late = start + ms_frames(30.0);
        let refined = refine_only(&s, late, end);
        assert_eq!(refined.start_frame, start);
        assert_eq!(refine_snap(&s, late, end, snap_40()), refined);
    }

    #[test]
    fn edge_snap_is_a_no_op_on_a_sub_floor_bucket_edge() {
        // Below the floor throughout: refinement already walks the band and lands on the crossing.
        let mut s = Vec::new();
        level(&mut s, -10.0, 300.0);
        let crossing = s.len() / 2;
        level(&mut s, -66.0, 26.0);
        silence(&mut s, 500.0);
        let end = s.len() / 2;
        level(&mut s, -10.0, 300.0);
        let late = crossing + ms_frames(60.0);
        let refined = refine_only(&s, late, end);
        assert_eq!(refined.start_frame, crossing);
        assert_eq!(refine_snap(&s, late, end, snap_40()), refined);
    }

    #[test]
    fn edge_snap_moves_a_band_ending_in_a_hard_step() {
        // The band steps straight to silence (36/g5's shape) rather than decaying through the
        // floor. Room tone after a word has the same shape; only the cap tells them apart.
        let mut s = Vec::new();
        level(&mut s, -10.0, 300.0);
        let band_start = s.len() / 2;
        level(&mut s, -45.0, 30.0);
        let start = s.len() / 2;
        silence(&mut s, 500.0);
        let end = s.len() / 2;
        level(&mut s, -45.0, 30.0);
        let band_end = s.len() / 2;
        level(&mut s, -10.0, 300.0);
        let late = start + ms_frames(30.0);
        let refined = refine_only(&s, late, end);
        assert_eq!((refined.start_frame, refined.end_frame), (start, end));
        let snapped = refine_snap(&s, late, end, snap_40());
        assert_eq!(
            (snapped.start_frame, snapped.end_frame),
            (band_start, band_end)
        );
    }

    #[test]
    fn edge_snap_does_not_undo_a_start_the_refinement_advanced() {
        // The scanner opened the run early, inside programme. Refinement advances the start past
        // it; the frames before the advanced start are programme, so the lead does not snap.
        let (s, band_start, _, _, re) = soft_edged(20.0);
        let early = band_start - ms_frames(100.0);
        let refined = refine_only(&s, early, re);
        assert!(refined.start_frame > early, "refinement advanced the start");
        let snapped = refine_snap(&s, early, re, snap_40());
        assert_eq!(snapped.start_frame, refined.start_frame);
    }

    fn snap_report(samples: &[f32], start: usize, end: usize, snap: EdgeSnap) -> EdgeSnapReport {
        let max_refine = (GAP_EDGE_REFINE_SECS * SNAP_RATE).round() as usize;
        let refined = refine_gap_frames(samples, 2, start, end, 0.01, SNAP_FLOOR, max_refine);
        edge_snap_report(samples, 2, (start, end), refined, snap).expect("snap on, gap non-empty")
    }

    /// The window the onset search had before step 4: the cap plus the reference bins.
    fn cap_window_only(snap: EdgeSnap) -> EdgeSnap {
        EdgeSnap {
            observe_frames: 0,
            ..snap
        }
    }

    #[test]
    fn edge_snap_report_snapped_records_the_band() {
        let (s, band_start, band_end, rs, re) = soft_edged(20.0);
        let refined = refine_only(&s, rs, re);
        let r = snap_report(&s, rs, re, snap_40());
        for (edge, onset) in [
            (r.lead, refined.start_frame - band_start),
            (r.trail, band_end - refined.end_frame),
        ] {
            assert_eq!(edge.outcome, EdgeSnapOutcome::Snapped);
            assert_eq!(edge.onset_frames, Some(onset));
            assert_eq!(edge.moved_frames, onset);
            assert_eq!(edge.reaches_zero, Some(true));
            // The zero run starts where the ramp ends: past the band and the rest of the ramp.
            let to_zero = edge.onset_to_zero_frames.expect("reaches zero");
            assert!(to_zero > onset && to_zero <= onset + ms_frames(20.0) + 1);
            let peak = edge.band_peak_dbfs.expect("band");
            assert!((peak + 35.0).abs() < 0.1, "band peak {peak}");
            let step = edge.step_db.expect("step");
            assert!((step - 25.0).abs() < 0.1, "step {step}");
            assert_eq!(edge.gap_floor_dbfs, None);
        }
        assert_eq!(r.lead.refine_frames, refined.start_frame as i64 - rs as i64);
        assert_eq!(r.trail.refine_frames, refined.end_frame as i64 - re as i64);
        assert_eq!(r.apply(refined), refine_snap(&s, rs, re, snap_40()));
    }

    #[test]
    fn edge_snap_report_hard_edge_has_no_onset() {
        let mut s = Vec::new();
        level(&mut s, -10.0, 300.0);
        let start = s.len() / 2;
        silence(&mut s, 500.0);
        let end = s.len() / 2;
        level(&mut s, -10.0, 300.0);
        let r = snap_report(&s, start + ms_frames(30.0), end, snap_40());
        for edge in [r.lead, r.trail] {
            assert_eq!(edge.outcome, EdgeSnapOutcome::Hard);
            assert_eq!(edge.onset_frames, None);
            assert_eq!(edge.moved_frames, 0);
            assert_eq!(edge.reaches_zero, None);
            assert_eq!(edge.band_peak_dbfs, None);
        }
        assert_eq!(r.lead.refine_frames, -(ms_frames(30.0) as i64));
        assert_eq!(r.trail.refine_frames, 0);
    }

    #[test]
    fn edge_snap_report_refuses_a_band_beyond_its_cap_with_its_onset() {
        let (s, band_start, band_end, rs, re) = soft_edged(60.0);
        let refined = refine_only(&s, rs, re);
        let r = snap_report(&s, rs, re, snap_40());
        for (edge, onset) in [
            (r.lead, refined.start_frame - band_start),
            (r.trail, band_end - refined.end_frame),
        ] {
            assert!(onset > snap_40().max_frames);
            assert_eq!(edge.outcome, EdgeSnapOutcome::RefusedCap);
            assert_eq!(edge.onset_frames, Some(onset));
            assert_eq!(edge.moved_frames, 0);
            // Cap first: the zero test still reads, and passes, beside a digital-zero gap.
            assert_eq!(edge.reaches_zero, Some(true));
            assert_eq!(edge.gap_floor_dbfs, None);
        }
        // Before the observation window, the search ended at the cap and saw no fall: hard.
        let old = snap_report(&s, rs, re, cap_window_only(snap_40()));
        assert_eq!(old.lead.outcome, EdgeSnapOutcome::Hard);
        assert_eq!(old.trail.outcome, EdgeSnapOutcome::Hard);
    }

    #[test]
    fn edge_snap_report_refuses_a_band_beside_quiet_programme_with_its_floor() {
        let (mut s, _, _, rs, re) = soft_edged(20.0);
        let quiet = dbfs(-70.0);
        for x in &mut s {
            if x.abs() < quiet {
                *x = quiet;
            }
        }
        let r = snap_report(&s, rs, re, snap_40());
        for edge in [r.lead, r.trail] {
            assert_eq!(edge.outcome, EdgeSnapOutcome::RefusedNoZero);
            assert!(edge.onset_frames.is_some());
            assert_eq!(edge.moved_frames, 0);
            assert_eq!(edge.reaches_zero, Some(false));
            assert_eq!(edge.onset_to_zero_frames, None);
            let floor = edge.gap_floor_dbfs.expect("floor on refused_no_zero");
            assert!((floor + 70.0).abs() < 0.1, "floor {floor}");
        }
    }

    #[test]
    fn edge_snap_report_does_not_attempt_a_lead_the_refinement_advanced() {
        let (s, band_start, _, _, re) = soft_edged(20.0);
        let early = band_start - ms_frames(100.0);
        let r = snap_report(&s, early, re, snap_40());
        assert_eq!(r.lead.outcome, EdgeSnapOutcome::NotAttemptedAdvanced);
        assert_eq!(r.lead.onset_frames, None);
        assert_eq!(r.lead.moved_frames, 0);
        assert!(r.lead.refine_frames > 0, "refinement advanced the start");
        assert_eq!(r.trail.outcome, EdgeSnapOutcome::Snapped);
    }

    #[test]
    fn edge_snap_report_is_absent_when_the_snap_is_off() {
        let (s, _, _, rs, re) = soft_edged(20.0);
        let refined = refine_only(&s, rs, re);
        assert_eq!(
            edge_snap_report(&s, 2, (rs, re), refined, EdgeSnap::OFF),
            None
        );
    }

    #[test]
    fn edge_snap_wider_window_keeps_the_nearest_onset() {
        // Two falls into the lead: programme at 0 dBFS steps to −20 dBFS 100 ms out, then to a −40
        // dBFS band 20 ms out. Only the observation window sees the far one; it is 80 ms from the
        // near one, far outside the merge chain, so the move is unchanged.
        let mut s = Vec::new();
        level(&mut s, 0.0, 300.0);
        level(&mut s, -20.0, 80.0);
        let band_start = s.len() / 2;
        level(&mut s, -40.0, 20.0);
        let start = s.len() / 2;
        silence(&mut s, 500.0);
        let end = s.len() / 2;
        level(&mut s, -10.0, 300.0);
        let late = start + ms_frames(30.0);
        let wide = snap_report(&s, late, end, snap_40());
        let narrow = snap_report(&s, late, end, cap_window_only(snap_40()));
        assert_eq!(wide.lead.outcome, EdgeSnapOutcome::Snapped);
        assert_eq!(wide.lead.moved_frames, start - band_start);
        assert_eq!(wide, narrow);
    }

    #[test]
    fn edge_snap_off_is_refine_alone() {
        let (s, _, _, rs, re) = soft_edged(20.0);
        let max_refine = (GAP_EDGE_REFINE_SECS * SNAP_RATE).round() as usize;
        assert_eq!(
            refine_only(&s, rs, re),
            refine_gap_frames(&s, 2, rs, re, 0.01, SNAP_FLOOR, max_refine)
        );
        assert!(EdgeSnap::from_ms(0.0, SNAP_RATE).is_off());
        assert!(EdgeSnap::from_ms(f64::NAN, SNAP_RATE).is_off());
    }

    #[test]
    fn edge_chain_records_a_long_refine_retraction_then_a_short_snap() {
        // 83/g7's shape: the scanner started the run 90.5 ms late, so refinement retracts the lead
        // through that silence to the floor edge, and the snap then moves only the 2.3 ms soft
        // band beyond it. The record must carry both steps, and the written edge must be
        // reported + refine - moved, the onset.
        const RETRACT_MS: f64 = 90.5;
        const BAND_MS: f64 = 2.3;
        let mut s = Vec::new();
        level(&mut s, -10.0, 300.0);
        let onset = s.len() / 2;
        level(&mut s, -35.0, BAND_MS);
        let floor_edge = s.len() / 2;
        silence(&mut s, 600.0);
        let end = s.len() / 2;
        level(&mut s, -10.0, 300.0);
        let reported_start = floor_edge + ms_frames(RETRACT_MS);

        let max_refine = (GAP_EDGE_REFINE_SECS * SNAP_RATE).round() as usize;
        let chain = refine_and_snap_gap_edges(
            &s,
            2,
            reported_start,
            end,
            0.01,
            SNAP_FLOOR,
            max_refine,
            snap_40(),
        );
        assert_eq!(
            chain.unsnapped.start_frame, floor_edge,
            "refine stops at the floor edge"
        );
        assert_eq!(
            chain.refined.start_frame, onset,
            "the snap reaches the onset"
        );
        assert_eq!(chain.snap_moves().0, floor_edge - onset);

        let r = snap_report(&s, reported_start, end, snap_40());
        assert_eq!(r.lead.outcome, EdgeSnapOutcome::Snapped);
        assert_eq!(r.lead.refine_frames, -(ms_frames(RETRACT_MS) as i64));
        assert_eq!(r.lead.moved_frames, floor_edge - onset);
        assert_eq!(r.lead.onset_frames, Some(floor_edge - onset));
        let written = reported_start as i64 + r.lead.refine_frames - r.lead.moved_frames as i64;
        assert_eq!(written, onset as i64);
        assert_eq!(r.apply(chain.unsnapped), chain.refined);
    }

    #[test]
    fn refine_gap_frames_retracts_through_leading_silence_before_reported_start() {
        let channels = 2usize;
        let loud = 8_000.0_f32 / 32767.0;
        // [loud 5][silent 10][loud 5] — reported gap starts two frames late.
        let mut samples = Vec::new();
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }
        for _ in 0..10 {
            samples.extend([0.0f32, 0.0f32]);
        }
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }

        let refined = refine_gap_frames(&samples, channels, 7, 14, 0.01, 0.0, 10);
        assert_eq!(refined.start_frame, 5);
        assert_eq!(refined.end_frame, 15);
    }

    #[test]
    fn refine_gap_frames_advances_past_leading_audio_and_extends_trailing_silence() {
        let channels = 2usize;
        let loud = 8_000.0_f32 / 32767.0;
        // [loud 5][silent 10][loud 5] — reported gap starts one frame too early and ends one frame too early.
        let mut samples = Vec::new();
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }
        for _ in 0..10 {
            samples.extend([0.0f32, 0.0f32]);
        }
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }

        let refined = refine_gap_frames(&samples, channels, 4, 14, 0.01, 0.0, 10);
        assert_eq!(refined.start_frame, 5);
        assert_eq!(refined.end_frame, 15);
    }

    #[test]
    fn refine_gap_frames_caps_start_advance_before_reported_end() {
        let channels = 1usize;
        let mut samples = Vec::new();
        // Leading audio (2 frames), low-level dropout (8 frames), trailing audio.
        let loud = 8_000.0_f32 / 32767.0;
        samples.extend([loud; 2]);
        samples.extend([3.0_f32 / 32767.0; 8]);
        samples.extend([loud; 2]);

        let refined = refine_gap_frames(&samples, channels, 2, 10, 0.01, 0.0, 20);
        assert!(
            refined.start_frame < 10,
            "start should not advance all the way to reported end"
        );
        assert_eq!(refined.end_frame, 10);
    }

    fn test_border_spec(
        gap_start_frame: usize,
        gap_end_frame: usize,
        border_frames: usize,
        border_standoff_frames: usize,
    ) -> GapBorderSpec {
        GapBorderSpec {
            gap_start_frame,
            gap_end_frame,
            border_frames,
            border_standoff_frames,
            silence_peak_fraction: 0.01,
            absolute_rms_floor: 0.0,
        }
    }

    #[test]
    fn border_templates_trim_quiet_fade_before_dropout() {
        let channels = 1usize;
        let loud = 8_000.0_f32 / 32767.0;
        let low = 200.0_f32 / 32767.0;
        let mut samples = Vec::new();
        samples.extend(vec![loud; 8]);
        samples.extend(vec![low; 4]);
        samples.extend(vec![0.0f32; 4]);
        samples.extend(vec![low; 4]);
        samples.extend(vec![loud; 8]);

        let (pre, post) =
            border_templates_for_gap(&samples, channels, &test_border_spec(16, 20, 12, 0));
        assert!(!pre.is_empty());
        assert!(pre.iter().all(|&v| v.abs() > 1_000.0 / 32767.0));
        assert!(!post.is_empty());
        assert!(post.iter().all(|&v| v.abs() > 1_000.0 / 32767.0));
    }

    #[test]
    fn border_standoff_excludes_audio_adjacent_to_dropout() {
        let channels = 1usize;
        let loud = 8_000.0_f32 / 32767.0;
        let mut samples = vec![loud; 20];
        samples.extend(vec![0.0f32; 5]);
        samples.extend(vec![loud; 20]);

        let (pre_no, _) =
            border_templates_for_gap(&samples, channels, &test_border_spec(20, 25, 15, 0));
        let (pre_standoff, _) =
            border_templates_for_gap(&samples, channels, &test_border_spec(20, 25, 15, 5));
        assert_eq!(pre_no.len(), 15);
        assert_eq!(pre_standoff.len(), 10);
    }

    /// The displacement is the walk-off *plus* the trim, per side, and it is what a lag sweep
    /// anchored on the gap edge reads at true registration (registration plan § "Step 5 result").
    #[test]
    fn border_edge_displacement_counts_walk_off_and_trim() {
        let channels = 1usize;
        let loud = 8_000.0_f32 / 32767.0;
        let low = 200.0_f32 / 32767.0; // above the silence floor, below the 12 %-of-peak trim
        let mut samples = vec![loud; 8]; // 0..8
        samples.extend(vec![low; 4]); //  8..12  trimmed off the pre template
        samples.extend(vec![0.0f32; 4]); // 12..16  walked off the pre template
        samples.extend(vec![0.0f32; 4]); // 16..20  the gap
        samples.extend(vec![0.0f32; 2]); // 20..22  walked off the post template
        samples.extend(vec![low; 3]); //    22..25  trimmed off the post template
        samples.extend(vec![loud; 8]); //   25..33

        let spec = test_border_spec(16, 20, 13, 0);
        let (pre, post) = border_templates_for_gap(&samples, channels, &spec);
        assert_eq!(pre.len(), 5);
        assert_eq!(post.len(), 8);

        let (d_pre, d_post) =
            border_edge_displacement_frames(&samples, channels, &spec, pre.len(), post.len());
        assert_eq!(d_pre, 8, "4 walked + 4 trimmed");
        assert_eq!(d_post, 5, "2 walked + 3 trimmed");

        // A standoff that reaches past the already-trimmed fade eats real audio, and the
        // displacement grows by exactly that much.
        let spec = test_border_spec(16, 20, 13, 6);
        let (pre, post) = border_templates_for_gap(&samples, channels, &spec);
        let (d_pre, _) =
            border_edge_displacement_frames(&samples, channels, &spec, pre.len(), post.len());
        assert_eq!(
            d_pre, 10,
            "walk to 12, standoff back to 6, no fade left to trim"
        );
    }

    #[test]
    fn border_templates_for_gap_skip_adjacent_silence() {
        let channels = 1usize;
        let loud = 8_000.0_f32 / 32767.0;
        let mut samples = vec![loud; 5];
        samples.extend(vec![0.0f32; 5]);
        samples.extend(vec![loud; 5]);

        let (pre, post) =
            border_templates_for_gap(&samples, channels, &test_border_spec(5, 10, 5, 0));
        assert_eq!(pre.len(), 5);
        assert!(pre.iter().all(|&v| (v - loud as f64).abs() < 0.001));
        assert_eq!(post.len(), 5);
        assert!(post.iter().all(|&v| (v - loud as f64).abs() < 0.001));
    }

    // ---- Per-channel (multichannel) residual ----------------------------------------------------

    /// Build interleaved f32 A samples (normalized) from per-channel f64 timelines (raw level).
    fn interleave_a(channels_f64: &[Vec<f64>], norm: f64) -> Vec<f32> {
        let channels = channels_f64.len();
        let total = channels_f64[0].len();
        let mut out = vec![0.0f32; total * channels];
        for frame in 0..total {
            for (ch, timeline) in channels_f64.iter().enumerate() {
                out[frame * channels + ch] = (timeline[frame] / norm) as f32;
            }
        }
        out
    }

    /// Pearson (`seam_channel_diagnostics`) and residual (`selected_seam_channels`) must agree on
    /// which A-side channels carry border energy for a gap.
    fn assert_channel_selection_parity(
        a_samples: &[f32],
        channels: usize,
        spec: &GapBorderSpec,
        b_ch: &[Vec<f64>],
        placement: SeamPlacement,
    ) {
        let (a_pre, a_post) = border_templates_for_gap(a_samples, channels, spec);
        let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
        let b_mono: Vec<f64> = b_ch
            .first()
            .map(|_| {
                (0..b_ch[0].len())
                    .map(|i| b_ch.iter().map(|ch| ch[i]).sum::<f64>() / b_ch.len() as f64)
                    .collect()
            })
            .unwrap_or_default();
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch,
        };
        let pearson = seam_channel_diagnostics(&templates, placement).selected;
        let residual = selected_seam_channels(a_samples, channels, spec);
        assert_eq!(
            pearson, residual,
            "Pearson diagnostics and residual must select the same channels"
        );
    }

    #[test]
    fn selected_seam_channels_matches_pearson_diagnostics() {
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let border_frames = 128usize;
        let standoff = 16usize;
        let window = 128usize;
        let start = gap_start;
        let gap_frames = gap_end - gap_start;
        let placement = || SeamPlacement {
            start,
            gap_frames,
            pre_window: window,
            post_window: window,
        };

        // Stereo, equal energy on both channels → both selected.
        {
            let total = 2000usize;
            let channels = 2usize;
            let l: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.17).sin() * 4000.0)
                .collect();
            let r: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.4).cos() * 4000.0)
                .collect();
            let a_samples = interleave_a(&[l.clone(), r.clone()], 4000.0);
            let b_ch = vec![l, r];
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(&a_samples, channels, &spec, &b_ch, placement());
            assert_eq!(
                selected_seam_channels(&a_samples, channels, &spec),
                vec![0, 1]
            );
        }

        // Center-dominant 3ch: only FC crosses the ~20 dB energy gate in the border templates.
        {
            let total = 2000usize;
            let channels = 3usize;
            let fc_b: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
                .collect();
            let fl_b: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.53).sin() * 2000.0)
                .collect();
            let fr_b: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.91).cos() * 2000.0)
                .collect();
            let b_ch = vec![fl_b.clone(), fr_b.clone(), fc_b.clone()];
            let fc_a: Vec<f64> = fc_b.iter().map(|s| s * 0.5).collect();
            // Fronts well below FC in the border region (>20 dB down) so only the center is selected.
            let fl_a: Vec<f64> = vec![5.0; total];
            let fr_a: Vec<f64> = vec![5.0; total];
            let a_samples = interleave_a(&[fl_a, fr_a, fc_a], 4000.0);
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(&a_samples, channels, &spec, &b_ch, placement());
            assert_eq!(selected_seam_channels(&a_samples, channels, &spec), vec![2]);
        }

        // Near-silent borders → empty selection (mono fallback for both paths).
        {
            let total = 2000usize;
            let channels = 2usize;
            let a_samples = vec![0.0f32; total * channels];
            let b_ch = vec![vec![0.0f64; total], vec![0.0f64; total]];
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(&a_samples, channels, &spec, &b_ch, placement());
            assert!(selected_seam_channels(&a_samples, channels, &spec).is_empty());
        }
    }

    #[test]
    fn loudest_seam_channel_picks_by_energy_not_index() {
        let (gap_start, gap_end, border_frames, standoff) =
            (800usize, 1000usize, 128usize, 16usize);
        let total = 2000usize;
        let channels = 3usize;
        let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);

        // ch0 (L) moderate, ch1 (R) silent, ch2 (C) loudest. Both 0 and 2 pass the −20 dB energy gate,
        // so `selected_seam_channels` returns [0, 2] in index order and `.first()` = 0 (the *quieter*
        // channel). `loudest_seam_channel` must instead return 2 (the channel that carries the level).
        let l_a: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.17).sin() * 2000.0)
            .collect();
        let r_a: Vec<f64> = vec![5.0; total];
        let c_a: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.31).sin() * 4000.0)
            .collect();
        let a_samples = interleave_a(&[l_a, r_a, c_a], 4000.0);

        assert_eq!(
            selected_seam_channels(&a_samples, channels, &spec),
            vec![0, 2]
        );
        assert_eq!(
            selected_seam_channels(&a_samples, channels, &spec)
                .first()
                .copied(),
            Some(0),
            ".first() picks the lowest-index passing channel (the quieter L)"
        );
        assert_eq!(
            loudest_seam_channel(&a_samples, channels, &spec),
            Some(2),
            "loudest_seam_channel follows the level to the center channel"
        );
    }
}
