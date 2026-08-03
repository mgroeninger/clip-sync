//! Shared **donor-interior occupancy** — does B carry audio across the span that would fill A's hole?
//! Used by both the diagnostic `splice_dualfit` (scan) and the production dual-fit repair (A3), on the
//! **aligned** bridge span (bridges vs BROKEN) and the **nominal** geometry span (program-quiet, D11). One
//! implementation so the two paths can't drift.

use crate::domain::gap_equivalence::ChannelReduction;
use serde::{Deserialize, Serialize};

const SILENCE_FLOOR_DB: f32 = -120.0;
/// Bin width every reading on this path uses — stamped onto [`DonorInteriorBasis::bin_ms`] rather than
/// restated at the call sites, so the constant and the provenance cannot drift apart.
pub const DONOR_BIN_MS: f64 = 50.0;
/// A donor with no internal sub-floor run longer than this is treated as continuous (bridges the gap).
pub const DONOR_CONTINUITY_MS: f64 = 150.0;
/// B `silence_fraction` at the **nominal** `b_mapped` span at/above this ⇒ program-quiet (D11/G5) — quiet in
/// both masters, nothing to fill. Calibrated on the re-anchor corpus (bimodal cluster ≥0.83 vs dropouts ≈0).
pub const PROGRAM_QUIET_SILENCE_FRAC: f64 = 0.5;

fn to_db(rms: f32) -> f32 {
    if rms <= 1e-9 {
        SILENCE_FLOOR_DB
    } else {
        20.0 * rms.log10()
    }
}

/// **Donor-interior energy** over a B span mapped to fill a gap. The gap is a hole in A; this measures
/// whether **B carries audio there** — the donor half of the fill predicate (§4). `silence_fraction` /
/// `longest_silence_ms` come from 50 ms RMS bins vs the gap floor; `continuous` ⇒ no internal sub-floor run
/// longer than [`DONOR_CONTINUITY_MS`], i.e. B bridges the hole unbroken.
///
/// **`silence_fraction` is not comparable to the scan gate's `donor_silence_fraction`.** Both carry that
/// name and both count "silent donor bins ÷ total", but they are measured on different bases — 50 ms mono
/// bins against the loudest bin *anywhere* in the gap span here, versus 100 ms interleaved blocks against
/// the loudest **silent** block there. On a hold-bridged gap the two floors diverge by tens of dB (56 dB
/// observed on the 2026-08-03 run), and the fractions diverge in proportion. [`DonorInteriorBasis`] is
/// stamped on every reading so that divergence is visible in the dump instead of only in this doc comment;
/// the scan side records the same thing via `GapEquivalenceVerdict::gap_floor_db` / `measurement`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DonorInterior {
    pub rms_db: f64,
    pub silence_fraction: f64,
    pub longest_silence_ms: f64,
    pub continuous: bool,
    /// What this reading was measured on — see [`DonorInteriorBasis`]. `None` only on corpora written
    /// before the basis was stamped (2026-08-03); those are the dumps whose fractions cannot be
    /// safely compared against anything.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub basis: Option<DonorInteriorBasis>,
}

/// The measurement basis behind one [`DonorInterior`] — bin width, channel reduction, and the floor the
/// bins were thresholded against.
///
/// Recorded for the same reason [`crate::domain::gap_equivalence::GapEquivalenceThresholds`] is: every
/// input to `silence_fraction` was emitted except the ones it was *compared against*, so a reader could
/// only interpret it by assuming the constants in force on the day of the dump. Unlike the scan gate's
/// thresholds these are not configurable, but they do differ **between paths**, which is the failure mode
/// that actually bit: a nominal-vs-aligned comparison within one basis is sound, a scan-vs-fingerprint
/// comparison across two is not, and nothing in the file distinguished them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DonorInteriorBasis {
    /// Bin width the span was chopped into — [`DONOR_BIN_MS`] on this path.
    pub bin_ms: f64,
    /// Channel reduction behind each bin's RMS. Mono downmix on this path, which reads ~`10·log10(N)`
    /// quieter than the scan's interleaved reduction on uncorrelated multichannel content (7.8 dB at 6
    /// channels, confirmed by the corpus's own `noise_floor_probes`).
    pub reduction: ChannelReduction,
    /// The `gap_floor_db` each bin was tested against (`rms < floor ⇒ silent`). The single number that
    /// makes two `silence_fraction`s comparable or not.
    pub floor_db: f64,
}

