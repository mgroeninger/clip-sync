//! Gap content-equivalence gate — "does this gap actually need patching?" (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate).
//!
//! **Silence-character classification.** A scanned silent run in A is worth repairing only when A's signal
//! genuinely *died* (a dropout) **and** B carries the missing content. The two signals that decide it, both
//! already in the fingerprint:
//!
//! - **A-side (`a_rms` vs the recording's `noise_floor_db`):** a true dropout sits **far below** A's own noise
//!   floor (the signal is gone); a genuine quiet passage sits **at** the noise floor (room tone). Measuring
//!   *relative to the noise floor* makes the threshold **self-calibrating** — no hard-coded absolute dB.
//! - **B-side (`donor_silence_fraction`):** if B is silent at the nominal span there is nothing to fill with.
//!   A B block counts as silent when [`BlockLevel::silent`] (scanner predicate) **or** quieter than
//!   A's gap floor — so digital silence / abs-floor quiet is not misread as occupied via a strict
//!   `rms_db < gap_floor` compare at the −120 floor.
//!
//! Empirically (licensed media): the two silence signals separate cleanly (dropouts ≥35 dB below noise floor,
//! `donor_silence` bimodal at ~0 vs ~1) where the seam/lag approach failed — the recordings drift, so "B matches
//! A" is never a lag-0 match. This gate replaces that approach.

use serde::{Deserialize, Serialize};

use crate::domain::policies::{BlockLevel, BLOCK_LEVEL_FLOOR_DB};

/// Context window (seconds) each side of a gap used to estimate A's local noise floor from the scan's
/// per-block level timeline — blocks in `[a_start − ctx, a_end + ctx]` but **outside** the gap. The
/// scan-time analogue of the fingerprint's `gap_signature_context_secs`.
pub const EQUIVALENCE_CONTEXT_SECS: f64 = 2.0;

/// Tunable thresholds for the equivalence gate (all overridable; gate is **off by default**).
#[derive(Debug, Clone, Copy)]
pub struct GapEquivalenceParams {
    /// Master on/off. When `false`, every gap classifies `NotEvaluated` (keep) — zero behavior change.
    pub enabled: bool,
    /// A counts as a **dropout** when `a_rms_db < noise_floor_db − dropout_margin_db` (default `35.0`).
    /// Relative to the recording's own noise floor, so it self-calibrates across noisy/clean sources.
    pub dropout_margin_db: f64,
    /// B counts as **occupied** when `donor_silence_fraction < donor_silence_thresh` (default `0.5`, the
    /// program-quiet valley); at/above ⇒ B silent ⇒ nothing to fill.
    pub donor_silence_thresh: f64,
    /// Register the donor window against A's envelope before measuring it — see
    /// [`DonorRegistrationParams`]. `None` (the default) measures at the nominal offset map, which is
    /// what every dump before 2026-08-03 recorded.
    pub donor_registration: Option<DonorRegistrationParams>,
}

impl Default for GapEquivalenceParams {
    fn default() -> Self {
        Self {
            enabled: false,
            dropout_margin_db: 35.0,
            donor_silence_thresh: 0.5,
            donor_registration: None,
        }
    }
}

/// Tunables for **local donor registration** — the fix for the defect in
/// `docs/dev/TEMP-equivalence-band/` §2.5 / §6.4 (`06-donor-registration.md`).
///
/// The gate runs pre-decode with a single global `offset_secs` for the whole pair, and one constant
/// cannot track local drift: on the 12-gap listen set the donor window was misplaced by 80–410 ms,
/// which is what clustered `donor_silence_fraction` at exactly 0.500 and made the margin band look
/// like a threshold problem. Re-registering B against A's own block envelope — no decode, ~21 dot
/// products over 40–70 bins — recovered the lag on all 12 and brought the in-gap levels to within
/// 0.7 dB on 11 of them. The twelfth is a real dropout A rides at digital zero while B keeps its
/// −68 dB bed; it registers cleanly (r = 0.970) and separates on `interior_delta_db` (+35.3 dB)
/// instead. Registration is still allowed to **abstain** — see [`Self::min_envelope_r`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DonorRegistrationParams {
    /// Whether the registration is allowed to change the verdict — see [`DonorRegistrationMode`].
    pub mode: DonorRegistrationMode,
    /// Search half-width in blocks. `10` at the 100 ms default block ⇒ ±1.0 s, comfortably past the
    /// 410 ms worst case observed.
    pub max_lag_blocks: usize,
    /// Peak envelope correlation below which the registration is not trusted and the gap
    /// **abstains** ([`NotEvaluatedReason::DonorRegistrationUnreliable`]) instead of classifying.
    /// Every gap on the listen set registered at 0.883 or better, so `0.70` fires only on material
    /// where the two timelines genuinely do not correspond — it is a floor, not a tuned split.
    ///
    /// Read only under [`DonorRegistrationMode::Apply`]; under `Observe` the abstain would *be* a
    /// verdict change, so `peak_r` is merely recorded and the reader applies their own floor.
    pub min_envelope_r: f64,
}

/// What a computed [`DonorRegistration`] is allowed to do.
///
/// The two are separate because the open question was a **rate**, not a mechanism: the registration
/// was validated on twelve gaps that were listened to, and nobody could say how often it abstained,
/// or how often it moved the window far enough to flip a class, across the whole corpus.
/// [`Observe`](Self::Observe) answered that from a dump without putting a single verdict at risk.
///
/// **The rate is now measured and [`Apply`](Self::Apply) is the production default**
/// (`RepairConfig::apply_donor_registration`, 2026-08-04): over 39 pairs / 782 registrations it
/// abstains on 4.3 %, moves 16 gaps (2.05 %), touches none of the 236 dropouts at the digital-zero
/// rail, and the three patches it stops were all listened to and were all degrading undamaged
/// material (`docs/dev/TEMP-equivalence-band/` §6.10, §7.4a). `Observe` remains
/// the enum's `#[default]` — the mode is chosen from config at the one production construction
/// site, so a caller that asks for registration without saying what for still cannot silently move
/// a decision — and it is what `--no-apply-donor-registration` selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DonorRegistrationMode {
    /// Compute the registration and record it on the verdict, then classify at the **nominal** map
    /// exactly as an un-registered gate would. Provenance only — byte-identical verdicts, plus one
    /// `donor_registration` block per gap. The enum's default, so a caller that asks for
    /// registration without saying what for cannot silently move a decision — but *not* what the
    /// repair binary runs: production selects `Apply` from config. Chosen by
    /// `--no-apply-donor-registration`.
    #[default]
    Observe,
    /// Measure the donor window at the registered lag, and **abstain**
    /// ([`NotEvaluatedReason::DonorRegistrationUnreliable`]) when `peak_r < min_envelope_r`. The
    /// fix, and the shipped production behaviour since 2026-08-04; it changes classes by
    /// construction.
    Apply,
}

impl DonorRegistrationMode {
    fn applies(self) -> bool {
        matches!(self, Self::Apply)
    }
}

impl Default for DonorRegistrationParams {
    fn default() -> Self {
        Self {
            mode: DonorRegistrationMode::Observe,
            max_lag_blocks: 10,
            min_envelope_r: 0.70,
        }
    }
}

/// Fewer bins than this and the envelope correlation is not worth believing (the ±2 s context at the
/// 100 ms default block yields 40–70).
const MIN_REGISTRATION_BINS: usize = 8;

/// Where the donor window actually sits relative to the nominal offset map, and what B looks like
/// once it is put there. Provenance **and** input: when `peak_r >= min_envelope_r` the donor fraction
/// on the verdict is the one measured at `lag_blocks`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DonorRegistration {
    /// Blocks B's window must move to line up with A. Positive ⇒ later in B.
    pub lag_blocks: i64,
    /// `lag_blocks` in milliseconds, at the level stream's own bin width.
    pub lag_ms: f64,
    /// Envelope correlation at `lag_blocks`, over the **shoulders only** (see
    /// [`register_donor_window`]). Low means the two timelines do not correspond here at all — a
    /// registration failure, not an interior difference. Observed range on the listen set:
    /// 0.883–0.999.
    pub peak_r: f64,
    /// Envelope correlation at the **nominal** map (lag 0), i.e. what the un-registered gate was
    /// implicitly assuming. `peak_r − nominal_r` is how much the misregistration cost.
    pub nominal_r: f64,
    /// Bins behind both correlations.
    pub bins: usize,
    /// A's in-gap level with one bin eroded from each edge. Erosion is **required**: without it the
    /// 100 ms grid quantization produced +25 / −15 dB artifacts on two of the twelve.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_interior_db: Option<f64>,
    /// B's level over the registered donor window, eroded the same way.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub b_interior_db: Option<f64>,
    /// `b_interior_db − a_interior_db`. Near zero ⇒ B is as quiet as A and there is nothing to fill;
    /// large and positive ⇒ B carries content A lost. Recorded, **not** classified on yet.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub interior_delta_db: Option<f64>,
    /// The two envelopes the numbers above were derived from — see [`RegistrationEnvelopes`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub envelopes: Option<RegistrationEnvelopes>,
}

/// The dB envelopes [`register_donor_window`] correlated, recorded so that **every question which
/// moves the donor window can be re-asked from a dump alone**.
///
/// **Why the summary numbers were not enough.** Everything else on [`DonorRegistration`] is an
/// *output*: one lag, two correlations, two interior levels. That is enough to read what the
/// registration decided and not enough to ask it anything else — a different `max_lag_blocks`, a
/// different `min_envelope_r`, the correlation with the core left in, or the donor fraction at the
/// lag the gate *didn't* use. [`GapEquivalenceVerdict::donor_blocks`] has the same limit in the other
/// direction: it reproduces the fraction exactly, but only at the one window that was measured. So
/// answering "how often would `Apply` have flipped a class" cost a full corpus re-dump (2026-08-03)
/// instead of a script over the corpus already on disk. With both envelopes recorded it is a script.
///
/// The slices are the raw scanner bins, not a re-derivation: replaying `register_donor_window` over
/// them reproduces `lag_blocks` / `peak_r` / `nominal_r` exactly, and re-running the donor count
/// (`silent || rms_db < gap_floor_db`) over B reproduces
/// [`GapEquivalenceVerdict::donor_silence_fraction`] at any lag in range. Both are pinned by tests.
///
/// Size is ~40–70 bins for A and that plus `2 * max_lag_blocks` for B — about 1 KB of JSON against
/// an ~18 KB gap dump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrationEnvelopes {
    /// Bin width in ms. One scan recipe per pair, so both sides share it. The streams are the
    /// scanner's fixed-duration blocks, so bin `i` of a slice starts at `start_secs + i * bin_ms`.
    pub bin_ms: f64,
    /// A's bins over the gap ± [`EQUIVALENCE_CONTEXT_SECS`], **core included** — the core is excluded
    /// from the correlation by `core_bins`, not by being left out of the record, so the exclusion is
    /// a policy a replay can vary rather than a hole in the data.
    pub a: EnvelopeSlice,
    /// B's bins over the same window, padded by [`DonorRegistrationParams::max_lag_blocks`] on each
    /// side so that every lag the search tried can be replayed. Shorter than that when the stream
    /// ends first; `b_nominal_bin` says where the nominal map actually landed.
    pub b: EnvelopeSlice,
    /// Index into `b` that the **nominal** map aligns with `a[0]`, i.e. lag 0. Bin `i` of `a` pairs
    /// with bin `b_nominal_bin + i + lag` of `b`.
    pub b_nominal_bin: usize,
    /// `[start, end)` bins of `a` falling in the gap core — the range the correlation excluded.
    pub core_bins: (usize, usize),
}

