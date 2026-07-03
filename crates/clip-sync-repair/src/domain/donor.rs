//! Shared **donor-interior occupancy** — does B carry audio across the span that would fill A's hole?
//! Used by both the diagnostic `splice_dualfit` (scan) and the production dual-fit repair (A3), on the
//! **aligned** bridge span (bridges vs BROKEN) and the **nominal** geometry span (program-quiet, D11). One
//! implementation so the two paths can't drift.

use serde::{Deserialize, Serialize};

const SILENCE_FLOOR_DB: f32 = -120.0;
const DONOR_BIN_MS: f64 = 50.0;
/// A donor with no internal sub-floor run longer than this is treated as continuous (bridges the gap).
pub const DONOR_CONTINUITY_MS: f64 = 150.0;

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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DonorInterior {
    pub rms_db: f64,
    pub silence_fraction: f64,
    pub longest_silence_ms: f64,
    pub continuous: bool,
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
    let bin = ((DONOR_BIN_MS / 1000.0) * f64::from(sample_rate)).round().max(1.0) as usize;
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
    })
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
        assert!(d.continuous && d.silence_fraction < 0.05, "bridged donor: {d:?}");
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
}
