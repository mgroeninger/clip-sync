# Repair config bundles — as landed (CLOSED 2026-07-23)

Status: **Closed.** Treats P2 review finding
[M-CFG](../TEMP-rust-review-findings.md) (four-layer config field copying). Every phase with a
correctness argument shipped; the rest is either explicitly declined or trigger-gated with no
trigger in sight.

The original plan proposed collapsing four parallel knob structs into shared **policy bundles**
embedded by value. That is **not** what happened, and the bundles were never built. Collapsing
the duplicated *layers* (P0, P1) plus removing the last independent default *source* (P3 step 1)
achieved the goal on its own — a knob now has one definition and one seed, so it cannot drift.
Grouping the remaining 56 flat fields into bundles would have been tidiness, not correctness.
**Do not resurrect the bundle catalogue without a fresh argument**; see "Declined" below.

---

## 1. Before / after

| Struct | Before | After | Conversion |
|--------|-------:|------:|------------|
| `RepairConfig` | 69 | 69 | — (TOML + CLI + scan + patch) |
| `PatchRequestSettings` | 56 | 56 | `RepairConfig::patch_settings` — the one remaining hand-written list |
| `PatchAudioRequest` | 58 | **3** | `into_request` — moves settings whole, no field list |
| `SeamGateConfig` | 53 `pub` | **deleted** | — |
| `SeamGateDerived` | — | 13 | `from_repair` — frame math only |
| `PatchTestOptions` | 31 | 31 | `patch_request_with_options`, now seeded from production |

Three hand-maintained conversion lists became one. Patch policy has exactly one owner
(`PatchRequestSettings`); everything downstream embeds or borrows it.

## 2. What landed

| Phase | Work | Commit |
|-------|------|--------|
| **P0** | Embed `PatchRequestSettings` in `PatchAudioRequest` (`report` + `settings` + `measure_residual`), read-only `Deref`, **no `DerefMut`**. `into_request` 56 lines → 4. | `92571bc`, guards in `e02dfad` |
| **P1** | Delete the `SeamGateConfig` near-twin. `SeamGateParams` now holds `&PatchRequestSettings` + owned `SeamGateDerived` (frames, rate/channels, `silence_peak_fraction`, `measure_residual`, `anchor_matchability`) + geom. | `abb4bd2` |
| **P3 step 1** | Seed both surviving `PatchRequestSettings` literals — harness `patch_request_with_options` and `query_reference_integration` — from `..RepairConfig::default().patch_settings()`. Value-identical to the prior hand-written literals; kills the "new knob drifts unseen" mechanism. | `47cef0e` |
| **P5** | `gate_mode_ignores_fill_fit_knobs`: perturbing all five `fill_fit_*` knobs leaves Gate-mode PCM byte-identical. Documents the mode coupling and fails loudly if a fit knob leaks into the Gate path. | `c97e6e1` |

`SeamGateConfig` was itself only a month old — introduced by `894d353` (2026-06-26) as part of a
seam-gate parameter refactor, then grown into a near-twin of settings. The plan's copy-cost gate
for P1 ("measure whether a 53-field `Copy` struct is copied per-bracket") was answered by the
types: the hot path was already behind `&`, so P1 was duplication cleanup with no perf component.

## 3. Standing invariants (check these in review)

1. **`PatchAudioRequest { … }` appears as a struct literal exactly once** in the workspace —
   inside `into_request` (`application/patch_audio.rs`). Anything else is a new drift surface.
2. **No `DerefMut` on `PatchAudioRequest`.** A stray `request.fill_mode = …` must stay a compile
   error; deliberate overrides spell out `request.settings.fill_mode = …`. `measure_residual` is
   the only field callers assign on a request.
3. **`PatchRequestSettings { … }` literals are seeded from production.** Three exist: production
   `patch_settings` plus two test sites, both ending `..RepairConfig::default().patch_settings()`.
   A new test literal that does not seed is a regression.
4. **`SeamGateDerived` holds no policy.** It is frames + run constants; anything readable from
   `settings` belongs there, not here. Re-introducing a policy field rebuilds the twin P1 deleted.

Guards: `deref_reads_reach_embedded_settings_and_are_not_shadowed`,
`into_request_defaults_measure_residual_off` (`patch_audio.rs` tests),
`gate_mode_ignores_fill_fit_knobs` (`patch_audio_integration`).

## 4. Open (optional, undated) — P3 step 3

Two test sites deliberately deviate from production defaults. They are **test-local settings,
not a coverage hole**: production settings are separately exercised via
`patch_request_from_repair(report, &RepairConfig::default())` across ~10 test files
(`anchor_seam_oracle`, `oracle_energy`, `seam_residual_oracle`, `validate_residual_gate`,
`integration_energy_smoke`, the gap corpus). The deviations and their justifying hypotheses are
already written as comments at `clip-sync-repair-harness/src/patch_audio.rs` (deliberate-deviation
block) and `clip-sync-repair/tests/query_reference_integration.rs`.

