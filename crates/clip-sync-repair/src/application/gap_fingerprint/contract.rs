//! Co-located measurement contracts — the `_contract` object a metric group carries on the wire.
//!
//! **Why this exists.** Field *names* answer "which field?"; they cannot answer "measured where, over
//! what window, and what is this *not*?". Readers (agents especially) treat names as the schema and
//! invent the rest, and this corpus's real failure mode is wrong placement / wrong window, not wrong
//! field. A corpus-root notes map is too easy to skip and too far from the number it explains, so the
//! contract travels **inside the same JSON object as the values**. Current shape and the contracted
//! groups: `docs/dev/gap-fingerprint.md` § `_contract`. Design rationale (why the
//! `Contracted<T>` wrapper was mandatory rather than stylistic, and why some quoted numbers are
//! config rather than consts): `docs/dev/archive/TEMP-fingerprint-field-clarity-plan.md` § 2.
//!
//! **Not authoritative.** Nothing gates, classifies, or diffs on `_contract`. It is write-time reader
//! metadata: absent on old corpora, absent from golden Tier-1/2 axes, and always `Option`. A consumer
//! that needs the full story still reads `docs/dev/gap-fingerprint.md`; this is a reminder, not a
//! replacement.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Longest a single contract string may be. Contracts are read *beside* the numbers they describe, so
/// they must stay scannable; past ~120 chars a reader skips the line and we are back to guessing.
/// Target is ≤100 — the cap is the hard stop, enforced by [`MetricContract::within_length_cap`].
pub const CONTRACT_MAX_LEN: usize = 120;

/// What a metric group measures, where, over what window, and which sibling it is **not**.
///
/// Four fixed keys rather than one prose field, deliberately: a single free-form string invites a
/// reader (or a summarizing model) to compress placement away, and placement is the axis that
/// actually gets misread. Every key is `Option` so a group can omit one that genuinely does not
/// apply, rather than filling it with "n/a".
///
/// `Cow<'static, str>` so the shipped table can be `const` (borrowed) while a deserialized corpus
/// still round-trips (owned).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MetricContract {
    /// What the numbers are.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub measures: Option<Cow<'static, str>>,
    /// Where on the timeline / which seam geometry.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub placement: Option<Cow<'static, str>>,
    /// Border, search radius, binning — whatever is load-bearing for reading the value.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub window: Option<Cow<'static, str>>,
    /// The confusable sibling(s) this is **not** — named **first**, before any property a reader
    /// might otherwise assume. A `not` that opens with a property ("authoritative: …") reads
    /// correctly only if you mentally prefix the key, which a summarizing model will not do.
    #[serde(rename = "not", skip_serializing_if = "Option::is_none", default)]
    pub not: Option<Cow<'static, str>>,
}

impl MetricContract {
    /// Every present string is within [`CONTRACT_MAX_LEN`].
    pub fn within_length_cap(&self) -> bool {
        [
            self.measures.as_deref(),
            self.placement.as_deref(),
            self.window.as_deref(),
            self.not.as_deref(),
        ]
        .into_iter()
        .flatten()
        .all(|s| s.chars().count() <= CONTRACT_MAX_LEN)
    }
}

/// A metric group `T` plus its co-located `_contract`.
///
/// `#[serde(flatten)]` keeps the **wire shape unchanged**: `T`'s keys stay at the same level they were
/// at before the wrapper existed, with `_contract` beside them. So adding a contract to a group is not
/// a schema break for anything reading the values.
///
/// Generic on purpose. Four of the groups slated for contracts are *pairs* sharing one type
/// (`equivalence_diagnostic`/`equivalence_production` over `GapEquivalenceVerdict`,
/// `lag_editorial`/`lag_decision` over `LagFingerprint`, `donor_interior_aligned`/`donor_interior_nominal` over
/// `DonorInterior`) — and two of those types are **domain** types also serialized on the production
/// `--json` surface. A per-field wrapper is what lets the two halves of a pair carry *different*
/// contract text without pushing reader metadata down into the domain.
///
/// Leading underscore on the key is the convention for "reader metadata": minimal parsers that ignore
/// unknown fields are unaffected. Anything using `deny_unknown_fields` on a wrapped type must allow
/// `_contract` — and note that `flatten` is incompatible with `deny_unknown_fields` regardless.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contracted<T> {
    /// Absent on pre-2026-08-07 corpora, and absent whenever a writer chooses not to stamp one.
    /// Never required to classify, gate, or diff.
    #[serde(rename = "_contract", skip_serializing_if = "Option::is_none", default)]
    pub contract: Option<MetricContract>,
    #[serde(flatten)]
    pub value: T,
}

