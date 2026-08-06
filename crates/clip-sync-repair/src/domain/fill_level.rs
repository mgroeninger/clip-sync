//! Fill-level check — how loud the assembled fill is against the A material it sits between.
//!
//! **Record-only.** Nothing here vetoes a patch. A false positive on a veto costs an unrepaired
//! hole, which is worse than the thing being measured, so the number is recorded first and the
//! threshold is a calibration question for the corpus (see [docs/pipeline.md](docs/pipeline.md)
//! § Fill-level check / [docs/json-output.md](docs/json-output.md) § FillLevelCheck).
//!
//! **What it measures.** The audible failure this exists to catch is *substitution magnitude*: a
//! fill placed into quiet A material at a level the surrounding program never reaches. On ear-checked
//! cases the fill ran 11–35 dB above what it replaced, and audibility tracked the peak — one 100 ms
//! bin at +35 dB was enough to make the patch sound worse than the unpatched A. So the statistic is
//! a **per-bin peak**, not a whole-fill aggregate: averaging over the fill is exactly what hides a
//! single loud bin.
//!
//! **`peak_delta_db` alone does not order audibility** — 2026-08-06 ear labels over the 39-pair
//! corpus refuted it. The corpus maximum (+24.34 dB) is clean; the one clip heard as too loud
//! (+15.93 dB) sits 0.6 dB from a clean one whose every emitted field matched it. What separated
//! them was *where* the fill's excess lived. The audible one was uniformly above its neighbourhood,
//! seams included, so the ear had A's own ongoing material either side to compare against. The clean
//! +24 dB one was a drop inside silence: quiet at both seams with a loud event in its middle, which
//! is what the missing content actually was. So the peak is kept and two shape fields are recorded
//! beside it — [`FillLevelCheck::edge_delta_db`], which asks whether the fill is hot *at its seams*
//! rather than somewhere inside, and [`FillLevelCheck::reference_spread_db`], which asks whether the
//! neighbourhood being compared against is uniform or sparse. Both are record-only, like the peak.
//!
//! **Reduction is interleaved**, not a mono downmix: the two differ by 3–8 dB on 5.1 content, and
//! interleaved is what the scan envelope and the WAV analysis both used.
//!
//! **The crossfade is ignored.** `apply_seam_crossfade` writes the fill offset by the crossfade
//! width and blends that many frames into A at each seam, so the fill's first and last bins are
//! measured as pure fill while what lands in A is a blend, and the bin grid sits one crossfade
//! width off A's timeline. At the 10 ms default against 100 ms bins that is under a tenth of one
//! edge bin; it scales with `--crossfade-ms`, so a run at a large crossfade should not have its
//! edge bins read as gospel. Excluding the seam frames was considered and rejected: it would make
//! the statistic depend on splice geometry the caller would then have to thread through, to move a
//! number that the peak bin is almost never in.

use serde::{Deserialize, Serialize};

/// Floor for the dB conversion. Digital silence has no dB value; clamping keeps every field finite
/// and comparable, and no real 100 ms bin of program material lands anywhere near it.
pub const FILL_LEVEL_FLOOR_DB: f64 = -120.0;

/// Bin width the peak is taken over. Matches `scan_block_ms`' default and the 100 ms grid the
/// §6.10.12 measurements were made on.
pub const DEFAULT_FILL_LEVEL_BIN_MS: f64 = 100.0;

/// How much A on each side of the gap defines "what the neighbourhood sounds like".
pub const DEFAULT_FILL_LEVEL_SHOULDER_SECS: f64 = 1.0;