| Field | Test | Production | Hypothesis for keeping |
|-------|------|------------|------------------------|
| `fill_mode` | `Gate` | `Fit` | The only one with a proven case for *changing* — but see below |
| `fill_border_search_secs` | 30.0 | 10.0 | Synthetic fixtures place borders further out |
| `max_fill_align_adjustment_secs` | 1.0 | 0.5 | ditto |
| `absolute_silence_rms` | 0.0 | ~0.00101 | Synthetic fixtures are true digital silence |
| `border_standoff_secs` | 0.0 | 0.35 | No codec edge artifacts in generated audio |
| `skip_equivalent_gaps` | false | true | Tests want every gap processed, not deduped |
| `residual_gate` | `Off` | `Veto` | Tests want the patch observable, not vetoed |
| `fill_fit_energy_nominal_bias_scale` | mirrors nominal (1.0) | 0.25 | Inert under `Gate`; no `PatchTestOptions` field exists |

Remaining work is a **comment audit, not a refactor**: justify each override in place or drop it,
one at a time. Five of the eight have no `PatchTestOptions` field, so extending `PatchTestOptions`
field-by-field is the wrong move — that mechanism is what let them drift unseen. No deadline.

## 5. Declined — do not reopen without new evidence

- **Policy bundles** (`FillSearchParams`, `SeamGatePolicy`, `FitScoringParams`, …). The motivation
  was drift across hand-copied layers; P0 + P1 + P3.1 removed the drift. Post-hoc grouping is
  organization only. Revisit if a single knob family genuinely becomes unwieldy on its own terms.
- **Nesting TOML** under `[repair.fill_search]`. Fights `repair_profile_field_mask_from_toml`,
  which does flat `contains_key` lookups, and forces alias keys for compatibility. Close to pure
  cost. Revisit only on a real user-facing TOML-ergonomics complaint.
- **`#[serde(flatten)]` on the repair root.** Incompatible with `unknown_toml_keys`.
- **Enum-carried mode params** (`FillMode::Fit(FitParams)` / `FillMode::Gate`). Type-correct for
  the liveness problem — five `fill_fit_*` knobs are dead under `Gate`, and `min_fill_correlation`
  is dead whenever structure-trust fires — but it collides with flat TOML keys, the field-mask
  walker, and the profile machinery. The P5 test buys the practical 80%. Escalate only if
  fit-mode-only knobs keep multiplying.
- **Flipping the eight test overrides to production values** ("flip everything and see what
  breaks"). Explicitly abandoned: several overrides are correct for synthetic fixtures, and the
  production path is covered elsewhere.

## 6. Verification used

- `cargo build -p clip-sync-repair --all-targets`
- `cargo test -p clip-sync-repair --test config_roundtrip`
- `.\scripts\test-tier.ps1 -Tier pr-repair` — the byte-preservation proof
  (`golden_baseline_invariance` + `gap_repair_spec_diff`)
- `cargo test -p clip-sync-repair --test patch_audio_integration --test anchor_seam_oracle`
  — request construction via `patch_settings` / `patch_request_from_repair`; not in the tier

`-Tier validation` (hours) was not required: every phase was pure structure. A phase that would
change a validation outcome is not pure structure.

## 7. Methodological note (the part worth keeping)

Two measurements in this plan's history were confidently wrong in the same way — **their scope
silently excluded the evidence**:

1. *"3 of 31 knobs drift"* — derived by diffing `PatchTestOptions` against production. A knob
   absent from `PatchTestOptions` cannot appear in a `PatchTestOptions`-keyed comparison, yet it
   still lands in the built request as a hardcoded literal. The real figure was 8, five of them
   structurally invisible to that method. Correct method: diff the **built request's** `.settings`
   against `patch_settings()`, all 56 fields.
2. *"Nothing in the suite can distinguish production `Fit` from `Gate`"* — derived by shadowing
   `fill_mode` and observing 381 lib + golden + spec-diff + 26 patch tests still pass. That
   selection contained no production-path test; the two goldens construct no patch config at all.
   Re-run including `anchor_seam_oracle`, the same shadow **failed 3 of 11**. Production
   `fill_mode` was guarded the whole time, and the "production-fidelity test" this conclusion
   justified was correctly dropped as redundant.

"The whole suite passes" is meaningful only with the selection stated. Name the selection.

---

## Related

- [../TEMP-rust-review-findings.md](../TEMP-rust-review-findings.md) — M-CFG (closed by this
  work); M-HARNESS item 1 (closed by P3 step 1)
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — orthogonal
  M-MOD slice (module layout, not config shape)
- `crates/clip-sync-repair/src/infrastructure/config.rs` — `RepairConfig`, `patch_settings`
- `crates/clip-sync-repair/src/application/patch_audio.rs` — `PatchAudioRequest`,
  `PatchRequestSettings`, `SeamGateDerived::from_repair`
- `crates/clip-sync-repair/src/application/patch_region.rs` — `SeamGateParams` (settings borrow
  + derived + geom), `SeamGateDerived`
- `crates/clip-sync-repair-harness/src/patch_audio.rs` — `PatchTestOptions`, seeded literal
- `crates/clip-sync/src/infrastructure/config/toml_keys.rs` — `unknown_toml_keys` flatten caveat
