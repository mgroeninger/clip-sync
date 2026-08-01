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
//! [`measure_gap_equivalence`] and `docs/dev/archive/TEMP-equivalence-divergence-findings.md`
//! § *The three F15 fixes*.
//!
//! Consequently this module now measures its **own** A levels and donor occupancy from PCM rather than
//! reusing `fp.levels.*` and `donor_interior_nominal`, which are downmix, whole-span, unfiltered reads with
//! other consumers that must not move.
//!
//! **Instrument convergence (2026-07-30/31) closed two more differences:** **I1** moved the whole overlay
//! onto `scan_block_ms` bins (it had inherited `gap_signature_bin_ms` = 50 ms by proximity, a value tuned
//! for structure *pattern matching*, where finer is better — the opposite of what a max statistic and a
//! threshold-crossing fraction want); **I3** gave the donor predicate scan's `b.silent ||` disjunct. After
//! I1 the two front-ends compute `gap_floor_db` and `a_gap_rms_db` **identically** — 0.00 dB apart on all
//! ten gaps of the characterized pair.
//!
//! **One difference remains open by choice:** the noise-floor context window, ±2.0 s (scan) vs ±3.0 s
//! (here). Measured residual **0.606 dB** median, flipping one gap of ten in the safe direction; not
//! converged because both values encode a real judgement and `gap_signature_context_secs` has unrelated
//! consumers. A surviving divergence is therefore informative, but still not proof the scan gate is wrong.
//! `docs/dev/gap-fingerprint.md` § *`equivalence` vs `scan_equivalence`*.

