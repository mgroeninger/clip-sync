//! Application-side gap-equivalence: measure A's gap interior and B's donor occupancy from decoded PCM,
//! then classify via the domain gate ([`crate::domain::gap_equivalence`]). No new decode and no
//! seam/residual math — but, since F15, no longer a reuse of `fp.levels.*` / `donor_interior_nominal`
//! either; see below.
//!
//! **This front-end is diagnostic, not authoritative.** Production drops gaps on the *scan* verdict
//! ([`crate::domain::gap_equivalence::derive_gap_equivalence`]); this one only lands in the fingerprint
//! dump for calibration. The two share `classify_gap_equivalence` but differ in what they feed it.
//!
//! **F15 (2026-07-30): three of those differences were defects and are now fixed here** — the silent-core
//! filter on `gap_floor_db` / `a_gap_rms_db`, the interleaved channel reduction, and the block-confirmed
//! span. All three previously biased this side toward `drop`, the dangerous direction. See
//! [`measure_gap_equivalence`] and `docs/dev/TEMP-equivalence-divergence-findings.md`
//! § *The three F15 fixes*.
//!
//! Consequently this module now measures its **own** A levels and donor occupancy from PCM rather than
//! reusing `fp.levels.*` and `donor_interior_nominal`, which are downmix, whole-span, unfiltered reads with
//! other consumers that must not move. Two differences remain open by choice: the noise-floor context
//! window and bin size (a policy call, median 2.1 dB), and the donor predicate's missing `b.silent ||`
//! disjunct. A surviving divergence is therefore informative, but still not proof the scan gate is wrong.
//! `docs/dev/gap-fingerprint.md` § *`equivalence` vs `scan_equivalence`*.

use crate::domain::gap_equivalence::{
    aggregate_rms_db, classify_gap_equivalence, ChannelReduction, GapEquivalenceParams,
    GapEquivalenceVerdict, SilentCoreProbe,
};
use crate::domain::pcm::interleaved_to_mono;
use crate::domain::policies::{is_silent_interleaved, rms_interleaved};

/// dBFS a fully-silent span floors to — kept identical to the fingerprint's `to_db` / `LevelProfile`
/// convention so `a_gap_rms_db` is on the **same scale** as the `noise_floor_db` it's compared against.
/// (Using a lower floor here, e.g. −180 dB, would spuriously read a silent gap as far below a silent-context
/// noise floor and misclassify it as a dropout — see the domain gate's `a_below_noise_db`.)
const SILENCE_FLOOR_DB: f64 = -120.0;

