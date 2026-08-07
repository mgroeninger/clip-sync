//! Residual / floor cancellation measurement for gap seams.
//!
//! The cancellation half of the seam gate: least-squares fit of A's border against B at a searched
//! lag, reported as residual dB plus a floor probe for what "as good as it gets" looks like on this
//! material. Builds on `seam_scoring`'s Pearson and channel-timeline helpers.
use super::seam_scoring::{interleaved_channel_timeline_f64, seam_pearson};

/// Integer lag search result for one seam side (`seam_residual_for_side`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct LagFitResult {
    gain: f64,
    best_lag: i64,
    residual_db: f64,
}

/// B-window energy below this is unmeasurable — abstain the lag (L13).
const LSQ_B_ENERGY_FLOOR: f64 = 1.0;

/// Least-squares scalar fit `a ≈ g·b`; returns `(gain, ||a - g·b||² / ||a||²)`.
fn lsq_residual_ratio(a: &[f64], b: &[f64]) -> Option<(f64, f64)> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let aa: f64 = a.iter().map(|x| x * x).sum();
    if aa <= f64::EPSILON {
        return None;
    }
    let bb: f64 = b.iter().map(|y| y * y).sum();
    if bb < LSQ_B_ENERGY_FLOOR {
        return None;
    }
    let ab: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let g = ab / bb;
    let res: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            let d = x - g * y;
            d * d
        })
        .sum();
    Some((g, (res / aa).max(0.0)))
}

/// Search integer lags for the minimum normalized residual of `a_win` against B windows in
/// `b_haystack` at offsets returned by `b_window_bounds`.
///
/// `lag_center` shifts the search window (e.g. `floor.best_lag + nominal_delta − chosen_delta`
/// when placement slide is within reach).
fn seam_residual_for_side<F>(
    a_win: &[f64],
    b_haystack: &[f64],
    b_window_bounds: F,
    max_lag: i64,
    lag_center: i64,
) -> Option<LagFitResult>
where
    F: Fn(i64) -> Option<(usize, usize)>,
{
    let w = a_win.len();
    if w == 0 {
        return None;
    }
    let aa: f64 = a_win.iter().map(|x| x * x).sum();
    if aa <= f64::EPSILON {
        return None;
    }

    let max_lag = max_lag.max(0);
    let mut best_ratio = f64::INFINITY;
    let mut best_lag = 0i64;
    let mut best_gain = 0.0;
    let mut found = false;
    for lag in (lag_center - max_lag)..=(lag_center + max_lag) {
        let Some((lo, hi)) = b_window_bounds(lag) else {
            continue;
        };
        if hi - lo != w {
            continue;
        }
        let b_win = &b_haystack[lo..hi];
        let Some((g, ratio)) = lsq_residual_ratio(a_win, b_win) else {
            continue;
        };
        if ratio < best_ratio {
            best_ratio = ratio;
            best_lag = lag;
            best_gain = g;
            found = true;
        }
    }

    if !found {
        return None;
    }

    Some(LagFitResult {
        gain: best_gain,
        best_lag,
        residual_db: 10.0 * best_ratio.max(1e-12).log10(),
    })
}

/// Reference window must peak at least this multiple of the silence floor to anchor a floor probe.
const SEAM_FLOOR_ENERGY_MARGIN: f64 = 4.0;

/// Which side of the gap a probe was taken from / where its reference window came from.
///
/// `Deserialize` because the gap fingerprint emits this on `ResidualInfo` and reads its own dumps
/// back (`--check`, the corpus analysis) — the variant is what separates "no floor was found"
/// from "a floor was measured and it happens to be weak".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamFloorSource {
    /// Immediate border window (just past the standoff) was energetic and usable.
    Border,
    /// Border was empty/quiet; reference came from an energetic window walked further out.
    Walked,
    /// No energetic, in-coverage reference window found within the horizon.
    None,
}

/// Why a residual verdict carries no usable headroom reading.
///
/// `informative: false` covers four unrelated events, and a reader outside the fingerprint schema
/// cannot tell them apart from the flag alone. The first three are abstentions ("we could not
/// measure here"); [`FloorAboveOkDb`](ResidualUninformative::FloorAboveOkDb) is a *measurement*, and
/// naming only the abstentions would leave that fork unexplained.
///
/// Report-only: nothing that decides anything reads this. The gate branches on
/// [`SeamResidualVerdict::gate_abstains`], which this enum *names* — the dependency runs one way,
/// so extending or reordering the vocabulary cannot move a decision. `Deserialize` because the gap
/// fingerprint emits it on `ResidualInfo` and reads its own dumps back, like [`SeamFloorSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidualUninformative {
    /// Placement slid past the unified lag radius — the gate's deliberate abstention
    /// ([`SeamResidualVerdict::beyond_lag_reach`]). A property of the placement, not of a side.
    BeyondLagReach,
    /// No energetic, in-coverage A reference window within `max_walk_frames`.
    NoReferenceWindow,
    /// A reference window was found; the lag fit still produced nothing (non-finite probe).
    ProbeNonFinite,
    /// The floor **was** measured and sits above `floor_ok_db` — not an abstention.
    ///
    /// **Carries the same caveat as `DonorRelation::DiffCapture`:** it means the same-master regime
    /// was *not established at this seam*, **not** that B is proven a different master. A
    /// same-master pair yields it whenever it drifts beyond the probe radius, or when the reference
    /// window is quiet enough that cancellation never gets deep. Read it as "no cancellation
    /// evidence here", never as a provenance finding.
    ///
    /// **Threshold-relative.** `floor_ok_db` is a setting (`residual_floor_ok_db`, TOML and CLI);
    /// [`DEFAULT_RESIDUAL_FLOOR_OK_DB`] is only the default and `-50.0` is a documented variant, so
    /// this variant is only interpretable against the run's own settings. Fingerprint dumps record
    /// the threshold on `CorpusGateRecipe` for exactly this reason.
    FloorAboveOkDb,
}

impl ResidualUninformative {
    /// Stable label for human output and the `-v` tag line; matches the serde representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BeyondLagReach => "beyond_lag_reach",
            Self::NoReferenceWindow => "no_reference_window",
            Self::ProbeNonFinite => "probe_non_finite",
            Self::FloorAboveOkDb => "floor_above_ok_db",
        }
    }
}

/// Per-side reason, from the probes already computed — no new measurement (§1.2).
///
/// `BeyondLagReach` is not derivable here: it belongs to the placement, and is applied at the
/// verdict level by [`SeamResidualVerdict::uninformative_reason`].
fn side_uninformative(floor: &SeamFloorProbe, floor_ok_db: f64) -> Option<ResidualUninformative> {
    if floor.source == SeamFloorSource::None {
        Some(ResidualUninformative::NoReferenceWindow)
    } else if !floor.residual_db.is_finite() {
        Some(ResidualUninformative::ProbeNonFinite)
    } else if floor.residual_db > floor_ok_db {
        Some(ResidualUninformative::FloorAboveOkDb)
    } else {
        None
    }
}

/// Multichannel per-side reason, derived from the slice **before** [`side_worst_headroom_summary`]
/// collapses it — that collapse is what made "found a window, fit failed" indistinguishable from
/// "never found a window".
///
/// Reads the **min-floor** channel, the same one [`side_floor_state_channels`] follows, so the reason
/// explains `informative` rather than the worst-headroom channel the scalars summarize.
fn side_uninformative_channels(
    side: &[SeamChannelResidual],
    floor_ok_db: f64,
) -> Option<ResidualUninformative> {
    let best_floor = side
        .iter()
        .filter(|c| c.floor.source != SeamFloorSource::None && c.floor.residual_db.is_finite())
        .map(|c| c.floor.residual_db)
        .reduce(f64::min);
    match best_floor {
        Some(floor_db) if floor_db <= floor_ok_db => None,
        Some(_) => Some(ResidualUninformative::FloorAboveOkDb),
        // No channel has a finite sourced floor. A channel that *did* anchor a window and then
        // failed the fit is a different event from a side that never found one.
        None if side.iter().any(|c| c.floor.source != SeamFloorSource::None) => {
            Some(ResidualUninformative::ProbeNonFinite)
        }
        None => Some(ResidualUninformative::NoReferenceWindow),
    }
}

/// Per-gap measured noise-floor probe (report-only): best A-vs-B residual of a clean, energetic
/// reference window at the *nominal* offset, used to interpret a seam residual as headroom.
#[derive(Debug, Clone, Copy)]
pub struct SeamFloorProbe {
    pub source: SeamFloorSource,
    /// Normalized residual in dB at the reference window (`NaN` when `source == None`).
    pub residual_db: f64,
    pub gain: f64,
    pub best_lag: i64,
}

impl SeamFloorProbe {
    fn none() -> Self {
        Self {
            source: SeamFloorSource::None,
            residual_db: f64::NAN,
            gain: f64::NAN,
            best_lag: 0,
        }
    }

    pub fn source_label(&self) -> &'static str {
        match self.source {
            SeamFloorSource::Border => "border",
            SeamFloorSource::Walked => "walked",
            SeamFloorSource::None => "none",
        }
    }
}

/// Which gap edge a floor probe walks out from.
#[derive(Debug, Clone, Copy)]
pub enum SeamSide {
    Pre,
    Post,
}

/// Inputs to [`seam_floor_probe`] (report-only diagnostic).
pub struct SeamFloorParams<'a> {
    /// Full A audio (interleaved) on the same clock as the gap frames.
    pub a_samples: &'a [f32],
    pub channels: usize,
    /// B haystack mono (same buffer the seam scores against).
    pub b_mono: &'a [f64],
    /// Reference window length in frames (use the matching seam window).
    pub window: usize,
    /// Frames skipped immediately adjacent to the gap (reuse the seam standoff).
    pub standoff_frames: usize,
    /// `b_haystack_frame = a_frame + a_to_b_delta` (nominal alignment mapping).
    pub a_to_b_delta: i64,
    /// Outward walk step in frames when the immediate border is quiet.
    pub step_frames: usize,
    /// How far from the gap edge the outward walk may reach.
    pub max_walk_frames: usize,
    pub absolute_silence_rms: f32,
    /// Integer sample lag searched on each side when cancelling A against B.
    pub max_lag_frames: i64,
}

fn mono_window(a_samples: &[f32], channels: usize, lo: usize, hi: usize) -> Vec<f64> {
    let channels = channels.max(1);
    (lo..hi)
        .map(|frame| {
            let base = frame * channels;
            let sum: f64 = (0..channels)
                .map(|c| a_samples.get(base + c).copied().unwrap_or(0.0) as f64)
                .sum();
            sum / channels as f64
        })
        .collect()
}