use crate::domain::gap_equivalence::{
    aggregate_rms_db, classify_gap_equivalence, ChannelReduction, GapEquivalenceParams,
    GapEquivalenceVerdict,
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
/// 6 channels) when they are uncorrelated. See `docs/dev/archive/TEMP-equivalence-divergence-findings.md` § F15.
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

/// How the diagnostic path bins and thresholds A's gap interior — the scan path's block size and silence
/// predicate, supplied by the caller because they are *scan recipe* knobs (`scan_block_ms`,
/// `silence_peak_fraction`, `absolute_silence_rms`), not equivalence parameters.
#[derive(Debug, Clone, Copy)]
pub struct SilentCoreConfig {
    /// Bin width in frames. **I1 (2026-07-30): this is `scan_block_ms`, not `gap_signature_bin_ms`.**
    ///
    /// The two statistics this feeds are a **max** ([`GapEquivalenceVerdict::gap_floor_db`]) and a
    /// **threshold-crossing fraction** (the donor), and both are upward-biased by finer bins — measured
    /// at max ≥ coarser on 10/10 gaps, donor fraction up on 5/6. `gap_signature_bin_ms` is tuned for the
    /// opposite property: it bins the binary active/silent *structure signature*, where fine bins
    /// discriminate. This overlay had inherited it by proximity. Keep this matched to scan so the
    /// diagnostic compares like-for-like with the path it audits.
    ///
    /// The noise-floor **context window** is still split (2.0 s scan vs 3.0 s diagnostic) — that is I2 and
    /// remains open. See `docs/dev/archive/TEMP-equivalence-instrument-convergence.md`.
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
/// Mirrors `domain::donor::donor_interior_at`'s `r < floor` predicate, changing the two things F15 fixes:
/// the floor is A's **silent-core** floor, and levels are an **interleaved power
/// mean** rather than a downmix. Binning is `scan_block_ms`, not that function's `gap_signature_bin_ms`
/// (I1). Both sides of the comparison must move together — thresholding a mono
/// donor against an interleaved floor would reintroduce up to `10·log10(N)` of bias in the *dangerous*
/// direction (donor reads spuriously silent ⇒ `shared_silence` ⇒ drop).
///
/// `None` when there is no donor span, so "not evaluated" stays distinct from "measured as occupied".
///
/// # I3 (2026-07-30): the predicate is a **disjunction**, matching scan
///
/// A bin counts silent when the scanner's own predicate ([`is_silent_interleaved`]) calls it silent
/// **or** it is quieter than A's silent-core floor — the same `b.silent || rms_db < gap_floor` the scan
/// path applies in `derive_gap_equivalence`. The floor test alone is **not** equivalent, and the gap is
/// not cosmetic:
///
/// A digitally silent block reads exactly [`BLOCK_LEVEL_FLOOR_DB`] (−120), because `block_rms_db` clamps
/// there rather than returning `-inf`. On a gap whose silent core is *also* digital silence, `gap_floor_db`
/// is −120 too — and `-120.0 < -120.0` is **false**. A floor-only donor therefore reads digital silence as
/// **occupied**, yielding `repairable_dropout` (keep) where scan yields `shared_silence` (drop).
///
/// That is the **dangerous** direction — scan drops, diagnostic keeps — and the one condition
/// `bin/equivalence_calibration` exits 1 on. It went unnoticed because every corpus pair measured to date
/// is lossy (AAC), whose decoded floors bottom out near −101 dB and never reach the −120 clamp; the
/// 17-pair population check (5/297 divergent, 0 dangerous) could not have produced it. Lossless or
/// genuinely muted material can. See `docs/dev/archive/TEMP-equivalence-instrument-convergence.md` § I3.
///
/// Consequently `floor_db: None` is **not** an early return: scan still evaluates the donor from the
/// silence bit alone when A's gap has no silent block to set a floor, and so does this.
/// Silent / total donor bins under the I3 disjunction — the population behind
/// [`donor_silence_fraction_at_floor`]. Kept as a counts helper so the fraction's return type (and its
/// four test call sites) stay unchanged; see
/// `docs/dev/archive/TEMP-fingerprint-provenance-plan.md` §3b.
fn donor_silence_counts_at_floor(
    donor: &DonorSpan<'_>,
    channels: usize,
    core: &SilentCoreConfig,
    floor_db: Option<f64>,
) -> Option<(usize, usize)> {
    let ch = channels.max(1);
    let total_frames = donor.samples.len() / ch;
    let end = donor.frames.end.min(total_frames);
    if donor.frames.start >= end {
        return None;
    }
    let bf = core.bin_frames.max(1);
    let (mut total, mut silent) = (0usize, 0usize);
    let mut frame = donor.frames.start;
    while frame < end {
        let bin_end = (frame + bf).min(end);
        total += 1;
        let block = &donor.samples[frame * ch..bin_end * ch];
        let scanner_silent = is_silent_interleaved(
            block,
            ch,
            core.silence_peak_fraction,
            core.absolute_silence_rms,
        );
        let below_floor = floor_db.is_some_and(|floor| {
            bin_level_db(
                donor.samples,
                ch,
                frame,
                bin_end,
                ChannelReduction::Interleaved,
            )
            .is_some_and(|db| db < floor)
        });
        if scanner_silent || below_floor {
            silent += 1;
        }
        frame = bin_end;
    }
    (total > 0).then_some((silent, total))
}

/// Thin wrapper over [`donor_silence_counts_at_floor`] — kept so existing unit tests assert on the
/// fraction without churn. Production uses the counts helper directly.
#[cfg_attr(not(test), allow(dead_code))]
fn donor_silence_fraction_at_floor(
    donor: &DonorSpan<'_>,
    channels: usize,
    core: &SilentCoreConfig,
    floor_db: Option<f64>,
) -> Option<f64> {
    donor_silence_counts_at_floor(donor, channels, core, floor_db)
        .map(|(silent, total)| silent as f64 / total as f64)
}

/// Compute the equivalence verdict for one gap: A's gap-interior RMS (from decoded A) + the recording's noise
/// floor + the donor silence at the nominal span → the domain classification.
///
/// # The three F15 fixes
///
/// This front-end used to read three sensors differently from the authoritative scan path, each difference
/// large enough to flip a class on its own, and **all three biased it toward `drop`** — the dangerous
/// direction. All three are corrected here (`docs/dev/archive/TEMP-equivalence-divergence-findings.md` § *The three
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
    let (floor_db, a_rms, silent_bins, total_bins) = silent_core_levels(
        a_samples,
        ch,
        block_grid_bins(gap_frames, core.bin_frames, total_frames),
        ChannelReduction::Interleaved,
        core.silence_peak_fraction,
        core.absolute_silence_rms,
    );
    let donor_counts = donor
        .as_ref()
        .and_then(|d| donor_silence_counts_at_floor(d, ch, core, floor_db));
    let donor_fraction = donor_counts.map(|(silent, total)| silent as f64 / total as f64);
    classify_gap_equivalence(a_rms, noise_floor_db, donor_fraction, params).with_scan_provenance(
        floor_db,
        // Fine path always walks a bin grid (possibly empty) — `Some(0, 0)` is a real measurement
        // of zero bins, unlike scan's empty level stream (`None`).
        Some((silent_bins, total_bins)),
        donor_counts,
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
    ///
    /// The donor must clear **both** occupancy terms (I3): above A's silent-core floor *and* above the
    /// recipe's `absolute_silence_rms`. It was 1e-4 until 2026-07-31 — below the 0.001 absolute floor, so
    /// the scan path would have read it silent while this path read it occupied. That is the divergence
    /// I3 fixes, and encoding it here made the test assert the defect.
    #[test]
    fn silent_gap_occupied_donor_is_repairable() {
        let a = vec![0.0f32; 48_000]; // A gap interior = digital silence
        let b = vec![2e-3f32; 48_000]; // −54 dBFS: above A's −120 floor and the 0.001 abs floor ⇒ occupied
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
    ///
    /// The donor was 2e-4 until 2026-07-31, when I3 added the scanner's own silence bit to the donor
    /// predicate: 2e-4 is below the recipe's 0.001 `absolute_silence_rms`, so scan read that donor silent
    /// and only this path read it occupied. 2e-3 preserves the band (it still sits strictly between the
    /// silent-core floor and the content peak, which is what the test is about) while being occupied on
    /// both paths.
    #[test]
    fn band_donor_mechanism_now_classifies_as_repairable() {
        let (quiet, loud, donor) = (1e-5f32, 0.5f32, 2e-3f32);
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
        // `absolute_silence_rms: 0.0` disables the scanner's absolute peak floor, isolating the *floor*
        // term of the I3 disjunction. At the run recipe's 0.001 this 2e-4 donor would be scanner-silent
        // outright and the floor comparison could not be observed. The relative rule stays live and does
        // not fire here: rms == peak, so `rms < peak × 0.01` is false.
        let floor_only = SilentCoreConfig {
            absolute_silence_rms: 0.0,
            ..core()
        };
        let whole_span_peak = 20.0 * f64::from(0.5f32).log10();
        assert_eq!(
            donor_silence_fraction_at_floor(&span, 1, &floor_only, Some(whole_span_peak)),
            Some(1.0),
            "against the content peak the donor reads fully silent — the pre-fix bug"
        );
        let silent_core = 20.0 * f64::from(1e-5f32).log10();
        assert_eq!(
            donor_silence_fraction_at_floor(&span, 1, &floor_only, Some(silent_core)),
            Some(0.0),
            "against the silent core it reads fully occupied"
        );
    }

    /// No floor ⇒ the donor is still evaluated, from the **silence bit alone** — matching scan, which
    /// computes `donor_blocks` whether or not A's gap yielded a floor (I3). Only an empty span is absent.
    ///
    /// Inert at the classifier: `floor_db` and `a_gap_rms_db` come from the same silent set, so a gap with
    /// no floor also has no A RMS and classifies `NotEvaluated` regardless of the donor.
    #[test]
    fn donor_without_a_floor_is_evaluated_from_the_silence_bit() {
        let floor_only = SilentCoreConfig {
            absolute_silence_rms: 0.0,
            ..core()
        };
        let b = vec![1e-4f32; 4_800];
        let span = DonorSpan {
            samples: &b,
            frames: 0..4_800,
        };
        assert_eq!(
            donor_silence_fraction_at_floor(&span, 1, &floor_only, None),
            Some(0.0),
            "no floor and not scanner-silent ⇒ measured occupied, not absent"
        );
        let empty = DonorSpan {
            samples: &b,
            frames: 4_800..4_800,
        };
        assert_eq!(
            donor_silence_fraction_at_floor(&empty, 1, &floor_only, Some(-60.0)),
            None,
            "an empty span is the only 'not evaluated' case"
        );
    }

    /// **I3 acceptance test.** A digitally silent gap with a digitally silent donor must classify
    /// `SharedSilence` (drop), as the scan path does.
    ///
    /// This is the case the corpus cannot produce: every pair measured to date is lossy, whose decoded
    /// floors bottom out near −101 dB and never reach the −120 clamp. Before the disjunct landed, the
    /// floor-only predicate read this donor **occupied** — `−120 < −120` is false — giving
    /// `RepairableDropout` (keep) against scan's drop. That is the *dangerous* direction and the one
    /// condition `equivalence-calibration` exits 1 on.
    #[test]
    fn digitally_silent_donor_reads_silent_against_a_digitally_silent_floor() {
        let a = vec![0.0f32; 48_000];
        let b = vec![0.0f32; 48_000];

        // The mechanism, pinned inline so this test cannot pass for an unrelated reason: a digitally
        // silent bin sits *at* the floor, and the strict `<` therefore excludes it.
        let bin = bin_level_db(&b, 1, 0, 2_400, ChannelReduction::Interleaved).expect("bin level");
        assert_eq!(bin, SILENCE_FLOOR_DB, "digital silence clamps to the floor");
        let floor_only_says_silent = bin < SILENCE_FLOOR_DB;
        assert!(
            !floor_only_says_silent,
            "floor-only reads digital silence as occupied — the I3 defect"
        );

        let v = measure_gap_equivalence(
            &a,
            1,
            0..24_000,
            Some(-50.0),
            donor(&b),
            &core(),
            &GapEquivalenceParams {
                enabled: true,
                ..Default::default()
            },
        );
        assert_eq!(v.donor_silence_fraction, Some(1.0), "{v:?}");
        assert_eq!(v.gap_floor_db, Some(SILENCE_FLOOR_DB), "{v:?}");
        assert_eq!(v.class, GapEquivalenceClass::SharedSilence, "{v:?}");
        assert!(v.drop, "{v:?}");
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
        assert_eq!(
            v.a_gap_total_blocks,
            Some(20),
            "total includes the loud bin: {v:?}"
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
        assert_eq!(v.a_gap_total_blocks, Some(20), "{v:?}");
    }

    /// Track B: donor population counts ride with the fraction on the diagnostic verdict.
    #[test]
    fn measured_donor_population_counts_are_recorded() {
        let a = vec![0.0f32; 48_000];
        let b = vec![2e-3f32; 48_000];
        let v = measure_gap_equivalence(&a, 1, 0..48_000, Some(-50.0), donor(&b), &core(), &on());
        assert_eq!(v.donor_silent_blocks, Some(0), "{v:?}");
        assert_eq!(v.donor_total_blocks, Some(20), "{v:?}");
        assert_eq!(v.donor_silence_fraction, Some(0.0), "{v:?}");
    }

    /// Track B: `with_measurement` is provenance-only — class/drop unchanged (attached at the caller).
    #[test]
    fn attaching_measurement_leaves_the_class_untouched() {
        use crate::domain::gap_equivalence::{EquivalenceMeasurement, SpanKind};
        let a = vec![0.0f32; 48_000];
        let v = measure_gap_equivalence(&a, 1, 0..48_000, Some(-50.0), None, &core(), &on());
        let with = v.clone().with_measurement(EquivalenceMeasurement {
            context_secs: 3.0,
            bin_ms: 100,
            reduction: ChannelReduction::Interleaved,
            a_span: SpanKind::Core,
            donor_span: Some(SpanKind::Nominal),
        });
        assert_eq!((with.class, with.drop), (v.class, v.drop));
        let m = with.measurement.expect("attached");
        assert_eq!(m.donor_span, Some(SpanKind::Nominal));
        assert!((m.context_secs - 3.0).abs() < f64::EPSILON);
    }

    /// The grid is phase-locked to the media, not the gap: shifting a gap by less than a bin changes
    /// which block centres it captures, exactly as the scan path's filter does. This is the property that
    /// made the diagnostic path's tiling disagree with scan by one to two blocks at the edges.
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
    /// That is scan's semantics and the reason the diagnostic path's edge blocks used to differ: content ramps
    /// live at the edges, and a max statistic is maximally sensitive to them.
    #[test]
    fn block_grid_blocks_are_whole_even_when_they_overrun_the_gap_edge() {
        let bins: Vec<_> = block_grid_bins(140..260, 100, 10_000).collect();
        assert_eq!(bins, vec![100..200, 200..300], "{bins:?}");
    }

    /// No trailing partial bin — centre-containment discards it by construction.
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

    #[test]
    fn empty_span_yields_none_rms_and_not_evaluated() {
        let a = vec![0.0f32; 100];
        let v = measure_gap_equivalence(&a, 1, 50..50, Some(-50.0), None, &core(), &on());
        assert_eq!(v.class, GapEquivalenceClass::NotEvaluated);
        assert_eq!(v.gap_floor_db, None, "no bins ⇒ no floor: {v:?}");
        assert_eq!(v.a_gap_total_blocks, Some(0), "{v:?}");
    }
}
