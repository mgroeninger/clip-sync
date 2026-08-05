//! Fill-level check — how loud the assembled fill is against the A material it sits between.
//!
//! **Record-only.** Nothing here vetoes a patch. A false positive on a veto costs an unrepaired
//! hole, which is worse than the thing being measured, so the number is recorded first and the
//! threshold is a calibration question for the corpus (see
//! `docs/dev/TEMP-equivalence-band-gate-off-findings.md` §7.4 item 1).
//!
//! **What it measures.** The audible failure this exists to catch is *substitution magnitude*: a
//! fill placed into quiet A material at a level the surrounding program never reaches. On the four
//! gaps measured by ear (§6.10.12) the fill ran 11–35 dB above what it replaced, and audibility
//! tracked the peak — one 100 ms bin at +35 dB was enough to make the patch sound worse than the
//! unpatched A. So the statistic is a **per-bin peak**, not a whole-fill aggregate: averaging over
//! the fill is exactly what hides a single loud bin.
//!
//! **Reduction is interleaved**, not a mono downmix: the two differ by 3–8 dB on 5.1 content, and
//! interleaved is what the scan envelope and the WAV analysis both used.

use serde::{Deserialize, Serialize};

/// Floor for the dB conversion. Digital silence has no dB value; clamping keeps every field finite
/// and comparable, and no real 100 ms bin of program material lands anywhere near it.
pub const FILL_LEVEL_FLOOR_DB: f64 = -120.0;

/// Bin width the peak is taken over. Matches `scan_block_ms`' default and the 100 ms grid the
/// §6.10.12 measurements were made on.
pub const DEFAULT_FILL_LEVEL_BIN_MS: f64 = 100.0;

/// How much A on each side of the gap defines "what the neighbourhood sounds like".
pub const DEFAULT_FILL_LEVEL_SHOULDER_SECS: f64 = 1.0;

/// Skipped at each shoulder's gap-facing edge. The gap edges carry the dropout's own ramp, and a
/// scan-detected edge is only accurate to a block; measuring right up to it reads the ramp instead
/// of the program.
pub const DEFAULT_FILL_LEVEL_STANDOFF_MS: f64 = 100.0;

/// Geometry for [`measure_fill_level`]. The defaults are the shipped ones; the fields exist so the
/// calibration tooling can sweep them without a second implementation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FillLevelParams {
    pub bin_ms: f64,
    pub shoulder_secs: f64,
    pub standoff_ms: f64,
    pub floor_db: f64,
}

impl Default for FillLevelParams {
    fn default() -> Self {
        Self {
            bin_ms: DEFAULT_FILL_LEVEL_BIN_MS,
            shoulder_secs: DEFAULT_FILL_LEVEL_SHOULDER_SECS,
            standoff_ms: DEFAULT_FILL_LEVEL_STANDOFF_MS,
            floor_db: FILL_LEVEL_FLOOR_DB,
        }
    }
}

/// What the fill's loudest bin costs relative to its A neighbourhood. Report-only.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FillLevelCheck {
    /// Interleaved RMS of the pre-gap A shoulder, dBFS. `None` when there is no room for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_shoulder_db: Option<f64>,
    /// Interleaved RMS of the post-gap A shoulder, dBFS. `None` when there is no room for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_shoulder_db: Option<f64>,
    /// The **louder** shoulder — the level the fill is judged against. Taking the louder side is
    /// the conservative choice for anything that later reads this as a threshold: a fill only
    /// looks bad here if it beats both sides of its own neighbourhood.
    pub reference_db: f64,
    /// Level of the fill's loudest bin, dBFS.
    pub peak_bin_db: f64,
    /// `peak_bin_db - reference_db`. Positive means the fill is louder than the A around it.
    pub peak_delta_db: f64,
    /// Which bin that was, from the start of the fill.
    pub peak_bin_index: usize,
    /// Bins measured (a trailing partial bin under half width is dropped).
    pub bins: usize,
    pub bin_ms: f64,
}

/// Interleaved RMS of a sample slice, in dBFS, clamped at `floor_db`.
fn rms_db(samples: &[f32], floor_db: f64) -> f64 {
    if samples.is_empty() {
        return floor_db;
    }
    let sum_sq: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let rms = (sum_sq / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        return floor_db;
    }
    (20.0 * rms.log10()).max(floor_db)
}