/// Mono RMS of an interleaved A span, in dBFS (finite; a silent span floors at [`SILENCE_FLOOR_DB`], not −∞).
pub fn gap_interior_rms_db(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> Option<f64> {
    let ch = channels.max(1);
    let end = end_frame.min(samples.len() / ch);
    if start_frame >= end {
        return None;
    }
    let mono = interleaved_to_mono(&samples[start_frame * ch..end * ch], ch);
    if mono.is_empty() {
        return None;
    }
    let rms = (mono.iter().map(|v| v * v).sum::<f64>() / mono.len() as f64).sqrt();
    if rms <= 1e-9 {
        Some(SILENCE_FLOOR_DB)
    } else {
        Some(20.0 * rms.log10())
    }
}

/// Per-bin level in dBFS under a chosen [`ChannelReduction`], on the same scale as
/// [`gap_interior_rms_db`] (silence floors at [`SILENCE_FLOOR_DB`], never −∞).
///
/// The two arms are the reductions the two front-ends actually use: the fingerprint path's amplitude-mean
/// downmix, and the scan path's interleaved power mean (`block_rms_db` → [`rms_interleaved`]). They are
/// **not** interchangeable — Cauchy–Schwarz makes `Downmix ≤ Interleaved` on the same samples, with
/// equality only when every channel carries an identical signal, and the gap is `10·log10(N)` (7.78 dB at
/// 6 channels) when they are uncorrelated. See `docs/dev/TEMP-equivalence-divergence-findings.md` § F15.
fn bin_level_db(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    reduction: ChannelReduction,
) -> Option<f64> {
    match reduction {
        ChannelReduction::Downmix => gap_interior_rms_db(samples, channels, start_frame, end_frame),
        ChannelReduction::Interleaved => {
            let ch = channels.max(1);
            let end = end_frame.min(samples.len() / ch);
            if start_frame >= end {
                return None;
            }
            let rms = f64::from(rms_interleaved(&samples[start_frame * ch..end * ch]));
            Some(if rms <= 1e-9 {
                SILENCE_FLOOR_DB
            } else {
                20.0 * rms.log10()
            })
        }
    }
}

/// Levels over the *silent* subset of a supplied bin sequence: the max (a `gap_floor_db`) and the energy
/// mean (an `a_gap_rms_db`), plus the silent/total bin populations behind them.
///
/// Factored out because the fixed measurement and the [`silent_core_probe`] scaffolding differ **only** in
/// which bins they are handed and which reduction they read them with — keeping the shared arithmetic in
/// one place is what makes that difference legible rather than a diff between two similar loops.
///
/// The empty-silent-set case yields `None` for both statistics rather than a floored value, mirroring the
/// scan path's `NEG_INFINITY` fold.
fn silent_core_levels(
    samples: &[f32],
    channels: usize,
    bins: impl Iterator<Item = std::ops::Range<usize>>,
    reduction: ChannelReduction,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
) -> (Option<f64>, Option<f64>, usize, usize) {
    let ch = channels.max(1);
    let mut silent: Vec<f64> = Vec::new();
    let mut total = 0usize;
    for bin in bins {
        total += 1;
        let block = &samples[bin.start * ch..bin.end * ch];
        if is_silent_interleaved(block, ch, silence_peak_fraction, absolute_silence_rms) {
            if let Some(db) = bin_level_db(samples, ch, bin.start, bin.end, reduction) {
                silent.push(db);
            }
        }
    }
    let floor = silent.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (
        floor.is_finite().then_some(floor),
        aggregate_rms_db(silent.iter().copied()),
        silent.len(),
        total,
    )
}

/// The **media-absolute** block grid the scan path measures on: whole bins of `bin_frames`, phase-locked to
/// the start of the track, of which the ones whose **centre** falls in `[gap.start, gap.end)` are kept.
///
/// This is `derive_gap_equivalence`'s `block_center(b) >= a_start_secs && < a_end_secs` filter expressed in
/// frames. Two consequences distinguish it from tiling the gap span from its own start, and both are the
/// point of F15 fix 3:
///
/// - the grid is phase-locked to the **media**, not to the gap, so a bin generally straddles each gap edge
///   rather than aligning to it — a whole block can therefore lie partly outside the span, which is correct:
///   the scan path measures whole blocks and selects them by centre;
/// - there is **no trailing partial bin**, because centre-containment discards it by construction.
///
/// Bins are clamped to `total_frames` at the end of the track only. An empty range yields no bins.
fn block_grid_bins(
    gap: std::ops::Range<usize>,
    bin_frames: usize,
    total_frames: usize,
) -> impl Iterator<Item = std::ops::Range<usize>> {
    let bf = bin_frames.max(1);
    // Smallest k with `centre(k) >= bound`, where `centre(k) = k*bf + bf/2`. Compared at doubled
    // precision (`2*centre = 2*k*bf + bf` vs `2*bound`) so an **odd** `bin_frames` does not lose the
    // half-frame to integer truncation — scan takes `(start + end) / 2` in floats, and rounding the
    // centre down would shift the selected block by one at exactly the boundary.
    let first_centred_at_or_after = move |bound: usize| -> usize {
        match (2 * bound).checked_sub(bf) {
            Some(0) | None => 0,
            Some(d) => d.div_ceil(2 * bf),
        }
    };
    let k_lo = first_centred_at_or_after(gap.start);
    let k_hi = first_centred_at_or_after(gap.end);
    (k_lo..k_hi).filter_map(move |k| {
        let start = k * bf;
        let end = ((k + 1) * bf).min(total_frames);
        (start < end).then_some(start..end)
    })
}

/// Measure a candidate **silent-core** floor over A's gap interior at `bin_ms`: bin the span, keep only
/// bins the *scanner's own predicate* ([`is_silent_interleaved`]) calls silent, and report the max
/// (candidate `gap_floor_db`) and energy mean (candidate `a_gap_rms_db`) over them.
///
/// **This is the pre-fix recipe, deliberately frozen.** It applies the silence filter (F15 fix 1) but keeps
/// the *downmix* reduction and gap-anchored tiling that fixes 2 and 3 replaced, so a re-dump can put the old
/// and new numbers side by side. It is no longer "the scan path's way" and no longer previews what
/// [`measure_gap_equivalence`] computes — comparing the two is its whole remaining purpose. Delete both
/// probe sets once the combined re-dump has validated the fixes.
///
/// **Provenance only** — see [`SilentCoreProbe`]. The empty-silent-set case returns `None` for both
/// statistics rather than a floored value, mirroring the scan path's `NEG_INFINITY` fold; whether that
/// fallback is load-bearing is one of the things this probe exists to find out.
///
/// A trailing partial bin is included (it is a real part of the gap); a zero-length span yields a probe
/// with `total_bins == 0`.
pub fn silent_core_probe(
    samples: &[f32],
    channels: usize,
    gap_frames: std::ops::Range<usize>,
    sample_rate: u32,
    bin_ms: u64,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
) -> SilentCoreProbe {
    let ch = channels.max(1);
    let bin_frames = ((f64::from(sample_rate) * bin_ms as f64) / 1000.0).round() as usize;
    let bin_frames = bin_frames.max(1);
    let end = gap_frames.end.min(samples.len() / ch);
    let span_tiling = std::iter::successors(Some(gap_frames.start), move |&f| {
        let next = (f + bin_frames).min(end);
        (next < end).then_some(next)
    })
    .map(move |f| f..(f + bin_frames).min(end))
    .take_while(|b| b.start < b.end);
    let (floor_db, a_rms_db, silent_bins, total_bins) = silent_core_levels(
        samples,
        ch,
        span_tiling,
        ChannelReduction::Downmix,
        silence_peak_fraction,
        absolute_silence_rms,
    );
    SilentCoreProbe {
        bin_ms,
        floor_db,
        a_rms_db,
        silent_bins,
        total_bins,
    }
}

/// How the fine path bins and thresholds A's gap interior — the scan path's block size and silence
/// predicate, supplied by the caller because they are *scan recipe* knobs (`scan_block_ms`,
/// `silence_peak_fraction`, `absolute_silence_rms`), not equivalence parameters.
#[derive(Debug, Clone, Copy)]
pub struct SilentCoreConfig {
    /// Bin width in frames. The fine path keeps its **own** bin size here; matching scan's is the
    /// undecided window/bin policy leg, deliberately *not* part of the three F15 fixes.
    pub bin_frames: usize,
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
}

/// The donor span to measure occupancy over: **interleaved** B samples and the nominal `b_mapped` frame
/// range. Passed as PCM rather than as a precomputed fraction so the donor is thresholded against the
/// *same* floor, under the *same* reduction, that [`measure_gap_equivalence`] measured on A.
///
/// That coupling is the point. `donor_interior_nominal.silence_fraction` — the fraction this used to be
/// handed — is measured on a **mono downmix** of B against `levels.gap_floor_db`, the unfiltered whole-span
/// peak. Feeding that in alongside a fixed A floor would have left F15 fix 1 half-applied: the floor moves,
/// but the predicate that actually reaches the class still tests against the old one. On `band_donor` that
/// predicate *is* the class flip.
#[derive(Debug, Clone)]
pub struct DonorSpan<'a> {
    pub samples: &'a [f32],
    pub frames: std::ops::Range<usize>,
}