/// A clean, energetic raw A reference window selected for residual/floor measurement.
struct ReferenceWindow {
    /// Raw mono A samples (no standoff/low-energy trim — see `mono_window`).
    a_win: Vec<f64>,
    /// First A frame of the window (maps to B via a delta).
    a_lo: usize,
    /// Whether the window is the immediate border or one walked outward.
    source: SeamFloorSource,
}

/// Frame range and source of a reference window, decoupled from channel extraction so the same
/// raw range can feed either the mono downmix or per-channel residual measurement (§ channel
/// alignment in `seam-scoring.md`).
struct ReferenceFrames {
    a_lo: usize,
    a_hi: usize,
    source: SeamFloorSource,
}

/// Walk outward from the gap edge to the first reference window that passes `energetic` (the energy
/// gate — B-side availability is checked at measurement time so the same window can be measured at
/// multiple deltas). `energetic(a_lo, a_hi)` decides whether a candidate range carries usable audio.
fn walk_reference_frames(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
    energetic: impl Fn(usize, usize) -> bool,
) -> Option<ReferenceFrames> {
    let channels = params.channels.max(1);
    let w = params.window;
    if w == 0 {
        return None;
    }
    let a_total = params.a_samples.len() / channels;
    let step = params.step_frames.max(1);

    let mut k = 0usize;
    loop {
        let walked = k * step;
        if walked > params.max_walk_frames {
            return None;
        }
        let window = match side {
            SeamSide::Pre => match gap_start_frame.checked_sub(params.standoff_frames + walked) {
                Some(hi) if hi >= w => Some((hi - w, hi)),
                _ => None,
            },
            SeamSide::Post => {
                let lo = gap_end_frame + params.standoff_frames + walked;
                let hi = lo + w;
                (hi <= a_total).then_some((lo, hi))
            }
        };
        let (a_lo, a_hi) = window?;
        if energetic(a_lo, a_hi) {
            let source = if k == 0 {
                SeamFloorSource::Border
            } else {
                SeamFloorSource::Walked
            };
            return Some(ReferenceFrames { a_lo, a_hi, source });
        }
        k += 1;
    }
}

/// Energy gate for the mono downmix path: the downmixed window peak must clear the silence floor.
fn mono_window_energetic<'a>(
    a_samples: &'a [f32],
    channels: usize,
    energy_floor: f64,
) -> impl Fn(usize, usize) -> bool + 'a {
    move |a_lo, a_hi| {
        let win = mono_window(a_samples, channels, a_lo, a_hi);
        win.iter().map(|s| s.abs()).fold(0.0f64, f64::max) >= energy_floor
    }
}

/// Walk outward from the gap edge to the first energetic raw A window (mono downmix energy gate).
fn select_reference_window(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
) -> Option<ReferenceWindow> {
    let channels = params.channels.max(1);
    let energy_floor = f64::from(params.absolute_silence_rms) * SEAM_FLOOR_ENERGY_MARGIN;
    let frames = walk_reference_frames(
        params,
        side,
        gap_start_frame,
        gap_end_frame,
        mono_window_energetic(params.a_samples, channels, energy_floor),
    )?;
    Some(ReferenceWindow {
        a_win: mono_window(params.a_samples, channels, frames.a_lo, frames.a_hi),
        a_lo: frames.a_lo,
        source: frames.source,
    })
}

/// Cancel a raw A window against B at `delta` (`b_frame = a_frame + delta`) with `max_lag` around
/// `lag_center`. `source` is stamped on the resulting probe.
fn measure_a_win_at_delta(
    a_win: &[f64],
    a_lo: usize,
    source: SeamFloorSource,
    b: &[f64],
    delta: i64,
    max_lag: i64,
    lag_center: i64,
) -> SeamFloorProbe {
    let w = a_win.len() as i64;
    let b_len = b.len() as i64;
    let b_start0 = a_lo as i64 + delta;
    let probe = seam_residual_for_side(
        a_win,
        b,
        |lag| {
            let lo = b_start0 + lag;
            let hi = lo + w;
            if lo < 0 || hi > b_len {
                return None;
            }
            Some((lo as usize, hi as usize))
        },
        max_lag,
        lag_center,
    );
    match probe {
        Some(r) => SeamFloorProbe {
            source,
            residual_db: r.residual_db,
            gain: r.gain,
            best_lag: r.best_lag,
        },
        None => SeamFloorProbe {
            source,
            residual_db: f64::NAN,
            gain: f64::NAN,
            best_lag: 0,
        },
    }
}

fn measure_window_at_delta(
    window: &ReferenceWindow,
    b_mono: &[f64],
    delta: i64,
    max_lag: i64,
    lag_center: i64,
) -> SeamFloorProbe {
    measure_a_win_at_delta(
        &window.a_win,
        window.a_lo,
        window.source,
        b_mono,
        delta,
        max_lag,
        lag_center,
    )
}

/// Measure `(chosen, floor)` for one raw window against one B timeline: floor at the nominal mapping
/// (wide lag), chosen at `chosen_delta` with its lag search centered on the floor's best lag when the
/// placement slide is within reach (so headroom is a pure where-on-B difference). Shared by the mono
/// and per-channel paths.
fn chosen_and_floor_on_window(
    a_win: &[f64],
    a_lo: usize,
    source: SeamFloorSource,
    b: &[f64],
    nominal_delta: i64,
    chosen_delta: i64,
    max_lag: i64,
) -> (SeamFloorProbe, SeamFloorProbe) {
    let floor = measure_a_win_at_delta(a_win, a_lo, source, b, nominal_delta, max_lag, 0);
    let lag_center = chosen_lag_center(&floor, nominal_delta, chosen_delta, max_lag);
    let chosen = measure_a_win_at_delta(a_win, a_lo, source, b, chosen_delta, max_lag, lag_center);
    (chosen, floor)
}

/// Lag the chosen-placement probe should center on: the floor's best lag shifted by the placement
/// slide, so chosen and floor compare the same B content. Falls back to `0` when the slide exceeds
/// the lag radius or the floor did not cancel.
fn chosen_lag_center(
    floor: &SeamFloorProbe,
    nominal_delta: i64,
    chosen_delta: i64,
    max_lag: i64,
) -> i64 {
    let placement_error = chosen_delta - nominal_delta;
    if placement_error.abs() <= max_lag && floor.residual_db.is_finite() {
        floor.best_lag + nominal_delta - chosen_delta
    } else {
        0
    }
}

/// Measure the per-gap noise floor: slide a clean, energetic A reference window against B at the
/// nominal offset (wide lag search). Starts at the immediate border, walks outward if it is quiet.
pub fn seam_floor_probe(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
) -> SeamFloorProbe {
    match select_reference_window(params, side, gap_start_frame, gap_end_frame) {
        Some(window) => measure_window_at_delta(
            &window,
            params.b_mono,
            params.a_to_b_delta,
            params.max_lag_frames,
            0,
        ),
        None => SeamFloorProbe::none(),
    }
}

/// Measure one gap side's residual at the **chosen** placement and the **nominal** floor on the
/// *same* raw reference window (so headroom is a pure difference of where-on-B, not of reference
/// audio or lag radius). `params.a_to_b_delta` is the nominal mapping; `chosen_delta` is the
/// chosen-placement mapping. Returns `(chosen, floor)`.
pub fn seam_chosen_and_floor(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
    chosen_delta: i64,
) -> (SeamFloorProbe, SeamFloorProbe) {
    match select_reference_window(params, side, gap_start_frame, gap_end_frame) {
        Some(window) => chosen_and_floor_on_window(
            &window.a_win,
            window.a_lo,
            window.source,
            params.b_mono,
            params.a_to_b_delta,
            chosen_delta,
            params.max_lag_frames,
        ),
        None => (SeamFloorProbe::none(), SeamFloorProbe::none()),
    }
}

/// One selected channel's chosen-placement residual and nominal floor on the same raw reference
/// window (per-channel analog of [`seam_chosen_and_floor`]; see § residual channel policy).
#[derive(Debug, Clone, Copy)]
pub struct SeamChannelResidual {
    pub channel: usize,
    pub chosen: SeamFloorProbe,
    pub floor: SeamFloorProbe,
}

/// Per-channel chosen/floor for one gap side, measured on the **same** energy-selected channels as
/// Pearson seam scoring. Each selected channel is cancelled against its own `b_ch[ch]` timeline on a
/// shared raw reference window (frame range chosen by a per-channel energy gate). Returns one entry
/// per usable selected channel; when `score_channels` is empty or no channel is usable (e.g. B has
/// fewer channels), falls back to a single mono-downmix entry so callers get uniform handling.
///
/// **Alignment is shared, depth is per-channel.** The integer lag is found once across *all* selected
/// channels by summing their per-channel correlation (see [`shared_alignment_lag`]), with no
/// dependence on which channel happens to carry the gap, then each selected channel fits its own
/// scalar gain and residual at that fixed lag. This keeps the time alignment robust on surround/center
/// mixes while still measuring cancellation depth on the right channel.
///
/// `score_channels` must already be filtered to the channels of interest; indices `>= b_ch.len()`
/// are skipped (A/B channel-count mismatch, Risk §8).
pub fn seam_chosen_and_floor_multichannel(
    params: &SeamFloorParams<'_>,
    b_ch: &[Vec<f64>],
    score_channels: &[usize],
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
    chosen_delta: i64,
) -> Vec<SeamChannelResidual> {
    let channels = params.channels.max(1);
    let usable: Vec<usize> = score_channels
        .iter()
        .copied()
        .filter(|&ch| ch < b_ch.len() && ch < channels)
        .collect();

    if usable.is_empty() {
        // Mono downmix fallback — one entry, identical signal path to `seam_chosen_and_floor`.
        let (chosen, floor) =
            seam_chosen_and_floor(params, side, gap_start_frame, gap_end_frame, chosen_delta);
        return vec![SeamChannelResidual {
            channel: 0,
            chosen,
            floor,
        }];
    }

    // Shared frame range: walk outward until *any* selected channel carries usable audio, so quiet
    // surrounds can't push the window past energetic center content (§4c energy gate).
    let energy_floor = f64::from(params.absolute_silence_rms) * SEAM_FLOOR_ENERGY_MARGIN;
    let energetic = |a_lo: usize, a_hi: usize| {
        usable.iter().any(|&ch| {
            interleaved_channel_timeline_f64(params.a_samples, channels, ch, a_lo, a_hi)
                .iter()
                .map(|s| s.abs())
                .fold(0.0f64, f64::max)
                >= energy_floor
        })
    };

    let Some(frames) =
        walk_reference_frames(params, side, gap_start_frame, gap_end_frame, energetic)
    else {
        // No energetic window for any selected channel within the horizon.
        return usable
            .into_iter()
            .map(|ch| SeamChannelResidual {
                channel: ch,
                chosen: SeamFloorProbe::none(),
                floor: SeamFloorProbe::none(),
            })
            .collect();
    };

    // Find the shared integer lag once by summing the per-channel correlation across selected
    // channels, then measure each channel's depth at that fixed lag. Summing *correlations* (not a
    // downmixed waveform) is what makes this robust: a loud channel whose B content differs (or is
    // noise) correlates ~0 at every lag and so never pulls the alignment, while the channel(s) that
    // actually match contribute a sharp peak at the true lag — no dependence on which channel that is.
    let a_wins: Vec<Vec<f64>> = usable
        .iter()
        .map(|&ch| {
            interleaved_channel_timeline_f64(
                params.a_samples,
                channels,
                ch,
                frames.a_lo,
                frames.a_hi,
            )
        })
        .collect();
    let shared_floor_lag = shared_alignment_lag(
        &a_wins,
        &usable,
        b_ch,
        frames.a_lo,
        params.a_to_b_delta,
        params.max_lag_frames,
    );
    let placement_error = chosen_delta - params.a_to_b_delta;
    let shared_chosen_lag = if placement_error.abs() <= params.max_lag_frames {
        shared_floor_lag + params.a_to_b_delta - chosen_delta
    } else {
        0
    };

    // Measure each selected channel's depth at the shared lag (max_lag = 0 → fixed lag); only the
    // per-channel scalar gain adapts.
    usable
        .iter()
        .zip(a_wins.iter())
        .map(|(&ch, a_win)| {
            let floor = measure_a_win_at_delta(
                a_win,
                frames.a_lo,
                frames.source,
                &b_ch[ch],
                params.a_to_b_delta,
                0,
                shared_floor_lag,
            );
            let chosen = measure_a_win_at_delta(
                a_win,
                frames.a_lo,
                frames.source,
                &b_ch[ch],
                chosen_delta,
                0,
                shared_chosen_lag,
            );
            SeamChannelResidual {
                channel: ch,
                chosen,
                floor,
            }
        })
        .collect()
}