/// Donor-interior energy of `b_mono` over `[start_frame, end_frame)`. For the **aligned** bridge the caller
/// passes the sequentially-registered shoulders (`b_mapped_start + L_pre`, `b_mapped_end + L_post`); for the
/// registration-independent program-quiet read, the **nominal** span (`b_mapped_start .. + gap_frames`).
/// `None` for an empty/over-range span.
pub fn donor_interior_at(
    b_mono: &[f64],
    start_frame: usize,
    end_frame: usize,
    gap_floor_db: f64,
    sample_rate: u32,
) -> Option<DonorInterior> {
    let end = end_frame.min(b_mono.len());
    if start_frame >= end {
        return None;
    }
    let span = &b_mono[start_frame..end];
    let rms = (span.iter().map(|v| v * v).sum::<f64>() / span.len() as f64).sqrt();
    let bin = ((DONOR_BIN_MS / 1000.0) * f64::from(sample_rate))
        .round()
        .max(1.0) as usize;
    let floor_amp = 10f64.powf(gap_floor_db / 20.0);
    let (mut total, mut silent, mut run, mut longest) = (0usize, 0usize, 0usize, 0usize);
    for chunk in span.chunks(bin) {
        let r = (chunk.iter().map(|v| v * v).sum::<f64>() / chunk.len() as f64).sqrt();
        total += 1;
        if r < floor_amp {
            silent += 1;
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let longest_silence_ms = longest as f64 * DONOR_BIN_MS;
    Some(DonorInterior {
        rms_db: f64::from(to_db(rms as f32)),
        silence_fraction: silent as f64 / total.max(1) as f64,
        longest_silence_ms,
        continuous: longest_silence_ms < DONOR_CONTINUITY_MS,
        // Taken from the values this call actually used, not from the constants — `floor_db` is the
        // caller's argument, and a caller passing a floor from a different basis is exactly what the
        // stamp exists to expose.
        basis: Some(DonorInteriorBasis {
            bin_ms: DONOR_BIN_MS,
            reduction: ChannelReduction::Downmix,
            floor_db: gap_floor_db,
        }),
    })
}

/// Registration-independent **program-quiet** label (D11): is B mostly silent at the nominal geometry
/// `b_mapped` span (no per-shoulder lag), measured against A's gap floor?
///
/// Used by the gap-fingerprint analyzer (`donor_interior_nominal`), dual-fit donor decline, and corpus
/// metrics — **not** as a production pre-gate skip (nominal-hole silence alone cannot distinguish true
/// program-quiet from patchable quiet-content pauses).
pub fn program_quiet_at_nominal(
    b_mono: &[f64],
    b_mapped_start: usize,
    gap_frames: usize,
    gap_floor_db: f64,
    sample_rate: u32,
) -> bool {
    let end = b_mapped_start.saturating_add(gap_frames);
    donor_interior_at(b_mono, b_mapped_start, end, gap_floor_db, sample_rate)
        .is_some_and(|d| d.silence_fraction >= PROGRAM_QUIET_SILENCE_FRAC)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn donor_interior_detects_bridge_vs_hole() {
        let sr = 48_000u32;
        let half = sr as usize / 2; // 0.5 s
        let floor_db = -60.0;
        let tone: Vec<f64> = (0..half).map(|i| 0.3 * (i as f64 * 0.05).sin()).collect();

        // Continuous donor: B carries audio across the whole span → bridges the hole.
        let d = donor_interior_at(&tone, 0, half, floor_db, sr).expect("donor");
        assert!(
            d.continuous && d.silence_fraction < 0.05,
            "bridged donor: {d:?}"
        );
        assert!(d.rms_db > floor_db);

        // Donor with its OWN hole: 250 ms of silence inside the span breaks continuity.
        let mut holed = tone.clone();
        for v in holed.iter_mut().take(half).skip(sr as usize / 4) {
            *v = 0.0;
        }
        let d2 = donor_interior_at(&holed, 0, half, floor_db, sr).expect("donor");
        assert!(!d2.continuous, "internal silence breaks continuity: {d2:?}");
        assert!(d2.longest_silence_ms >= DONOR_CONTINUITY_MS, "{d2:?}");

        // Empty / over-range spans guarded.
        assert!(donor_interior_at(&tone, 10, 0, floor_db, sr).is_none());
    }

    #[test]
    fn program_quiet_at_nominal_detects_silent_b_span() {
        let sr = 48_000u32;
        let n = sr as usize / 2;
        let silent: Vec<f64> = vec![0.0; n];
        let floor_db = -60.0;
        assert!(program_quiet_at_nominal(&silent, 0, n, floor_db, sr));
        let tone: Vec<f64> = (0..n).map(|i| 0.3 * (i as f64 * 0.05).sin()).collect();
        assert!(!program_quiet_at_nominal(&tone, 0, n, floor_db, sr));
    }
}