/// Fraction of donor bins reading below `floor_db`, under the interleaved reduction.
///
/// Mirrors `domain::donor::donor_interior_at`'s `r < floor` predicate and its 50 ms binning, changing only
/// the two things F15 fixes: the floor is A's **silent-core** floor, and levels are an **interleaved power
/// mean** rather than a downmix. Both sides of the comparison must move together — thresholding a mono
/// donor against an interleaved floor would reintroduce up to `10·log10(N)` of bias in the *dangerous*
/// direction (donor reads spuriously silent ⇒ `shared_silence` ⇒ drop).
///
/// `None` when there is no floor to compare against or no donor span, so "not evaluated" stays distinct
/// from "measured as occupied".
///
/// One known difference from the scan path remains and is deliberately **not** closed here: scan's donor
/// predicate is `b.silent || rms_db < gap_floor`, a disjunction with the scanner's own silence bit, where
/// this is the floor test alone. That is a separate axis from the three fixes and is unmeasured.
fn donor_silence_fraction_at_floor(
    donor: &DonorSpan<'_>,
    channels: usize,
    bin_frames: usize,
    floor_db: Option<f64>,
) -> Option<f64> {
    let floor = floor_db?;
    let ch = channels.max(1);
    let total_frames = donor.samples.len() / ch;
    let end = donor.frames.end.min(total_frames);
    if donor.frames.start >= end {
        return None;
    }
    let bf = bin_frames.max(1);
    let (mut total, mut silent) = (0usize, 0usize);
    let mut frame = donor.frames.start;
    while frame < end {
        let bin_end = (frame + bf).min(end);
        total += 1;
        if bin_level_db(
            donor.samples,
            ch,
            frame,
            bin_end,
            ChannelReduction::Interleaved,
        )
        .is_some_and(|db| db < floor)
        {
            silent += 1;
        }
        frame = bin_end;
    }
    (total > 0).then(|| silent as f64 / total as f64)
}