impl<T> Contracted<T> {
    /// Wrap a value with its contract.
    pub fn new(value: T, contract: MetricContract) -> Self {
        Self {
            contract: Some(contract),
            value,
        }
    }
}

/// Stamp a contract onto an optional metric group, leaving `None` as `None`.
///
/// Every C2 group is an `Option` that is absent on the tiers that do not measure it, and `None` must
/// stay `None`: a `_contract` with no values beside it would assert that something was measured. This
/// is the whole write-side API — sites say `stamp(value, THE_CONTRACT)` and never build the wrapper
/// by hand.
pub fn stamp<T>(value: Option<T>, contract: MetricContract) -> Option<Contracted<T>> {
    value.map(|v| Contracted::new(v, contract))
}

impl<T> std::ops::Deref for Contracted<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

// ---------------------------------------------------------------------------------------------
// The contract table — single source of truth for the strings that ship on dumps
// ---------------------------------------------------------------------------------------------

/// Declare a shipped contract **and** enrol it in [`SHIPPED_CONTRACTS`] in one step.
///
/// Registration is structural on purpose: the length cap and the distinctness checks are only worth
/// anything if they see every contract, and a hand-maintained list is exactly the kind of second
/// place a new entry gets forgotten. Declaring a contract outside this macro compiles, but escapes
/// those checks — so don't. All four axes are required; a group that genuinely lacks one wants a
/// macro arm, not a bare `const`.
macro_rules! contracts {
    ($(
        $(#[$meta:meta])*
        $konst:ident for $group:literal {
            measures: $measures:literal,
            placement: $placement:literal,
            window: $window:literal,
            not: $not:literal,
        }
    )*) => {
        $(
            $(#[$meta])*
            pub const $konst: MetricContract = MetricContract {
                measures: Some(Cow::Borrowed($measures)),
                placement: Some(Cow::Borrowed($placement)),
                window: Some(Cow::Borrowed($window)),
                not: Some(Cow::Borrowed($not)),
            };
        )*

        /// Every contract this build ships, paired with the wire key that carries it.
        ///
        /// Tests sweep this; nothing on the write path consults it (each site names its own const,
        /// so a typo is a compile error rather than a lookup miss).
        pub const SHIPPED_CONTRACTS: &[(&str, MetricContract)] = &[$(($group, $konst),)*];
    };
}

contracts! {
    /// Contract for `equivalence_production` — the authoritative silence-character verdict.
    EQUIVALENCE_PRODUCTION_CONTRACT for "equivalence_production" {
        measures: "silence-character class the production gate acted on: dropout vs mutual/program silence",
        placement: "scan-time gap: A's silent core; donor window = that core on B via the NOMINAL map, which can be off",
        window: "scan blocks of `scan_recipe.scan_block_ms`; noise floor = median over +/-2.0s context",
        not: "equivalence_diagnostic (second opinion, gates nothing); not a seam or lag measurement",
    }

    /// Contract for `equivalence_diagnostic` — the second opinion, which nothing reads.
    EQUIVALENCE_DIAGNOSTIC_CONTRACT for "equivalence_diagnostic" {
        measures: "same classifier as equivalence_production, re-run from the fingerprint's own sensors",
        placement: "the scan verdict's own a_span_secs / donor_span_secs - adopted, not re-derived",
        window: "scan blocks of `scan_recipe.scan_block_ms`; bin lattice phased differently than scan's",
        not: "equivalence_production, the verdict skip_equivalent_gaps acts on; not a 'fine' reading of it",
    }

    /// Contract for `lag_decision` — the registration the patch/skip decision is made on.
    LAG_DECISION_CONTRACT for "lag_decision" {
        measures: "per-shoulder waveform lag the gate decides patch/skip on: peak_r, frac_lag_ms, peak_z, prominence",
        placement: "fine sweep at b_mapped; dead (r<0.5) retries at envelope peak (>=0.70) anchored on its own capture; kept if recoverable",
        window: "per shoulder window/max_lag (defaults 1000 / 600); mono; gross to b_mapped_end; D no longer corrects frac_lag_ms",
        not: "lag_editorial (diagnostic placement); prominence is not uniqueness - no exclusion radius, see top2_spacing_ms",
    }

    /// Contract for `lag_editorial` — the Tier-3 diagnostic placement nothing decides on.
    LAG_EDITORIAL_CONTRACT for "lag_editorial" {
        measures: "same lag sweep as lag_decision but at a diagnostic placement; nothing gates, plans, or skips on it",
        placement: "best-energy editorial bracket / structure throat, which can sit far from the seam the gate scores",
        window: "recorded per shoulder as window_ms / max_lag_ms; Tier-3, only with --fingerprint-diagnostics",
        not: "lag_decision (the registration production acts on); this is not a refinement of it",
    }

    /// Contract for `donor_interior_aligned` — B occupancy over the lag-adjusted bridge.
    DONOR_INTERIOR_ALIGNED_CONTRACT for "donor_interior_aligned" {
        measures: "B occupancy over the span a fill would take: rms_db, silence_fraction, longest_silence_ms, continuous",
        placement: "aligned bridge [b_mapped_start + L_pre, b_mapped_end + L_post) - each shoulder at its own lag",
        window: "the bridge span itself; continuous = no sub-floor run longer than 150ms",
        not: "donor_interior_nominal (registration-independent); this span carries the aliased-lag confound",
    }

    /// Contract for `donor_interior_nominal` — the registration-independent program-quiet read.
    DONOR_INTERIOR_NOMINAL_CONTRACT for "donor_interior_nominal" {
        measures: "B occupancy at nominal program time: rms_db, silence_fraction, longest_silence_ms, continuous",
        placement: "nominal b_mapped span, NO lag adjustment - registration-independent, so it inherits the map's error",
        window: "the nominal gap span; continuous = no sub-floor run longer than 150ms; map error to ~400ms observed",
        not: "donor_interior_aligned (the lag-adjusted bridge); silence_fraction ~1 is not by itself program-quiet",
    }

    /// Contract for `residual` — the same-source cancellation confirm at the throat.
    RESIDUAL_CONTRACT for "residual" {
        measures: "least-squares same-source cancellation depth, dB below the local floor - the strong confirm",
        placement: "the gate's structure throat (`gate_structure_align`), per shoulder - NOT b_mapped nominal",
        window: "seam window = fill_seam_search_secs (default 250ms); a side is absent when it has no usable floor",
        not: "seam_probe (Tier-3, gates nothing); deeper dB is better here - it is not a correlation",
    }

    /// Contract for `seam_probe` — Tier-3 seam diagnosis, read by no gate.
    SEAM_PROBE_CONTRACT for "seam_probe" {
        measures: "encoding-robust seam metrics (R2, R4, envelope, recovered lag, snr) - why a dead seam is dead",
        placement: "b_mapped, not the throat - so these do not compare against residual's numbers",
        window: "10ms envelope bins; recovery +/-25ms pre and +/-max_lag_ms post, sequentially centered",
        not: "residual (the gate's same-source confirm at the throat); no gate reads seam_probe",
    }

    /// Contract for `splice` — the single-lag registration step the length-reconciliation acts on.
    SPLICE_CONTRACT for "splice" {
        measures: "registration step (post_lag - pre_lag) + per-side peak r and robust uniqueness peak_z",
        placement: "derived from lag_decision's gross per-shoulder lags at b_mapped - not re-measured here",
        window: "inherits lag_decision's window/max_lag; edge_pinned=true makes step_ms GIGO (A5/C6)",
        not: "splice_dualfit (per-shoulder seam-local locks); this step is single-lag, one number per gap",
    }

    /// Contract for `splice_dualfit` — dual-fit viability, whose `*_seam_lag_ms` are not placements.
    SPLICE_DUALFIT_CONTRACT for "splice_dualfit" {
        measures: "dual-fit viability: per-shoulder seam r/z/prominence + gate_pass for a length-reconciled fill",
        placement: "each shoulder at its OWN seam-local lock re-anchored on nominal b_mapped, not on lag_decision",
        window: "fill_seam_search_secs (250ms default); trim_frames = bridge - gap = the step, in frames",
        not: "splice (single-lag step); *_seam_lag_ms is residual AT the lock, ~0, never a placement",
    }

    /// Contract for `outcome` — the bracket gate in isolation, upstream of the equivalence gate.
    GATE_OUTCOME_CONTRACT for "outcome" {
        measures: "bracket-gate disposition: plan_kind, tier (any_ok), closest failing stage, dual-fit rescue",
        placement: "per gap, over the fingerprint's own bracket enumeration - the production patch gate never runs",
        window: "dual_fit_rescue is predictive (assumes --dual-fit), not an observed production result",
        not: "equivalence_production, the real disposition; tier is upstream of skip_equivalent_gaps",
    }

    /// Contract for `dual_fit_counterfactual` — the admission decision on **every** gap, legs included.
    DUAL_FIT_COUNTERFACTUAL_CONTRACT for "dual_fit_counterfactual" {
        measures: "would dual-fit have admitted this gap: the verdict plus every leg it was made from, patched gaps too",
        placement: "the six conditions skip_or_dual_fit evaluates, from this gap's brackets / splice_dualfit / donor reads",
        window: "reached_in_production=false on a patched gap - the keep path took it, so would_rescue is counterfactual",
        not: "outcome.dual_fit_rescue, which is None once a gap is patched; no placement or lag is claimed here",
    }

    /// Contract for `source.alignment` — pair-level chromaprint scan map, not a gap shoulder.
    /// Contract for `geometry` — the gap's edges, and the join key every census must use.
    GEOMETRY_CONTRACT for "geometry" {
        measures: "gap edges: a_start/a_end as scanned, a_refined_* as spliced, duration_secs = the REFINED span",
        placement: "a_* on A's timeline; b_mapped_* = the refined edges + fill_offset_secs, nominal map, no lag",
        window: "a_start_secs rounded to 1ms is this gap's join key; the gap index is 0-based and joins nothing",
        not: "alignment, the pair-level map fill_offset_secs came from; b_mapped_* carry no per-shoulder lag",
    }

    /// Contract for `levels` — A's envelope, and the two floors that are not the same reduction.
    LEVELS_CONTRACT for "levels" {
        measures: "A's RMS envelope: noise_floor_db = MEDIAN of context bins, gap_floor_db = MAX of the gap's bins",
        placement: "A's own timeline: the refined gap span plus gap_signature_context_secs either side",
        window: "bins of gap_signature_bin_ms; gap bins excluded from noise_floor_db; floor_db substitutes silence",
        not: "b_levels (the same profile on B); the two floors are different reductions, not a range",
    }

    /// Contract for `b_levels` — the same profile on B, at the nominal map, Tier-3 only.
    B_LEVELS_CONTRACT for "b_levels" {
        measures: "the same RMS envelope as levels, measured on B - occupancy context only, no gate reads it",
        placement: "NOMINAL b_mapped span with no lag adjustment - donor_interior_nominal's frame, not the bridge",
        window: "Tier-3: present only with --fingerprint-diagnostics; same bin_ms and context span as levels",
        not: "levels (A's own profile); a quiet b_levels is not a dropout - see donor_interior_nominal",
    }

    /// Contract for `structure` — the coarse tier, and the sibling it shares field names with.
    STRUCTURE_CONTRACT for "structure" {
        measures: "structure-tier baseline correlation per shoulder - the coarse match that precedes seam fit",
        placement: "the baseline placement at nominal b_mapped (place_on_b), not the fill's final seam",
        window: "ABSENT on every from-decode dump (skip_baseline_placement); projected rows set pre==post==the min",
        not: "seams, whose baseline_pre/baseline_post are the SAME field names one tier finer",
    }

    /// Contract for `seams` — the waveform tier, same field names as `structure`.
    SEAMS_CONTRACT for "seams" {
        measures: "waveform-tier baseline correlation: baseline_pre/post plus per_channel and mono_pre/mono_post",
        placement: "the same baseline placement at nominal b_mapped as structure - not the throat residual uses",
        window: "ABSENT on from-decode dumps; projected rows fabricate pre==post==mono==the min, per_channel empty",
        not: "structure, one tier coarser with the SAME baseline_pre/baseline_post field names",
    }

    ALIGNMENT_CONTRACT for "alignment" {
        measures: "pair-level chromaprint scan alignment that mapped A onto B",
        placement: "scan-time, whole pair - not a gap shoulder",
        window: "the clip windows in `clips[]`",
        not: "lag_decision (per-gap shoulder lag at b_mapped); this is the map that produced fill_offset_secs",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contracts are read inline, beside the numbers. A long one gets skipped, which defeats the point.
    ///
    /// Sweeps [`SHIPPED_CONTRACTS`] rather than a hand-written list: the `contracts!` macro enrols
    /// every declaration, so a contract added for C2 cannot ship uncapped by being left off a roster.
    #[test]
    fn shipped_contracts_are_within_the_length_cap() {
        assert!(!SHIPPED_CONTRACTS.is_empty(), "registry lost its entries");
        for (group, c) in SHIPPED_CONTRACTS {
            assert!(
                c.within_length_cap(),
                "{group} contract exceeds {CONTRACT_MAX_LEN} chars: {c:?}"
            );
        }
    }

    /// Two groups sharing contract text means the reader gets no help telling them apart — the exact
    /// failure the wrapper exists to prevent. Registry-wide, so it holds for C2's pairs too.
    #[test]
    fn no_two_shipped_contracts_are_identical() {
        for (i, (group, c)) in SHIPPED_CONTRACTS.iter().enumerate() {
            for (other, o) in &SHIPPED_CONTRACTS[i + 1..] {
                assert_ne!(c, o, "{group} and {other} ship identical contract text");
                assert_ne!(group, other, "{group} is registered twice");
            }
        }
    }

    /// A `not` that opens with a property instead of the sibling's name reads as a fragment once the
    /// key is stripped — see [`MetricContract::not`]. Every shipped `not` must name a real wire key,
    /// and the equivalence pair must point at each other.
    #[test]
    fn every_not_names_a_sibling_first() {
        let keys: Vec<&str> = SHIPPED_CONTRACTS.iter().map(|(g, _)| *g).collect();
        for (group, c) in SHIPPED_CONTRACTS {
            let not = c
                .not
                .as_deref()
                .unwrap_or_else(|| panic!("{group}: no not"));
            let head = not.split([' ', ',', ';', '(', '.']).next().unwrap_or("");
            assert!(
                keys.contains(&head),
                "{group}: `not` must open with a sibling wire key, got {head:?} in {not:?}"
            );
            assert_ne!(head, *group, "{group}: `not` points at itself");
        }
    }

    /// The contract strings quote two things they cannot interpolate: the `±2.0 s` context window
    /// (a `const`, and these tables are `const` so `format!` is unavailable) and the wire path
    /// `scan_recipe.scan_block_ms`. Both drift silently otherwise — a contract that lies about the
    /// window is worse than no contract, since the whole point is that readers trust it.
    #[test]
    fn quoted_constants_still_match_the_code() {
        assert_eq!(
            crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS,
            2.0,
            "EQUIVALENCE_CONTEXT_SECS moved; update the `+/-2.0s` literal in the contract table"
        );
        assert!(
            EQUIVALENCE_PRODUCTION_CONTRACT
                .window
                .as_deref()
                .is_some_and(|w| w.contains("+/-2.0s")),
            "the production contract stopped quoting the context window"
        );

        // `scan_recipe.scan_block_ms` is a *wire path*, so R2/R3-style renames would strip it out
        // from under the prose. Serialize the recipe and look for the key itself.
        let recipe = serde_json::to_value(crate::application::gap_fingerprint::CorpusScanRecipe {
            scan_block_ms: Some(250),
            ..Default::default()
        })
        .expect("serialize recipe");
        assert!(
            recipe.get("scan_block_ms").is_some(),
            "`scan_block_ms` was renamed; both contract `window` strings quote it"
        );
        for (group, c) in SHIPPED_CONTRACTS {
            if let Some(w) = c.window.as_deref() {
                assert!(
                    !w.contains("scan_recipe.") || w.contains("scan_recipe.scan_block_ms"),
                    "{group}: quotes a `scan_recipe.` path this test does not pin: {w:?}"
                );
            }
        }
    }

    /// C2's contracts quote four more numbers as prose. Same rule as
    /// [`quoted_constants_still_match_the_code`]: if the code moves and the string does not, the
    /// contract lies to exactly the readers who trust it most.
    ///
    /// `fill_seam_search_secs` is a config default (tunable). Fine-sweep `lag_window_secs` /
    /// `lag_max_lag_ms` are the production A4 pin (`LAG_DECISION_*`); dump defaults must follow
    /// them. The contracts still say *"defaults"* and name the recorded wire fields
    /// (`window_ms` / `max_lag_ms`) that carry the value actually used. Do not "simplify"
    /// that wording away.
    #[test]
    fn quoted_c2_constants_still_match_the_code() {
        use super::super::measure::probes::{SEAM_PROBE_ENV_BIN_MS, SEAM_PROBE_FINE_LAG_MS};
        use crate::domain::donor::DONOR_CONTINUITY_MS;

        assert_eq!(
            DONOR_CONTINUITY_MS, 150.0,
            "DONOR_CONTINUITY_MS moved; both donor contracts quote `150ms`"
        );
        for key in ["donor_interior_aligned", "donor_interior_nominal"] {
            let c = shipped(key);
            assert!(
                c.window.as_deref().is_some_and(|w| w.contains("150ms")),
                "{key} stopped quoting the continuity threshold"
            );
        }

        assert_eq!(
            SEAM_PROBE_ENV_BIN_MS, 10.0,
            "SEAM_PROBE_ENV_BIN_MS moved; the seam_probe contract quotes `10ms`"
        );
        assert_eq!(
            SEAM_PROBE_FINE_LAG_MS, 25.0,
            "SEAM_PROBE_FINE_LAG_MS moved; the seam_probe contract quotes `+/-25ms`"
        );
        let probe = shipped("seam_probe");
        let probe_window = probe.window.as_deref().expect("seam_probe window");
        assert!(probe_window.contains("10ms") && probe_window.contains("+/-25ms"));

        // Config *defaults*, quoted as defaults. A changed default silently makes the parenthetical
        // wrong even though nothing is renamed.
        let defaults = super::super::measure::FingerprintConfig::default();
        assert_eq!(
            defaults.lag_window_secs,
            crate::domain::LAG_DECISION_WINDOW_SECS,
            "lag_window_secs default drifted from production A4 fine sweep"
        );
        assert_eq!(
            defaults.lag_max_lag_ms,
            crate::domain::LAG_DECISION_MAX_LAG_MS,
            "lag_max_lag_ms default drifted from production A4 fine sweep"
        );
        assert_eq!(
            defaults.max_refine_secs,
            crate::domain::policies::GAP_EDGE_REFINE_SECS,
            "max_refine_secs default drifted from production refine budget"
        );
        assert_eq!(
            defaults.lag_envelope_max_lag_ms,
            super::super::measure::DEFAULT_LAG_ENVELOPE_MAX_LAG_MS,
            "lag_envelope_max_lag_ms default moved"
        );
        assert_eq!(
            crate::infrastructure::config::RepairConfig::default().lag_envelope_max_lag_ms,
            defaults.lag_envelope_max_lag_ms,
            "RepairConfig and FingerprintConfig envelope defaults drifted"
        );
        assert_eq!(
            defaults.border_capture,
            super::super::schema::BorderCapture::Narrow,
            "border_capture default moved; `Wide` re-baselines border_displacement_ms corpus-wide,              so it must never become the default by accident"
        );
        assert_eq!(
            crate::infrastructure::config::RepairConfig::default().border_capture,
            defaults.border_capture,
            "RepairConfig and FingerprintConfig border_capture defaults drifted"
        );
        assert_eq!(
            defaults.lag_envelope_window_secs,
            super::super::measure::DEFAULT_LAG_ENVELOPE_WINDOW_SECS,
            "lag_envelope_window_secs default moved"
        );
        assert_eq!(
            crate::infrastructure::config::RepairConfig::default().lag_envelope_window_secs,
            defaults.lag_envelope_window_secs,
            "RepairConfig and FingerprintConfig envelope-window defaults drifted"
        );
        // The rescue window and the Tier-3 `wide_envelope` diagnostic's own window are separate
        // knobs (BACKLOG § *Fill placement* — *Envelope width knobs move only the rescue path*),
        // and they must AGREE at the default or the shipped arm itself ships two different searches.
        // There is deliberately no assertion for that here: `DEFAULT_LAG_ENVELOPE_WINDOW_SECS` is
        // *defined as* `pcm::WIDE_ENV_WINDOW_SECS` (see `measure/config.rs`), so the agreement is
        // structural and nothing runtime can break it. An earlier version asserted the literal
        // `2.0`, which tested the value rather than the agreement — retuning the diagnostic's
        // constant would have moved both together, still agreeing, and failed the test anyway.
        assert_eq!(
            defaults.fill_seam_search_secs, 0.25,
            "fill_seam_search_secs default moved; the residual contract quotes `default 250ms`"
        );
        assert!(shipped("lag_decision")
            .window
            .as_deref()
            .is_some_and(|w| w.contains("defaults 1000 / 600")));
        assert!(shipped("residual")
            .window
            .as_deref()
            .is_some_and(|w| w.contains("default 250ms")));
    }

    /// Contracts also quote **wire keys** (`window_ms`, `rms_db`, …). Those are renameable, and R2/R3
    /// showed a rename sails straight past prose. Serialize each type and confirm the keys the
    /// contracts name are still the keys it emits.
    #[test]
    fn quoted_wire_keys_are_still_emitted_by_their_types() {
        // Built by hand, with the `Option` axes populated: `peak_z` / `prominence` are
        // `skip_serializing_if`, so a zero-value stand-in would omit exactly the keys under test and
        // the assertion would pass for the wrong reason.
        let lag = serde_json::to_value(super::super::LagSummary {
            window_ms: 1000,
            max_lag_ms: 600,
            channel: super::super::LagChannel::Mono,
            lag0_r: Some(0.1),
            peak_r: 0.9,
            second_peak_r: Some(0.4),
            peak_z: Some(14.0),
            prominence: Some(0.5),
            top2_spacing_ms: Some(30.0),
            peak_lag_samples: 12,
            frac_lag_samples: 12.5,
            frac_lag_ms: 0.26,
            edge_pinned: Some(false),
            border_displacement_ms: Some(31.6),
            envelope_window_ms: None,
            envelope_border_displacement_ms: None,
            envelope_lag_ms: None,
            envelope_peak_r: None,
            envelope_edge_pinned: None,
            envelope_rescue: None,
            verdict: super::super::LagVerdict::TimingOffset,
        })
        .expect("lag summary");
        for key in [
            "peak_r",
            "frac_lag_ms",
            "peak_z",
            "prominence",
            "border_displacement_ms",
        ] {
            assert!(
                lag.get(key).is_some(),
                "`{key}` is quoted by the lag contracts but LagSummary no longer emits it"
            );
        }
        for key in ["window_ms", "max_lag_ms", "lag0_r"] {
            assert!(
                lag.get(key).is_some(),
                "`{key}` is quoted by both lag contracts as where the real window is recorded"
            );
        }

        let donor = serde_json::to_value(crate::domain::donor::DonorInterior {
            rms_db: -60.0,
            silence_fraction: 0.5,
            longest_silence_ms: 120.0,
            continuous: true,
            basis: None,
        })
        .expect("donor");
        for key in [
            "rms_db",
            "silence_fraction",
            "longest_silence_ms",
            "continuous",
        ] {
            assert!(
                donor.get(key).is_some(),
                "`{key}` is quoted by donor_interior_aligned's contract but DonorInterior dropped it"
            );
        }

        let align = serde_json::to_value(super::super::CorpusScanAlignment {
            recommended_offset_secs: Some(-1.15),
            offset_drift_secs: Some(0.05),
            offsets_consistent: false,
            start_aligned: true,
            end_aligned: Some(true),
            query_reference_mode: false,
            start_overlap: None,
            clips: vec![super::super::CorpusAlignedClip {
                role: super::super::CorpusClipRole::Start,
                window_start_secs: 0.0,
                window_end_secs: 900.0,
                aligned: true,
                offset_secs: Some(-1.15),
                confidence: 0.95,
            }],
            silence_cross_check: None,
        })
        .expect("alignment");
        for key in ["clips", "recommended_offset_secs"] {
            assert!(
                align.get(key).is_some(),
                "`{key}` is quoted by the alignment contract but CorpusScanAlignment no longer emits it"
            );
        }

        // C3. `geometry` and the two `LevelProfile` halves name their own fields; `seams` names the
        // per-channel detail that separates it from `structure`, whose two fields it also shares.
        let geometry = serde_json::to_value(super::super::GapGeometry {
            a_start_secs: 10.0,
            a_end_secs: 10.5,
            a_refined_start_secs: 10.01,
            a_refined_end_secs: 10.49,
            duration_secs: 0.48,
            b_mapped_start_secs: Some(4.0),
            b_mapped_end_secs: Some(4.48),
            fill_offset_secs: Some(-6.0),
        })
        .expect("geometry");
        for key in [
            "a_start_secs",
            "a_refined_start_secs",
            "duration_secs",
            "b_mapped_start_secs",
            "fill_offset_secs",
        ] {
            assert!(
                geometry.get(key).is_some(),
                "`{key}` is quoted by the geometry contract but GapGeometry no longer emits it"
            );
        }

        let levels = serde_json::to_value(super::super::LevelProfile {
            bin_ms: Some(50),
            profile_db: vec![-40.0],
            floor_db: Some(-120.0),
            speech_peak_db: Some(-12.0),
            noise_floor_db: -52.0,
            gap_floor_db: -98.0,
        })
        .expect("levels");
        for key in ["noise_floor_db", "gap_floor_db", "floor_db", "bin_ms"] {
            assert!(
                levels.get(key).is_some(),
                "`{key}` is quoted by the levels contracts but LevelProfile no longer emits it"
            );
        }

        let seams = serde_json::to_value(super::super::SeamScores {
            baseline_pre: 0.3,
            baseline_post: 0.3,
            selected_channels: vec![0],
            per_channel: vec![(0.3, 0.3)],
            mono_pre: 0.3,
            mono_post: 0.3,
        })
        .expect("seams");
        for key in ["baseline_pre", "baseline_post", "per_channel", "mono_pre"] {
            assert!(
                seams.get(key).is_some(),
                "`{key}` is quoted by the seams contract but SeamScores no longer emits it"
            );
        }
        let structure = serde_json::to_value(super::super::StructureScores {
            baseline_pre: 0.9,
            baseline_post: 0.9,
        })
        .expect("structure");
        for key in ["baseline_pre", "baseline_post"] {
            assert!(
                structure.get(key).is_some(),
                "`{key}` is the name collision the structure/seams contracts exist to disambiguate"
            );
        }
    }

    /// Look a contract up by its wire key, failing loudly if the registry lost it.
    fn shipped(group: &str) -> &'static MetricContract {
        SHIPPED_CONTRACTS
            .iter()
            .find(|(g, _)| *g == group)
            .map(|(_, c)| c)
            .unwrap_or_else(|| panic!("{group} is not registered in SHIPPED_CONTRACTS"))
    }

    /// Every group the plan lists for C2 actually ships text — the registry sweep proves the
    /// contracts it *has* are well-formed, not that C2 finished.
    #[test]
    fn every_c2_group_ships_a_contract() {
        for key in [
            "lag_decision",
            "lag_editorial",
            "donor_interior_aligned",
            "donor_interior_nominal",
            "residual",
            "seam_probe",
            "splice",
            "splice_dualfit",
            "outcome",
            "geometry",
            "levels",
            "b_levels",
            "structure",
            "seams",
        ] {
            let c = shipped(key);
            // Presence is the point (the registry sweep only validates the contracts that exist), but
            // a half-filled entry would pass a bare presence check while telling the reader nothing.
            for (axis, text) in [
                ("measures", &c.measures),
                ("placement", &c.placement),
                ("window", &c.window),
                ("not", &c.not),
            ] {
                assert!(
                    text.as_deref().is_some_and(|s| !s.trim().is_empty()),
                    "{key}: `{axis}` is empty — all four axes are the contract"
                );
            }
        }
    }

    /// The wrapper must not move the wrapped type's keys — that is the whole point of `flatten`.
    #[test]
    fn contract_sits_beside_the_values_not_around_them() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Inner {
            class: String,
            drop: bool,
        }

        let wrapped = Contracted::new(
            Inner {
                class: "shared_silence".into(),
                drop: true,
            },
            EQUIVALENCE_PRODUCTION_CONTRACT,
        );
        let json: serde_json::Value = serde_json::to_value(&wrapped).expect("serialize");

        // Values stay at the top level of the object, exactly where they were pre-wrapper.
        assert_eq!(json["class"], "shared_silence");
        assert_eq!(json["drop"], true);
        assert!(json["_contract"]["placement"].is_string());
        assert!(json.get("value").is_none(), "flatten must not nest: {json}");
    }

    /// Old corpora have no `_contract`; they must deserialize to `None`, never fail.
    #[test]
    fn absent_contract_deserializes_as_none() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Inner {
            class: String,
        }

        let bare: Contracted<Inner> =
            serde_json::from_str(r#"{"class":"repairable_dropout"}"#).expect("deserialize");
        assert_eq!(bare.contract, None);
        assert_eq!(bare.class, "repairable_dropout"); // via Deref
    }

    /// A round-trip through the wrapper preserves both halves.
    #[test]
    fn contract_round_trips() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Inner {
            class: String,
        }

        let wrapped = Contracted::new(
            Inner {
                class: "ambient_quiet".into(),
            },
            EQUIVALENCE_DIAGNOSTIC_CONTRACT,
        );
        let json = serde_json::to_string(&wrapped).expect("serialize");
        let back: Contracted<Inner> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(wrapped, back);
    }

    /// The two halves of the pair must not ship the same words — the wrapper exists so they can differ.
    #[test]
    fn the_two_equivalence_contracts_are_distinct() {
        assert_ne!(
            EQUIVALENCE_PRODUCTION_CONTRACT,
            EQUIVALENCE_DIAGNOSTIC_CONTRACT
        );
        assert_ne!(
            EQUIVALENCE_PRODUCTION_CONTRACT.placement,
            EQUIVALENCE_DIAGNOSTIC_CONTRACT.placement
        );
    }
}