/// A shoulder shorter than this fraction of the requested width is declined rather than measured.
/// Near the head or tail of the media the available room saturates to whatever is left, and a
/// 20 ms sliver would otherwise carry the same authority as a full second while being far noisier
/// — the same reasoning that drops the fill's trailing part-bin, applied to the other side of the
/// comparison. Head-of-media gaps are not hypothetical: 7 of the 9 candidates in §6.10.6 are.
const MIN_SHOULDER_FRACTION: f64 = 0.5;

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
    /// Interleaved RMS of the pre-gap A shoulder, dBFS. `None` when there is not enough room for
    /// one (see [`MIN_SHOULDER_FRACTION`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_shoulder_db: Option<f64>,
    /// Interleaved RMS of the post-gap A shoulder, dBFS. `None` when there is not enough room for
    /// one (see [`MIN_SHOULDER_FRACTION`]).
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
    /// The reference itself bottomed out at [`FillLevelParams::floor_db`] — every shoulder that
    /// was measurable is digital silence. Usually a **neighbouring dropout** inside the shoulder
    /// window, not a judgement about the fill: the delta then reports the fill's absolute level
    /// plus 120 dB and means nothing about substitution magnitude. Calibration must exclude these
    /// rows or the histogram grows a tail made entirely of artifact.
    pub reference_at_floor: bool,
    /// Which bin that was, from the start of the fill.
    pub peak_bin_index: usize,
    /// Level of the fill's first measured bin, dBFS — what lands against A at the head seam.
    pub head_bin_db: f64,
    /// Level of the fill's last measured bin, dBFS — what lands against A at the tail seam.
    pub tail_bin_db: f64,
    /// `min(head_bin_db, tail_bin_db) - reference_db`: how far the fill sits above its neighbourhood
    /// **at both seams**, where the ear has A's own material directly either side to compare with.
    ///
    /// Read against `peak_delta_db`, this says where the fill's excess lives. Both large together
    /// is a fill that is uniformly hot — the shape of the one clip in the 2026-08-06 labels heard as
    /// too loud. A large `peak_delta_db` with `edge_delta_db` near zero is a fill that meets A at
    /// both seams and contains a loud event in its middle, which is what a dropout inside silence
    /// looks like when it has been repaired *correctly* — the +24.34 dB corpus maximum has this
    /// shape and was heard as clean.
    ///
    /// Taking the quieter seam means it takes both seams sitting high to look bad, the same
    /// conservatism as `reference_db` taking the louder shoulder. Two caveats. The seam bins are the
    /// ones the crossfade actually blends (see the module note), so this field is more sensitive to
    /// `--crossfade-ms` than the peak is. And on a fill of one bin the head, tail, and peak are all
    /// the same bin, so `edge_delta_db == peak_delta_db` says nothing.
    pub edge_delta_db: f64,
    /// Spread of the reference shoulder's own bins: its loudest bin minus its median bin, in dB, on
    /// the same grid as the fill. `None` when the shoulder is under one bin wide.
    ///
    /// Conditions how much the comparison is worth. Near zero is a uniform neighbourhood — room
    /// tone, or steady program — where `reference_db` genuinely predicts what belongs in the gap. A
    /// large spread is a sparse one, a quiet pocket with something occasional in it, where the
    /// shoulder's level is an artifact of whether that something happened to fall inside the window
    /// and predicts the gap's contents much more weakly. It is recorded rather than applied because
    /// which way it cuts is exactly what the corpus has not yet shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_spread_db: Option<f64>,
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

/// Per-bin interleaved RMS in dBFS, on the grid the fill's peak is taken on. A trailing part-bin
/// under half width is dropped for the same reason it is dropped from the fill: an RMS over a
/// handful of samples is noise, and every statistic built on these bins is an extremum.
fn bin_levels(samples: &[f32], bin_samples: usize, floor_db: f64) -> Vec<f64> {
    if bin_samples == 0 {
        return Vec::new();
    }
    samples
        .chunks(bin_samples)
        .take_while(|chunk| chunk.len() * 2 >= bin_samples)
        .map(|chunk| rms_db(chunk, floor_db))
        .collect()
}

/// Median of already-collected bin levels. Median rather than mean because the thing it is used to
/// characterize — a quiet pocket with one loud event in it — is exactly the shape a mean smears.
fn median_db(levels: &[f64]) -> Option<f64> {
    if levels.is_empty() {
        return None;
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("bin levels are finite"));
    let mid = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    })
}

