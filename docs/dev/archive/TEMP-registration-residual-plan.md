# S1: record the registration residual on the patch record

**Status:** shipped, 2026-08-20. Archived on write.

## Problem

Every `Patched` gap already records `align_adjustment_secs` — the signed B slide the unified
fit search *applied* (`patch_audio/region.rs:1084`, `:1248`). Nothing on the record says whether
that placement agrees with an independent read of where the surviving A audio actually sits in B.
[`TEMP-31g7-model-vs-findings.md`](archive/TEMP-31g7-model-vs-findings.md) §6 built that comparison
by hand, offline, against `--gap-fingerprints` dumps (`frac_lag_ms` — a per-shoulder correlation-peak
lag, computed by `application::gap_fingerprint::measure::lag`) and found gaps where the two disagree
by hundreds to thousands of ms (31/g7: 930 ms; 26/g6: 1.39 s). That comparison currently requires a
separate fingerprint run per gap of interest — it is not something production ever measures or
records for itself.

S1 makes that comparison a first-class field on every patched gap, computed in-process at patch
time: `registration_residual_ms = applied_ms − frac_lag_ms`, taking whichever shoulder is
*confident* and has the smaller absolute residual; abstain (`None`) when neither shoulder is
confident. This is a patch-time diagnostic, not a new gap class — see findings §8 ("the §8 lag-verdict
one-dead row is a dump predicate, not a gap class").

## Non-goals

- **No gate reads it.** `registration_residual_ms` is record-only, exactly like `residual_db` /
  `floor_db` / `headroom_db` (`patch_result.rs:384-392`) before P1 wired a veto on top of them. No
  veto, no skip-reason, no confidence change here. If a gate is ever built on this field, that is a
  separate, later Change with its own corpus evidence — same shape as the residual-gate history in
  [`archive/residual-gate-findings.md`](archive/residual-gate-findings.md).
- **No re-decode.** Both the A shoulder templates (`a_pre_border`/`a_post_border`, already built by
  `policies::border_templates_for_gap` at `region.rs:1004-1005`) and the B search window
  (`b_extract_start_secs..b_extract_end_secs`, sized by `fill_border_search_secs` — default 10 s,
  `characterize_region`, `region.rs:1498,1576-1583`) are already decoded and in scope at the point
  `align_adjustment_secs` is computed. S1 slices an independent per-shoulder correlation peak out of
  buffers that already exist; it must not touch the decoder.
- **No change to placement.** The unified fit search still wins the placement it wins today.
  `registration_residual_ms` is computed *after* the placement is decided, purely as a measurement of
  it — byte-identical `RegionPatch`/fill audio before and after this Change on every gap.
- **Do not import `gap_fingerprint::measure::lag` types.** Same constraint the archived mis-step
  plan already worked out and is worth keeping: that module is `application`-layer and
  `pub(in crate::application::gap_fingerprint::measure)`; `patch_audio` cannot depend on it, and
  reproducing `LagSummary`/`EnvelopeCfg`-shaped state here would be new coupling for no reason. The
  reusable primitive is `domain::seam_local::seam_local_peak` (`seam_local.rs:183`) — already shared
  between the fingerprint path and `dual_fit.rs`'s own per-shoulder sweep — and it is sufficient: it
  returns `(peak_r, peak_lag_frames, peak_z)` from an A template against a bounded B window at an
  anchor, which is exactly one shoulder's `frac_lag_ms`-equivalent measurement.
- **Do not restructure `try_dual_fit`.** Dual-fit gaps (`dual_fit_used: true`) already run
  `seam_local_peak` per shoulder inside `try_dual_fit` and carry `pre_lag`/`post_lag`/`pre_seam_r`/
  `post_seam_r` out of it (`region.rs:684` reads them for `SeamScoreAttempt`). For those gaps S1 is
  pure arithmetic on values already computed — no new sweep, no new plumbing into `dual_fit.rs`. Only
  the bracket path (`dual_fit_used: false`, the majority of patched gaps) needs a new sweep call.
- **Not a knob.** No new CLI flag, no `RepairConfig` field. Follow the `measure_residual` precedent
  (`region.rs:2739,2789`, default off, opt-in on the request) only if the sweep turns out to have a
  measurable perf cost worth gating — check that in Exit before adding a flag nobody asked for. If it's
  cheap (a handful of `seam_local_peak` calls over a ≤10 s window vs the E1-E7 gate that already ran),
  prefer always-on, matching `anchor_seam_used`/`dual_fit_used`'s "always serialized" precedent
  (`patch_result.rs:400-401,413-414`) rather than adding another opt-in bool to thread through.

## Change

1. **Define "confident."** Reuse the existing acceptance floor, not a new threshold:
   `min_fill_correlation` / `fill_absolute_floor` (already on `DualFitParams`, `dual_fit.rs:39-40`,
   defaults 0.35 / 0.12) — a shoulder's `peak_r` clears the same bar `try_dual_fit` and the bracket
   gate already use to call a placement good. Do not invent a second floor for this diagnostic.
2. **Bracket path.** `a_pre_border`/`a_post_border` and the interleaved B extract (`b_samples`) coexist
   only inside `execute_bracket_fill` (`region.rs:967-1044`) — `ExecuteBracketOutputCtx` (the struct
   `execute_bracket_output` actually reads, `region.rs:1050-1077`) is a *resolved* struct with no raw
   PCM on it. So the sweep has to run inside/alongside `execute_bracket_fill` (both the characterize-side
   inline assembly and the executor's independent re-derivation — this function is deliberately called
   twice per gap, "assemble twice" §6b.3a — and its result threaded out as a new field on
   `ExecuteBracketOutputCtx`, the same way `residual: Option<policies::SeamResidualVerdict>` already is
   (`region.rs:1063`); only the characterize-side call's result is the one that reaches the report —
   confirm which of the two call sites that is before wiring the field, do not assume it's whichever is
   read first).
   - **Mono B is not built on this path today.** Unlike the dual-fit path (`b_mono` at `region.rs:531`),
     `execute_bracket_fill` only has interleaved `b_samples`. Build `b_mono` once per call the same way
     (`interleaved_to_mono`) — a cheap re-interleave of already-decoded PCM, not a new decode.
   - **Anchor arithmetic — mirror `try_dual_fit` exactly** (`dual_fit.rs:97-119`), using
     `offset_nominal_start` as the fiducial (the same value `align_adjustment_secs` itself is computed
     against two lines away, `region.rs:1084-1086` — reuse it, don't recompute it):
     `pre_anchor = offset_nominal_start.checked_sub(a_pre_border.len())`,
     `post_anchor = offset_nominal_start + gap_frames`. Call
     `seam_local_peak(&a_pre_border, &b_mono, pre_anchor, max_lag)` and
     `seam_local_peak(&a_post_border, &b_mono, post_anchor, max_lag)`, where `max_lag` is clamped to
     whatever of `fill_border_search_secs` the B extract actually covers (`seam_local_peak` self-clamps
     to the available context, `seam_local.rs:190-192`, so passing the nominal `fill_border_search_secs`
     in frames is safe — no widening past what's already decoded).
   - **Use the templates as returned, not a fixed sub-window.** `a_pre_border`/`a_post_border` come out of
     `border_templates_for_gap` already trimmed for low-energy edges (`trim_low_energy_suffix`/`_prefix`,
     `gap_borders.rs:236-237`) — they can be shorter than the configured `border_frames`. Use
     `a_pre_border.len()`/`a_post_border.len()` as returned for both the anchor subtraction above and the
     `seam_local_peak` call — do not re-slice to a fixed width, and do not assume the configured width.
   - Each call yields `(peak_r, peak_lag_frames, _z)`. `peak_lag_frames` is *already* a displacement from
     the shared `offset_nominal_start` fiducial by construction of the anchor formula above — the same
     fiducial `align_adjustment_secs` is computed against — so `shoulder_lag_ms = peak_lag_frames * 1000
     / sample_rate` is directly comparable to `align_adjustment_secs * 1000` with no further zero-offset
     correction. **Verify this in Exit, don't assume it**: findings §6.3 point 2 documents a real
     zero-offset defect in the *fingerprint* path's older `lag_pair` convention (fixed 2026-08-19 by
     map-coordinates) — confirm the bracket-path anchor formula above doesn't reproduce an analogous
     defect before trusting the byte-identical unit test's "near 0" case as proof (a systematic offset
     that happens to be tiny on the test fixture would pass it and still be wrong on real media).
3. **Dual-fit path**: read `pre_lag`/`post_lag`/`pre_seam_r`/`post_seam_r` straight off the
   `DualFitResult` already produced — no new sweep. Apply the same confident/min-abs selection as
   step 4.
4. **Selection**: among shoulders whose `peak_r` clears the confident floor, take the one with the
   smaller `|align_adjustment_ms − shoulder_lag_ms|`; if neither shoulder is confident, the field is
   `None` (abstain — do not fall back to an unconfident reading).
5. **New field**: `registration_residual_ms: Option<f64>` on `GapPatchStatus::Patched`
   (`patch_result.rs:363`), `#[serde(default, skip_serializing_if = "Option::is_none")]`, following the
   exact `residual_db`/`floor_db`/`headroom_db` precedent immediately above it in the struct
   (`patch_result.rs:384-392`) — same optionality shape, same "absent on gaps this couldn't be measured
   for or on pre-this-Change runs" reading.
6. **Docs**: add `registration_residual_ms` to [`json-output.md`](json-output.md)'s `Patched` field
   table (same section documenting `align_adjustment_secs`, `residual_db` et al.), stating explicitly
   that it is record-only and no gate consults it — mirroring how `fill_level`/`FillLevelCheck` is
   documented as "never affects the verdict" (`patch_result.rs:318-319`).

## Exit

- Unit tests, `region.rs`'s existing test module shape (mirrors the `measure_residual` tests around
  `region.rs:2857-2960`):
  1. A gap with both shoulders confident and correlation-peak lag equal to the applied placement →
     `registration_residual_ms` near 0.
  2. A gap where the independent per-shoulder peak disagrees with the applied placement (synthetic
     31/g7-shape: one shoulder's true match sits at a large offset) → residual reflects the disagreement
     in ms, sign correct (matches findings §6.2's convention: negative = independent read sits earlier
     in B than the applied cut). **This test is also the zero-offset check**: construct it so the applied
     placement is known exactly (not just "near nominal") and assert `registration_residual_ms` against
     the exact expected value, not just its sign — a fixture that only checks direction would pass even
     with the Change §2 zero-offset defect this bullet exists to catch.
  3. Neither shoulder confident → `None`, not a stale/zero value.
  4. Dual-fit path: `registration_residual_ms` derived from `try_dual_fit`'s own returned
     `pre_lag`/`post_lag`, not a re-swept value — same "single authority, no re-derivation" bar
     `dual_fit_result_passes_the_production_revalidation_gate` already enforces for confidence
     classification (`dual_fit.rs:337-429`).
  5. **Byte-identical placement/fill audio** before and after this Change on every existing fixture —
     the new field must be purely additive to the record.
- Real-media re-run: re-patch the corpus, confirm 31/g7 and 26/g6 (the two findings-§6 outliers) land
  a `registration_residual_ms` on the same order as the hand-computed 930 ms / 1.39 s findings §6.2
  already measured offline — this is the validation that the in-process measurement reproduces the
  known-by-hand numbers, not a new investigation into what those gaps mean (26/g6's verdict and
  31/g7's collapsed-bridge verdict are tracked separately — see BACKLOG § Fill placement — this Change
  does not touch either).
- Perf sanity: confirm the added per-shoulder sweep is cheap against the bracket path's own search cost
  (it reuses an already-decoded, already-bounded window — no reason to expect otherwise, but this
  codebase's history of per-bracket costs dominating wall-clock means confirm it, don't assume it —
  [[production-perf-gate-search-dominates]], [[gate-refusal-costs-anchor-oracle]]).
- `cargo clippy -p clip-sync-repair --all-targets -- -D warnings` clean; full `cargo test --lib` passing.

## Explicitly out of scope (leave for later BACKLOG items)

- `lag_editorial` sharing `lag_at_placement` — BACKLOG's item 5, listed as "after S1, or piggyback on a
  `--fingerprint-diagnostics` dump." Do not fold that confirmation work in here.
- Any gate, threshold, or veto keyed off `registration_residual_ms` — needs its own corpus census
  first, same bar every other gate on this codebase has had to clear.
- 26/g6's axis-2 start-sweep oracle and 31/g7's collapsed-bridge skip verdict — both explicitly parallel
  to S1 in BACKLOG's "Order of work," not prerequisites and not blocked by it.