/// The integer lag that best aligns A and B across *all* selected channels at once: the lag maximizing
/// the summed peak-normalized correlation. Channels whose B content does not match contribute ~0 at
/// every lag, so they neither pull nor veto the alignment — only matching channels shape the peak.
/// Returns the nominal lag (`0`) when no lag produces positive aggregate correlation.
fn shared_alignment_lag(
    a_wins: &[Vec<f64>],
    channels: &[usize],
    b_ch: &[Vec<f64>],
    a_lo: usize,
    nominal_delta: i64,
    max_lag: i64,
) -> i64 {
    let max_lag = max_lag.max(0);
    let mut best_lag = 0i64;
    let mut best_score = f64::NEG_INFINITY;
    for lag in -max_lag..=max_lag {
        let mut score = 0.0;
        for (a_win, &ch) in a_wins.iter().zip(channels) {
            let b = &b_ch[ch];
            let w = a_win.len() as i64;
            let lo = a_lo as i64 + nominal_delta + lag;
            let hi = lo + w;
            if lo < 0 || hi > b.len() as i64 {
                continue;
            }
            score += seam_pearson(a_win, &b[lo as usize..hi as usize]).max(0.0);
        }
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }
    if best_score > 0.0 {
        best_lag
    } else {
        0
    }
}

/// Default `floor_db` ceiling for an established same-master cancellation floor.
///
/// Floors at or below this value are treated as informative for residual gating; see
/// [`floor_probe_informative`] and [`residual_verdict_informative`]. Calibrated in
/// `tests/seam_residual_corpus.rs`; overridable via config when the gate ships.
pub const DEFAULT_RESIDUAL_FLOOR_OK_DB: f64 = -15.0;

/// True when a floor probe measured nominal cancellation at or below `floor_ok_db`.
pub fn floor_probe_informative(probe: &SeamFloorProbe, floor_ok_db: f64) -> bool {
    probe.source != SeamFloorSource::None
        && probe.residual_db.is_finite()
        && probe.residual_db <= floor_ok_db
}

/// Per-side floor regime for [`combine_informative`] — shared by mono and multichannel constructors.
///
/// Derived from the same rules as [`side_uninformative`] / [`side_uninformative_channels`].
/// §5.1 (toward MC): [`SideFloorState::ProbeFailed`] is ignored like [`SideFloorState::Unmeasured`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideFloorState {
    /// [`ResidualUninformative::NoReferenceWindow`]
    Unmeasured,
    /// [`ResidualUninformative::ProbeNonFinite`] — naming only; does not block `informative`
    ProbeFailed,
    /// [`ResidualUninformative::FloorAboveOkDb`] — measured failure
    RegimeFailed,
    RegimeOk,
}

fn side_floor_state(floor: &SeamFloorProbe, floor_ok_db: f64) -> SideFloorState {
    match side_uninformative(floor, floor_ok_db) {
        None => SideFloorState::RegimeOk,
        Some(ResidualUninformative::NoReferenceWindow) => SideFloorState::Unmeasured,
        Some(ResidualUninformative::ProbeNonFinite) => SideFloorState::ProbeFailed,
        Some(ResidualUninformative::FloorAboveOkDb) => SideFloorState::RegimeFailed,
        Some(ResidualUninformative::BeyondLagReach) => {
            unreachable!("BeyondLagReach is placement-level, not per-side")
        }
    }
}

fn side_floor_state_channels(side: &[SeamChannelResidual], floor_ok_db: f64) -> SideFloorState {
    match side_uninformative_channels(side, floor_ok_db) {
        None => SideFloorState::RegimeOk,
        Some(ResidualUninformative::NoReferenceWindow) => SideFloorState::Unmeasured,
        Some(ResidualUninformative::ProbeNonFinite) => SideFloorState::ProbeFailed,
        Some(ResidualUninformative::FloorAboveOkDb) => SideFloorState::RegimeFailed,
        Some(ResidualUninformative::BeyondLagReach) => {
            unreachable!("BeyondLagReach is placement-level, not per-side")
        }
    }
}

/// Whether headroom is regime-informative given both sides' floor states.
///
/// **Toward-MC policy** ([TEMP-residual-measured-unify-plan.md](docs/dev/archive/TEMP-residual-measured-unify-plan.md)
/// §5.1): `Unmeasured` and `ProbeFailed` are ignored; every remaining side must be `RegimeOk`.
/// Returns false when neither side contributes a governing reading. `RegimeFailed` always fails
/// regardless of constructor path.
fn combine_informative(pre: SideFloorState, post: SideFloorState) -> bool {
    let govern = |s: SideFloorState| -> Option<bool> {
        match s {
            SideFloorState::Unmeasured | SideFloorState::ProbeFailed => None,
            SideFloorState::RegimeOk => Some(true),
            SideFloorState::RegimeFailed => Some(false),
        }
    };
    match (govern(pre), govern(post)) {
        (None, None) => false,
        (a, b) => a.unwrap_or(true) && b.unwrap_or(true),
    }
}

/// Whether headroom on this gap is regime-informative (same-master + aligned at nominal).
///
/// Shared measuredness for [`SeamResidualVerdict::from_parts`] /
/// [`SeamResidualVerdict::from_parts_with_placement`] and the multichannel constructor: a side
/// *governs* only when it has a finite sourced floor. Sourced-NaN (`ProbeNonFinite`) is ignored
/// like an unmeasured side — the other side may keep `informative` and a live veto. Per-side
/// `uninformative_*` still names [`ResidualUninformative::ProbeNonFinite`]. See
/// [`combine_informative`].
pub fn residual_verdict_informative(
    floor_pre: &SeamFloorProbe,
    floor_post: &SeamFloorProbe,
    floor_ok_db: f64,
) -> bool {
    combine_informative(
        side_floor_state(floor_pre, floor_ok_db),
        side_floor_state(floor_post, floor_ok_db),
    )
}

/// Combined residual/floor verdict for one gap (P1 report-only, uniform schema).
///
/// Both the chosen-placement residual and the nominal floor are measured on the **same raw A
/// reference window** with the same lag radius — they differ only in *where on B* (chosen vs
/// nominal mapping). When placement slide is within `max_lag_frames`, the chosen probe's lag
/// search is centered on `floor.best_lag + nominal_delta − chosen_delta`. `informative` uses
/// the supplied `floor_ok_db`. When `placement_slide_frames > max_lag_frames`, the gate abstains
/// (`beyond_lag_reach`) — headroom is not meaningful outside the unified lag radius.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct SeamResidualVerdict {
    pub chosen_pre_db: f64,
    pub chosen_post_db: f64,
    pub floor_pre_db: f64,
    pub floor_post_db: f64,
    pub floor_source_pre: SeamFloorSource,
    pub floor_source_post: SeamFloorSource,
    /// Nominal floor established cancellation on every *governing* side (`floor_db ≤ FLOOR_OK`).
    /// Unmeasured and sourced-NaN (`ProbeNonFinite`) sides are ignored — see [`combine_informative`].
    pub informative: bool,
    /// `|chosen_delta − nominal_delta|` in frames (0 when unset / harness default).
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub placement_slide_frames: u64,
    /// Unified lag radius used for this verdict (`0` = reach check disabled).
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub max_lag_frames: i64,
    /// Why the pre side carries no usable floor; `None` when that side is usable.
    ///
    /// Diagnostic detail. [`uninformative_reason`](Self::uninformative_reason) is authoritative for
    /// reporting and may legitimately disagree (a side reading `FloorAboveOkDb` under a combined
    /// `BeyondLagReach` is correct, not a bug).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uninformative_pre: Option<ResidualUninformative>,
    /// Why the post side carries no usable floor; `None` when that side is usable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uninformative_post: Option<ResidualUninformative>,
}