/// One side's bins, as recorded by [`RegistrationEnvelopes`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvelopeSlice {
    /// Seconds at which bin `0` starts, on this side's own timeline.
    pub start_secs: f64,
    /// Per-bin [`BlockLevel::rms_db`], at the stream's own `f64`. `f32` was tried and reverted: it
    /// is far below audible resolution but *not* below decision resolution — the donor count is
    /// `rms_db < gap_floor_db`, so a bin sitting on the floor can flip under rounding, and that is
    /// precisely the comparison a replay exists to re-ask. The extra ~1 KB per gap buys an exact
    /// replay instead of an almost-exact one.
    pub rms_db: Vec<f64>,
    /// Bins whose scanner [`BlockLevel::silent`] flag fired, by index. Sparse rather than a parallel
    /// `Vec<bool>` — the flag is the level-blind half of the donor count and is usually a minority of
    /// the window, so indices are both smaller and easier to read.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub silent_bins: Vec<u32>,
}

impl EnvelopeSlice {
    /// Capture `levels[start..end]`. Empty ranges are recorded as empty rather than refused — the
    /// caller has already decided the window is worth keeping.
    fn capture(levels: &[BlockLevel], start: usize, end: usize) -> Self {
        let bins = &levels[start..end];
        Self {
            start_secs: bins.first().map_or(0.0, |b| b.start_secs),
            rms_db: bins.iter().map(|b| b.rms_db).collect(),
            silent_bins: bins
                .iter()
                .enumerate()
                .filter(|(_, b)| b.silent)
                .map(|(i, _)| i as u32)
                .collect(),
        }
    }
}

/// Why a gap was not classified. `NotEvaluated` is the only variant that asserts nothing about the
/// audio, so it always fails open — but "the gate is off" and "B does not resemble A here" are very
/// different refusals, and a plan that keeps a gap should be able to say which one it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotEvaluatedReason {
    /// `params.enabled == false`.
    GateDisabled,
    /// A signal the classifier needs was absent (no A blocks, no donor mapped, empty window).
    MissingSignal,
    /// The donor envelope was correlated against A and came back below
    /// [`DonorRegistrationParams::min_envelope_r`] — the donor window cannot be placed, so no
    /// statement about B's occupancy is defensible.
    DonorRegistrationUnreliable,
}

/// Vocabulary for the gate — the reason a gap does or doesn't need patching. These are the
/// **scan-time silence-character cells** in [`docs/dev/gap-vocabulary.md`] (§ *Silence-character pre-gate*),
/// a pre-filter that runs before the seam/donor cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapEquivalenceClass {
    /// A's signal died (RMS ≥ `dropout_margin_db` below the recording's noise floor) **and** B carries content
    /// — a real dropout with a fill source. **Keep** — *not a skip cell*: the gap proceeds into the normal
    /// seam/donor cells (Bracket-patch / Silence-splice / …).
    RepairableDropout,
    /// B is silent at the nominal span (`donor_silence ≥ thresh`) — nothing to fill with, patching can't help.
    /// **Drop.** (Both "A dropped out but the donor is also dead" and "quiet in both" land here.) This is the
    /// **plan-time detection of the Program-quiet cell** — the same disposition the patch path skips as
    /// `GapPatchSkipReason::ProgramQuiet`, surfaced before decode as `GapFillSkipReason::AlreadyMatchesReference`.
    SharedSilence,
    /// A is only ambient room tone (near its own noise floor), not a signal failure, though B has content — a
    /// genuine quiet passage, not a dropout. **Drop** (don't inject content into intentional quiet). A cell with
    /// no seam/donor counterpart — decided on A's own character, not B's donor state.
    AmbientQuiet,
    /// Gate disabled or a required signal missing — **keep** (no decision made). Also the `Default`:
    /// the only variant that asserts nothing about the audio, so a default-constructed verdict can
    /// never fabricate a drop.
    #[default]
    NotEvaluated,
}

impl GapEquivalenceClass {
    /// Whether this gap should be dropped from the fill plan (no patching needed).
    pub fn drops(self) -> bool {
        matches!(self, Self::SharedSilence | Self::AmbientQuiet)
    }
}

/// The gate's per-gap readout: the class + the signals it was derived from (for tuning + reporting).
///
/// `Default` is `NotEvaluated` with every signal absent, so tests and constructors can spread
/// `..Default::default()` and stay correct as provenance fields are added.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GapEquivalenceVerdict {
    pub class: GapEquivalenceClass,
    /// `class.drops()` — surfaced so consumers don't re-derive it.
    pub drop: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_gap_rms_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub noise_floor_db: Option<f64>,
    /// `a_gap_rms_db − noise_floor_db` — how far below the noise floor A's gap sits (the self-calibrated signal).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_below_noise_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_silence_fraction: Option<f64>,
    /// The floor `donor_silence_fraction` was measured against. **Provenance, not a classification
    /// input** — nothing reads it to decide a class; it is recorded so the donor fraction can be
    /// audited after the fact.
    ///
    /// The two equivalence front-ends define this differently, and the difference is decision-sized
    /// (~20 dB observed): the scan path uses the loudest **silent** A block in the gap (immune to
    /// hold-bridging and edge refinement — the F2/R1 definition), while the fingerprint path uses
    /// the loudest content **anywhere** in the gap span, unfiltered. Recording it is what makes the
    /// two comparable at all; before this it was derivable only as an arithmetic bound.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gap_floor_db: Option<f64>,
    /// Count of A blocks inside the gap that passed the silence test — the population behind
    /// `a_gap_rms_db` (energy mean) and `gap_floor_db` (max). Provenance only.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_gap_silent_blocks: Option<usize>,
    /// Blocks whose centre falls inside the gap (silent or not) — the denominator behind
    /// [`Self::a_gap_silent_blocks`]. With [`EquivalenceMeasurement::bin_ms`],
    /// `total × bin_ms ≈ span` is the corpus-wide bin-width check (I1 class). Provenance only.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_gap_total_blocks: Option<usize>,
    /// Silent / total donor blocks behind `donor_silence_fraction`. Provenance only — a fraction
    /// alone cannot distinguish `1/10` from `1.1/11`, which matters when comparing paths that bin
    /// the same span differently.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_silent_blocks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_total_blocks: Option<usize>,
    /// The A-side window actually measured, `(start, end)` in seconds on **A's** timeline.
    ///
    /// [`EquivalenceMeasurement::a_span`] records the window's *kind*; this records its *extent*.
    /// The token alone proved insufficient: through 2026-08-01 both front-ends emitted `core` while
    /// measuring different intervals — scan the block-confirmed silent core, the diagnostic path the
    /// raw hold-bridged run — and `a_gap_total_blocks` disagreed on 66.9 % of a 39-pair corpus with
    /// nothing in the dump able to say why. A span the reader can subtract makes that arithmetic,
    /// not archaeology.
    ///
    /// It is also the **transport** for the convergence fix: the diagnostic overlay reads this off
    /// the index-parallel scan verdict so both paths bin one interval by construction rather than by
    /// agreement. Provenance plus plumbing; never classified on.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_span_secs: Option<(f64, f64)>,
    /// The donor window actually measured, `(start, end)` in seconds on **B's** timeline. `None`
    /// when no donor was mapped. Same rationale as [`Self::a_span_secs`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_span_secs: Option<(f64, f64)>,
    /// The one measurement recipe that produced this verdict — see [`EquivalenceMeasurement`].
    /// Absent on pre-Track-B corpora and when the scan path has no level stream to derive `bin_ms`
    /// from. Provenance only; never classified on.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub measurement: Option<EquivalenceMeasurement>,
    /// Candidate **noise floors** over a grid of context windows × bin sizes — see [`NoiseFloorProbe`].
    /// Provenance only; empty (and omitted) unless a front-end computes them. **Retained** for I2
    /// residual attribution.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub noise_floor_probes: Vec<NoiseFloorProbe>,
    /// The thresholds `class` was decided against — see [`GapEquivalenceThresholds`].
    ///
    /// `Some` **iff the classifier actually compared something**: absent on both `NotEvaluated`
    /// returns (gate off, or a missing signal), present on every decided class. That is a stronger
    /// signal than [`Self::measurement`], which is attached by the front-ends after the fact and is
    /// present on all four classes — including the 20 `not_evaluated` gaps of the 39-pair corpus —
    /// so it cannot answer "was a comparison made".
    ///
    /// Provenance only; never read to classify (it *is* what classified).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thresholds: Option<GapEquivalenceThresholds>,
    /// Where the donor window was placed and how well it correlated — see [`DonorRegistration`].
    /// `None` when registration was not requested, or when the envelopes carried no variance to
    /// align on (a flat envelope has no features; the nominal map is then as good as any other and
    /// today's behaviour stands).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_registration: Option<DonorRegistration>,
    /// Why `class` is [`NotEvaluated`](GapEquivalenceClass::NotEvaluated). `Some` **iff** it is —
    /// see [`NotEvaluatedReason`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub not_evaluated_reason: Option<NotEvaluatedReason>,
    /// Per-block evidence behind [`Self::donor_silence_fraction`] — see [`DonorBlockEvidence`].
    /// Provenance only; never classified on. Empty (and omitted) when no donor window was measured.
    ///
    /// **Why the counts were not enough.** [`GapEquivalenceThresholds`] already makes the distance to
    /// the occupancy boundary computable *in blocks* (`1 / donor_total_blocks`), and on the 39-pair
    /// corpus 14.5 % of gaps flip that boundary on a one-block change. What it cannot say is whether
    /// any given block is *near* flipping: a block 0.2 dB from the floor and one 30 dB below it both
    /// count as one silent block. That distinction is what separates a fraction that would survive a
    /// re-measurement from one that would not, and it is the question the 2026-08-03 run raised and
    /// could not answer — ten silent-donor gaps sitting at 0.500–0.625 with no way to tell how much
    /// slack was behind the count.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub donor_blocks: Vec<DonorBlockEvidence>,
}

/// One donor block's contribution to [`GapEquivalenceVerdict::donor_silence_fraction`], recorded so the
/// fraction can be re-derived — and its robustness judged — from the dump alone.
///
/// The gate counts a donor block silent when `flagged_silent || margin_db < 0`, so
/// `donor_silent_blocks` is exactly the number of these satisfying that predicate. Keeping the two
/// clauses apart is deliberate: they answer different questions and fail differently. `flagged_silent`
/// is the scanner's peak/crest test on **every channel**, which is level-blind by construction;
/// `margin_db` is a level comparison against A's gap floor, which is the clause a mis-placed donor
/// window moves.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DonorBlockEvidence {
    /// `rms_db − gap_floor_db` for this block. Negative ⇒ it cleared the level test, and the magnitude
    /// is the slack: how far the block would have to move to stop counting. `None` when the gap had no
    /// finite floor to compare against, in which case only `flagged_silent` could have fired.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub margin_db: Option<f64>,
    /// Whether the scanner's own per-block [`BlockLevel::silent`] flag fired. Independent of
    /// `margin_db` — either clause alone counts the block silent.
    pub flagged_silent: bool,
}