/// Measure the assembled fill against the A shoulders either side of the gap it replaces.
///
/// `fill` is the interleaved PCM **as it will be written** — gain applied and truncated to the
/// destination span, or the number describes something the listener never hears. In the repair
/// path that is exactly what `gained_fill` returns. `a_samples` is A's interleaved PCM
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

    // A shoulder is used only if it is at least half the requested width; a saturated sliver at the
    // head or tail of the media is declined rather than measured.
    let min_shoulder_frames =
        ((shoulder_frames as f64 * MIN_SHOULDER_FRACTION).ceil() as usize).max(1);

    // Pre shoulder: [start - standoff - shoulder, start - standoff). Post: mirrored past the end.
    let pre_end = gap_start_frame
        .min(a_frames)
        .saturating_sub(standoff_frames);
    let pre_start = pre_end.saturating_sub(shoulder_frames);
    let pre_shoulder_db = (pre_end - pre_start >= min_shoulder_frames).then(|| {
        rms_db(
            &a_samples[pre_start * channels..pre_end * channels],
            params.floor_db,
        )
    });

    let post_start = (gap_end_frame + standoff_frames).min(a_frames);
    let post_end = (post_start + shoulder_frames).min(a_frames);
    let post_shoulder_db = (post_end - post_start >= min_shoulder_frames).then(|| {
        rms_db(
            &a_samples[post_start * channels..post_end * channels],
            params.floor_db,
        )
    });

    // The reference shoulder's *range* is carried, not just its level: `reference_spread_db`
    // characterizes the same shoulder the fill is being judged against, not the other one.
    let mut shoulders: Vec<(f64, std::ops::Range<usize>)> = Vec::new();
    if let Some(db) = pre_shoulder_db {
        shoulders.push((db, pre_start..pre_end));
    }
    if let Some(db) = post_shoulder_db {
        shoulders.push((db, post_start..post_end));
    }
    // `max_by` keeps the *last* maximum; reversing makes a tie resolve to the pre shoulder, matching
    // the `pre.max(post)` this replaced.
    let (reference_db, reference_range) = shoulders
        .into_iter()
        .rev()
        .max_by(|(a, _), (b, _)| a.partial_cmp(b).expect("shoulder levels are finite"))?;

    let bin_frames = ((params.bin_ms / 1000.0) * rate).round().max(1.0) as usize;
    let bin_samples = bin_frames * channels;

    let fill_bins = bin_levels(fill, bin_samples, params.floor_db);
    let (&head_bin_db, &tail_bin_db) = fill_bins.first().zip(fill_bins.last())?;
    // First maximum wins, so the reported index is the earliest loudest bin.
    let (peak_bin_index, peak_bin_db) = fill_bins.iter().enumerate().fold(
        (0usize, params.floor_db),
        |(best_index, best_db), (index, &db)| {
            if index == 0 || db > best_db {
                (index, db)
            } else {
                (best_index, best_db)
            }
        },
    );

    let reference_bins = bin_levels(
        &a_samples[reference_range.start * channels..reference_range.end * channels],
        bin_samples,
        params.floor_db,
    );
    let reference_spread_db = median_db(&reference_bins).map(|median| {
        let loudest = reference_bins
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        loudest - median
    });

    Some(FillLevelCheck {
        pre_shoulder_db,
        post_shoulder_db,
        reference_db,
        peak_bin_db,
        peak_delta_db: peak_bin_db - reference_db,
        reference_at_floor: reference_db <= params.floor_db,
        peak_bin_index,
        head_bin_db,
        tail_bin_db,
        edge_delta_db: head_bin_db.min(tail_bin_db) - reference_db,
        reference_spread_db,
        bins: fill_bins.len(),
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

    /// A shoulder saturated to a sliver by the head of the media is declined, not measured — and
    /// the other side carries the comparison alone.
    #[test]
    fn a_shoulder_shorter_than_half_the_requested_width_is_declined() {
        // Gap at frame 300 of a 1 kHz file: the pre shoulder has 200 frames of room against a
        // requested 1000, well under the 500-frame floor.
        let a = a_with_hole(4000, 1, 0.1, 300..800);
        let check = measure_fill_level(
            &tone(500, 1, 0.1),
            &a,
            1,
            300,
            800,
            RATE,
            FillLevelParams::default(),
        )
        .expect("the post shoulder is full width");
        assert!(
            check.pre_shoulder_db.is_none(),
            "200 frames is under half of 1000: {check:?}"
        );
        assert_eq!(
            check.reference_db,
            check.post_shoulder_db.expect("post shoulder"),
            "{check:?}"
        );
    }

    /// A shoulder that is itself a dropout floors the reference. The delta is then meaningless, so
    /// the flag exists to let calibration drop the row instead of reading a +120 dB outlier.
    #[test]
    fn a_reference_at_the_floor_is_flagged() {
        // A is silent everywhere: both shoulders read the floor.
        let a = vec![0.0f32; 4000];
        let check = measure_fill_level(
            &tone(500, 1, 0.1),
            &a,
            1,
            2000,
            2500,
            RATE,
            FillLevelParams::default(),
        )
        .expect("shoulders exist either side, they are just silent");
        assert!(check.reference_at_floor, "{check:?}");
        assert_eq!(check.reference_db, FILL_LEVEL_FLOOR_DB, "{check:?}");
        assert!(
            check.peak_delta_db > 100.0,
            "the artifact this flags: {check:?}"
        );
    }

    #[test]
    fn a_normal_reference_is_not_flagged() {
        let a = a_with_hole(4000, 1, 0.1, 2000..2500);
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
        assert!(!check.reference_at_floor, "{check:?}");
    }

    /// The shape of the one clip the 2026-08-06 labels heard as too loud: the fill sits above its
    /// neighbourhood everywhere, seams included. Both deltas large together is what says so.
    #[test]
    fn a_uniformly_hot_fill_shows_the_excess_at_its_seams_too() {
        let a = a_with_hole(4000, 1, 0.01, 2000..2500);
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
        assert!((check.peak_delta_db - 20.0).abs() < 0.01, "{check:?}");
        assert!(
            (check.edge_delta_db - 20.0).abs() < 0.01,
            "hot at both seams, not just inside: {check:?}"
        );
    }

    /// The shape of the +24.34 dB corpus maximum, which was heard as clean: a dropout inside quiet
    /// material, repaired with content that meets A at both seams and is loud only in its middle.
    /// `peak_delta_db` cannot tell this from the case above; `edge_delta_db` is the field that can.
    #[test]
    fn a_fill_loud_only_in_its_middle_meets_a_at_both_seams() {
        let a = a_with_hole(4000, 1, 0.01, 2000..2500);
        let mut fill = tone(500, 1, 0.01);
        for sample in fill[200..300].iter_mut() {
            *sample = 0.1;
        }
        let check = measure_fill_level(&fill, &a, 1, 2000, 2500, RATE, FillLevelParams::default())
            .expect("shoulders exist either side");
        assert!(
            (check.peak_delta_db - 20.0).abs() < 0.01,
            "the interior event is still reported: {check:?}"
        );
        assert!(
            check.edge_delta_db.abs() < 0.01,
            "but the seams match A: {check:?}"
        );
        assert_eq!(check.peak_bin_index, 2, "{check:?}");
    }

    /// The quieter seam decides, so one hot seam alone does not read as a uniformly hot fill — the
    /// same conservatism as the reference taking the louder shoulder.
    #[test]
    fn the_edge_delta_takes_the_quieter_seam() {
        let a = a_with_hole(4000, 1, 0.01, 2000..2500);
        let mut fill = tone(500, 1, 0.01);
        for sample in fill[..100].iter_mut() {
            *sample = 0.1;
        }
        let check = measure_fill_level(&fill, &a, 1, 2000, 2500, RATE, FillLevelParams::default())
            .expect("shoulders exist either side");
        assert!((check.head_bin_db - check.tail_bin_db) > 15.0, "{check:?}");
        assert!(
            check.edge_delta_db.abs() < 0.01,
            "the quiet tail decides: {check:?}"
        );
    }

    /// A uniform neighbourhood spreads by nothing; the shoulder level then genuinely predicts what
    /// belongs in the gap.
    #[test]
    fn a_steady_shoulder_has_no_spread() {
        let a = a_with_hole(4000, 1, 0.1, 2000..2500);
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
        assert!(
            check.reference_spread_db.expect("shoulder is bins wide") < 0.01,
            "{check:?}"
        );
    }

    /// A quiet pocket with one loud event in it reads the event as `reference_db` while most of the
    /// neighbourhood sits far below — the case where the comparison is worth least.
    #[test]
    fn a_sparse_shoulder_spreads_by_the_event_that_set_its_level() {
        let mut a = a_with_hole(4000, 1, 0.01, 2000..2500);
        // One loud 100 ms bin inside the post shoulder (which starts 100 ms past the gap).
        for sample in a[2700..2800].iter_mut() {
            *sample = 0.1;
        }
        let check = measure_fill_level(
            &tone(500, 1, 0.01),
            &a,
            1,
            2000,
            2500,
            RATE,
            FillLevelParams::default(),
        )
        .expect("shoulders exist either side");
        let spread = check.reference_spread_db.expect("shoulder is bins wide");
        assert!(spread > 15.0, "one loud bin over a quiet median: {check:?}");
        assert_eq!(
            check.reference_db,
            check.post_shoulder_db.expect("post shoulder"),
            "the spread describes the shoulder that set the reference: {check:?}"
        );
    }

    #[test]
    fn an_empty_fill_declines_to_measure() {
        let a = a_with_hole(4000, 1, 0.1, 2000..2500);
        assert!(
            measure_fill_level(&[], &a, 1, 2000, 2500, RATE, FillLevelParams::default()).is_none()
        );
    }
}