/// `PartialEq` for probe scalars: `NaN` compares equal to `NaN` (L1).
fn residual_scalar_eq(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

impl PartialEq for SeamResidualVerdict {
    fn eq(&self, other: &Self) -> bool {
        residual_scalar_eq(self.chosen_pre_db, other.chosen_pre_db)
            && residual_scalar_eq(self.chosen_post_db, other.chosen_post_db)
            && residual_scalar_eq(self.floor_pre_db, other.floor_pre_db)
            && residual_scalar_eq(self.floor_post_db, other.floor_post_db)
            && self.floor_source_pre == other.floor_source_pre
            && self.floor_source_post == other.floor_source_post
            && self.informative == other.informative
            && self.placement_slide_frames == other.placement_slide_frames
            && self.max_lag_frames == other.max_lag_frames
            && self.uninformative_pre == other.uninformative_pre
            && self.uninformative_post == other.uninformative_post
    }
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

impl SeamResidualVerdict {
    /// Assemble from the per-side chosen and floor probes (see [`seam_chosen_and_floor`]).
    pub fn from_parts(
        chosen_pre: &SeamFloorProbe,
        chosen_post: &SeamFloorProbe,
        floor_pre: &SeamFloorProbe,
        floor_post: &SeamFloorProbe,
    ) -> Self {
        Self::from_parts_with_floor_ok(
            chosen_pre,
            chosen_post,
            floor_pre,
            floor_post,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
        )
    }

    /// Like [`from_parts`] but uses a custom `floor_ok_db` (calibration sweeps).
    pub fn from_parts_with_floor_ok(
        chosen_pre: &SeamFloorProbe,
        chosen_post: &SeamFloorProbe,
        floor_pre: &SeamFloorProbe,
        floor_post: &SeamFloorProbe,
        floor_ok_db: f64,
    ) -> Self {
        Self::from_parts_with_placement(
            chosen_pre,
            chosen_post,
            floor_pre,
            floor_post,
            floor_ok_db,
            0,
            0,
        )
    }

    /// Production path: includes placement slide and lag radius for reach abstention.
    pub fn from_parts_with_placement(
        chosen_pre: &SeamFloorProbe,
        chosen_post: &SeamFloorProbe,
        floor_pre: &SeamFloorProbe,
        floor_post: &SeamFloorProbe,
        floor_ok_db: f64,
        placement_slide_frames: u64,
        max_lag_frames: i64,
    ) -> Self {
        Self {
            chosen_pre_db: chosen_pre.residual_db,
            chosen_post_db: chosen_post.residual_db,
            floor_pre_db: floor_pre.residual_db,
            floor_post_db: floor_post.residual_db,
            floor_source_pre: floor_pre.source,
            floor_source_post: floor_post.source,
            informative: residual_verdict_informative(floor_pre, floor_post, floor_ok_db),
            placement_slide_frames,
            max_lag_frames,
            uninformative_pre: side_uninformative(floor_pre, floor_ok_db),
            uninformative_post: side_uninformative(floor_post, floor_ok_db),
        }
    }

    /// Assemble from per-channel residuals (energy-selected channels; see
    /// [`seam_chosen_and_floor_multichannel`]). The scalar side summaries report the **worst-headroom
    /// channel** on each side (so `worst_headroom_db()` is the conservative max over channels × sides
    /// for the veto, and the skip message names the channel that drove it), while `informative`
    /// follows the **best-cancelling (min-floor) channel** per side — a noisy surround must not flip
    /// the same-master regime off (Non-goal §2, residual-channel-alignment-plan §4d/§4e).
    pub fn from_channel_residuals(
        pre: &[SeamChannelResidual],
        post: &[SeamChannelResidual],
        floor_ok_db: f64,
        placement_slide_frames: u64,
        max_lag_frames: i64,
    ) -> Self {
        let (chosen_pre_db, floor_pre_db, floor_source_pre) = side_worst_headroom_summary(pre);
        let (chosen_post_db, floor_post_db, floor_source_post) = side_worst_headroom_summary(post);

        Self {
            chosen_pre_db,
            chosen_post_db,
            floor_pre_db,
            floor_post_db,
            floor_source_pre,
            floor_source_post,
            informative: combine_informative(
                side_floor_state_channels(pre, floor_ok_db),
                side_floor_state_channels(post, floor_ok_db),
            ),
            placement_slide_frames,
            max_lag_frames,
            uninformative_pre: side_uninformative_channels(pre, floor_ok_db),
            uninformative_post: side_uninformative_channels(post, floor_ok_db),
        }
    }

    /// Placement slide exceeds the unified lag radius — residual gate abstains.
    pub fn beyond_lag_reach(&self) -> bool {
        self.max_lag_frames > 0 && self.placement_slide_frames as i64 > self.max_lag_frames
    }

    /// **The residual gate guard**, in one place: no usable headroom reading, so every consumer
    /// leaves its input untouched (`apply_residual_to_confidence` returns Pearson unchanged,
    /// `classify_residual_band` returns `NoFloor`).
    ///
    /// A disjunction, not `!informative`: a verdict whose floor established the regime is still
    /// unusable when the placement slid outside the lag radius the floor was measured within.
    ///
    /// Decision-facing. [`uninformative_reason`](Self::uninformative_reason) *names* this condition
    /// for reporting and is defined in terms of it; the dependency runs that way and not the other,
    /// so no change to the naming vocabulary can move a gate decision.
    pub fn gate_abstains(&self) -> bool {
        !self.informative || self.beyond_lag_reach()
    }

    /// Why this verdict carries no usable headroom reading; `None` when the residual is usable.
    ///
    /// **This is not the negation of `informative`.** It names [`gate_abstains`](Self::gate_abstains)
    /// — a disjunction — so a verdict can have `informative: true` and still carry a reason
    /// (`BeyondLagReach`). "Why is there no usable headroom reading" is the question every reporting
    /// surface is actually asking. `is_some() == gate_abstains()`, exactly; do not assume
    /// `is_some() == !informative`.
    ///
    /// Combine rule over the two sides, which can disagree:
    ///
    /// 1. [`BeyondLagReach`](ResidualUninformative::BeyondLagReach) — a property of the placement,
    ///    dominates both sides.
    /// 2. The reason that actually failed `informative` among the **governing** sides
    ///    (`ProbeNonFinite` / `FloorAboveOkDb`); on a tie prefer `ProbeNonFinite` (less measured).
    ///    A sourced-NaN side still *names* `ProbeNonFinite` even when [`combine_informative`]
    ///    ignores it like unmeasured — so this step can surface a per-side name that did not drive
    ///    the abstention when a governing side also failed.
    /// 3. [`NoReferenceWindow`](ResidualUninformative::NoReferenceWindow) — only when *nothing*
    ///    governed (both sides unmeasured and/or probe-failed).
    ///
    /// Step 3 is last: unmeasured and probe-failed sides are ignored by
    /// [`combine_informative`], so they cannot by themselves make a verdict uninformative when the
    /// other side is `RegimeOk`. If the other side was governing and failed, that failure is why
    /// the gate abstains; if it passed, the verdict is informative and there is no combined reason.
    ///
    /// **[`gate_abstains`](Self::gate_abstains) is the authority on whether a reason exists at all**;
    /// the per-side fields only say *which*. Deriving the combined value from the sides alone would
    /// widen the guard past `!informative || beyond_lag_reach()` whenever a `ProbeNonFinite` side
    /// coexisted with a regime-OK side — killing a live headroom veto — so the guard is consulted
    /// first and the sides only explain it.
    pub fn uninformative_reason(&self) -> Option<ResidualUninformative> {
        if !self.gate_abstains() {
            return None;
        }
        if self.beyond_lag_reach() {
            return Some(ResidualUninformative::BeyondLagReach);
        }
        let sides = [self.uninformative_pre, self.uninformative_post];
        let has = |r: ResidualUninformative| sides.contains(&Some(r));
        if has(ResidualUninformative::ProbeNonFinite) {
            Some(ResidualUninformative::ProbeNonFinite)
        } else if has(ResidualUninformative::FloorAboveOkDb) {
            Some(ResidualUninformative::FloorAboveOkDb)
        } else {
            // Step 3. Total by construction: an uninformative verdict always leaves a reason on at
            // least one side, so this is `both sides unmeasured`. Answering `None` here would narrow
            // the guard below `!informative`, which is the mirror of the widening above — the tail
            // stays a reason, not an absence.
            Some(ResidualUninformative::NoReferenceWindow)
        }
    }

    /// Worst-side headroom at the chosen placement (`chosen − floor`); larger = worse match.
    ///
    /// Ignores sides where either value is non-finite (unmeasured floor or chosen).
    pub fn worst_headroom_db(&self) -> f64 {
        let headrooms = [
            self.chosen_pre_db - self.floor_pre_db,
            self.chosen_post_db - self.floor_post_db,
        ]
        .into_iter()
        .filter(|h| h.is_finite());
        headrooms.fold(f64::NAN, |acc, h| if acc.is_nan() { h } else { acc.max(h) })
    }

    /// Worst-side chosen-placement residual (higher = less cancellation).
    pub fn worst_chosen_db(&self) -> f64 {
        worst_finite_max([self.chosen_pre_db, self.chosen_post_db])
    }

    /// Worst-side nominal floor (higher = weaker cancellation).
    pub fn worst_floor_db(&self) -> f64 {
        worst_finite_max([self.floor_pre_db, self.floor_post_db])
    }
}

fn worst_finite_max(values: [f64; 2]) -> f64 {
    values
        .into_iter()
        .filter(|v| v.is_finite())
        .fold(f64::NAN, |acc, v| if acc.is_nan() { v } else { acc.max(v) })
}

/// Side summary for the scalar verdict fields: the worst-headroom channel's `(chosen_db, floor_db,
/// source)`. Ignores channels where headroom is non-finite.
///
/// When **no** channel has finite headroom, falls back to the **min-floor** channel that did anchor
/// a floor — the same channel the reason and `informative` follow, so all three describe one
/// channel. Reporting a bare `(NaN, NaN, None)` there would claim no reference window was ever
/// found on a side that measured one, which is precisely what `floor_source` exists to disambiguate.
/// That case includes a channel with a **finite, sourced floor and a non-finite chosen probe**,
/// since this keys on `chosen − floor`, not on the floor alone.
///
/// All three values come from one channel in both paths, fallback included: mixing a floor from one
/// channel with a `chosen_db` from another reproduces the cross-channel confusion `ResidualInfo`
/// documents for `informative` vs the scalars. `(NaN, NaN, None)` only when no channel anchored a
/// floor at all — and then `NoReferenceWindow` is the honest reason.
fn side_worst_headroom_summary(side: &[SeamChannelResidual]) -> (f64, f64, SeamFloorSource) {
    let mut best: Option<(f64, &SeamChannelResidual)> = None;
    for c in side {
        let headroom = c.chosen.residual_db - c.floor.residual_db;
        if headroom.is_finite() && best.is_none_or(|(h, _)| headroom > h) {
            best = Some((headroom, c));
        }
    }
    let sourced = || {
        side.iter()
            .filter(|c| c.floor.source != SeamFloorSource::None)
    };
    let fallback = || {
        // Min-floor among channels that measured one; else any channel that anchored a window and
        // then failed the fit, so `floor_source` still says `border`/`walked` with an absent
        // `floor_db` — the "measured, then non-finite" reading, not "no window found".
        sourced()
            .filter(|c| c.floor.residual_db.is_finite())
            .reduce(|a, b| {
                if b.floor.residual_db < a.floor.residual_db {
                    b
                } else {
                    a
                }
            })
            .or_else(|| sourced().next())
    };
    match best.map(|(_, c)| c).or_else(fallback) {
        Some(c) => (c.chosen.residual_db, c.floor.residual_db, c.floor.source),
        None => (f64::NAN, f64::NAN, SeamFloorSource::None),
    }
}

#[cfg(test)]
mod tests {
    use super::super::seam_scoring::seam_score_channel_indices;
    use super::*;

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

    fn probe_at(db: f64) -> SeamFloorProbe {
        SeamFloorProbe {
            source: SeamFloorSource::Border,
            residual_db: db,
            gain: 1.0,
            best_lag: 0,
        }
    }

    #[test]
    fn seam_residual_cancels_identical_scaled_source() {
        // Same-master: A border is B at half level → deep cancellation and gain ≈ 0.5.
        let pre_window = 16usize;
        let post_window = 16usize;
        let gap_frames = 8usize;
        let start = 64usize;

        let b_mono: Vec<f64> = (0..200)
            .map(|i| (i as f64 * 0.21).sin() * 1000.0 + (i as f64 * 0.07).cos() * 400.0)
            .collect();
        let a_pre: Vec<f64> = b_mono[start - pre_window..start]
            .iter()
            .map(|s| s * 0.5)
            .collect();
        let a_post: Vec<f64> = b_mono[start + gap_frames..start + gap_frames + post_window]
            .iter()
            .map(|s| s * 0.5)
            .collect();

        let pre = seam_residual_for_side(
            &a_pre,
            &b_mono,
            |lag| {
                let lo = start as i64 - pre_window as i64 + lag;
                let hi = start as i64 + lag;
                if lo < 0 || hi > b_mono.len() as i64 || hi <= lo {
                    return None;
                }
                Some((lo as usize, hi as usize))
            },
            512,
            0,
        )
        .expect("pre lag fit");
        let post = seam_residual_for_side(
            &a_post,
            &b_mono,
            |lag| {
                let tail = (start + gap_frames) as i64;
                let lo = tail + lag;
                let hi = tail + post_window as i64 + lag;
                if lo < 0 || hi > b_mono.len() as i64 {
                    return None;
                }
                Some((lo as usize, hi as usize))
            },
            512,
            0,
        )
        .expect("post lag fit");
        assert_eq!(pre.best_lag, 0, "true lag is 0, got {}", pre.best_lag);
        assert!(
            pre.residual_db < -60.0,
            "expected deep cancellation, got {} dB",
            pre.residual_db
        );
        assert!(
            (pre.gain - 0.5).abs() < 1e-6,
            "expected gain ~0.5, got {}",
            pre.gain
        );
        assert!(
            post.residual_db < -60.0,
            "expected deep cancellation, got {} dB",
            post.residual_db
        );
    }

    #[test]
    fn seam_residual_recovers_integer_lag() {
        // A's border equals B shifted by +3 samples; lag search should report best_lag = 3.
        let pre_window = 16usize;
        let start = 64usize;
        let true_lag = 3i64;

        let b_mono: Vec<f64> = (0..200).map(|i| (i as f64 * 0.3).sin() * 1000.0).collect();
        let lo = (start as i64 - pre_window as i64 + true_lag) as usize;
        let hi = (start as i64 + true_lag) as usize;
        let a_pre: Vec<f64> = b_mono[lo..hi].to_vec();

        let pre = seam_residual_for_side(
            &a_pre,
            &b_mono,
            |lag| {
                let b_lo = start as i64 - pre_window as i64 + lag;
                let b_hi = start as i64 + lag;
                if b_lo < 0 || b_hi > b_mono.len() as i64 || b_hi <= b_lo {
                    return None;
                }
                Some((b_lo as usize, b_hi as usize))
            },
            64,
            0,
        )
        .expect("pre lag fit");
        assert_eq!(
            pre.best_lag, true_lag,
            "expected lag {true_lag}, got {}",
            pre.best_lag
        );
        assert!(
            pre.residual_db < -60.0,
            "shifted copy should cancel, got {} dB",
            pre.residual_db
        );
    }

    #[test]
    fn seam_floor_probe_uses_border_when_energetic() {
        // A and B share the same master; the immediate border is energetic, so the floor probe
        // anchors on it (source = Border) and cancels deeply.
        let rate_frames = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;
        let standoff = 16usize;

        let b_mono: Vec<f64> = (0..rate_frames)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        // A = same master, half level (nominal map is exact → delta 0).
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: standoff,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: rate_frames,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let pre = seam_floor_probe(&params, SeamSide::Pre, gap_start, gap_end);
        assert_eq!(pre.source, SeamFloorSource::Border);
        assert!(
            pre.residual_db < -60.0,
            "border floor should cancel, got {}",
            pre.residual_db
        );

        let post = seam_floor_probe(&params, SeamSide::Post, gap_start, gap_end);
        assert_eq!(post.source, SeamFloorSource::Border);
        assert!(
            post.residual_db < -60.0,
            "post floor should cancel, got {}",
            post.residual_db
        );
    }

    #[test]
    fn seam_floor_probe_walks_past_quiet_border() {
        // The immediate pre-border is silent; the probe must walk outward to energetic audio.
        let total = 2000usize;
        let gap_start = 1200usize;
        let gap_end = 1400usize;
        let window = 128usize;
        let standoff = 16usize;

        let b_mono: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.23).sin() * 4000.0)
            .collect();
        let mut a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();
        // Silence the region just before the gap (the immediate border), forcing an outward walk.
        let quiet_lo = gap_start - standoff - window;
        for s in a_samples.iter_mut().take(gap_start).skip(quiet_lo) {
            *s = 0.0;
        }

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: standoff,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let pre = seam_floor_probe(&params, SeamSide::Pre, gap_start, gap_end);
        assert_eq!(
            pre.source,
            SeamFloorSource::Walked,
            "should walk past the quiet border"
        );
        assert!(
            pre.residual_db < -60.0,
            "walked floor should still cancel, got {}",
            pre.residual_db
        );
    }

    #[test]
    fn seam_chosen_and_floor_same_window_zero_headroom_when_nominal_correct() {
        // Same-master: A's border equals B (scaled). With chosen_delta == nominal_delta (correct
        // placement), chosen and floor are measured on the same raw window at the same mapping, so
        // both cancel deeply and headroom ≈ 0.
        let rate = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let b_mono: Vec<f64> = (0..rate)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0, // nominal == truth (same timeline)
            step_frames: window,
            max_walk_frames: rate,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let (chosen, floor) = seam_chosen_and_floor(&params, SeamSide::Pre, gap_start, gap_end, 0);
        assert_eq!(chosen.source, SeamFloorSource::Border);
        assert!(
            chosen.residual_db < -60.0,
            "chosen should cancel: {}",
            chosen.residual_db
        );
        assert!(
            floor.residual_db < -60.0,
            "floor should cancel: {}",
            floor.residual_db
        );

        let verdict = SeamResidualVerdict::from_parts(&chosen, &chosen, &floor, &floor);
        assert!(
            verdict.worst_headroom_db().abs() < 1.0,
            "headroom should be ~0 at correct placement, got {}",
            verdict.worst_headroom_db()
        );
        assert!(
            verdict.informative,
            "same-master floor should be informative"
        );
    }

    #[test]
    fn seam_chosen_and_floor_lag_center_low_headroom_within_reach() {
        // Same-master with a modest placement slide inside the lag radius: lag-centered chosen
        // should cancel like the floor (headroom ≈ 0).
        let rate = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;
        let slide = 50i64;

        let b_mono: Vec<f64> = (0..rate)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: rate,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let (chosen, floor) =
            seam_chosen_and_floor(&params, SeamSide::Pre, gap_start, gap_end, slide);
        assert!(
            floor.residual_db < -60.0,
            "floor should cancel: {}",
            floor.residual_db
        );
        assert!(
            chosen.residual_db < -60.0,
            "lag-centered chosen should cancel within reach: {}",
            chosen.residual_db
        );

        let verdict = SeamResidualVerdict::from_parts_with_placement(
            &chosen,
            &chosen,
            &floor,
            &floor,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
            slide as u64,
            512,
        );
        assert!(
            verdict.worst_headroom_db().abs() < 1.0,
            "headroom should be ~0 with lag centering, got {}",
            verdict.worst_headroom_db()
        );
        assert!(!verdict.beyond_lag_reach());
    }

    #[test]
    fn seam_chosen_and_floor_headroom_large_when_chosen_wrong() {
        // Two-region B: the reference window (near gap_start) is region-1 content; the floor maps to
        // region 1 (cancels) while a wrong chosen delta maps into region-2 (different) content that
        // does not cancel at any lag → large headroom.
        let half = 2000usize;
        let total = half * 2;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let b_mono: Vec<f64> = (0..total)
            .map(|i| {
                if i < half {
                    (i as f64 * 0.23).sin() * 4000.0
                } else {
                    (i as f64 * 0.71).sin() * 4000.0
                }
            })
            .collect();
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0, // nominal maps the region-1 window to region-1 → cancels
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        // Chosen delta maps the region-1 reference window into region-2 content (in bounds, ±512 lag
        // stays within region 2) → no cancellation.
        let (chosen, floor) =
            seam_chosen_and_floor(&params, SeamSide::Pre, gap_start, gap_end, half as i64);
        assert!(
            floor.residual_db < -60.0,
            "floor should cancel at nominal: {}",
            floor.residual_db
        );
        assert!(
            chosen.residual_db > -6.0,
            "chosen should not cancel into different content: {}",
            chosen.residual_db
        );
        let verdict = SeamResidualVerdict::from_parts(&chosen, &chosen, &floor, &floor);
        assert!(
            verdict.worst_headroom_db() > 40.0,
            "headroom should be large: {}",
            verdict.worst_headroom_db()
        );
        assert!(verdict.informative, "floor still cancels at nominal");
    }

    #[test]
    fn seam_chosen_and_floor_multichannel_follows_center_when_fronts_are_noise() {
        // Center-dominant 5.1-style: FC carries same-master signal; FL/FR carry noise that does NOT
        // cancel against B's (different) FL/FR noise. Per-channel residual on the selected center
        // cancels deeply; the mono downmix is polluted by the surrounds and cancels far worse.
        let total = 2000usize;
        let channels = 3usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

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
        let b_mono: Vec<f64> = (0..total)
            .map(|i| (fl_b[i] + fr_b[i] + fc_b[i]) / 3.0)
            .collect();

        // A: FC is B's center at half level (same master). FL/FR are *different* noise.
        let fc_a: Vec<f64> = fc_b.iter().map(|s| s * 0.5).collect();
        let fl_a: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.37).cos() * 2000.0)
            .collect();
        let fr_a: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.71).sin() * 2000.0)
            .collect();
        let a_samples = interleave_a(&[fl_a, fr_a, fc_a], 4000.0);

        let params = |window: usize| SeamFloorParams {
            a_samples: &a_samples,
            channels,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window.max(1),
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };

        // Selection follows the center (FL/FR noise is >20 dB below FC), as Pearson would score it.
        let a_pre_ch = vec![
            vec![0.01; window],
            vec![0.01; window],
            (0..window).map(|i| i as f64 + 1.0).collect(),
        ];
        assert_eq!(
            seam_score_channel_indices(&a_pre_ch, &a_pre_ch),
            vec![2],
            "center channel should be the only selected channel"
        );

        let mc_pre = seam_chosen_and_floor_multichannel(
            &params(window),
            &b_ch,
            &[2],
            SeamSide::Pre,
            gap_start,
            gap_end,
            0,
        );
        let mc_post = seam_chosen_and_floor_multichannel(
            &params(window),
            &b_ch,
            &[2],
            SeamSide::Post,
            gap_start,
            gap_end,
            0,
        );
        let mc = SeamResidualVerdict::from_channel_residuals(
            &mc_pre,
            &mc_post,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
            0,
            512,
        );

        let (chosen_pre, floor_pre) =
            seam_chosen_and_floor(&params(window), SeamSide::Pre, gap_start, gap_end, 0);
        let (chosen_post, floor_post) =
            seam_chosen_and_floor(&params(window), SeamSide::Post, gap_start, gap_end, 0);
        let mono =
            SeamResidualVerdict::from_parts(&chosen_pre, &chosen_post, &floor_pre, &floor_post);

        assert!(
            mc.worst_floor_db() < -40.0,
            "center channel should cancel deeply, got {}",
            mc.worst_floor_db()
        );
        assert!(
            mono.worst_floor_db() > mc.worst_floor_db() + 20.0,
            "mono downmix should cancel far worse than the center channel (mono={}, mc={})",
            mono.worst_floor_db(),
            mc.worst_floor_db()
        );
        assert!(
            mc.informative,
            "center cancellation establishes the same-master regime"
        );
        assert!(
            mc.worst_headroom_db().abs() < 1.0,
            "headroom at truth should be ~0, got {}",
            mc.worst_headroom_db()
        );
    }

    #[test]
    fn seam_chosen_and_floor_multichannel_stereo_equal_matches_mono() {
        // Stereo, both channels same-master and equal energy → both selected, and the per-channel
        // result matches the mono-downmix path (both cancel deeply, headroom ≈ 0).
        let total = 2000usize;
        let channels = 2usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let l_b: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0)
            .collect();
        let r_b: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.4).cos() * 4000.0)
            .collect();
        let b_ch = vec![l_b.clone(), r_b.clone()];
        let b_mono: Vec<f64> = (0..total).map(|i| (l_b[i] + r_b[i]) / 2.0).collect();
        let a_samples = interleave_a(
            &[
                l_b.iter().map(|s| s * 0.5).collect(),
                r_b.iter().map(|s| s * 0.5).collect(),
            ],
            4000.0,
        );

        let params = |window: usize| SeamFloorParams {
            a_samples: &a_samples,
            channels,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window.max(1),
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };

        let mc_pre = seam_chosen_and_floor_multichannel(
            &params(window),
            &b_ch,
            &[0, 1],
            SeamSide::Pre,
            gap_start,
            gap_end,
            0,
        );
        let mc_post = seam_chosen_and_floor_multichannel(
            &params(window),
            &b_ch,
            &[0, 1],
            SeamSide::Post,
            gap_start,
            gap_end,
            0,
        );
        let mc = SeamResidualVerdict::from_channel_residuals(
            &mc_pre,
            &mc_post,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
            0,
            512,
        );
        assert_eq!(mc_pre.len(), 2, "both stereo channels measured");

        let (cp, fp) = seam_chosen_and_floor(&params(window), SeamSide::Pre, gap_start, gap_end, 0);
        let (cq, fq) =
            seam_chosen_and_floor(&params(window), SeamSide::Post, gap_start, gap_end, 0);
        let mono = SeamResidualVerdict::from_parts(&cp, &cq, &fp, &fq);

        assert!(
            mc.worst_floor_db() < -40.0,
            "per-channel cancels: {}",
            mc.worst_floor_db()
        );
        assert!(
            mono.worst_floor_db() < -40.0,
            "mono cancels: {}",
            mono.worst_floor_db()
        );
        assert!(
            mc.worst_headroom_db().abs() < 1.0,
            "mc headroom ~0: {}",
            mc.worst_headroom_db()
        );
        assert!(
            mono.worst_headroom_db().abs() < 1.0,
            "mono headroom ~0: {}",
            mono.worst_headroom_db()
        );
        assert!(mc.informative && mono.informative);
    }

    #[test]
    fn seam_chosen_and_floor_multichannel_empty_selection_is_mono_fallback() {
        // Empty selection → single mono-downmix entry identical to `seam_chosen_and_floor`.
        let total = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let b_mono: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
            .collect();
        let a_samples: Vec<f32> = b_mono.iter().map(|&s| (s * 0.5 / 4000.0) as f32).collect();
        let b_ch = vec![b_mono.clone()];

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };

        let mc = seam_chosen_and_floor_multichannel(
            &params,
            &b_ch,
            &[],
            SeamSide::Pre,
            gap_start,
            gap_end,
            0,
        );
        let (chosen, floor) = seam_chosen_and_floor(&params, SeamSide::Pre, gap_start, gap_end, 0);

        assert_eq!(mc.len(), 1, "empty selection collapses to one mono entry");
        assert!((mc[0].floor.residual_db - floor.residual_db).abs() < 1e-9);
        assert!((mc[0].chosen.residual_db - chosen.residual_db).abs() < 1e-9);
        assert_eq!(mc[0].floor.source, floor.source);
    }

    #[test]
    fn seam_chosen_and_floor_multichannel_shared_lag_follows_matching_channel() {
        // The gap content lives in ch2 with a true A→B lag of +3; ch0 is *louder* but its B content
        // does not match A at all. The shared lag must come from the matching channel (ch2), and the
        // non-matching loud channel must be measured at that same shared lag — proving alignment is
        // not hijacked by the loudest channel. A naive mono downmix would let ch0's energy corrupt it.
        let total = 2200usize;
        let channels = 3usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;
        let true_lag = 3i64;

        // Broadband pseudo-random noise → sharply peaked autocorrelation (single, unambiguous lag).
        let prng = |seed: u32| -> Vec<f64> {
            let mut x = seed;
            (0..total)
                .map(|_| {
                    x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                    (x >> 8) as f64 / f64::from(1u32 << 24) * 8000.0 - 4000.0
                })
                .collect()
        };
        let sig = prng(12345);
        // ch2 B = signal; ch0 B = a *different* loud broadband waveform; ch1 silent.
        let b_ch2 = sig.clone();
        let b_ch0 = prng(999);
        let b_ch1 = vec![0.0f64; total];
        let b_ch = vec![b_ch0.clone(), b_ch1.clone(), b_ch2.clone()];
        let b_mono: Vec<f64> = (0..total)
            .map(|i| (b_ch0[i] + b_ch1[i] + b_ch2[i]) / 3.0)
            .collect();

        // A: ch2 is B's signal at half level, shifted by +3 frames (the true lag); ch0 is loud,
        // unrelated to ch0's B; ch1 silent.
        let shift = true_lag as usize;
        let a_ch2: Vec<f64> = (0..total)
            .map(|i| {
                if i + shift < total {
                    sig[i + shift] * 0.5
                } else {
                    0.0
                }
            })
            .collect();
        let a_ch0 = prng(7777);
        let a_samples = interleave_a(&[a_ch0, b_ch1.clone(), a_ch2], 4000.0);

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };

        // Both loud channels are selected (within 20 dB); ch1 is silent and excluded by the caller.
        let mc = seam_chosen_and_floor_multichannel(
            &params,
            &b_ch,
            &[0, 2],
            SeamSide::Pre,
            gap_start,
            gap_end,
            0,
        );
        let by_ch = |ch: usize| {
            mc.iter()
                .find(|c| c.channel == ch)
                .expect("channel present")
        };

        // Shared lag came from the matching channel: BOTH channels were measured at lag +3.
        assert_eq!(
            by_ch(2).floor.best_lag,
            true_lag,
            "matching channel sets the shared lag"
        );
        assert_eq!(
            by_ch(0).floor.best_lag,
            true_lag,
            "the loud non-matching channel is measured at the shared lag, not its own"
        );
        assert!(
            by_ch(2).floor.residual_db < -40.0,
            "matching channel cancels deeply at the shared lag, got {}",
            by_ch(2).floor.residual_db
        );
        assert!(
            by_ch(0).floor.residual_db > -6.0,
            "non-matching channel does not cancel, got {}",
            by_ch(0).floor.residual_db
        );

        let mc_post = seam_chosen_and_floor_multichannel(
            &params,
            &b_ch,
            &[0, 2],
            SeamSide::Post,
            gap_start,
            gap_end,
            0,
        );
        let verdict = SeamResidualVerdict::from_channel_residuals(
            &mc,
            &mc_post,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
            0,
            512,
        );
        assert!(
            verdict.informative,
            "matching channel establishes the same-master regime"
        );
    }

    #[test]
    fn from_channel_residuals_worst_headroom_and_best_floor_informative() {
        // Aggregation: a well-cancelling channel and a channel that cancels at nominal (low floor)
        // but not at the chosen placement (high chosen) → the bad channel drives `worst_headroom_db`.
        let good = SeamChannelResidual {
            channel: 0,
            chosen: probe_at(-50.0),
            floor: probe_at(-50.0),
        };
        let bad = SeamChannelResidual {
            channel: 2,
            chosen: probe_at(-2.0),
            floor: probe_at(-45.0),
        };
        let v =
            SeamResidualVerdict::from_channel_residuals(&[good, bad], &[good, bad], -15.0, 0, 0);
        assert!(
            (v.worst_headroom_db() - 43.0).abs() < 0.5,
            "worst channel should drive headroom, got {}",
            v.worst_headroom_db()
        );

        // Decoupling: a noisy surround whose floor never cancels (−3 dB > floor_ok) must NOT flip
        // `informative` off when another selected channel established the regime (best floor −50 dB).
        let center = SeamChannelResidual {
            channel: 2,
            chosen: probe_at(-50.0),
            floor: probe_at(-50.0),
        };
        let surround = SeamChannelResidual {
            channel: 4,
            chosen: probe_at(-3.0),
            floor: probe_at(-3.0),
        };
        let v2 = SeamResidualVerdict::from_channel_residuals(
            &[center, surround],
            &[center, surround],
            -15.0,
            0,
            0,
        );
        assert!(
            v2.informative,
            "best-floor channel establishes the regime; a noisy surround must not veto informative"
        );
    }

    #[test]
    fn residual_verdict_informative_boundary_at_floor_ok() {
        let deep = SeamFloorProbe {
            source: SeamFloorSource::Border,
            residual_db: -20.0,
            gain: 1.0,
            best_lag: 0,
        };
        let shallow = SeamFloorProbe {
            source: SeamFloorSource::Border,
            residual_db: -10.0,
            gain: 1.0,
            best_lag: 0,
        };
        let none = SeamFloorProbe::none();
        let floor_ok = -15.0;

        assert!(residual_verdict_informative(&deep, &deep, floor_ok));
        assert!(!residual_verdict_informative(&shallow, &deep, floor_ok));
        assert!(!residual_verdict_informative(&deep, &shallow, floor_ok));
        assert!(!residual_verdict_informative(&none, &none, floor_ok));
        assert!(residual_verdict_informative(&deep, &none, floor_ok));
    }

    #[test]
    fn seam_floor_probe_none_when_all_quiet() {
        // No energetic reference anywhere → source None.
        let total = 1000usize;
        let b_mono: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.2).sin() * 4000.0)
            .collect();
        let a_samples = vec![0.0f32; total];
        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 1,
            b_mono: &b_mono,
            window: 128,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: 128,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let pre = seam_floor_probe(&params, SeamSide::Pre, 600, 800);
        assert_eq!(pre.source, SeamFloorSource::None);
        assert!(pre.residual_db.is_nan());
        let none = SeamFloorProbe::none();
        let verdict = SeamResidualVerdict::from_parts(&none, &none, &none, &none);
        assert!(!verdict.informative);
    }

    #[test]
    fn seam_residual_verdict_partial_eq_treats_nan_as_equal() {
        let none = SeamFloorProbe::none();
        let a = SeamResidualVerdict::from_parts(&none, &none, &none, &none);
        let b = SeamResidualVerdict::from_parts(&none, &none, &none, &none);
        assert!(a.chosen_pre_db.is_nan() && a.floor_pre_db.is_nan());
        assert_eq!(a, b);
    }

    #[test]
    fn lsq_residual_ratio_abstains_when_b_silent() {
        let a: Vec<f64> = (0..16).map(|i| (i as f64 * 0.3).sin() * 1000.0).collect();
        let b = vec![0.0; 16];
        assert!(lsq_residual_ratio(&a, &b).is_none());
    }

    #[test]
    fn seam_residual_abstains_when_b_silent_at_placement() {
        // L13: silent B at every lag must not read as ~0 dB "no cancellation".
        let pre_window = 16usize;
        let start = 64usize;

        let b_mono = vec![0.0; 200];
        let a_pre: Vec<f64> = (0..pre_window)
            .map(|i| (i as f64 * 0.3).sin() * 1000.0)
            .collect();

        let fit = seam_residual_for_side(
            &a_pre,
            &b_mono,
            |lag| {
                let lo = start as i64 - pre_window as i64 + lag;
                let hi = start as i64 + lag;
                if lo < 0 || hi > b_mono.len() as i64 || hi <= lo {
                    return None;
                }
                Some((lo as usize, hi as usize))
            },
            64,
            0,
        );
        assert!(
            fit.is_none(),
            "silent B should abstain, not report ~0 dB residual"
        );
    }

    #[test]
    fn seam_residual_high_for_unrelated_audio() {
        // A's border is unrelated to B at the placement: residual stays near 0 dB (no cancellation).
        let pre_window = 16usize;
        let start = 64usize;

        let b_mono: Vec<f64> = (0..200).map(|i| (i as f64 * 0.3).sin() * 1000.0).collect();
        let a_pre: Vec<f64> = (0..pre_window)
            .map(|i| (i as f64 * 1.7 + 0.5).cos() * 1000.0)
            .collect();

        let pre = seam_residual_for_side(
            &a_pre,
            &b_mono,
            |lag| {
                let lo = start as i64 - pre_window as i64 + lag;
                let hi = start as i64 + lag;
                if lo < 0 || hi > b_mono.len() as i64 || hi <= lo {
                    return None;
                }
                Some((lo as usize, hi as usize))
            },
            64,
            0,
        )
        .expect("pre lag fit");
        assert!(
            pre.residual_db > -6.0,
            "unrelated audio should not cancel, got {} dB",
            pre.residual_db
        );
    }

    fn ch(channel: usize, chosen_db: f64, floor: SeamFloorProbe) -> SeamChannelResidual {
        SeamChannelResidual {
            channel,
            chosen: SeamFloorProbe {
                source: floor.source,
                residual_db: chosen_db,
                gain: 1.0,
                best_lag: 0,
            },
            floor,
        }
    }

    fn sourced(db: f64) -> SeamFloorProbe {
        SeamFloorProbe {
            source: SeamFloorSource::Walked,
            residual_db: db,
            gain: 1.0,
            best_lag: 0,
        }
    }

    #[test]
    fn side_uninformative_names_each_cause() {
        let floor_ok = -15.0;
        // No window found at all.
        assert_eq!(
            side_uninformative(&SeamFloorProbe::none(), floor_ok),
            Some(ResidualUninformative::NoReferenceWindow)
        );
        // Window found, fit produced nothing — `measure_a_win_at_delta` preserves `source`.
        assert_eq!(
            side_uninformative(&sourced(f64::NAN), floor_ok),
            Some(ResidualUninformative::ProbeNonFinite)
        );
        // Measured, above FLOOR_OK: a measurement, not an abstention.
        assert_eq!(
            side_uninformative(&sourced(-10.0), floor_ok),
            Some(ResidualUninformative::FloorAboveOkDb)
        );
        // Measured at/below FLOOR_OK — usable. The boundary is inclusive, matching
        // `floor_probe_informative`.
        assert_eq!(side_uninformative(&sourced(-15.0), floor_ok), None);
        assert_eq!(side_uninformative(&sourced(-40.0), floor_ok), None);
    }

    #[test]
    fn uninformative_reason_combine_rule() {
        let floor_ok = -15.0;
        let verdict = |pre: SeamFloorProbe, post: SeamFloorProbe, slide: u64, max_lag: i64| {
            SeamResidualVerdict::from_parts_with_placement(
                &sourced(-40.0),
                &sourced(-40.0),
                &pre,
                &post,
                floor_ok,
                slide,
                max_lag,
            )
        };

        // 1. Placement dominates — and note `informative` is *true* here: the reason is not the
        //    negation of the flag, it mirrors the gate guard.
        let beyond = verdict(sourced(-40.0), sourced(-40.0), 600, 480);
        assert!(beyond.informative && beyond.beyond_lag_reach());
        assert_eq!(
            beyond.uninformative_reason(),
            Some(ResidualUninformative::BeyondLagReach)
        );
        // Per-side detail may disagree with the combined value, by design.
        assert_eq!(
            verdict(sourced(-10.0), sourced(-40.0), 600, 480).uninformative_pre,
            Some(ResidualUninformative::FloorAboveOkDb)
        );

        // 2. The measured failure wins over the unmeasured side.
        assert_eq!(
            verdict(sourced(-10.0), SeamFloorProbe::none(), 0, 0).uninformative_reason(),
            Some(ResidualUninformative::FloorAboveOkDb)
        );
        // Tie between two measured failures prefers the less-measured one.
        assert_eq!(
            verdict(sourced(-10.0), sourced(f64::NAN), 0, 0).uninformative_reason(),
            Some(ResidualUninformative::ProbeNonFinite)
        );

        // 3. `NoReferenceWindow` only when nothing was measured.
        let none = verdict(SeamFloorProbe::none(), SeamFloorProbe::none(), 0, 0);
        assert!(!none.informative);
        assert_eq!(
            none.uninformative_reason(),
            Some(ResidualUninformative::NoReferenceWindow)
        );
        // One side unmeasured, the other usable ⇒ informative, and so no reason at all — the
        // ordering that `residual_verdict_informative`'s "unmeasured sides are ignored" forces.
        let half = verdict(SeamFloorProbe::none(), sourced(-40.0), 0, 0);
        assert!(half.informative);
        assert_eq!(half.uninformative_reason(), None);

        // Both usable.
        assert_eq!(
            verdict(sourced(-40.0), sourced(-40.0), 0, 0).uninformative_reason(),
            None
        );
    }

    #[test]
    fn multichannel_windows_found_but_b_out_of_coverage_is_probe_non_finite() {
        // §0.2's regression, end to end: the reference walk succeeds (A is energetic, `b_mono` is
        // full length) but the per-channel B buffers are too short to hold the window, so every
        // channel probe comes back sourced-with-NaN. That must read as `ProbeNonFinite` ("we found a
        // window, the fit produced nothing"), never as `NoReferenceWindow`, and `floor_source` must
        // keep saying where the window came from.
        let total = 2000usize;
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let window = 128usize;

        let l_b: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.17).sin() * 4000.0)
            .collect();
        let r_b: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.4).cos() * 4000.0)
            .collect();
        let b_mono: Vec<f64> = (0..total).map(|i| (l_b[i] + r_b[i]) / 2.0).collect();
        let a_samples = interleave_a(
            &[
                l_b.iter().map(|s| s * 0.5).collect(),
                r_b.iter().map(|s| s * 0.5).collect(),
            ],
            4000.0,
        );
        // The only difference from the healthy stereo fixture: B's per-channel buffers stop well
        // before the gap, so no window fits at any lag.
        let b_ch = vec![l_b[..100].to_vec(), r_b[..100].to_vec()];

        let params = SeamFloorParams {
            a_samples: &a_samples,
            channels: 2,
            b_mono: &b_mono,
            window,
            standoff_frames: 16,
            a_to_b_delta: 0,
            step_frames: window,
            max_walk_frames: total,
            absolute_silence_rms: 33.0 / 32767.0,
            max_lag_frames: 512,
        };
        let side = |s| {
            seam_chosen_and_floor_multichannel(&params, &b_ch, &[0, 1], s, gap_start, gap_end, 0)
        };
        let (pre, post) = (side(SeamSide::Pre), side(SeamSide::Post));
        assert_eq!(pre.len(), 2, "both channels measured");
        assert!(
            pre.iter().all(
                |c| c.floor.source != SeamFloorSource::None && !c.floor.residual_db.is_finite()
            ),
            "fixture must produce sourced-but-non-finite probes"
        );

        let v = SeamResidualVerdict::from_channel_residuals(
            &pre,
            &post,
            DEFAULT_RESIDUAL_FLOOR_OK_DB,
            0,
            512,
        );
        assert!(!v.informative);
        assert_eq!(
            v.uninformative_reason(),
            Some(ResidualUninformative::ProbeNonFinite)
        );
        assert_ne!(
            v.floor_source_pre,
            SeamFloorSource::None,
            "the summary must not rewrite a sourced floor to `none` (§1.4)"
        );
        assert_ne!(v.floor_source_post, SeamFloorSource::None);
    }

    #[test]
    fn summary_falls_back_to_sourced_floor_when_no_channel_has_finite_headroom() {
        // §1.4's broader case: the floor was genuinely measured; only the *chosen* probe is
        // non-finite. Keying the summary on headroom alone reported this as "no window ever found".
        let side = [
            ch(0, f64::NAN, sourced(-42.0)),
            ch(1, f64::NAN, sourced(-30.0)),
        ];
        let v = SeamResidualVerdict::from_channel_residuals(&side, &side, -15.0, 0, 0);

        assert_eq!(
            v.floor_source_pre,
            SeamFloorSource::Walked,
            "sourced floor must survive the summary"
        );
        assert!(
            (v.floor_pre_db - (-42.0)).abs() < 1e-9,
            "fallback reports the min-floor channel, got {}",
            v.floor_pre_db
        );
        assert!(
            !v.chosen_pre_db.is_finite(),
            "`chosen_db` comes from the same channel, not a leftover"
        );
        assert!(v.informative, "-42 dB establishes the regime");
        assert_eq!(
            v.uninformative_reason(),
            None,
            "a measured floor below FLOOR_OK is usable even with no headroom reading"
        );
        // The one scalar the gate reads is untouched by the fallback.
        assert!(
            v.worst_headroom_db().is_nan(),
            "headroom stays absent: {}",
            v.worst_headroom_db()
        );

        // No channel anchored a floor at all ⇒ still `none`, and the honest reason.
        let unmeasured = [ch(0, f64::NAN, SeamFloorProbe::none())];
        let v2 = SeamResidualVerdict::from_channel_residuals(&unmeasured, &unmeasured, -15.0, 0, 0);
        assert_eq!(v2.floor_source_pre, SeamFloorSource::None);
        assert_eq!(
            v2.uninformative_reason(),
            Some(ResidualUninformative::NoReferenceWindow)
        );
    }

    /// Phase 4: mono and multichannel agree on sourced-NaN measuredness (toward-MC §5.1).
    ///
    /// Cell `(ProbeNonFinite, regime-OK)`: both constructors ignore the failed fit like unmeasured;
    /// the regime-OK side keeps `informative` and the gate does not abstain. Per-side fields still
    /// name `ProbeNonFinite`.
    #[test]
    fn mono_and_multichannel_agree_on_sourced_nan_measuredness() {
        let floor_ok = -15.0;
        let nan_side = sourced(f64::NAN);
        let deep = sourced(-40.0);

        let mono = SeamResidualVerdict::from_parts_with_placement(
            &sourced(-30.0),
            &sourced(-40.0),
            &nan_side,
            &deep,
            floor_ok,
            0,
            480,
        );
        let multi = SeamResidualVerdict::from_channel_residuals(
            &[ch(0, -30.0, nan_side)],
            &[ch(0, -40.0, deep)],
            floor_ok,
            0,
            480,
        );

        assert_eq!(
            mono.uninformative_pre,
            Some(ResidualUninformative::ProbeNonFinite)
        );
        assert_eq!(
            multi.uninformative_pre,
            Some(ResidualUninformative::ProbeNonFinite)
        );
        assert_eq!(mono.uninformative_post, None);
        assert_eq!(multi.uninformative_post, None);

        assert!(
            mono.informative && !mono.gate_abstains(),
            "mono: ProbeNonFinite ignored → other side governs: {mono:?}"
        );
        assert!(
            multi.informative && !multi.gate_abstains(),
            "multichannel: ProbeNonFinite ignored → other side governs: {multi:?}"
        );
        assert_eq!(mono.informative, multi.informative);
        assert_eq!(mono.gate_abstains(), multi.gate_abstains());
        assert_eq!(mono.uninformative_reason(), multi.uninformative_reason());
        assert_eq!(mono.uninformative_reason(), None);
    }

    /// Shared policy: a ProbeNonFinite side must not widen the gate guard and kill a live veto.
    ///
    /// Both constructors ignore sourced-NaN like unmeasured. Per-side fields still name
    /// `ProbeNonFinite`. Letting that name reach the combined reason while the verdict stays
    /// informative would widen the guard past `!informative || beyond_lag_reach()`.
    #[test]
    fn asymmetric_probe_non_finite_side_does_not_widen_the_gate_guard() {
        // Pre: window found, fit produced nothing. Post: measured, and B is a bad match —
        // headroom +10 dB, well past any margin the gate uses.
        let nan_side = sourced(f64::NAN);
        let bad_floor = sourced(-40.0);
        let bad_chosen = sourced(-30.0);

        let mono = SeamResidualVerdict::from_parts_with_placement(
            &sourced(f64::NAN),
            &bad_chosen,
            &nan_side,
            &bad_floor,
            -15.0,
            0,
            480,
        );
        let multi = SeamResidualVerdict::from_channel_residuals(
            &[ch(0, f64::NAN, nan_side)],
            &[ch(0, -30.0, bad_floor)],
            -15.0,
            0,
            480,
        );

        for v in [mono, multi] {
            assert!(
                v.informative,
                "a side with no fitted floor is ignored, not a failure: {v:?}"
            );
            assert!(!v.beyond_lag_reach());
            assert_eq!(
                v.uninformative_pre,
                Some(ResidualUninformative::ProbeNonFinite),
                "the per-side field still names what happened"
            );
            assert_eq!(
                v.uninformative_reason(),
                None,
                "but the combined value must not claim the verdict is unusable"
            );
            assert_eq!(
                v.worst_headroom_db(),
                10.0,
                "the veto the gate would drop if the reason fired"
            );
        }
    }

    /// `uninformative_reason()` is `Some` exactly when the gate abstains — across every shape the
    /// reason field distinguishes, mono and multichannel. The gate reads `gate_abstains()` directly,
    /// so this pins the *reporting* side: a named reason always corresponds to a real abstention,
    /// and an abstention is always named.
    #[test]
    fn uninformative_reason_is_exactly_the_gate_guard() {
        let floor_ok = -15.0;
        let probes = [
            SeamFloorProbe::none(),
            sourced(f64::NAN),
            sourced(-10.0),
            sourced(-40.0),
        ];
        for pre in probes {
            for post in probes {
                for (slide, max_lag) in [(0u64, 0i64), (0, 480), (600, 480)] {
                    let mono = SeamResidualVerdict::from_parts_with_placement(
                        &sourced(-40.0),
                        &sourced(-40.0),
                        &pre,
                        &post,
                        floor_ok,
                        slide,
                        max_lag,
                    );
                    let multi = SeamResidualVerdict::from_channel_residuals(
                        &[ch(0, -40.0, pre)],
                        &[ch(0, -40.0, post)],
                        floor_ok,
                        slide,
                        max_lag,
                    );
                    for v in [mono, multi] {
                        assert_eq!(
                            v.uninformative_reason().is_some(),
                            v.gate_abstains(),
                            "reason/guard drift: {v:?}"
                        );
                        assert_eq!(
                            v.gate_abstains(),
                            !v.informative || v.beyond_lag_reach(),
                            "the guard is still the disjunction it always was: {v:?}"
                        );
                    }
                    assert_eq!(
                        mono.informative, multi.informative,
                        "cross-path informative: mono={mono:?} multi={multi:?}"
                    );
                    assert_eq!(
                        mono.gate_abstains(),
                        multi.gate_abstains(),
                        "cross-path gate_abstains: mono={mono:?} multi={multi:?}"
                    );
                    assert_eq!(
                        mono.uninformative_reason(),
                        multi.uninformative_reason(),
                        "cross-path reason: mono={mono:?} multi={multi:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn multichannel_reason_follows_min_floor_channel() {
        // A noisy surround (floor above FLOOR_OK) alongside a channel that established the regime:
        // `informative` follows the best-cancelling channel, so the reason must too — otherwise the
        // field would explain a channel the flag never consulted.
        let center = ch(2, -50.0, sourced(-50.0));
        let surround = ch(4, -3.0, sourced(-3.0));
        let v = SeamResidualVerdict::from_channel_residuals(
            &[center, surround],
            &[center, surround],
            -15.0,
            0,
            0,
        );
        assert!(v.informative);
        assert_eq!(v.uninformative_pre, None, "min-floor channel is usable");
        assert_eq!(v.uninformative_reason(), None);

        // Both channels above FLOOR_OK ⇒ the side is a measurement that failed, not an abstention.
        let quiet = ch(2, -10.0, sourced(-10.0));
        let v2 = SeamResidualVerdict::from_channel_residuals(
            &[quiet, surround],
            &[quiet, surround],
            -15.0,
            0,
            0,
        );
        assert!(!v2.informative);
        assert_eq!(
            v2.uninformative_reason(),
            Some(ResidualUninformative::FloorAboveOkDb)
        );
    }

    #[test]
    fn uninformative_reasons_are_absent_on_the_wire_for_clean_gaps() {
        let clean = SeamResidualVerdict::from_parts(
            &sourced(-50.0),
            &sourced(-50.0),
            &sourced(-40.0),
            &sourced(-40.0),
        );
        let json = serde_json::to_string(&clean).expect("serialize");
        assert!(
            !json.contains("uninformative"),
            "clean gaps must stay byte-identical on the wire: {json}"
        );

        let abstained = SeamResidualVerdict::from_parts(
            &sourced(-50.0),
            &sourced(-50.0),
            &SeamFloorProbe::none(),
            &sourced(f64::NAN),
        );
        let json = serde_json::to_string(&abstained).expect("serialize");
        assert!(
            json.contains("\"uninformative_pre\":\"no_reference_window\""),
            "{json}"
        );
        assert!(
            json.contains("\"uninformative_post\":\"probe_non_finite\""),
            "{json}"
        );
    }
}
