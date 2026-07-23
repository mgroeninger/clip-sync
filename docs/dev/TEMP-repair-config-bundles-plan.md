# Repair config bundles — plan (DRAFT)

Status: **draft / not started** (written **2026-07-23** from current source). Collapse the
four-layer hand-copy of repair patch knobs into shared policy bundles embedded by value.
**Opportunistic** — extract when adding or touching a knob family, not as a standalone mega-refactor.

**M-CFG context.** This plan is the full treatment of P2 finding
[M-CFG](TEMP-rust-review-findings.md#m-cfg-four-layer-config-field-copying--open) (four-layer
config field copying). Related: [M-HARNESS](TEMP-rust-review-findings.md#m-harness-drift-from-production--open)
(harness `PatchTestOptions` default drift) shares Phase 3 here; do not wait on a full M-HARNESS
pass to start Phases 0–2.

Companions: [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) (orthogonal
M-MOD slice — module layout, not config shape), `AlignConfig` nesting in
`clip-sync` (`clip` / `alignment` / `validation`) as the TOML nesting precedent,
`unknown_toml_keys` in `clip-sync` (constraint on `#[serde(flatten)]`).

---

## 1. Problem (one paragraph)

Patch policy knobs are declared and copied across four nearly parallel structs:
`RepairConfig` (TOML/CLI, **69** fields) → `PatchRequestSettings` (**56**) →
`PatchAudioRequest` (**58**) → `SeamGateConfig` (**45**, with secs→frames derivation). Each
new knob requires edits in three hand-written conversion lists
(`RepairConfig::patch_settings`, `PatchRequestSettings::into_request`,
`SeamGateConfig::from_repair`). Layers 2 and 3 are isomorphic aside from `GapReport` and
opt-in flags (`measure_residual`). A fifth surface — harness `PatchTestOptions` (**31**
fields) — maintains its own `Default` and already drifts from production (e.g. `fill_mode:
Gate` vs production `Fit`). Precedents for bundling already exist
(`RepairProfileBundle`, `AnchorMatchabilityParams`, `RepairPatchConfigView`) but the
majority of knobs remain flat and duplicated.

## 2. Non-goals

- **Big-bang rewrite** of all four layers in one PR.
- **Moving scan-only knobs** onto the patch request (`min_gap_ms`, `scan_block_ms`,
  `silence_*`, `decode_chunk_secs`, `dry_run`, `output`, `scan_both`, … stay on
  `RepairConfig` only).
- **Making `SeamGateConfig` a serde mirror** of `RepairConfig` — it stays a *derived*
  runtime view (sample rate + policy + frame fields).
- **Reusing the name `SeamGateParams`** for a policy bundle — that name already means
  cfg+geom in `application/patch_region.rs`. Prefer `SeamGatePolicy` (or similar).
- **`#[serde(flatten)]` on the repair root** — incompatible with the intent of
  `unknown_toml_keys` (see `clip-sync` `toml_keys.rs`). Prefer named nested tables when
  nesting TOML, matching `AlignConfig`.
- **Bundling behavior changes** into a structural PR — each phase is byte-preserving for
  defaults and validation outcomes unless explicitly scoped otherwise.
- **Waiting on M-MOD / policies split** — orthogonal; either may proceed independently.

## 3. Current layers (re-derived 2026-07-23)

| # | Type | ~Fields | Role | Conversion |
|---|------|--------:|------|------------|
| 1 | `RepairConfig` | 69 | TOML + CLI + scan + patch knobs | — |
| 2 | `PatchRequestSettings` | 56 | Patch subset without `GapReport` | `RepairConfig::patch_settings` |
| 3 | `PatchAudioRequest` | 58 | Settings + report + `measure_residual` | `PatchRequestSettings::into_request` |
| 4 | `SeamGateConfig` | 45 | Run-constant gate inputs (frames + policy) | `SeamGateConfig::from_repair` |
| — | `PatchTestOptions` | 31 | Harness overrides (drift surface) | `patch_request_with_options` |

Production path: `load_repair_app_config` → CLI overrides → `repair.patch_settings()` →
`into_request(report)` → `SeamGateConfig::from_repair(...)`.

Fixture path: `patch_request_from_repair` = `repair.patch_settings().into_request(report)`.

## 4. Principles

1. **One owner per concern** — a knob lives on exactly one policy bundle; outer structs
   embed the bundle by value.
2. **Scan stays on `RepairConfig` only** — never enter patch structs.
3. **TOML** — when nesting, use named tables (`[repair.fill_search]`) like `AlignConfig`.
   Flat keys may be kept temporarily via aliases during a migration window. Do not flatten
   nested bundles onto the repair root.
4. **CLI stays flat** — clap continues to write nested fields
   (`config.repair.fill_search.border_search_secs = …`).
5. **`SeamGateConfig` stays derived** — embed secs-based policy bundles + sample rate;
   compute frame fields once in `from_repair` (or via accessors). Delete the parallel flat
   policy list on that struct as bundles land.
6. **Pair with new knobs** — any new knob that would otherwise extend the three copy lists
   lands on (or creates) a bundle in the same PR.

## 5. Proposed bundles (by cohesion)

| Bundle | Example fields | Notes |
|--------|----------------|-------|
| `FillSearchParams` | `fill_border_search_secs`, `fill_align_margin_secs`, `max_fill_align_adjustment_secs`, `fill_length_slack_secs`, `fill_seam_search_secs`, `min_border_discovery_secs`, `border_standoff_secs` | First extract candidate; haystack geometry |
| `SeamGatePolicy` | `min_fill_correlation`, structure-trust knobs, short-gap fallbacks, `fill_mode`, `gap_signature_*` | Gate / structure path; **not** `SeamGateParams` |
| `FitScoringParams` | fit weights, nominal/late-start scales, marginal/floor, repeat penalty, `fft_seam_search` | Fit-mode scoring |
| `GapExtensionParams` | end/start extend flags, max/step ms | Boundary extension |
| `FillAnchorParams` | min corr, exclude structure-trusted, max adj frac, prior weight, retry marginal | Offset anchors / `anchored_retry` |
| `AnchorSeamConfig` | mode, bracket, prominence + existing `AnchorMatchabilityParams` | Editorial anchors |
| `ResidualGateParams` | mode, floor, headroom, lag | Already partially grouped conceptually |
| `NormalizeParams` | `normalize_fill`, window, max gain | Loudness match |

Defaults and range `validate()` move onto each bundle (mirror `ClipConfig` /
`AlignmentConfig`). `RepairProfileBundle` remains the profile overlay for the four
profile-owned fields; it may *write into* `FillSearchParams` /
`GapExtensionParams` rather than flat `RepairConfig` fields once those exist.

## 6. Extraction order (phasing)

Each phase lands as a **separate structural PR** — never bundled into a behavior-change PR
unless the PR’s only behavior is an intentional harness-default alignment called out in
the description.

| Phase | Work | Trigger | Status | Notes |
|-------|------|---------|--------|-------|
| **P0** | Embed `PatchRequestSettings` in `PatchAudioRequest` (request = settings + report + opt-in flags). Delete `into_request`’s field list. | Convenience / next patch-knob touch | **Done 2026-07-23** | Biggest mechanical win; **no TOML change**. Read-only `Deref`, no `DerefMut` — see §6.1. |
| **P1** | Extract `FillSearchParams` + one of `FitScoringParams` / `SeamGatePolicy`. Embed in `RepairConfig` + settings. Teach `SeamGateConfig::from_repair` to consume bundles. | Next knob in that family, or clearing M-CFG | Pending | Update `config_roundtrip` + one patch smoke. |
| **P2** | Remaining bundles (extension, fill-anchor, residual, anchor-seam, normalize) — **one PR per family** | When that feature is touched | Pending | Stop growing flat copy lists. |
| **P3** | Harness: `PatchTestOptions::default()` from `RepairConfig::default().patch_settings()` with overrides only; or drop options and use settings directly | With M-HARNESS / next harness patch test edit | Pending | Closes the fifth drift surface for defaults. |
| **P4** | Optional: nest TOML under `[repair.fill_search]` (etc.); docs/examples; alias flat keys if needed | After ≥2 bundles exist and TOML churn is acceptable | Optional | `unknown_toml_keys` already walks nested tables. |

**Do not** execute P0–P4 as a single “restructure config” PR. Prefer P0 whenever patch
request construction is already open; P1 when adding a fill-search or fit-scoring knob.

### 6.1 P0 as landed (2026-07-23)

```rust
pub struct PatchAudioRequest {
    pub report: GapReport,
    pub settings: PatchRequestSettings, // policy
    pub measure_residual: bool,         // per-run opt-in, no config key
}

impl std::ops::Deref for PatchAudioRequest {  // read-only: NO DerefMut
    type Target = PatchRequestSettings;
    fn deref(&self) -> &Self::Target { &self.settings }
}
```

**Layering rule.** Policy is read at use sites as `request.fill_mode` via `Deref` and is
writable only where the settings are built (`RepairConfig::patch_settings`). Omitting
`DerefMut` makes a stray `request.fill_mode = …` a compile error, so there is no second
source of truth; deliberate overrides spell out `request.settings.fill_mode = …`.
`measure_residual` is the only field callers mutate on a request (`dual_fit` /
`skip_equivalent_gaps` are config-derived and stay inside `settings` — every existing
`.dual_fit = …` site turned out to be on `RepairConfig` or `PatchTestOptions`, not a request).

Measured outcome: the ~160 `request.<knob>` **read** sites needed **zero** edits (`Deref`);
the real diff was `into_request` (56 lines → 4) plus 4 hand-rolled request literals
(harness `patch_request_with_options`, `query_reference_integration`, `anchor_seam_oracle` ×2).
`PatchRequestSettings` is now re-exported from `application` so callers can name it.

**Invariant (check in review):** `PatchAudioRequest { … }` appears as a struct literal exactly
once in the workspace — inside `into_request`. Anything else is a new drift surface.

## 7. Verification

**Principle:** structural moves must preserve defaults and validation outcomes. Failure modes
are compile errors, missed fields in conversions, and TOML round-trip / unknown-key
regressions — all caught by fast gates.

**Per phase (P0–P3):**

- `cargo build -p clip-sync-repair --all-targets`
- `cargo test -p clip-sync-repair --test config_roundtrip`
- `.\scripts\test-tier.ps1 -Tier pr-repair` — this is the real byte-preservation proof:
  it carries `golden_baseline_invariance` + `gap_repair_spec_diff`
- `cargo test -p clip-sync-repair --test patch_audio_integration --test anchor_seam_oracle`
  (request construction via `patch_settings` / `patch_request_from_repair`; not in the tier)
- Confirm no knob is settable in two places — after P0, `measure_residual` is the only field
  assigned on a request outside `patch_settings`

**When nesting TOML (P4):**

- Round-trip nested tables; confirm misspelled nested keys still surface via
  `unknown_toml_keys`
- Update example TOML / README snippets in the same PR

**Not required for a pure structural phase:** `-Tier validation` (hours). A phase that
would change a validation outcome is not pure structure — re-scope or call it out.

## 8. Risks

| Risk | Mitigation |
|------|------------|
| Missed field in a conversion → silent default | Compile-time: remove the flat field so call sites fail; round-trip + patch smoke |
| Name clash with existing `SeamGateParams` | Use `SeamGatePolicy` (or `FillGateParams`); document in this plan |
| TOML break for users with flat keys | Keep flat aliases during P4 migration; unknown-key walker still works on nested tables |
| Harness tests assert old Gate/30s defaults | P3 intentionally aligns to production; update assertions in that PR |
| Profile mask still targets flat fields | Update `RepairProfileFieldMask` / `apply_profile_bundle` to write into bundles |
| CLI help strings / default display | Keep reading defaults from the embedded bundle’s `Default` |

## 9. Success criteria

- Adding a knob means: define once on a bundle, wire CLI/TOML once, embed/propagate the
  bundle — **no new lines** in three parallel field lists.
- `PatchRequestSettings` and `PatchAudioRequest` no longer duplicate ~50 scalar fields.
- `SeamGateConfig::from_repair` copies bundles (and derives frames), not a long flat list.
- Harness defaults for shared knobs come from `RepairConfig::default()` (P3).
- M-CFG closable after P0–P2 (P3/P4 optional cleanup / UX).

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-CFG** (this plan); **M-HARNESS** (P3)
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — orthogonal M-MOD slice
- `crates/clip-sync-repair/src/infrastructure/config.rs` — `RepairConfig`, `patch_settings`
- `crates/clip-sync-repair/src/application/patch_audio.rs` — `PatchAudioRequest`,
  `PatchRequestSettings`, `SeamGateConfig::from_repair`
- `crates/clip-sync-repair/src/application/patch_region.rs` — `SeamGateConfig`, existing
  `SeamGateParams` (cfg+geom — do not overload)
- `crates/clip-sync-repair-harness/src/patch_audio.rs` — `PatchTestOptions`
- `crates/clip-sync/src/application/config.rs` — `AlignConfig` nesting precedent
- `crates/clip-sync/src/infrastructure/config/toml_keys.rs` — `unknown_toml_keys` / flatten caveat
