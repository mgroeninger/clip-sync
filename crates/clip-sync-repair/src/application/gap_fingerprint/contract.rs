//! Co-located measurement contracts — the `_contract` object a metric group carries on the wire.
//!
//! **Why this exists.** Field *names* answer "which field?"; they cannot answer "measured where, over
//! what window, and what is this *not*?". Readers (agents especially) treat names as the schema and
//! invent the rest, and this corpus's real failure mode is wrong placement / wrong window, not wrong
//! field. A corpus-root notes map is too easy to skip and too far from the number it explains, so the
//! contract travels **inside the same JSON object as the values**. See
//! `docs/dev/TEMP-fingerprint-field-clarity-plan.md` § 2.
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
/// `lag_editorial`/`lag_decision` over `LagFingerprint`, `donor_interior`/`donor_interior_nominal` over
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
        placement: "scan-time gap: A's silent core, donor window = that core offset-mapped onto B",
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