/// The two threshold constants one [`GapEquivalenceVerdict`] was decided against — the configured
/// half of the classification, opposite the measured half already on the verdict.
///
/// **Why this is recorded.** Every other input to the class is emitted: `a_gap_rms_db`,
/// `noise_floor_db`, `a_below_noise_db`, `donor_silence_fraction`, and the block populations behind
/// the last of these. The numbers they are *compared against* were not, so a reader could recompute
/// the class only by assuming the defaults in force on the day the dump was written — and
/// [`GapEquivalenceParams`] is explicitly overridable. Both front-ends hardcode
/// `..Default::default()` today, so the assumption happens to hold for every dump written before
/// 2026-08-01; recording it is what stops that from being a fact about this month.
///
/// This is also what makes the **margin band** computable from a dump rather than from source: the
/// band asks how far each gap sits from a boundary, and a distance needs both endpoints. Measured
/// against 35.0 dB / 0.5 on the 39-pair corpus, 2.9 % of gaps sit within 1 dB of the dropout
/// boundary and 14.5 % flip donor occupancy on a one-block change — but neither figure is
/// reproducible from a dump that does not say what "the boundary" was.
///
/// Deliberately **not** paired with an emitted `near_boundary` flag: the band width is a policy
/// under active calibration, and a stored boolean would freeze one width into every dump and drift
/// the moment it is retuned. Distances are derived; see `bin/equivalence_calibration.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GapEquivalenceThresholds {
    /// [`GapEquivalenceParams::dropout_margin_db`] as applied — A is a dropout below
    /// `noise_floor_db − dropout_margin_db`. Distance to that boundary is
    /// `a_below_noise_db + dropout_margin_db`.
    pub dropout_margin_db: f64,
    /// [`GapEquivalenceParams::donor_silence_thresh`] as applied — B is occupied below this
    /// fraction. Distance to that boundary is `donor_silence_fraction − donor_silence_thresh`;
    /// one block of it is `1 / donor_total_blocks`.
    pub donor_silence_thresh: f64,
}

impl From<&GapEquivalenceParams> for GapEquivalenceThresholds {
    fn from(p: &GapEquivalenceParams) -> Self {
        Self {
            dropout_margin_db: p.dropout_margin_db,
            donor_silence_thresh: p.donor_silence_thresh,
        }
    }
}

/// Which span a front-end measured on — A-side window or donor window.
///
/// One enum for both [`EquivalenceMeasurement::a_span`] and
/// [`EquivalenceMeasurement::donor_span`]: today A always emits [`Core`](SpanKind::Core), and the
/// live residual is donor **core vs nominal**. A single-variant A-side enum would have to widen the
/// first time A splits — exactly the event the field exists to make visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Block-confirmed / offset-mapped **core** (scan's A window and donor window; diagnostic A).
    Core,
    /// Nominal `b_mapped` span (diagnostic donor).
    Nominal,
}

/// The recipe that classified one [`GapEquivalenceVerdict`] — permanent replacement for the deleted
/// `silent_core_probes` grid. Nested so it stays visually distinct from the retained
/// [`NoiseFloorProbe`] **candidate** grid (I2).
///
/// See `docs/dev/archive/TEMP-fingerprint-provenance-plan.md` §3a. Provenance only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquivalenceMeasurement {
    /// Context half-width in seconds each side of the gap used for `noise_floor_db`.
    pub context_secs: f64,
    /// Bin width in milliseconds actually measured (from the level stream / configured overlay).
    pub bin_ms: u64,
    pub reduction: ChannelReduction,
    /// Which A-side window was measured. Always present: A is the gap itself, so there is always one.
    pub a_span: SpanKind,
    /// Which donor window was measured, or `None` when **no donor window was measured at all** —
    /// B unmapped, mapped before zero, or past B's end.
    ///
    /// `Option` since 2026-08-01. It was a bare `SpanKind` and therefore said `core` on every gap that
    /// had no donor, describing a window that did not exist; the reader could not distinguish "measured
    /// the donor core" from "there was no donor". That is the same unreadable-default defect as the
    /// projection fields — a type with no way to express "not asked" — and it sat in the one field
    /// whose entire job is to let you check which window produced a verdict. `donor_silence_fraction`
    /// is `None` on exactly these gaps; the two now agree.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_span: Option<SpanKind>,
}

/// How a bin's multiple channels are collapsed to one level before it is expressed in dB — the
/// **third** F15 noise-floor variable, and the one the `(2 s, scan_block_ms)` probe row exposed by
/// failing to reproduce the scan floor on any gap.
///
/// The two differ by the zero-lag cross-correlation between the channels. With mean pairwise
/// correlation `ρ̄` over `N` channels the ratio is `(1 + (N−1)·ρ̄) / N`: identical channels agree
/// exactly, decorrelated ones differ by `10·log10(N)` (7.78 dB at 6 channels), and anti-correlated
/// ones diverge without bound. Cauchy–Schwarz makes [`Downmix`](ChannelReduction::Downmix) ≤
/// [`Interleaved`](ChannelReduction::Interleaved) *always*, so the sign of the gap carries no
/// information — only its size does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChannelReduction {
    /// Average the channels per frame, **then** square: an amplitude mean, i.e. a mono downmix. What
    /// the diagnostic path's `mono_rms` / `interleaved_to_mono` do. The variant existing dumps recorded, so
    /// it is the `serde` default.
    #[default]
    Downmix,
    /// RMS over all interleaved samples: a power mean across channels, independent of inter-channel
    /// phase. What the scan path's `block_rms_db` does via `rms_interleaved`.
    Interleaved,
}

/// A candidate `noise_floor_db` — median dB over the context bins **outside** the gap — measured at one
/// `(context window, bin size, channel reduction)` combination.
///
/// This described I2 as "the one surviving axis" until 2026-08-01, when **I2 was closed by removal**:
/// the diagnostic overlay now estimates its equivalence floor over scan's ±2.0 s
/// (`EQUIVALENCE_CONTEXT_SECS`) rather than its own ±3.0 s, and the two floors agree to f32 rounding
/// (median 1e-6 dB, max 0.258 dB over a 4-pair re-dump). Do not quote the old 0.606 dB residual — it
/// was measured against an interval no path uses now. See `bin/equivalence_calibration.rs`.
///
/// The grid is **retained** anyway, and that is the point: it is what keeps context sensitivity
/// *askable* after the live measurement stopped varying. A labelled row in the provenance is where a
/// "what if the window were wider" question belongs — not as an unlabelled difference inside the
/// verdict being compared.
///
/// **Provenance only.** Emitted over a grid so the variables can be separated: the probe at scan's own
/// `(2 s, scan_block_ms, Interleaved)` should reproduce `scan_equivalence.noise_floor_db`, and the
/// crosses isolate each variable's contribution. The window/bin-only grid did *not* reproduce it —
/// undershooting 3.13–7.96 dB uniformly — which is what added [`ChannelReduction`] as the third
/// dimension. A residual after all three is most likely the excluded span (the diagnostic path excludes the *refined*
/// gap, scan the block-confirmed *core*).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoiseFloorProbe {
    /// Context half-width in seconds each side of the gap.
    pub context_secs: f64,
    /// Bin width in milliseconds the context was binned at.
    pub bin_ms: u64,
    /// How each bin's channels were collapsed to one level. Defaults to
    /// [`Downmix`](ChannelReduction::Downmix) when absent, which is what dumps predating this field
    /// recorded.
    #[serde(default)]
    pub reduction: ChannelReduction,
    /// Median dB over the context bins — the candidate floor. `None` when the context was empty,
    /// rather than the `median()` helper's −120 placeholder, so "no context" is distinguishable from
    /// "silent context".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub floor_db: Option<f64>,
    /// Context bins behind the median — the population. A median over 40 bins and one over 400 are
    /// not equally trustworthy, and the two windows differ in exactly this.
    pub context_bins: usize,
}

impl GapEquivalenceVerdict {
    fn of(class: GapEquivalenceClass, a: Option<f64>, nf: Option<f64>, ds: Option<f64>) -> Self {
        Self {
            class,
            drop: class.drops(),
            a_gap_rms_db: a,
            noise_floor_db: nf,
            a_below_noise_db: match (a, nf) {
                (Some(a), Some(nf)) => Some(a - nf),
                _ => None,
            },
            donor_silence_fraction: ds,
            gap_floor_db: None,
            a_gap_silent_blocks: None,
            a_gap_total_blocks: None,
            donor_silent_blocks: None,
            donor_total_blocks: None,
            donor_blocks: Vec::new(),
            a_span_secs: None,
            donor_span_secs: None,
            measurement: None,
            noise_floor_probes: Vec::new(),
            thresholds: None,
            donor_registration: None,
            not_evaluated_reason: None,
        }
    }

    /// Record why this verdict is `NotEvaluated`. Debug-asserts the class matches: a reason on a
    /// decided class would describe an intent instead of a measurement, the defect already corrected
    /// twice on the provenance fields.
    #[must_use]
    fn with_not_evaluated_reason(mut self, reason: NotEvaluatedReason) -> Self {
        debug_assert_eq!(self.class, GapEquivalenceClass::NotEvaluated);
        self.not_evaluated_reason = Some(reason);
        self
    }

    /// Attach the donor registration. Never changes `class` or `drop` — when registration *did*
    /// change the class it did so by moving the window the donor fraction was measured over, before
    /// the classifier ran.
    #[must_use]
    pub fn with_donor_registration(mut self, reg: Option<DonorRegistration>) -> Self {
        self.donor_registration = reg;
        self
    }

    /// Record the thresholds this verdict was decided against. Never changes `class` or `drop` —
    /// they were already decided *by* these values; this only writes them down.
    #[must_use]
    fn with_thresholds(mut self, params: &GapEquivalenceParams) -> Self {
        self.thresholds = Some(params.into());
        self
    }

    /// Attach population provenance (A silent/total + donor silent/total). Never changes `class` or
    /// `drop`. Both sides are `Option<(silent, total)>` — `None` means unanswerable (no level stream),
    /// not "zero blocks measured". See `docs/dev/archive/TEMP-fingerprint-provenance-plan.md` §3b.
    #[must_use]
    pub fn with_scan_provenance(
        mut self,
        gap_floor_db: Option<f64>,
        a_gap_blocks: Option<(usize, usize)>,
        donor_blocks: Option<(usize, usize)>,
    ) -> Self {
        self.gap_floor_db = gap_floor_db;
        (self.a_gap_silent_blocks, self.a_gap_total_blocks) = match a_gap_blocks {
            Some((silent, total)) => (Some(silent), Some(total)),
            None => (None, None),
        };
        (self.donor_silent_blocks, self.donor_total_blocks) = match donor_blocks {
            Some((silent, total)) => (Some(silent), Some(total)),
            None => (None, None),
        };
        self
    }

    // `with_gap_floor_db` (attach `levels.gap_floor_db` — the whole-span content peak) was removed with
    // F15 fix 1. The diagnostic path now measures its own silent-core floor and carries it via
    // `with_scan_provenance`; re-attaching the levels number would overwrite the fix with the statistic it
    // exists to replace. `levels.gap_floor_db` is still dumped in its own block.

    /// Attach the live measurement recipe. Never changes `class` or `drop`.
    #[must_use]
    pub fn with_measurement(mut self, measurement: EquivalenceMeasurement) -> Self {
        self.measurement = Some(measurement);
        self
    }

    /// Attach candidate noise floors. Never changes `class` or `drop`. **Retained** for I2
    /// attribution.
    #[must_use]
    pub fn with_noise_floor_probes(mut self, probes: Vec<NoiseFloorProbe>) -> Self {
        self.noise_floor_probes = probes;
        self
    }
}

/// After occupancy and donor silence both honor [`BlockLevel::silent`], a
/// donor-silent absolute read (`!b_has_energy`) must not disagree with a high
/// `donor_silence_fraction`. Missing donor fraction → vacuously true (nothing to compare).
///
/// Does **not** change plan precedence (`NotFillable` still wins); it only surfaces the
/// post-F1 consistency invariant (and catches regressions that reintroduce rms-only donor scoring).
pub fn occupancy_agrees_with_donor_silence(
    b_has_energy: bool,
    donor_silence_fraction: Option<f64>,
    donor_silence_thresh: f64,
) -> bool {
    match donor_silence_fraction {
        None => true,
        Some(ds) if !b_has_energy => ds >= donor_silence_thresh,
        Some(_) => true,
    }
}