/// Compute the equivalence verdict for one gap: A's gap-interior RMS (from decoded A) + the recording's noise
/// floor + the donor silence at the nominal span → the domain classification.
///
/// # The three F15 fixes
///
/// This front-end used to read three sensors differently from the authoritative scan path, each difference
/// large enough to flip a class on its own, and **all three biased it toward `drop`** — the dangerous
/// direction. All three are corrected here (`docs/dev/TEMP-equivalence-divergence-findings.md` § *The three
/// F15 fixes*):
///
/// 1. **silent core.** `a_gap_rms_db` and `gap_floor_db` are the energy mean and max over the bins the
///    scanner's own predicate calls silent, not over every bin in the span. Taking a max over *all* bins
///    yields a content peak and then uses it as a silence threshold; on the calibration pair that read
///    ~21 dB high and flipped the donor axis.
/// 2. **interleaved reduction.** Levels are an interleaved power mean, as the scan path's `block_rms_db`
///    takes, not an amplitude-mean downmix. The downmix under-reads by up to `10·log10(N)` on multichannel
///    material, and *level-dependently*, so the error does not cancel between `a_gap_rms_db` and the noise
///    floor it is compared against.
/// 3. **block-confirmed span.** Bins come from [`block_grid_bins`] over the **raw** gap span — scan's
///    media-absolute grid, selected by centre-containment — not from tiling the *refined* span. The
///    difference is one to two blocks at the gap edges, where content ramps, and it moved a max by up to
///    13 dB.
///
/// All three reach the class, not just the dump: the floor measured under fix 1 is what the **donor** is
/// thresholded against (see [`DonorSpan`]), which on the band-donor mechanism is where the class actually
/// flips. A refactor that reverts the donor to a precomputed fraction silently un-applies fix 1.
///
/// `noise_floor_db` must be measured under the **same reduction** (fix 2 applies to it too); the caller
/// supplies it because the fingerprint measures it over its own context window. `None` ⇒ `NotEvaluated`
/// rather than a substitute floor: a downmix fallback would un-apply fix 2 on exactly the gaps where the
/// context is too thin to measure.
///
/// Still diagnostic, not authoritative: production drops on the scan verdict. These fixes make the two
/// comparable, so a surviving divergence means something.
pub fn measure_gap_equivalence(
    a_samples: &[f32],
    channels: usize,
    gap_frames: std::ops::Range<usize>,
    noise_floor_db: Option<f64>,
    donor: Option<DonorSpan<'_>>,
    core: &SilentCoreConfig,
    params: &GapEquivalenceParams,
) -> GapEquivalenceVerdict {
    let ch = channels.max(1);
    let total_frames = a_samples.len() / ch;
    let (floor_db, a_rms, silent_bins, _total) = silent_core_levels(
        a_samples,
        ch,
        block_grid_bins(gap_frames, core.bin_frames, total_frames),
        ChannelReduction::Interleaved,
        core.silence_peak_fraction,
        core.absolute_silence_rms,
    );
    let donor_fraction = donor
        .as_ref()
        .and_then(|d| donor_silence_fraction_at_floor(d, ch, core.bin_frames, floor_db));
    classify_gap_equivalence(a_rms, noise_floor_db, donor_fraction, params).with_scan_provenance(
        floor_db,
        silent_bins,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap_equivalence::GapEquivalenceClass;

    fn on() -> GapEquivalenceParams {
        GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        }
    }

    /// 50 ms bins @ 48 kHz with the run recipe's silence thresholds.
    fn core() -> SilentCoreConfig {
        SilentCoreConfig {
            bin_frames: 2_400,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.001,
        }
    }

    /// A whole-span donor at one level, for the fraction tests.
    fn donor(samples: &[f32]) -> Option<DonorSpan<'_>> {
        Some(DonorSpan {
            samples,
            frames: 0..samples.len(),
        })
    }

    /// A silent gap (dig-silence) with an occupied donor → RepairableDropout (end-to-end from PCM).
    #[test]
    fn silent_gap_occupied_donor_is_repairable() {
        let a = vec![0.0f32; 48_000]; // A gap interior = digital silence
        let b = vec![1e-4f32; 48_000]; // −80 dBFS: above A's −120 floor ⇒ occupied
        let v = measure_gap_equivalence(&a, 1, 0..48_000, Some(-50.0), donor(&b), &core(), &on());
        assert_eq!(v.class, GapEquivalenceClass::RepairableDropout);
        // Digital silence floors at SILENCE_FLOOR_DB (−120), the same scale as noise_floor_db.
        assert_eq!(
            v.a_gap_rms_db.unwrap(),
            -120.0,
            "silent span floors at −120: {v:?}"
        );
    }

    /// Room-tone A (a few dB below the noise floor) with a silent donor → SharedSilence.
    #[test]
    fn roomtone_gap_silent_donor_is_shared_silence() {
        // amplitude ~1e-4 ⇒ ~−80 dBFS; noise floor −75 ⇒ only ~5 dB below ⇒ not a dropout; donor silent.
        let a = vec![1e-4f32; 48_000];
        let b = vec![1e-6f32; 48_000]; // −120 dBFS: below A's −80 floor ⇒ silent
        let v = measure_gap_equivalence(&a, 1, 0..48_000, Some(-75.0), donor(&b), &core(), &on());
        assert_eq!(v.class, GapEquivalenceClass::SharedSilence);
        assert!(v.drop);
    }

    // --- the band mechanism, end to end through the fixed path -------------------------------------

    /// **The acceptance signal for F15, in unit form.** `band_donor.json` pins the *pre-fix* numbers from
    /// committed JSON and cannot exercise this code; this reproduces the mechanism from synthetic PCM so
    /// the fix is verified by execution rather than by re-reading a fixture.
    ///
    /// The setup is `band_donor`'s shape: a gap whose interior is quiet except for one loud bin, and a
    /// donor sitting **between** the two floor definitions — above the silent-core floor, below the
    /// whole-span peak. Pre-fix, the whole-span max became the silence threshold, the donor read silent,
    /// and the class was `SharedSilence` ⇒ **drop** a repairable gap. Post-fix the floor is the silent
    /// core, the donor reads occupied, and the class is `RepairableDropout` ⇒ keep.
    #[test]
    fn band_donor_mechanism_now_classifies_as_repairable() {
        let (quiet, loud, donor) = (1e-5f32, 0.5f32, 2e-4f32);
        let mut a = vec![quiet; 48_000];
        for v in &mut a[24_000..26_400] {
            *v = loud; // the content peak that used to become `gap_floor_db`
        }
        let b = vec![donor; 48_000];

        let v = measure_gap_equivalence(
            &a,
            1,
            0..48_000,
            Some(-50.0),
            Some(DonorSpan {
                samples: &b,
                frames: 0..48_000,
            }),
            &core(),
            &on(),
        );

        // The donor sits in the band: above the silent-core floor, below the whole-span peak.
        let floor = v.gap_floor_db.expect("a silent core exists");
        let donor_db = 20.0 * f64::from(donor).log10();
        let peak_db = 20.0 * f64::from(loud).log10();
        assert!(
            floor < donor_db && donor_db < peak_db,
            "donor {donor_db:.1} must lie between floor {floor:.1} and peak {peak_db:.1}"
        );

        assert_eq!(
            v.class,
            GapEquivalenceClass::RepairableDropout,
            "the donor is above the silent-core floor ⇒ occupied ⇒ keep: {v:?}"
        );
        assert!(!v.drop, "{v:?}");
        assert_eq!(
            v.donor_silence_fraction,
            Some(0.0),
            "no donor bin is below the silent-core floor: {v:?}"
        );
    }

    /// The regression guard for the half-applied fix: thresholding the donor against the **whole-span
    /// peak** instead of the silent-core floor calls the same donor silent, which is what flips the class
    /// to `drop`. This asserts the two floors genuinely disagree on this donor, so
    /// `band_donor_mechanism_now_classifies_as_repairable` cannot pass for an unrelated reason.
    #[test]
    fn donor_would_read_silent_against_the_unfixed_whole_span_floor() {
        let b = vec![2e-4f32; 48_000];
        let span = DonorSpan {
            samples: &b,
            frames: 0..48_000,
        };
        let whole_span_peak = 20.0 * f64::from(0.5f32).log10();
        assert_eq!(
            donor_silence_fraction_at_floor(&span, 1, 2_400, Some(whole_span_peak)),
            Some(1.0),
            "against the content peak the donor reads fully silent — the pre-fix bug"
        );
        let silent_core = 20.0 * f64::from(1e-5f32).log10();
        assert_eq!(
            donor_silence_fraction_at_floor(&span, 1, 2_400, Some(silent_core)),
            Some(0.0),
            "against the silent core it reads fully occupied"
        );
    }

    /// No floor ⇒ no donor fraction, rather than a fraction measured against a substitute. Pairs with the
    /// empty-context noise-floor rule: an unmeasurable input must reach the class as `None`.
    #[test]
    fn donor_fraction_is_absent_without_a_floor() {
        let b = vec![1e-4f32; 4_800];
        let span = DonorSpan {
            samples: &b,
            frames: 0..4_800,
        };
        assert_eq!(donor_silence_fraction_at_floor(&span, 1, 2_400, None), None);
        let empty = DonorSpan {
            samples: &b,
            frames: 4_800..4_800,
        };
        assert_eq!(
            donor_silence_fraction_at_floor(&empty, 1, 2_400, Some(-60.0)),
            None
        );
    }

    /// An unmeasurable noise floor classifies `NotEvaluated` (⇒ keep) rather than falling back to a
    /// downmix number, which would un-apply fix 2 on that gap.
    #[test]
    fn absent_noise_floor_is_not_evaluated() {
        let a = vec![0.0f32; 48_000];
        let v = measure_gap_equivalence(&a, 1, 0..48_000, None, None, &core(), &on());
        assert_eq!(v.class, GapEquivalenceClass::NotEvaluated);
        assert!(!v.drop, "{v:?}");
    }

    // --- the three F15 fixes, on the measurement path itself ---------------------------------------

    /// **Fix 1.** A loud bin inside the gap must not become `gap_floor_db`. Before the fix this took the
    /// max over *every* bin, so a content peak became the silence threshold — the band mechanism that
    /// flipped the donor axis on the calibration pair.
    #[test]
    fn measured_gap_floor_is_the_silent_core_not_the_span_peak() {
        let mut a = vec![0.0f32; 48_000];
        for v in &mut a[24_000..26_400] {
            *v = 0.5; // one full-scale 50 ms bin mid-gap
        }
        let v = measure_gap_equivalence(&a, 1, 0..48_000, Some(-50.0), None, &core(), &on());
        assert_eq!(
            v.gap_floor_db,
            Some(-120.0),
            "the −6 dBFS bin must not become the floor: {v:?}"
        );
        assert_eq!(
            v.a_gap_silent_blocks,
            Some(19),
            "19 of 20 bins are silent: {v:?}"
        );
    }

    /// **Fix 2.** The reduction reaches the verdict's own numbers, not just the probes. Six decorrelated
    /// channels read `10·log10(6)` = 7.78 dB **higher** under the interleaved power mean than they would
    /// under an amplitude-mean downmix — and `is_dropout` compares against a noise floor, so reading A
    /// too low is what manufactured false dropouts.
    #[test]
    fn measured_levels_use_the_interleaved_reduction() {
        // One channel of 6 carries signal; the rest are silent. Power concentration ⇒ the full penalty.
        let mut a = vec![0.0f32; 6 * 48_000];
        for f in 0..48_000 {
            a[f * 6] = if f % 2 == 0 { 2e-3 } else { -2e-3 };
        }
        let inter = bin_level_db(&a, 6, 0, 48_000, ChannelReduction::Interleaved).unwrap();
        let down = bin_level_db(&a, 6, 0, 48_000, ChannelReduction::Downmix).unwrap();
        assert!(
            (inter - down - 10.0 * 6f64.log10()).abs() < 0.01,
            "expected a {:.2} dB reduction gap, got {:.2}",
            10.0 * 6f64.log10(),
            inter - down
        );
    }

    /// **Fix 3.** The measured span is the media-absolute block grid selected by centre-containment, so a
    /// gap that does not start on a bin boundary still measures whole blocks — and the count is the
    /// scan path's, not `ceil(span / bin)`. A gap of 20 bin-widths offset by half a bin covers 20 centres.
    #[test]
    fn measured_span_follows_the_media_absolute_block_grid() {
        let a = vec![0.0f32; 96_000];
        let v = measure_gap_equivalence(&a, 1, 1_200..49_200, Some(-50.0), None, &core(), &on());
        assert_eq!(v.a_gap_silent_blocks, Some(20), "{v:?}");
    }

    /// The grid is phase-locked to the media, not the gap: shifting a gap by less than a bin changes
    /// which block centres it captures, exactly as the scan path's filter does. This is the property that
    /// made fine's tiling disagree with scan by one to two blocks at the edges.
    #[test]
    fn block_grid_is_phase_locked_to_the_media_not_the_gap() {
        // Bin = 100 frames. Centres sit at 50, 150, 250, … Gap [120, 320) captures 150 and 250.
        let bins: Vec<_> = block_grid_bins(120..320, 100, 10_000).collect();
        assert_eq!(bins, vec![100..200, 200..300], "{bins:?}");
        // The same 200-frame width one bin later captures two different centres, still two blocks.
        let bins: Vec<_> = block_grid_bins(220..420, 100, 10_000).collect();
        assert_eq!(bins, vec![200..300, 300..400], "{bins:?}");
    }

    /// A block whose centre is inside the gap is measured **whole**, even where it overruns the gap edge.
    /// That is scan's semantics and the reason the fine path's edge blocks used to differ: content ramps
    /// live at the edges, and a max statistic is maximally sensitive to them.
    #[test]
    fn block_grid_blocks_are_whole_even_when_they_overrun_the_gap_edge() {
        let bins: Vec<_> = block_grid_bins(140..260, 100, 10_000).collect();
        assert_eq!(bins, vec![100..200, 200..300], "{bins:?}");
    }

    /// No trailing partial bin — centre-containment discards it by construction. Contrast
    /// `silent_core_probe_includes_a_trailing_partial_bin`, which keeps the pre-fix behaviour.
    #[test]
    fn block_grid_has_no_trailing_partial_bin() {
        // [0, 250) with bin 100: centres 50 and 150 are in, 250 is not. The 50-frame remainder is dropped.
        let bins: Vec<_> = block_grid_bins(0..250, 100, 10_000).collect();
        assert_eq!(bins, vec![0..100, 100..200], "{bins:?}");
    }

    /// A gap narrower than one bin can still capture a centre — or none at all. Neither case may panic
    /// or fabricate a level.
    #[test]
    fn block_grid_handles_subbin_gaps() {
        assert_eq!(
            block_grid_bins(40..60, 100, 10_000).collect::<Vec<_>>(),
            vec![0..100]
        );
        assert!(block_grid_bins(60..90, 100, 10_000).next().is_none());
        assert!(block_grid_bins(500..500, 100, 10_000).next().is_none());
    }

    // --- silent-core probe (F15 measurement scaffolding; provenance only) --------------------------

    /// Digital silence: every bin passes the predicate, so the candidate floor is the −120 floor and
    /// the silent-bin population is the whole span.
    #[test]
    fn silent_core_probe_counts_every_bin_of_a_silent_gap() {
        let a = vec![0.0f32; 48_000]; // 1 s @ 48 kHz
        let p = silent_core_probe(&a, 1, 0..48_000, 48_000, 50, 0.01, 0.001);
        assert_eq!((p.bin_ms, p.silent_bins, p.total_bins), (50, 20, 20));
        assert_eq!(p.floor_db, Some(-120.0));
        assert_eq!(p.a_rms_db, Some(-120.0));
    }

    /// The point of the probe: a loud bin inside the span is **excluded** from the floor, so the
    /// candidate floor tracks the quiet core rather than the span's content peak (which is exactly the
    /// difference that flips a class — F15's band mechanism).
    #[test]
    fn silent_core_probe_excludes_loud_bins_from_the_floor() {
        let mut a = vec![0.0f32; 48_000];
        // Bin 10 ([0.5 s, 0.55 s)) is loud: full-scale, so neither predicate branch calls it silent.
        for v in &mut a[24_000..26_400] {
            *v = 0.5;
        }
        let p = silent_core_probe(&a, 1, 0..48_000, 48_000, 50, 0.01, 0.001);
        assert_eq!((p.silent_bins, p.total_bins), (19, 20));
        assert_eq!(
            p.floor_db,
            Some(-120.0),
            "the −6 dBFS bin must not become the floor: {p:?}"
        );
    }

    /// No silent bin ⇒ both statistics absent, mirroring the scan path's `NEG_INFINITY` fold → `None`
    /// (which classifies `NotEvaluated` ⇒ keep). Whether this case occurs in the corpus is one of the
    /// things the probe is being dumped to find out.
    #[test]
    fn silent_core_probe_with_no_silent_bins_reports_none() {
        let a: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.3).sin() * 0.5).collect();
        let p = silent_core_probe(&a, 1, 0..48_000, 48_000, 50, 0.01, 0.001);
        assert_eq!(p.silent_bins, 0);
        assert_eq!(p.total_bins, 20);
        assert_eq!(p.floor_db, None);
        assert_eq!(p.a_rms_db, None);
    }

    /// A zero-length span bins to nothing rather than panicking or fabricating a floor.
    #[test]
    fn silent_core_probe_on_empty_span_is_empty() {
        let a = vec![0.0f32; 100];
        let p = silent_core_probe(&a, 1, 50..50, 48_000, 50, 0.01, 0.001);
        assert_eq!((p.silent_bins, p.total_bins), (0, 0));
        assert_eq!(p.floor_db, None);
    }

    /// A trailing partial bin is still a real part of the gap and is measured, not dropped.
    #[test]
    fn silent_core_probe_includes_a_trailing_partial_bin() {
        let a = vec![0.0f32; 3_600]; // 75 ms @ 48 kHz = one full 50 ms bin + a 25 ms remainder
        let p = silent_core_probe(&a, 1, 0..3_600, 48_000, 50, 0.01, 0.001);
        assert_eq!((p.silent_bins, p.total_bins), (2, 2));
    }

    /// The probe never touches the verdict's disposition — it is provenance, attached after the fact.
    #[test]
    fn attaching_probes_leaves_the_class_untouched() {
        let a = vec![0.0f32; 48_000];
        let v = measure_gap_equivalence(&a, 1, 0..48_000, Some(-50.0), None, &core(), &on());
        let p = silent_core_probe(&a, 1, 0..48_000, 48_000, 50, 0.01, 0.001);
        let with = v.clone().with_silent_core_probes(vec![p]);
        assert_eq!((with.class, with.drop), (v.class, v.drop));
        assert_eq!(with.silent_core_probes.len(), 1);
    }

    #[test]
    fn empty_span_yields_none_rms_and_not_evaluated() {
        let a = vec![0.0f32; 100];
        let v = measure_gap_equivalence(&a, 1, 50..50, Some(-50.0), None, &core(), &on());
        assert_eq!(v.class, GapEquivalenceClass::NotEvaluated);
        assert_eq!(v.gap_floor_db, None, "no bins ⇒ no floor: {v:?}");
    }

    // --- span sensitivity of the silent-core floor (F15, fully-silent residual) --------------------
    //
    // The corpus's fully-silent gaps showed silent-core (a downmix max) reading *above* scan (an
    // interleaved max) at the same bin size — which Cauchy–Schwarz forbids on the same samples. The
    // sample sets therefore differed, and the donor block counts pinned it to a 100–200 ms span delta:
    // scan measures over the block-confirmed core, fine over the wider refined span, and the extra
    // edge frames carry the ramp. These tests plant that geometry synthetically so the mechanism is
    // pinned without media.

    /// Deterministic low-level bed for `ch` channels, with an optional louder region planted in
    /// `[edge_start, edge_end)`. Channels use distinct 20 Hz-multiple frequencies, so they are exactly
    /// orthogonal over a 50 ms bin (`ρ̄ = 0`) and the downmix penalty sits at its `10·log10(N)` maximum.
    fn bed(ch: usize, frames: usize, quiet: f64, edge: Option<(usize, usize, f64)>) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * ch);
        for f in 0..frames {
            let amp = match edge {
                Some((s, e, a)) if f >= s && f < e => a,
                _ => quiet,
            };
            for c in 0..ch {
                let hz = 200.0 * (c as f64 + 1.0);
                let t = f as f64 / 48_000.0;
                out.push((amp * (std::f64::consts::TAU * hz * t).sin()) as f32);
            }
        }
        out
    }

    fn probe_floor(a: &[f32], ch: usize, span: std::ops::Range<usize>) -> f64 {
        silent_core_probe(a, ch, span, 48_000, 100, 0.01, 1.0)
            .floor_db
            .expect("bins present")
    }

    /// **The floor is a property of the span, not only of the content.** Widening the measured span by
    /// two 100 ms blocks to take in a planted 20 dB edge moves the floor by ~20 dB, while the interior
    /// is unchanged. This is the fully-silent residual's mechanism: 2.78–13.20 dB on the corpus came
    /// from a 100–200 ms span delta at the gap edges, not from the reduction or the bin size.
    #[test]
    fn silent_core_floor_is_set_by_the_span_not_only_the_content() {
        // 1.0 s bed at ~−80 dBFS, with the last 200 ms 20 dB louder.
        let frames = 48_000;
        let quiet = 1e-4;
        let a = bed(
            6,
            frames,
            quiet,
            Some((frames - 9_600, frames, quiet * 10.0)),
        );
        let core = probe_floor(&a, 6, 0..frames - 9_600);
        let refined = probe_floor(&a, 6, 0..frames);
        assert!(
            refined - core > 15.0,
            "the wider span must catch the planted edge: core {core:.2}, refined {refined:.2}"
        );
        assert!(
            (refined - core - 20.0).abs() < 3.0,
            "and by about the planted 20 dB, got {:.2}",
            refined - core
        );
    }

    /// **The invariant whose violation exposed the span delta.** On the *same* span the downmix floor
    /// can never exceed the interleaved one — so a corpus gap where fine reads above scan at the same
    /// bin size is proof the spans differ, not evidence about correlation. Pinned here so the
    /// diagnostic stays available.
    #[test]
    fn silent_core_downmix_floor_never_exceeds_interleaved_on_the_same_span() {
        let frames = 48_000;
        for edge in [None, Some((38_400usize, 48_000usize, 1e-3f64))] {
            for ch in [1usize, 2, 6] {
                let a = bed(ch, frames, 1e-4, edge);
                let downmix = probe_floor(&a, ch, 0..frames);
                // Interleaved max over the same bins, built the scan path's way.
                let bin = 4_800usize;
                let mut interleaved = f64::NEG_INFINITY;
                let mut f = 0usize;
                while f < frames {
                    let end = (f + bin).min(frames);
                    let r = f64::from(crate::domain::policies::rms_interleaved(
                        &a[f * ch..end * ch],
                    ));
                    interleaved = interleaved.max(20.0 * r.log10());
                    f = end;
                }
                assert!(
                    downmix <= interleaved + 0.01,
                    "ch={ch} edge={edge:?}: downmix {downmix:.2} exceeded interleaved {interleaved:.2}"
                );
            }
        }
    }

    /// The gap between the two reductions on the same span is the `10·log10(N)` penalty when the
    /// channels are decorrelated — the same quantity the noise-floor probe measures, confirmed here on
    /// the *floor* statistic (a max) rather than on a median.
    #[test]
    fn silent_core_floor_carries_the_full_downmix_penalty_when_decorrelated() {
        let frames = 48_000;
        let ch = 6;
        let a = bed(ch, frames, 1e-4, None);
        let downmix = probe_floor(&a, ch, 0..frames);
        let bin = 4_800usize;
        let r = f64::from(crate::domain::policies::rms_interleaved(&a[0..bin * ch]));
        let interleaved = 20.0 * r.log10();
        let expect = 10.0 * (ch as f64).log10();
        assert!(
            (interleaved - downmix - expect).abs() < 0.2,
            "expected a {expect:.2} dB penalty, got {:.2}",
            interleaved - downmix
        );
    }
}
