//! **Curated-fixture backfill for *derived* fingerprint fields.**
//!
//! Tier: **pr-repair** — media-free. Normally a no-op assertion; the rewrite only runs under an env knob.
//!
//! The curated fixtures under `tests/gap_corpus/fingerprints/curated/` were harvested from real media and
//! are the input to two differentials: `golden_baseline_invariance` (fixtures → committed golden) and
//! `gap_repair_spec_diff` (fixtures → projection → golden). When we add a field that is **derived** from
//! data the fixtures already carry — F14's `outcome.dual_fit_rescue` is the first — the projection starts
//! emitting it while the committed fixtures still read `null`, and the spec diff goes red for a reason that
//! is *not* a projection infidelity: the fixture is simply older than the field.
//!
//! Re-harvesting from media is not the fix (the media is licensed and the field is not a new measurement).
//! Instead we backfill the derived field into the committed JSON, in place, leaving every harvested field
//! byte-identical. Run after adding such a field:
//! ```powershell
//! $env:CURATED_FIXTURE_BACKFILL = "1"
//! cargo test -p clip-sync-repair --test curated_fixture_backfill
//! Remove-Item Env:\CURATED_FIXTURE_BACKFILL
//! # then re-freeze the golden, which now sees the new column:
//! $env:CURATED_GOLDEN_REGEN = "1"; cargo test -p clip-sync-repair --test golden_baseline_invariance
//! Remove-Item Env:\CURATED_GOLDEN_REGEN
//! ```
//!
//! **Only derived fields belong here.** A *measured* field has no correct value to synthesize, and writing
//! one would forge harvested data. If a new field cannot be recomputed from the fixture's own numbers by
//! the shipped production helper, the fixture set needs re-harvesting, not backfilling.

use std::path::Path;

use clip_sync_repair::application::gap_fingerprint::{
    dual_fit_rescue_flag, DualFitRescueInput, GapFingerprint,
};
use clip_sync_repair_fixtures::gap_cell_fixtures::{curated_fixtures_dir, load_gap_cell_fixtures};

/// The derived value for one fixture gap, via the same helper `measure`/`project` call.
fn derived_dual_fit_rescue(gap: &GapFingerprint) -> Option<bool> {
    dual_fit_rescue_flag(&DualFitRescueInput {
        patched: gap.outcome.as_ref().is_some_and(|o| o.tier == "patch"),
        brackets: &gap.brackets,
        splice_dualfit: gap.splice_dualfit.as_ref(),
        donor_aligned: gap.donor_interior.as_ref(),
        donor_nominal: gap.donor_interior_nominal.as_ref(),
    })
}

/// Splice `"dual_fit_rescue": <value>` into the file's single `"outcome"` object **as text**, leaving every
/// other byte untouched.
///
/// Neither serde route is byte-faithful here, and both matter on a committed harvested asset:
/// * a `serde_json::Value` round-trip alphabetizes every key (its map is a `BTreeMap` without the
///   `preserve_order` feature), rewriting all ~1400 lines;
/// * a **typed** `GapCorpus` round-trip keeps the order but shifts ~20 harvested floats per file by one ULP
///   (`-55.016197204589844` → `-55.01619720458984`), i.e. it silently edits measurements. Enabling
///   `preserve_order`/`arbitrary_precision` to dodge that would change `Value` semantics for the whole
///   workspace under `cargo test` only — worse.
///
/// So: a text splice. `dual_fit_rescue` is the **last** field of `GateOutcome`, so appending it as the last
/// key is also what a fresh dump emits.
fn splice_dual_fit_rescue(original: &str, value: bool) -> String {
    let key = "\"outcome\": {";
    let open = original
        .find(key)
        .expect("fixture has an \"outcome\" object");
    assert!(
        !original[open + key.len()..].contains(key),
        "fixture has more than one \"outcome\" object — this splice assumes the single-gap curated shape",
    );
    let body = open + key.len(); // just past the `{`

    // Brace-match to the object's close. Fingerprint JSON has no strings containing braces.
    let mut depth = 1usize;
    let close = original[body..]
        .char_indices()
        .find_map(|(i, c)| {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(body + i)
        })
        .expect("unbalanced braces in fixture JSON");

    assert!(
        !original[body..close].contains("\"dual_fit_rescue\""),
        "outcome already carries dual_fit_rescue — the caller only splices when it is absent",
    );

    // Indent + line ending copied from the object's last existing entry.
    let last_entry = original[body..close].trim_end();
    let entry_start = last_entry
        .rfind('\n')
        .expect("outcome spans multiple lines")
        + 1;
    let indent: String = last_entry[entry_start..]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let mut out = String::with_capacity(original.len() + 64);
    out.push_str(&original[..body + last_entry.len()]);
    out.push_str(&format!(",{newline}{indent}\"dual_fit_rescue\": {value}"));
    out.push_str(&original[body + last_entry.len()..]);
    out
}

/// Each committed fixture already carries the derived fields its own numbers imply — i.e. nothing needs
/// backfilling. Under `CURATED_FIXTURE_BACKFILL=1` this instead performs the backfill and reports it.
#[test]
fn curated_fixtures_carry_derived_fields() {
    let backfill = std::env::var("CURATED_FIXTURE_BACKFILL").as_deref() == Ok("1");
    let dir = curated_fixtures_dir();
    let fixtures = load_gap_cell_fixtures();
    assert!(!fixtures.is_empty(), "no curated fixtures");

    let mut stale = Vec::new();
    for fx in &fixtures {
        let path: &Path = &dir.join(&fx.file);
        let original = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

        let gap = fx.gap();
        let Some(outcome) = gap.outcome.as_ref() else {
            continue; // no gate outcome ⇒ nothing to hang the field on
        };
        // `outcome.dual_fit_rescue` is what the committed bytes say; `want` is what they imply.
        let Some(want) = derived_dual_fit_rescue(gap) else {
            // Not measurable from this fixture, so `None` on disk is already right.
            assert_eq!(
                outcome.dual_fit_rescue, None,
                "{}: stale dual_fit_rescue",
                fx.file
            );
            continue;
        };
        if outcome.dual_fit_rescue == Some(want) {
            continue;
        }

        if backfill {
            std::fs::write(path, splice_dual_fit_rescue(&original, want))
                .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
            eprintln!("backfilled {}", fx.file);
        } else {
            stale.push(fx.file.clone());
        }
    }

    assert!(
        backfill || stale.is_empty(),
        "{} curated fixture(s) predate a derived fingerprint field: {}\n\
         Backfill with CURATED_FIXTURE_BACKFILL=1, then re-freeze the golden with \
         CURATED_GOLDEN_REGEN=1 — see this file's header.",
        stale.len(),
        stale.join(", "),
    );
}