/// Measure the assembled fill against the A shoulders either side of the gap it replaces.
///
/// `fill` is the interleaved PCM **as it will be written** — apply the patch gain before calling,
/// or the number describes something the listener never hears. `a_samples` is A's interleaved PCM
/// and `gap_start_frame`/`gap_end_frame` are the refined gap on that same timeline.
///
/// Returns `None` when there is nothing to compare: an empty fill, a zero channel count or sample
/// rate, or no usable shoulder on either side.
pub fn measure_fill_level(
    fill: &[f32],
    a_samples: &[f32],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    sample_rate: u32,
    params: FillLevelParams,
) -> Option<FillLevelCheck> {
    if fill.is_empty() || channels == 0 || sample_rate == 0 || params.bin_ms <= 0.0 {
        return None;
    }
    let rate = f64::from(sample_rate);
    let shoulder_frames = (params.shoulder_secs * rate).round() as usize;
    let standoff_frames = (params.standoff_ms / 1000.0 * rate).round() as usize;
    let a_frames = a_samples.len() / channels;
    if shoulder_frames == 0 || a_frames == 0 {
        return None;
    }

    // Pre shoulder: [start - standoff - shoulder, start - standoff). Post: mirrored past the end.
    let pre_end = gap_start_frame.saturating_sub(standoff_frames);
    let pre_start = pre_end.saturating_sub(shoulder_frames);
    let pre_shoulder_db = (pre_end > pre_start && pre_end <= a_frames).then(|| {
        rms_db(
            &a_samples[pre_start * channels..pre_end * channels],
            params.floor_db,
        )
    });

    let post_start = (gap_end_frame + standoff_frames).min(a_frames);
    let post_end = (post_start + shoulder_frames).min(a_frames);
    let post_shoulder_db = (post_end > post_start).then(|| {
        rms_db(
            &a_samples[post_start * channels..post_end * channels],
            params.floor_db,
        )
    });

    let reference_db = match (pre_shoulder_db, post_shoulder_db) {
        (Some(pre), Some(post)) => pre.max(post),
        (Some(one), None) | (None, Some(one)) => one,
        (None, None) => return None,
    };

    let bin_frames = ((params.bin_ms / 1000.0) * rate).round().max(1.0) as usize;
    let bin_samples = bin_frames * channels;
    let mut bins = 0usize;
    let mut peak_bin_db = params.floor_db;
    let mut peak_bin_index = 0usize;
    for (index, chunk) in fill.chunks(bin_samples).enumerate() {
        // A trailing sliver is dropped: an RMS over a handful of samples is noise, and this
        // statistic is a peak, so noise on the tail would be the value that gets reported.
        if chunk.len() * 2 < bin_samples {
            break;
        }
        let db = rms_db(chunk, params.floor_db);
        if bins == 0 || db > peak_bin_db {
            peak_bin_db = db;
            peak_bin_index = index;
        }
        bins += 1;
    }
    if bins == 0 {
        return None;
    }

    Some(FillLevelCheck {
        pre_shoulder_db,
        post_shoulder_db,
        reference_db,
        peak_bin_db,
        peak_delta_db: peak_bin_db - reference_db,
        peak_bin_index,
        bins,
        bin_ms: params.bin_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000;

    /// Interleaved constant-amplitude PCM.
    fn tone(frames: usize, channels: usize, amp: f32) -> Vec<f32> {
        vec![amp; frames * channels]
    }

    /// A with a `gap_frames` hole at `gap_start`, shoulders at `amp`.
    fn a_with_hole(
        total: usize,
        channels: usize,
        amp: f32,
        gap: std::ops::Range<usize>,
    ) -> Vec<f32> {
        let mut a = tone(total, channels, amp);
        for frame in gap {
            for ch in 0..channels {
                a[frame * channels + ch] = 0.0;
            }
        }
        a
    }

    #[test]
    fn a_fill_matching_its_shoulders_reads_zero_delta() {
        let a = a_with_hole(4000, 2, 0.1, 2000..2500);
        let fill = tone(500, 2, 0.1);
        let check = measure_fill_level(&fill, &a, 2, 2000, 2500, RATE, FillLevelParams::default())
            .expect("shoulders exist either side");
        assert!(check.peak_delta_db.abs() < 1e-9, "{check:?}");
        assert_eq!(check.bins, 5, "500 frames at 100 ms of 1 kHz: {check:?}");
    }

    /// The §6.10.12 shape: a fill 20 dB over quiet neighbours.
    #[test]
    fn a_fill_louder_than_its_shoulders_reads_the_gain_in_db() {
        let a = a_with_hole(4000, 2, 0.01, 2000..2500);
        let fill = tone(500, 2, 0.1);
        let check = measure_fill_level(&fill, &a, 2, 2000, 2500, RATE, FillLevelParams::default())
            .expect("shoulders exist either side");
        assert!((check.peak_delta_db - 20.0).abs() < 0.01, "{check:?}");
    }

    /// One loud bin in an otherwise matched fill is the whole point: an aggregate would average it
    /// away, the peak reports it.
    #[test]
    fn one_loud_bin_sets_the_peak_and_is_located() {
        let a = a_with_hole(4000, 2, 0.1, 2000..2500);
        let mut fill = tone(500, 2, 0.1);
        for sample in fill[300 * 2..400 * 2].iter_mut() {
            *sample = 1.0;
        }
        let check = measure_fill_level(&fill, &a, 2, 2000, 2500, RATE, FillLevelParams::default())
            .expect("shoulders exist either side");
        assert_eq!(check.peak_bin_index, 3, "{check:?}");
        assert!((check.peak_delta_db - 20.0).abs() < 0.01, "{check:?}");
    }

    /// The reference is the louder shoulder, so a fill sitting between one quiet and one loud
    /// neighbour is judged against the loud one — it takes beating *both* sides to look bad.
    #[test]
    fn the_reference_is_the_louder_shoulder() {
        let mut a = a_with_hole(4000, 1, 0.01, 2000..2500);
        for sample in a[2600..4000].iter_mut() {
            *sample = 0.1;
        }
        let check = measure_fill_level(
            &tone(500, 1, 0.1),
            &a,
            1,
            2000,
            2500,
            RATE,
            FillLevelParams::default(),
        )
        .expect("shoulders exist either side");
        let pre = check.pre_shoulder_db.expect("pre shoulder");
        let post = check.post_shoulder_db.expect("post shoulder");
        assert!(post > pre, "{check:?}");
        assert!((check.reference_db - post).abs() < 1e-9, "{check:?}");
        assert!(
            check.peak_delta_db.abs() < 0.01,
            "matched to the loud side: {check:?}"
        );
    }

    #[test]
    fn a_gap_at_the_very_start_still_measures_from_the_post_shoulder() {
        let a = a_with_hole(2000, 1, 0.1, 0..500);
        let check = measure_fill_level(
            &tone(500, 1, 0.1),
            &a,
            1,
            0,
            500,
            RATE,
            FillLevelParams::default(),
        )
        .expect("post shoulder alone is enough");
        assert!(check.pre_shoulder_db.is_none(), "{check:?}");
        assert!(check.post_shoulder_db.is_some(), "{check:?}");
    }

    #[test]
    fn no_shoulder_on_either_side_declines_to_measure() {
        // A is the gap and nothing else.
        let a = vec![0.0f32; 500];
        assert!(measure_fill_level(
            &tone(500, 1, 0.1),
            &a,
            1,
            0,
            500,
            RATE,
            FillLevelParams::default()
        )
        .is_none());
    }

    #[test]
    fn a_digitally_silent_fill_floors_rather_than_diverging() {
        let a = a_with_hole(4000, 1, 0.1, 2000..2500);
        let check = measure_fill_level(
            &tone(500, 1, 0.0),
            &a,
            1,
            2000,
            2500,
            RATE,
            FillLevelParams::default(),
        )
        .expect("shoulders exist either side");
        assert_eq!(check.peak_bin_db, FILL_LEVEL_FLOOR_DB, "{check:?}");
        assert!(check.peak_delta_db.is_finite(), "{check:?}");
    }

    #[test]
    fn a_trailing_sliver_shorter_than_half_a_bin_is_dropped() {
        let a = a_with_hole(4000, 1, 0.1, 2000..2540);
        // 540 frames = 5 full bins + 40 frames (under half of 100).
        let check = measure_fill_level(
            &tone(540, 1, 0.1),
            &a,
            1,
            2000,
            2540,
            RATE,
            FillLevelParams::default(),
        )
        .expect("shoulders exist either side");
        assert_eq!(check.bins, 5, "{check:?}");
    }

    #[test]
    fn an_empty_fill_declines_to_measure() {
        let a = a_with_hole(4000, 1, 0.1, 2000..2500);
        assert!(
            measure_fill_level(&[], &a, 1, 2000, 2500, RATE, FillLevelParams::default()).is_none()
        );
    }
}