/// Classify one gap from its silence signals. Pure — no I/O, no measurement.
///
/// - `NotEvaluated` when the gate is off or any signal is missing.
/// - `SharedSilence` when B is silent (nothing to fill).
/// - `RepairableDropout` when A's signal died (below the noise floor by the margin) and B is occupied.
/// - `AmbientQuiet` when B is occupied but A is only room tone (not a dropout).
pub fn classify_gap_equivalence(
    a_gap_rms_db: Option<f64>,
    noise_floor_db: Option<f64>,
    donor_silence_fraction: Option<f64>,
    params: &GapEquivalenceParams,
) -> GapEquivalenceVerdict {
    if !params.enabled {
        return GapEquivalenceVerdict::of(
            GapEquivalenceClass::NotEvaluated,
            a_gap_rms_db,
            noise_floor_db,
            donor_silence_fraction,
        )
        .with_not_evaluated_reason(NotEvaluatedReason::GateDisabled);
    }
    let (Some(a), Some(nf), Some(ds)) = (a_gap_rms_db, noise_floor_db, donor_silence_fraction)
    else {
        return GapEquivalenceVerdict::of(
            GapEquivalenceClass::NotEvaluated,
            a_gap_rms_db,
            noise_floor_db,
            donor_silence_fraction,
        )
        .with_not_evaluated_reason(NotEvaluatedReason::MissingSignal);
    };
    let is_dropout = a < nf - params.dropout_margin_db;
    let b_occupied = ds < params.donor_silence_thresh;
    let class = match (is_dropout, b_occupied) {
        (true, true) => GapEquivalenceClass::RepairableDropout,
        (_, false) => GapEquivalenceClass::SharedSilence,
        (false, true) => GapEquivalenceClass::AmbientQuiet,
    };
    // `with_thresholds` only on this path, never on the two `NotEvaluated` returns above: its
    // presence is the dump's answer to "was a comparison made", and stamping it on a refusal would
    // make it describe an intent instead of a measurement — the defect already corrected twice this
    // month (`a_span: core` on a raw-span read, `donor_span: core` with no donor).
    GapEquivalenceVerdict::of(class, a_gap_rms_db, noise_floor_db, donor_silence_fraction)
        .with_thresholds(params)
}

/// A block's timeline center (used for gap/context membership). Block duration is the
/// `scan_block_ms` recipe knob — do not restate it as a literal here; that is how this comment came
/// to claim 250 ms long after the default moved to 100.
fn block_center(b: &BlockLevel) -> f64 {
    (b.start_secs + b.end_secs) / 2.0
}

/// Combine per-block dB levels into one aggregate RMS in dB (energy mean of the blocks). `None` when empty.
pub(crate) fn aggregate_rms_db(levels: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum_sq = 0.0f64;
    let mut n = 0usize;
    for db in levels {
        let amp = 10f64.powf(db / 20.0);
        sum_sq += amp * amp;
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let rms = (sum_sq / n as f64).sqrt();
    Some(if rms <= 1e-9 {
        BLOCK_LEVEL_FLOOR_DB
    } else {
        20.0 * rms.log10()
    })
}

/// Pearson correlation of two equal-length series. `None` when either side is flat — a constant
/// envelope has no features to align, which is "cannot ask", not "does not match".
fn pearson(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.is_empty() {
        return None;
    }
    let n = x.len() as f64;
    let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        let (dx, dy) = (a - mx, b - my);
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    (sxx > 1e-12 && syy > 1e-12).then(|| sxy / (sxx * syy).sqrt())
}

/// Half-open index range of blocks whose **centre** falls in `[lo, hi)`. Blocks are sequential, so
/// this is a contiguous run.
fn window_indices(levels: &[BlockLevel], lo: f64, hi: f64) -> Option<(usize, usize)> {
    let start = levels.partition_point(|b| block_center(b) < lo);
    let end = levels.partition_point(|b| block_center(b) < hi);
    (end > start).then_some((start, end))
}

/// Aggregate level over `[lo, hi)` with one bin eroded from each edge. See
/// [`DonorRegistration::a_interior_db`] for why the erosion is not optional.
fn eroded_interior_db(levels: &[BlockLevel], lo: f64, hi: f64) -> Option<f64> {
    let (mut start, mut end) = window_indices(levels, lo, hi)?;
    if end - start > 2 {
        start += 1;
        end -= 1;
    }
    aggregate_rms_db(levels[start..end].iter().map(|b| b.rms_db))
}

/// Align B's level envelope to A's over the gap ± [`EQUIVALENCE_CONTEXT_SECS`], by cross-correlating
/// the two dB envelopes over `±max_lag_blocks`. Pure, decode-free — this is exactly the data the
/// scanner already holds, which is what makes it usable in a **pre-decode** gate where the fitted
/// per-gap lag is not available.
///
/// `None` when the window is too short, falls outside either stream, or is flat on either side.
pub fn register_donor_window(
    a_levels: &[BlockLevel],
    core_start_secs: f64,
    core_end_secs: f64,
    b_levels: &[BlockLevel],
    b_mapped: (f64, f64),
    params: &DonorRegistrationParams,
) -> Option<DonorRegistration> {
    let delta = b_mapped.0 - core_start_secs;
    let ctx_lo = core_start_secs - EQUIVALENCE_CONTEXT_SECS;
    let ctx_hi = core_end_secs + EQUIVALENCE_CONTEXT_SECS;

    let (a0, a1) = window_indices(a_levels, ctx_lo, ctx_hi)?;
    let (b0, _) = window_indices(b_levels, ctx_lo + delta, ctx_hi + delta)?;

    // **Shoulders only** — the gap core is excluded from the correlation. It is the one stretch where
    // A and B are *expected* to differ, and including it makes registration fail on exactly the gaps
    // that most need placing: a deep A dropout against a live B is a run of −110 dB outliers that
    // dominates the variance. Measured on the 12-gap listen set, excluding the core recovered the
    // identical lag on all twelve while lifting the worst correlation from 0.447 to 0.883 — and the
    // one gap that scored 0.447 (A at digital zero, B carrying its click bed) went to 0.970, its real
    // signal moving to `interior_delta_db` (+35.3 dB) where it belongs. `r` then means "the two
    // timelines correspond here", and nothing else.
    let offsets: Vec<i64> = (a0..a1)
        .filter(|&i| {
            let c = block_center(&a_levels[i]);
            !(c >= core_start_secs && c < core_end_secs)
        })
        .map(|i| (i - a0) as i64)
        .collect();
    let n = offsets.len();
    if n < MIN_REGISTRATION_BINS {
        return None;
    }
    let xs: Vec<f64> = offsets
        .iter()
        .map(|&o| a_levels[a0 + o as usize].rms_db)
        .collect();

    let max = params.max_lag_blocks as i64;
    let (mut best, mut nominal_r) = (None::<(i64, f64)>, None);
    let mut ys = vec![0.0; n];
    'lag: for lag in -max..=max {
        for (slot, &o) in ys.iter_mut().zip(&offsets) {
            let Ok(j) = usize::try_from(b0 as i64 + o + lag) else {
                continue 'lag;
            };
            let Some(b) = b_levels.get(j) else {
                continue 'lag;
            };
            *slot = b.rms_db;
        }
        let Some(r) = pearson(&xs, &ys) else { continue };
        if lag == 0 {
            nominal_r = Some(r);
        }
        if best.is_none_or(|(_, br)| r > br) {
            best = Some((lag, r));
        }
    }
    let (lag_blocks, peak_r) = best?;

    let bin_secs = b_levels.first().map_or(0.0, |b| b.end_secs - b.start_secs);
    let lag_secs = lag_blocks as f64 * bin_secs;
    let a_interior_db = eroded_interior_db(a_levels, core_start_secs, core_end_secs);
    let b_interior_db = eroded_interior_db(b_levels, b_mapped.0 + lag_secs, b_mapped.1 + lag_secs);
    // Record the inputs next to the outputs. B is padded by `max` on each side — the same reach the
    // search had — so a replay can ask about lags this run rejected, not only the one it chose.
    let b_lo = b0.saturating_sub(params.max_lag_blocks);
    let b_hi = (b0 + (a1 - a0) + params.max_lag_blocks).min(b_levels.len());
    let core = core_bins(a_levels, a0, a1, core_start_secs, core_end_secs);
    let envelopes = (b_hi > b_lo).then(|| RegistrationEnvelopes {
        bin_ms: bin_secs * 1000.0,
        a: EnvelopeSlice::capture(a_levels, a0, a1),
        b: EnvelopeSlice::capture(b_levels, b_lo, b_hi),
        b_nominal_bin: b0 - b_lo,
        core_bins: core,
    });
    Some(DonorRegistration {
        lag_blocks,
        lag_ms: lag_secs * 1000.0,
        peak_r,
        // A lag-0 read is always in range when the peak was (same `n`, same stream), but keep the
        // fallback honest rather than folding the two into one number.
        nominal_r: nominal_r.unwrap_or(peak_r),
        bins: n,
        a_interior_db,
        b_interior_db,
        interior_delta_db: match (a_interior_db, b_interior_db) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        },
        envelopes,
    })
}

/// The `[start, end)` bins of `a_levels[a0..a1]` whose centres fall in the gap core, as offsets into
/// that slice. The core is contiguous by construction (the bins are ordered and the core is an
/// interval), so the first and last matching offsets bound it; `(0, 0)` when none match.
fn core_bins(
    a_levels: &[BlockLevel],
    a0: usize,
    a1: usize,
    core_start_secs: f64,
    core_end_secs: f64,
) -> (usize, usize) {
    let in_core = |i: usize| {
        let c = block_center(&a_levels[i]);
        c >= core_start_secs && c < core_end_secs
    };
    match (a0..a1).find(|&i| in_core(i)) {
        Some(first) => (
            first - a0,
            (a0..a1).rev().find(|&i| in_core(i)).unwrap_or(first) - a0 + 1,
        ),
        None => (0, 0),
    }
}

/// Median of a set of dB values (used for A's local noise floor). `None` when empty.
fn median_db(mut vals: Vec<f64>) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(vals[vals.len() / 2])
}

