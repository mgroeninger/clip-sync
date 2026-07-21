# Calibration tooling gate — plan (DRAFT)

Status: **draft / not started** (2026-07-21).

Fold the gap-fingerprint **calibration workflow** behind one `calibration` cargo feature so it stops
leaking into the production binary and the test tier. Tracked in [BACKLOG.md](../BACKLOG.md)
§ Build hygiene / calibration tooling.

## Problem

The workflow is "produce a licensing-safe gap-fingerprint corpus, then analyze it." Today its pieces
are scattered across three build surfaces, all built unconditionally:

- **Producer (CLI, production binary):** `--gap-fingerprints` / `--fingerprint-gap` /
  `--fingerprint-diagnostics` (`cli/args.rs:27-42`), wired in `cli/mod.rs` + `composition.rs`. Each is
  labelled *"Diagnostic"* in its own help text.
- **Analyzer #1 (bin):** `equivalence-calibration` (`src/bin/equivalence_calibration.rs`,
  `[[bin]]` in `Cargo.toml`) — diffs coarse scan vs fine reference equivalence per gap. No
  `required-features`, so it lands in every `cargo build` / release artifact.
- **Analyzer #2 (test):** `tests/diag_fingerprint_corpus.rs` — a stats tool wearing a `#[test]`
  costume (no assertions; "skips" unless `GAP_FP_DIRS` is set; prints lag/timing-offset prevalence
  tables/CSV via `GAP_FP_DIRS` / `GAP_FP_DRIFT_EPS_MS` / `GAP_FP_CSV`). Real logic already lives in
  `harness::gap_fingerprint_corpus::analyze_dirs`.

## Constraint (must not break the no-feature build)

`application/gap_fingerprint.rs` (the `GapCorpus` / `FileSource` serde schema + builder) **stays
compiled unconditionally**:

- the gated `equivalence-calibration` bin imports it (`GapCorpus`), and a bin cannot gate the lib
  module it depends on;
- committed corpus fixtures (`fingerprint_corpus_fixtures.rs`) and golden baseline tests depend on the
  schema.

So the gate moves the **CLI entry points and the executables**, not the schema. Default `cargo build`,
`cargo test`, and the golden/fixtures path must all still compile with the feature **off**.

## Steps

1. **Add feature.** `calibration = []` in `crates/clip-sync-repair/Cargo.toml` `[features]`. Document it
   next to `validation-tests` / `diagnostic-tests` (`docs/development.md`).
2. **Gate the bin.** `required-features = ["calibration"]` on the `[[bin]] equivalence-calibration`
   target.
3. **Gate the producer flags.** `#[cfg(feature = "calibration")]` on the `--gap-fingerprints` /
   `--fingerprint-gap` / `--fingerprint-diagnostics` args + their override wiring (`cli/mod.rs`) and
   scan/characterize plumbing (`composition.rs`). Confirm the non-feature build has no dead-code /
   unused-import breakage where the corpus writer was called.
4. **Promote `diag_fingerprint_corpus` to a bin.** Move the env-driven `main` into a `[[bin]]` under
   `calibration`, reusing `harness::gap_fingerprint_corpus::analyze_dirs`. **Decision:** subcommand of
   `equivalence-calibration` (both consume the same corpus dirs — preferred, one tool) **vs** sibling
   `gap-fingerprint-stats`. Drop the `#[test]` once the bin covers it.
5. **Verify.** `cargo build` (feature off) has no calibration surface; `cargo build --features calibration`
   exposes bin(s) + flags; golden/fixtures tests still green with the feature off.

## Out of scope

- `tests/calibrate_anchor_prominence.rs` — structurally the same "calibration-as-test" pattern (no
  assertions, `#[ignore]`, prints CSV, real logic in `harness::anchor_prominence`), **but not queued
  work.** Unlike the gap-fingerprint bin it does **not** leak — it is already gated behind
  `validation-tests` + `#[ignore]` — and its calibration **already ran and is settled**: it refuted
  raising the `anchor_seam_min_prominence` default above `0.0` (real anchors ≤0.073; a 0.10 floor would
  disable rescue). **Decision: keep as-is**, as a re-runnable probe for *if anchor detection changes*.
  The `validation-tests` tier is the right home for that — do **not** promote it to a bin. Only action
  taken: a header comment recording that its finding is settled so nobody mistakes it for pending work.
  (If the `calibration` feature lands and we later want strict consistency, folding it in is optional,
  not required.)
- The `application/gap_fingerprint.rs` schema — stays as shared library code (see Constraint).