/// Derive the per-gap silence-character signals from the scan's per-block level timelines and classify
/// (`docs/gap-scan.md`; vocabulary in `docs/dev/gap-vocabulary.md` § Silence-character pre-gate). All three signals are the scan-block
/// analogues of the fingerprint's finer-bin reads:
///
/// - **noise floor** — median dB of A blocks in `±`[`EQUIVALENCE_CONTEXT_SECS`] around the gap, **excluding**
///   blocks inside it (the recording's own floor; self-calibrating).
/// - **A gap RMS** — aggregate RMS of A's **silent** blocks inside the gap (hold-bridged non-silent
///   blocks inside the core interval are excluded so they cannot inflate dropout depth).
/// - **donor silence fraction** — fraction of B blocks over the nominal mapped span that are
///   scanner-silent ([`BlockLevel::silent`]) or quieter than A's gap floor.
///
/// `b_levels`/`b_mapped` are `None` when B was not scanned (missing/unaligned) ⇒ donor signal absent ⇒
/// `NotEvaluated`. Pure — no I/O.
///
/// Also records measurement **provenance** on the verdict (`gap_floor_db`, A/donor block populations,
/// [`EquivalenceMeasurement`]) — recorded, never classified on.
/// `core_start_secs`/`core_end_secs` are the **block-confirmed silent core** (`SilentRun::core_*`),
/// not the raw hold-bridged run — every caller passes the core and the classification is calibrated
/// on it. They were named `a_start_secs`/`a_end_secs` until 2026-08-01, which read as the nominal gap
/// span; the fingerprint's diagnostic path trusted the signature over the call site and bound the raw
/// span, diverging on 66.9 % of a 39-pair corpus while both sides printed `a_span: core`. The names
/// now say which interval this is.
pub fn derive_gap_equivalence(
    a_levels: &[BlockLevel],
    core_start_secs: f64,
    core_end_secs: f64,
    b_levels: Option<&[BlockLevel]>,
    b_mapped: Option<(f64, f64)>,
    params: &GapEquivalenceParams,
) -> GapEquivalenceVerdict {
    let centre_in_gap = |b: &BlockLevel| {
        let c = block_center(b);
        c >= core_start_secs && c < core_end_secs
    };
    // Silent A gap blocks only — hold can place non-silent levels inside the core interval.
    let gap_silent_blocks = || a_levels.iter().filter(|b| b.silent && centre_in_gap(b));
    // All A gap blocks (silent or not) — denominator for `a_gap_total_blocks` / the I1 bin check.
    let a_gap_total_blocks = a_levels.iter().filter(|b| centre_in_gap(b)).count();
    let a_gap_silent_blocks = gap_silent_blocks().count();
    let a_gap_rms_db = aggregate_rms_db(gap_silent_blocks().map(|b| b.rms_db));
    let gap_floor_db = gap_silent_blocks()
        .map(|b| b.rms_db)
        .fold(f64::NEG_INFINITY, f64::max);

    // A context blocks: within the context window but outside the gap → median = local noise floor.
    let ctx_lo = core_start_secs - EQUIVALENCE_CONTEXT_SECS;
    let ctx_hi = core_end_secs + EQUIVALENCE_CONTEXT_SECS;
    let noise_floor_db = median_db(
        a_levels
            .iter()
            .filter(|b| {
                let c = block_center(b);
                c >= ctx_lo && c < ctx_hi && !(c >= core_start_secs && c < core_end_secs)
            })
            .map(|b| b.rms_db)
            .collect(),
    );

    // Donor silence: scanner silent bit (peak/per-channel, abs floor baked in) OR quieter than
    // A's gap floor. Never re-threshold rms alone against the floor — digitally silent blocks
    // sit at BLOCK_LEVEL_FLOOR_DB and `rms < gap_floor` is false when both are −120.
    let gap_floor = gap_floor_db.is_finite().then_some(gap_floor_db);
    let count_donor = |bl: &[BlockLevel], (b_start, b_end): (f64, f64)| {
        let mut total = 0usize;
        let mut silent = 0usize;
        let mut evidence = Vec::new();
        for b in bl.iter().filter(|b| {
            let c = block_center(b);
            c >= b_start && c < b_end
        }) {
            total += 1;
            let margin_db = gap_floor.map(|g| b.rms_db - g);
            // Same disjunction as the count, written once — `margin_db < 0` *is* `rms_db < gap_floor`,
            // so the recorded evidence reproduces `silent` exactly rather than approximating it.
            let counted = b.silent || margin_db.is_some_and(|m| m < 0.0);
            if counted {
                silent += 1;
            }
            evidence.push(DonorBlockEvidence {
                margin_db,
                flagged_silent: b.silent,
            });
        }
        (silent, total, evidence)
    };

    // Register the donor window before measuring it. The nominal map is a single global constant for
    // the whole pair and drifts locally by 80–410 ms on real material, which is enough to slide the
    // window off B's silence and onto its content (§2.5/§6.4 of the band findings).
    let registration = match (params.donor_registration, b_levels, b_mapped) {
        (Some(rp), Some(bl), Some(bm)) => {
            register_donor_window(a_levels, core_start_secs, core_end_secs, bl, bm, &rp)
                .map(|reg| (rp, reg))
        }
        _ => None,
    };
    // Registration moves the window; it never edits the reading taken there. Under `Observe` it does
    // not even do that — the lag is recorded and the nominal window measured, so the dump can report
    // how far off the map was without any verdict depending on the answer.
    let donor_window = match (b_mapped, registration.as_ref()) {
        (Some((s, e)), Some((rp, reg))) if rp.mode.applies() => {
            let shift = reg.lag_ms / 1000.0;
            Some((s + shift, e + shift))
        }
        (bm, _) => bm,
    };

    let counted = match (b_levels, donor_window) {
        (Some(bl), Some(win)) => Some(count_donor(bl, win)),
        _ => None,
    };
    // Split the evidence off the counts: `with_scan_provenance` takes the pair, and the per-block
    // vector is attached separately so the existing provenance signature stays a pair of scalars.
    let (donor_blocks, donor_block_evidence) = match counted {
        Some((silent, total, ev)) => (Some((silent, total)), ev),
        None => (None, Vec::new()),
    };
    let donor_silence_fraction =
        donor_blocks.and_then(|(silent, total)| (total > 0).then(|| silent as f64 / total as f64));

    // Empty `a_levels` ⇒ no A population and no measurement (do not invent `Some(0)` / a bin — that
    // would claim "zero blocks measured" about a gap where nothing was measured).
    let a_gap_blocks = (!a_levels.is_empty()).then_some((a_gap_silent_blocks, a_gap_total_blocks));
    // A registration we don't believe is not a licence to fall back on the nominal map — that is the
    // window we already know is wrong. Abstain instead: `NotEvaluated` keeps the gap (fail open) and
    // the reason says why, so a keep here is honest rather than accidental.
    let unreliable = registration
        .as_ref()
        .is_some_and(|(rp, reg)| rp.mode.applies() && reg.peak_r < rp.min_envelope_r);
    let mut verdict = if unreliable && params.enabled {
        GapEquivalenceVerdict::of(
            GapEquivalenceClass::NotEvaluated,
            a_gap_rms_db,
            noise_floor_db,
            donor_silence_fraction,
        )
        .with_not_evaluated_reason(NotEvaluatedReason::DonorRegistrationUnreliable)
    } else {
        classify_gap_equivalence(a_gap_rms_db, noise_floor_db, donor_silence_fraction, params)
    }
    .with_scan_provenance(gap_floor, a_gap_blocks, donor_blocks)
    .with_donor_registration(registration.map(|(_, reg)| reg));
    verdict.donor_blocks = donor_block_evidence;
    // Record the intervals themselves, not just their kind — and hand the core to the fingerprint's
    // diagnostic path, which has no other way to see it (`GapFingerprint::geometry` carries the raw
    // span). Unconditional on `a_levels`: the window is a property of the caller's gap, known even
    // when no block fell in it.
    verdict.a_span_secs = Some((core_start_secs, core_end_secs));
    // The window actually measured, which is the registered one when registration ran — recording
    // the nominal span next to a fraction read somewhere else is the exact defect §2.5 documents.
    verdict.donor_span_secs = donor_window;
    // `bin_ms` is a property of the level stream, not the gap population — any block's width works.
    if let Some(b) = a_levels.first() {
        let bin_ms = ((b.end_secs - b.start_secs) * 1000.0).round().max(0.0) as u64;
        verdict = verdict.with_measurement(EquivalenceMeasurement {
            context_secs: EQUIVALENCE_CONTEXT_SECS,
            bin_ms,
            reduction: ChannelReduction::Interleaved,
            a_span: SpanKind::Core,
            // Only claim a donor window when one was mapped — `b_mapped` is `None` for a head gap
            // whose window starts before zero and for a tail gap past B's end, and this said `core`
            // on both until 2026-08-01.
            donor_span: b_mapped.map(|_| SpanKind::Core),
        });
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;
    use GapEquivalenceClass::*;

    fn on() -> GapEquivalenceParams {
        GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        }
    }

    fn class(a: f64, nf: f64, ds: f64) -> GapEquivalenceClass {
        classify_gap_equivalence(Some(a), Some(nf), Some(ds), &on()).class
    }

    /// `thresholds` is present exactly when a comparison happened — the property the margin band
    /// reads, and the one `measurement` cannot supply (it is attached by the front-ends after the
    /// fact and appears on `not_evaluated` gaps too).
    #[test]
    fn thresholds_are_recorded_iff_the_classifier_compared_something() {
        for (a, nf, ds) in [
            (-106.0, -47.0, 0.0), // repairable_dropout
            (-81.0, -71.0, 0.92), // shared_silence
            (-80.0, -70.0, 0.0),  // ambient_quiet
        ] {
            let v = classify_gap_equivalence(Some(a), Some(nf), Some(ds), &on());
            let t = v
                .thresholds
                .unwrap_or_else(|| panic!("{:?} must record what it was decided against", v.class));
            assert!((t.dropout_margin_db - 35.0).abs() < f64::EPSILON);
            assert!((t.donor_silence_thresh - 0.5).abs() < f64::EPSILON);
        }

        // Gate off: no comparison, so nothing to record.
        let off = classify_gap_equivalence(
            Some(-106.0),
            Some(-47.0),
            Some(0.0),
            &GapEquivalenceParams::default(),
        );
        assert_eq!(off.class, NotEvaluated);
        assert!(off.thresholds.is_none(), "gate off compared nothing");

        // Missing signal: same — a refusal must not carry the marks of a decision.
        let absent = classify_gap_equivalence(Some(-106.0), Some(-47.0), None, &on());
        assert_eq!(absent.class, NotEvaluated);
        assert!(absent.thresholds.is_none(), "no donor compared nothing");
    }

    /// Non-default params must round-trip, or the field records a constant instead of what ran.
    #[test]
    fn thresholds_record_the_params_in_force_not_the_defaults() {
        let tuned = GapEquivalenceParams {
            enabled: true,
            dropout_margin_db: 20.0,
            donor_silence_thresh: 0.8,
            donor_registration: None,
        };
        let v = classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.7), &tuned);
        let t = v.thresholds.expect("decided class records thresholds");
        assert!((t.dropout_margin_db - 20.0).abs() < f64::EPSILON);
        assert!((t.donor_silence_thresh - 0.8).abs() < f64::EPSILON);
        // And the class actually followed them: donor 0.7 is occupied at 0.8 but silent at 0.5.
        assert_eq!(v.class, RepairableDropout);
        assert_eq!(class(-106.0, -47.0, 0.7), SharedSilence);
    }

    /// The four measured licensed-media cases (noise floor ~−45 to −70; margin 35, donor thresh 0.5).
    #[test]
    fn measured_cases_classify_as_ground_truth() {
        // Repairable dropout: a_rms −106, noise_floor −47 ⇒ 59 dB below; donor 0.0.
        assert_eq!(class(-106.0, -47.0, 0.0), RepairableDropout);
        // Mutual silence: a_rms −81, noise_floor −71 ⇒ 10 dB below (not a dropout); donor 0.92 (B silent).
        assert_eq!(class(-81.0, -71.0, 0.92), SharedSilence);
        // Deep A but B dead (intro/tail): a_rms −108, noise_floor −46 ⇒ dropout, but donor 1.0 ⇒ nothing to fill.
        assert_eq!(class(-108.0, -46.0, 1.0), SharedSilence);
    }

    /// Ambient A with an occupied donor is a genuine quiet passage → drop (not a dropout).
    #[test]
    fn ambient_with_occupied_donor_is_quiet_passage() {
        assert_eq!(class(-80.0, -70.0, 0.0), AmbientQuiet); // only 10 dB below floor
        assert!(AmbientQuiet.drops());
    }

    /// The margin is self-calibrating: the same 40 dB drop is a dropout under both a low and a high noise floor.
    #[test]
    fn margin_is_relative_to_noise_floor() {
        assert_eq!(class(-100.0, -60.0, 0.0), RepairableDropout); // 40 dB below a −60 floor
        assert_eq!(class(-120.0, -80.0, 0.0), RepairableDropout); // 40 dB below a −80 floor
        assert_eq!(class(-90.0, -60.0, 0.0), AmbientQuiet); // only 30 dB below ⇒ not a dropout
    }

    #[test]
    fn drops_only_the_two_silence_classes() {
        assert!(!RepairableDropout.drops());
        assert!(!NotEvaluated.drops());
        assert!(SharedSilence.drops());
        assert!(AmbientQuiet.drops());
    }

    #[test]
    fn disabled_or_missing_signal_is_not_evaluated() {
        assert_eq!(
            classify_gap_equivalence(
                Some(-106.0),
                Some(-47.0),
                Some(0.0),
                &GapEquivalenceParams::default()
            )
            .class,
            NotEvaluated
        );
        assert_eq!(
            classify_gap_equivalence(None, Some(-47.0), Some(0.0), &on()).class,
            NotEvaluated
        );
        assert_eq!(
            classify_gap_equivalence(Some(-106.0), Some(-47.0), None, &on()).class,
            NotEvaluated
        );
    }

    #[test]
    fn verdict_reports_a_below_noise() {
        let v = classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &on());
        assert_eq!(v.a_below_noise_db, Some(-59.0));
        assert!(!v.drop && v.class == RepairableDropout);
    }

    #[test]
    fn occupancy_agrees_with_donor_when_both_say_silent() {
        assert!(occupancy_agrees_with_donor_silence(false, Some(1.0), 0.5));
        assert!(occupancy_agrees_with_donor_silence(false, Some(0.5), 0.5));
        assert!(occupancy_agrees_with_donor_silence(true, Some(0.0), 0.5));
        assert!(occupancy_agrees_with_donor_silence(false, None, 0.5));
    }

    #[test]
    fn occupancy_disagrees_when_absolute_silent_but_donor_occupied() {
        // Pre-F1 E1 shape: absolute silent + donor_silence 0.0.
        assert!(!occupancy_agrees_with_donor_silence(false, Some(0.0), 0.5));
        assert!(!occupancy_agrees_with_donor_silence(false, Some(0.49), 0.5));
    }

    // --- derive_gap_equivalence (scan-block timelines → signals → classification) --------------------

    /// One 250 ms block at `[t, t+0.25)` with level `db`. Gap-interior test blocks default to silent.
    fn blk(t: f64, db: f64) -> BlockLevel {
        blk_silent(t, db, true)
    }

    fn blk_silent(t: f64, db: f64, silent: bool) -> BlockLevel {
        BlockLevel {
            start_secs: t,
            end_secs: t + 0.25,
            rms_db: db,
            silent,
        }
    }

    /// A dropout gap (A blocks far below a −50 floor) with an occupied B donor → RepairableDropout, and
    /// the derived signals match the block inputs (context noise floor = median of the flanking blocks).
    #[test]
    fn derive_dropout_with_occupied_donor() {
        // Context blocks at −50 dB flank a 0.5 s gap of silence-floored blocks; B is loud across the span.
        let a = vec![
            blk(9.5, -50.0),
            blk(9.75, -50.0),
            blk(10.0, -119.0),
            blk(10.25, -119.0),
            blk(10.5, -50.0),
        ];
        let b = vec![
            blk_silent(10.0, -20.0, false),
            blk_silent(10.25, -20.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, RepairableDropout);
        assert_eq!(v.noise_floor_db, Some(-50.0));
        assert_eq!(v.donor_silence_fraction, Some(0.0));
        assert!(v.a_gap_rms_db.unwrap() < -100.0, "{v:?}");
    }

    /// A dropout on A but B is silent over the mapped span → SharedSilence (nothing to fill).
    #[test]
    fn derive_dropout_with_silent_donor_is_shared_silence() {
        let a = vec![
            blk(9.5, -48.0),
            blk(9.75, -48.0),
            blk(10.0, -100.0),
            blk(10.25, -100.0),
            blk(10.5, -48.0),
        ];
        let b = vec![blk(10.0, -120.0), blk(10.25, -120.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, SharedSilence);
        assert_eq!(v.donor_silence_fraction, Some(1.0));
        assert!(v.drop);
    }

    /// The per-block evidence must **reproduce** the fraction, not merely accompany it: one entry per
    /// donor block, and re-applying the gate's own disjunction to those entries recovers
    /// `donor_silent_blocks` exactly. A reader that cannot re-derive the count cannot trust the slack.
    #[test]
    fn donor_block_evidence_reproduces_the_counted_fraction() {
        let a = vec![
            blk(9.5, -45.0),
            blk(9.75, -45.0),
            blk(10.0, -100.0),
            blk(10.25, -100.0),
            blk(10.5, -45.0),
        ];
        // Four donor blocks spanning both clauses and both sides of the boundary: flagged-but-loud,
        // unflagged-and-well-below, unflagged-and-barely-below, unflagged-and-above.
        // `blk` blocks are 0.25 s wide and membership is by centre (`t + 0.125`), so these four start
        // at 9.875 to put every centre inside the half-open `[10.0, 10.5)` window.
        let b = vec![
            blk_silent(9.875, -20.0, true),
            blk_silent(10.0, -118.0, false),
            blk_silent(10.125, -100.5, false),
            blk_silent(10.25, -30.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());

        let floor = v.gap_floor_db.expect("a silent-block floor was measured");
        assert_eq!(
            v.donor_blocks.len(),
            v.donor_total_blocks.unwrap(),
            "one entry per measured donor block"
        );
        let recount = v
            .donor_blocks
            .iter()
            .filter(|e| e.flagged_silent || e.margin_db.is_some_and(|m| m < 0.0))
            .count();
        assert_eq!(
            recount,
            v.donor_silent_blocks.unwrap(),
            "evidence must recover the count: {:?}",
            v.donor_blocks
        );

        // And the margins are real distances to the floor, not a restatement of the boolean — the
        // barely-below block carries the small negative margin that the count alone flattens away.
        let margins: Vec<f64> = v
            .donor_blocks
            .iter()
            .map(|e| {
                e.margin_db
                    .expect("finite floor ⇒ every block has a margin")
            })
            .collect();
        assert!((margins[0] - (-20.0 - floor)).abs() < 1e-9, "{margins:?}");
        assert!(margins[1] < -15.0, "well below the floor: {margins:?}");
        assert!(
            margins[2] < 0.0 && margins[2] > -1.0,
            "barely below — the slack the fraction hides: {margins:?}"
        );
        assert!(margins[3] > 0.0, "above the floor: {margins:?}");
        // Block 0 counts silent on the flag alone despite sitting far above the floor: the two clauses
        // are independent, which is why they are recorded separately.
        assert!(v.donor_blocks[0].flagged_silent && margins[0] > 0.0);
    }

    /// Digitally silent / abs-floor-quiet B must not read occupied via `rms < gap_floor` alone (F1/R1).
    #[test]
    fn derive_donor_silent_bit_counts_even_when_rms_equals_gap_floor() {
        // A gap floor −120; B at −120 with silent=true (scanner). Strict `rms < thresh` would fail.
        let a = vec![
            blk(9.5, -45.0),
            blk(9.75, -45.0),
            blk(10.0, -120.0),
            blk(10.25, -120.0),
            blk(10.5, -45.0),
        ];
        let b = vec![blk(10.0, -120.0), blk(10.25, -120.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, SharedSilence, "{v:?}");
        assert_eq!(v.donor_silence_fraction, Some(1.0));
    }

    /// Scanner-silent dither below the abs floor (rms above A's gap floor) still counts as donor-silent.
    #[test]
    fn derive_donor_uses_silent_bit_for_abs_floor_quiet() {
        // A gap floor ≈ −101.5; B dither at −98.8 marked silent by the scanner abs floor.
        let a = vec![
            blk(9.5, -45.0),
            blk(9.75, -45.0),
            blk(10.0, -101.5),
            blk(10.25, -101.5),
            blk(10.5, -45.0),
        ];
        let b = vec![
            blk_silent(10.0, -98.8, true),
            blk_silent(10.25, -98.8, true),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, SharedSilence, "{v:?}");
        assert_eq!(v.donor_silence_fraction, Some(1.0));
    }

    /// Hold-bridged loud block inside the core must not inflate A dropout depth (F2).
    #[test]
    fn derive_ignores_non_silent_blocks_inside_gap_core() {
        let a = vec![
            blk(9.5, -45.0),
            blk(9.75, -45.0),
            blk(10.0, -101.0),
            blk_silent(10.25, -52.0, false), // bridged noise
            blk(10.5, -101.0),
            blk(10.75, -45.0),
        ];
        let b = vec![
            blk_silent(10.0, -20.0, false),
            blk_silent(10.25, -20.0, false),
            blk_silent(10.5, -20.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.75, Some(&b), Some((10.0, 10.75)), &on());
        assert_eq!(v.class, RepairableDropout, "{v:?}");
        assert!(
            v.a_gap_rms_db.unwrap() < -90.0,
            "silent-only aggregate must stay deep, got {v:?}"
        );
    }

    /// Room-tone gap (A only a few dB below its floor) with an occupied donor → AmbientQuiet (drop, but
    /// not a dropout) — the self-calibrating A-side at scan-block granularity.
    #[test]
    fn derive_roomtone_with_occupied_donor_is_ambient_quiet() {
        let a = vec![
            blk(9.5, -47.0),
            blk(9.75, -47.0),
            blk(10.0, -52.0),
            blk(10.25, -52.0),
            blk(10.5, -47.0),
        ];
        let b = vec![
            blk_silent(10.0, -20.0, false),
            blk_silent(10.25, -20.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, AmbientQuiet);
        assert!(v.drop);
    }

    /// No B timeline (unscanned/unaligned) ⇒ donor signal absent ⇒ NotEvaluated (conservative keep).
    #[test]
    fn derive_without_b_levels_is_not_evaluated() {
        let a = vec![blk(9.75, -48.0), blk(10.0, -119.0), blk(10.5, -48.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, None, None, &on());
        assert_eq!(v.class, NotEvaluated);
    }

    /// Track B: `bin_ms` from any block width; A total = centres in gap; measurement present.
    #[test]
    fn derive_attaches_measurement_and_a_gap_total_from_levels() {
        let a = vec![
            blk(9.75, -48.0),
            blk(10.0, -119.0),
            blk(10.25, -119.0),
            blk(10.5, -48.0),
        ];
        let b = vec![
            blk_silent(10.0, -20.0, false),
            blk_silent(10.25, -20.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        let m = v.measurement.expect("levels present ⇒ measurement");
        assert_eq!(m.bin_ms, 250);
        assert!((m.context_secs - EQUIVALENCE_CONTEXT_SECS).abs() < f64::EPSILON);
        assert_eq!(m.reduction, ChannelReduction::Interleaved);
        assert_eq!(m.a_span, SpanKind::Core);
        assert_eq!(m.donor_span, Some(SpanKind::Core));
        assert_eq!(v.a_gap_silent_blocks, Some(2));
        assert_eq!(v.a_gap_total_blocks, Some(2));
        assert_eq!(v.donor_silent_blocks, Some(0));
        assert_eq!(v.donor_total_blocks, Some(2));
    }

    /// Track B: empty level stream ⇒ unanswerable, not "zero blocks measured".
    #[test]
    fn derive_with_empty_levels_omits_measurement() {
        let v = derive_gap_equivalence(&[], 10.0, 10.5, None, None, &on());
        assert!(v.measurement.is_none(), "{v:?}");
        assert_eq!(v.a_gap_total_blocks, None, "{v:?}");
        assert_eq!(v.a_gap_silent_blocks, None, "{v:?}");
    }

    /// The gate is off by default: even a clean dropout classifies NotEvaluated (advisory computes with
    /// `enabled: true` explicitly; the plan-drop flag is separate).
    #[test]
    fn derive_respects_disabled_params() {
        let a = vec![blk(9.75, -48.0), blk(10.0, -119.0), blk(10.5, -48.0)];
        let b = vec![blk_silent(10.0, -20.0, false)];
        let v = derive_gap_equivalence(
            &a,
            10.0,
            10.5,
            Some(&b),
            Some((10.0, 10.5)),
            &GapEquivalenceParams::default(),
        );
        assert_eq!(v.class, NotEvaluated);
    }

    // --- Donor registration (§6.4 of the band findings) -----------------------------------------

    /// Registration in [`Apply`](DonorRegistrationMode::Apply) mode — the behaviour under test in
    /// this section. `Apply` is spelled out because it is *not* the default: see
    /// [`observed_registration_records_the_lag_without_moving_the_window`].
    fn with_registration() -> GapEquivalenceParams {
        GapEquivalenceParams {
            donor_registration: Some(DonorRegistrationParams {
                mode: DonorRegistrationMode::Apply,
                ..Default::default()
            }),
            ..on()
        }
    }

    fn observing_registration() -> GapEquivalenceParams {
        GapEquivalenceParams {
            donor_registration: Some(DonorRegistrationParams::default()),
            ..on()
        }
    }

    /// Deterministic pseudo-random content level in −55..−25 dB. Needs to be aperiodic: a smooth
    /// sinusoid would let the lag search lock onto a harmonic and pass for the wrong reason.
    fn content_db(i: i64) -> f64 {
        let h = (i as u64)
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        -55.0 + 30.0 * ((h >> 33) % 1000) as f64 / 1000.0
    }

    /// 12 s of 100 ms blocks. Blocks in `quiet` sit at `quiet_db` and are scanner-silent; the rest
    /// carry `content_db` **shifted by `shift` blocks**, so a stream built with `shift: 5` is the
    /// same program 500 ms late.
    fn timeline(
        n: usize,
        quiet: std::ops::Range<usize>,
        quiet_db: f64,
        shift: i64,
    ) -> Vec<BlockLevel> {
        (0..n)
            .map(|i| {
                let silent = quiet.contains(&i);
                BlockLevel {
                    start_secs: i as f64 * 0.1,
                    end_secs: i as f64 * 0.1 + 0.1,
                    rms_db: if silent {
                        quiet_db
                    } else {
                        content_db(i as i64 - shift)
                    },
                    silent,
                }
            })
            .collect()
    }

    /// The headline case. A has a 600 ms hole at 5.0 s; B is the same program 500 ms late, so B's
    /// matching quiet stretch is at 5.5 s. The nominal window reads B's *content* and calls the gap
    /// repairable — a false keep that the gate-off run turned into an audible injection. Registered,
    /// the window lands on B's silence and the gap correctly drops.
    #[test]
    fn registration_moves_the_donor_window_onto_bs_matching_silence() {
        let a = timeline(120, 50..56, -110.0, 0);
        let b = timeline(120, 55..61, -110.0, 5);

        let nominal = derive_gap_equivalence(&a, 5.0, 5.6, Some(&b), Some((5.0, 5.6)), &on());
        assert_eq!(
            nominal.class, RepairableDropout,
            "nominal map reads B's content"
        );
        assert_eq!(nominal.donor_silence_fraction, Some(1.0 / 6.0));
        assert!(nominal.donor_registration.is_none(), "opt-in");

        let v = derive_gap_equivalence(
            &a,
            5.0,
            5.6,
            Some(&b),
            Some((5.0, 5.6)),
            &with_registration(),
        );
        let reg = v.donor_registration.expect("registration ran");
        assert_eq!(reg.lag_blocks, 5, "{reg:?}");
        assert!((reg.lag_ms - 500.0).abs() < 1e-6, "{reg:?}");
        assert!(reg.peak_r > 0.99, "shifted copy correlates: {reg:?}");
        assert!(reg.nominal_r < 0.5, "nominal map does not: {reg:?}");
        assert_eq!(v.donor_silence_fraction, Some(1.0));
        assert_eq!(v.class, SharedSilence);
        assert!(v.drop);
        // The recorded span is the one measured, not the one requested.
        let (ds, _) = v.donor_span_secs.expect("donor span");
        assert!((ds - 5.5).abs() < 1e-6, "{:?}", v.donor_span_secs);
    }

    /// **The corpus-provenance contract.** `Observe` must produce the *un-registered* verdict — same
    /// class, same fraction, same donor span — while still recording the lag it found. This is what
    /// makes it safe to turn on across a whole corpus run to learn how often registration would move
    /// something: every field a downstream consumer reads is byte-identical to the gate-off-by-lag
    /// behaviour, and the one new field is inert.
    ///
    /// Run against the headline case, so the registration on offer is a big one (500 ms, class-
    /// flipping under [`DonorRegistrationMode::Apply`]) and "unchanged" is a real claim.
    #[test]
    fn observed_registration_records_the_lag_without_moving_the_window() {
        let a = timeline(120, 50..56, -110.0, 0);
        let b = timeline(120, 55..61, -110.0, 5);

        let nominal = derive_gap_equivalence(&a, 5.0, 5.6, Some(&b), Some((5.0, 5.6)), &on());
        let observed = derive_gap_equivalence(
            &a,
            5.0,
            5.6,
            Some(&b),
            Some((5.0, 5.6)),
            &observing_registration(),
        );

        let reg = observed.donor_registration.expect("registration still ran");
        assert_eq!(reg.lag_blocks, 5, "and found the same lag: {reg:?}");

        // Everything else is the nominal verdict, field for field.
        assert_eq!(
            GapEquivalenceVerdict {
                donor_registration: None,
                ..observed
            },
            nominal,
            "Observe must be provenance-only"
        );
        assert_eq!(nominal.class, RepairableDropout, "the un-registered call");
    }

    /// Rebuild a level stream from a recorded [`EnvelopeSlice`] — the operation a dump reader
    /// performs, expressed once so the replay tests below exercise the same path a script would.
    fn replay(slice: &EnvelopeSlice, bin_ms: f64) -> Vec<BlockLevel> {
        let bin = bin_ms / 1000.0;
        slice
            .rms_db
            .iter()
            .enumerate()
            .map(|(i, &db)| BlockLevel {
                start_secs: slice.start_secs + i as f64 * bin,
                end_secs: slice.start_secs + (i + 1) as f64 * bin,
                rms_db: db,
                silent: slice.silent_bins.contains(&(i as u32)),
            })
            .collect()
    }

    /// **The replay contract, half one.** The recorded envelopes must reproduce the registration
    /// they came from — same lag, same both correlations, same interiors — because that is what
    /// makes a *different* `max_lag_blocks` or core policy answerable offline. Nothing else on the
    /// dump can be re-derived if the inputs are a lossy copy of what the gate actually saw.
    #[test]
    fn recorded_envelopes_replay_the_registration() {
        let a = timeline(120, 50..56, -110.0, 0);
        let b = timeline(120, 55..61, -110.0, 5);
        let params = DonorRegistrationParams::default();
        let reg =
            register_donor_window(&a, 5.0, 5.6, &b, (5.0, 5.6), &params).expect("registration ran");
        let env = reg.envelopes.clone().expect("envelopes recorded");

        let replayed = register_donor_window(
            &replay(&env.a, env.bin_ms),
            5.0,
            5.6,
            &replay(&env.b, env.bin_ms),
            (5.0, 5.6),
            &params,
        )
        .expect("replays from the record alone");

        // Exact on every decision-bearing field — the envelopes are the input, not a summary of it.
        assert_eq!(
            DonorRegistration {
                lag_ms: reg.lag_ms,
                envelopes: None,
                ..replayed
            },
            DonorRegistration {
                envelopes: None,
                ..reg.clone()
            },
            "the record is the input, not a summary of it"
        );
        // `lag_ms` alone is derived from a bin width re-formed by subtraction, so it lands within a
        // float epsilon rather than on it. `lag_blocks` — the field anything downstream acts on — is
        // exact, and is covered by the assert above.
        assert!((replayed.lag_ms - reg.lag_ms).abs() < 1e-6, "{replayed:?}");
        // The core is marked, not omitted: bins 50..56 of A sit at ctx start 3.0 s ⇒ offsets 20..26.
        assert_eq!(env.core_bins, (20, 26), "{env:?}");
        assert_eq!(
            env.a.rms_db.len(),
            reg.bins + (env.core_bins.1 - env.core_bins.0),
            "`bins` counts the shoulders; the slice carries the core too"
        );
    }

    /// **The replay contract, half two — the question the 2026-08-03 re-dump was spent on.** With B's
    /// envelope recorded, "what would the donor fraction have been at the *other* lag" is a script
    /// over an existing dump. Under `Observe` the verdict carries the nominal reading; re-counting
    /// the recorded bins at `lag_blocks` must produce the `Apply` answer exactly, so a corpus can be
    /// asked how often `Apply` would flip a class without re-running the scan.
    #[test]
    fn recorded_envelopes_replay_the_donor_fraction_at_either_lag() {
        let a = timeline(120, 50..56, -110.0, 0);
        let b = timeline(120, 55..61, -110.0, 5);
        let observed = derive_gap_equivalence(
            &a,
            5.0,
            5.6,
            Some(&b),
            Some((5.0, 5.6)),
            &observing_registration(),
        );
        let reg = observed
            .donor_registration
            .as_ref()
            .expect("registration ran");
        let env = reg.envelopes.as_ref().expect("envelopes recorded");
        let bins = replay(&env.b, env.bin_ms);
        let floor = observed.gap_floor_db.expect("a finite gap floor");

        // The gate's own predicate, re-expressed against the record: `silent || rms_db < gap_floor`.
        let fraction_at = |shift_secs: f64| {
            let (lo, hi) = (5.0 + shift_secs, 5.6 + shift_secs);
            let win: Vec<_> = bins
                .iter()
                .filter(|bl| {
                    let c = block_center(bl);
                    c >= lo && c < hi
                })
                .collect();
            let silent = win
                .iter()
                .filter(|bl| bl.silent || bl.rms_db < floor)
                .count();
            silent as f64 / win.len() as f64
        };

        assert_eq!(
            Some(fraction_at(0.0)),
            observed.donor_silence_fraction,
            "the nominal reading the verdict was decided on"
        );
        let applied = derive_gap_equivalence(
            &a,
            5.0,
            5.6,
            Some(&b),
            Some((5.0, 5.6)),
            &with_registration(),
        );
        assert_eq!(
            Some(fraction_at(reg.lag_ms / 1000.0)),
            applied.donor_silence_fraction,
            "and the reading `Apply` would have taken, from the same record"
        );
        assert_ne!(
            observed.class, applied.class,
            "the fixture is class-flipping, so this is a claim with teeth"
        );
    }

    /// **Negative control for `Observe`.** A donor that cannot be registered is recorded as such and
    /// nothing else happens — no abstain. The abstain is a verdict change, so it belongs to `Apply`
    /// alone; a corpus run that started keeping gaps because of a mode change would answer a
    /// different question than the one it was turned on to ask.
    #[test]
    fn observing_an_unregistrable_donor_does_not_abstain() {
        let a = timeline(120, 50..56, -110.0, 0);
        let mut b = timeline(120, 50..56, -110.0, 0);
        for (i, blk) in b.iter_mut().enumerate() {
            if !blk.silent {
                blk.rms_db = content_db(i as i64 + 5_000);
            }
        }

        let v = derive_gap_equivalence(
            &a,
            5.0,
            5.6,
            Some(&b),
            Some((5.0, 5.6)),
            &observing_registration(),
        );
        let reg = v
            .donor_registration
            .as_ref()
            .expect("registration was attempted");
        assert!(reg.peak_r < 0.70, "same unregistrable donor: {reg:?}");
        assert_eq!(v.class, SharedSilence, "classified at the nominal map");
        assert!(
            v.not_evaluated_reason.is_none(),
            "Observe never abstains: {v:?}"
        );
        assert!(v.thresholds.is_some(), "a decision was made");
    }

    /// **Negative control.** A real dropout with a live donor must survive registration: B carries
    /// content across the hole at the correct lag, so the fraction stays 0 and the gap is still
    /// repairable. Registration must only move the window — never talk the gate out of a fill.
    #[test]
    fn registration_does_not_talk_the_gate_out_of_a_real_dropout() {
        let a = timeline(120, 50..56, -110.0, 0);
        let b = timeline(120, 0..0, -110.0, 5); // same program, 500 ms late, no hole of its own

        let v = derive_gap_equivalence(
            &a,
            5.0,
            5.6,
            Some(&b),
            Some((5.0, 5.6)),
            &with_registration(),
        );
        let reg = v.donor_registration.expect("registration ran");
        assert_eq!(reg.lag_blocks, 5, "shoulders still register: {reg:?}");
        assert!(
            reg.peak_r > 0.99,
            "excluding the core keeps a dropout registrable: {reg:?}"
        );
        assert_eq!(v.donor_silence_fraction, Some(0.0));
        assert_eq!(v.class, RepairableDropout);
        assert!(!v.drop);
        // B is tens of dB above A inside the hole — the signal that says "there is something to fill".
        assert!(reg.interior_delta_db.unwrap() > 40.0, "{reg:?}");
    }

    /// The 33/17 shape: A hits digital zero while B keeps a quiet bed ~35 dB above it. Both are far
    /// below A's noise floor, so the fraction alone cannot see the difference — `interior_delta_db`
    /// can. Recorded, not yet classified on.
    #[test]
    fn interior_delta_separates_a_live_bed_from_matching_silence() {
        let a = timeline(120, 50..56, -101.0, 0);
        let live = timeline(120, 50..56, -66.0, 0);
        let matching = timeline(120, 50..56, -101.0, 0);

        let d = |b: &[BlockLevel]| {
            derive_gap_equivalence(
                &a,
                5.0,
                5.6,
                Some(b),
                Some((5.0, 5.6)),
                &with_registration(),
            )
            .donor_registration
            .expect("registration ran")
            .interior_delta_db
            .expect("both interiors present")
        };
        assert!((d(&live) - 35.0).abs() < 1.0, "live bed: {}", d(&live));
        assert!(d(&matching).abs() < 0.01, "matching: {}", d(&matching));
    }

    /// **Negative control.** Unrelated timelines cannot be registered, so the gate abstains rather
    /// than classifying against a window it cannot place — and abstaining **keeps** the gap.
    #[test]
    fn unregistrable_donor_abstains_and_fails_open() {
        let a = timeline(120, 50..56, -110.0, 0);
        // B's content is a different program (offset the pseudo-random stream far past the search).
        let mut b = timeline(120, 50..56, -110.0, 0);
        for (i, blk) in b.iter_mut().enumerate() {
            if !blk.silent {
                blk.rms_db = content_db(i as i64 + 5_000);
            }
        }

        let v = derive_gap_equivalence(
            &a,
            5.0,
            5.6,
            Some(&b),
            Some((5.0, 5.6)),
            &with_registration(),
        );
        let reg = v.donor_registration.expect("registration was attempted");
        assert!(reg.peak_r < 0.70, "unrelated programs: {reg:?}");
        assert_eq!(v.class, NotEvaluated);
        assert_eq!(
            v.not_evaluated_reason,
            Some(NotEvaluatedReason::DonorRegistrationUnreliable),
            "a keep here must say why it is a keep"
        );
        assert!(!v.drop, "abstain fails open");
        assert!(
            v.thresholds.is_none(),
            "a refusal must not carry the marks of a decision"
        );
    }

    /// A flat envelope has no features to align. That is "cannot ask", not "does not match" — no
    /// registration is recorded and the nominal map stands, rather than an abstain that would keep
    /// every gap on quiet material.
    #[test]
    fn flat_envelopes_are_not_registrable_and_do_not_abstain() {
        let flat: Vec<BlockLevel> = (0..120)
            .map(|i| BlockLevel {
                start_secs: i as f64 * 0.1,
                end_secs: i as f64 * 0.1 + 0.1,
                rms_db: if (50..56).contains(&i) { -110.0 } else { -40.0 },
                silent: (50..56).contains(&i),
            })
            .collect();
        let mut b = flat.clone();
        for blk in &mut b {
            blk.rms_db = -40.0;
            blk.silent = false;
        }

        let v = derive_gap_equivalence(
            &flat,
            5.0,
            5.6,
            Some(&b),
            Some((5.0, 5.6)),
            &with_registration(),
        );
        assert!(v.donor_registration.is_none(), "nothing to align on");
        assert_eq!(v.class, RepairableDropout, "nominal map still applies");
        assert!(v.not_evaluated_reason.is_none());
    }

    /// Registration is opt-in: with the default params every input above classifies exactly as it
    /// did before the feature existed, and no registration field is emitted.
    #[test]
    fn registration_is_off_by_default() {
        let a = timeline(120, 50..56, -110.0, 0);
        let b = timeline(120, 55..61, -110.0, 5);
        for params in [on(), GapEquivalenceParams::default()] {
            assert!(params.donor_registration.is_none());
            let v = derive_gap_equivalence(&a, 5.0, 5.6, Some(&b), Some((5.0, 5.6)), &params);
            assert!(v.donor_registration.is_none(), "{v:?}");
            assert_eq!(v.donor_span_secs, Some((5.0, 5.6)), "nominal span unmoved");
        }
    }

    /// `not_evaluated_reason` is present exactly when the class is `NotEvaluated` — the same
    /// iff-property `thresholds` carries for the decided classes.
    #[test]
    fn not_evaluated_reason_is_present_iff_not_evaluated() {
        let a = vec![
            blk(9.5, -50.0),
            blk(9.75, -50.0),
            blk(10.0, -119.0),
            blk(10.5, -50.0),
        ];
        let b = vec![blk_silent(10.0, -20.0, false)];

        let decided = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(decided.class, RepairableDropout);
        assert!(decided.not_evaluated_reason.is_none());

        let off = derive_gap_equivalence(
            &a,
            10.0,
            10.5,
            Some(&b),
            Some((10.0, 10.5)),
            &GapEquivalenceParams::default(),
        );
        assert_eq!(
            off.not_evaluated_reason,
            Some(NotEvaluatedReason::GateDisabled)
        );

        let no_donor = derive_gap_equivalence(&a, 10.0, 10.5, None, None, &on());
        assert_eq!(
            no_donor.not_evaluated_reason,
            Some(NotEvaluatedReason::MissingSignal)
        );
    }

    // --- Scanner → levels → occupancy/donor (production recipe; not hand-built BlockLevels) -----

    fn mono_pcm(rate: u32, samples: Vec<f32>) -> crate::domain::pcm::InterleavedPcm {
        crate::domain::pcm::InterleavedPcm {
            sample_rate: rate,
            channels: 1,
            samples,
        }
    }

    fn sine_samples(rate: u32, secs: f64) -> Vec<f32> {
        let count = (rate as f64 * secs).round() as usize;
        (0..count)
            .map(|i| f32::sin(i as f32 * 0.3) * 0.244)
            .collect()
    }

    fn scan_levels(
        samples: Vec<f32>,
        abs_floor: f32,
    ) -> (Vec<crate::domain::policies::SilentRun>, Vec<BlockLevel>) {
        let rate = 11_025u32;
        let mut scanner =
            crate::domain::policies::SilenceRunScanner::new(0.25, 0.01, 1.0, 0, abs_floor)
                .retain_block_levels();
        scanner.feed(&mono_pcm(rate, samples), 0.0);
        scanner.finish_with_levels()
    }

    #[test]
    fn scanner_pipeline_digital_silence_occupancy_and_donor_agree() {
        use crate::domain::cross_check::b_has_energy_from_levels;

        let abs = 33.0 / 32767.0;
        let rate = 11_025u32;
        // A: loud shoulders + 2 s digital silence (noise-floor context present).
        let mut a = sine_samples(rate, 2.0);
        a.extend(std::iter::repeat_n(0.0f32, rate as usize * 2));
        a.extend(sine_samples(rate, 2.0));
        let b = vec![0.0f32; rate as usize * 6];

        let (runs, a_levels) = scan_levels(a, abs);
        let (_, b_levels) = scan_levels(b, abs);
        assert_eq!(runs.len(), 1, "expected one silent run on A");
        let core_s = runs[0].core_start_secs;
        let core_e = runs[0].core_end_secs;

        assert!(
            !b_has_energy_from_levels(&b_levels, core_s, core_e),
            "digitally silent B must be unoccupied"
        );
        let v = derive_gap_equivalence(
            &a_levels,
            core_s,
            core_e,
            Some(&b_levels),
            Some((core_s, core_e)),
            &on(),
        );
        assert_eq!(v.donor_silence_fraction, Some(1.0), "{v:?}");
        assert_eq!(v.class, SharedSilence, "{v:?}");
        assert!(occupancy_agrees_with_donor_silence(
            false,
            v.donor_silence_fraction,
            0.5
        ));
    }

    #[test]
    fn scanner_pipeline_abs_floor_dither_is_donor_silent_not_occupied() {
        use crate::domain::cross_check::b_has_energy_from_levels;

        let abs = 33.0 / 32767.0;
        let rate = 11_025u32;
        let mut a = sine_samples(rate, 2.0);
        a.extend(std::iter::repeat_n(0.0f32, rate as usize * 2));
        a.extend(sine_samples(rate, 2.0));
        // B: ±1/32767 dither — quieter than abs floor peak check, louder than digital −120 gap floor.
        let dither = 1.0f32 / 32767.0;
        let b: Vec<f32> = (0..rate as usize * 6)
            .map(|i| if i % 2 == 0 { dither } else { -dither })
            .collect();

        let (runs, a_levels) = scan_levels(a, abs);
        let (_, b_levels) = scan_levels(b, abs);
        let core_s = runs[0].core_start_secs;
        let core_e = runs[0].core_end_secs;

        assert!(
            b_levels.iter().any(|l| {
                let c = (l.start_secs + l.end_secs) / 2.0;
                c >= core_s && c < core_e && l.silent
            }),
            "scanner must mark dither blocks silent under abs floor"
        );
        assert!(!b_has_energy_from_levels(&b_levels, core_s, core_e));
        let v = derive_gap_equivalence(
            &a_levels,
            core_s,
            core_e,
            Some(&b_levels),
            Some((core_s, core_e)),
            &on(),
        );
        assert_eq!(v.class, SharedSilence, "{v:?}");
        assert_eq!(v.donor_silence_fraction, Some(1.0), "{v:?}");
        assert!(occupancy_agrees_with_donor_silence(
            false,
            v.donor_silence_fraction,
            0.5
        ));
    }
}
