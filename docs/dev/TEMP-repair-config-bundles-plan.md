# Repair config bundles — plan (DRAFT)

Status: **P0 done; plan re-scoped 2026-07-23 after P0 — see §6.4 before acting on P1–P4.**
Originally: collapse the four-layer hand-copy of repair patch knobs into shared policy bundles
embedded by value. **Opportunistic** — extract when adding or touching a knob family, not as a
standalone mega-refactor.

> **Read §6.4 first.** P0 removed more of the problem than expected, which dissolves most of
> the original motivation for the *bundles* idea (§5). P1 is reframed, P2/P4 are demoted, and
> a previously-unnamed problem (mode-coupled knob liveness) is added. §5's bundle catalogue is
> retained for reference but is **no longer the recommended direction**.

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

## 5. Proposed bundles (by cohesion) — *superseded, retained for reference*

> **Superseded by §6.4.** After P0, bundling is organization rather than correctness. Keep this
> catalogue as a cohesion map if a family is ever split for its own reasons; do not treat it as
> the recommended direction.

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
| **P1** | ~~Extract `FillSearchParams` + `FitScoringParams` / `SeamGatePolicy` into both layers~~ → **reframed: delete the `SeamGateConfig` twin**, have `SeamGateParams` borrow `&PatchRequestSettings` + keep only the derived frame fields | Clearing M-CFG | **Done 2026-07-23** | `SeamGateDerived` holds frames + scan/opt-in only; policy reads `settings`. Hot path was already `&` — duplication cleanup, not a perf change. |
| **P2** | Remaining bundles (extension, fill-anchor, residual, anchor-seam, normalize) — one PR per family | When that feature is touched | **Demoted to optional** (§6.4.3) | Post-P0/P3.1 this is organization, not correctness. Do it only if a family genuinely becomes unwieldy. |
| **P3** | Seed the harness literal from `RepairConfig::default().patch_settings()` — removes the last independent default source | Now | **Done 2026-07-23** | Reduced to one step: §6.5 retracted §6.2, so step 2 (production-fidelity test) was dropped as redundant and step 3 demoted to undated cleanup. |
| **P4** | ~~Nest TOML under `[repair.fill_search]`~~ | — | **Not recommended** (§6.4.3) | Fights `repair_profile_field_mask_from_toml`'s flat `contains_key` walker and forces alias keys. Revisit only on a real TOML-ergonomics complaint. |
| **P5** | **New:** mode-coupled knob liveness — assert `fill_fit_*` knobs cannot affect `Gate`-mode output (§6.4.4) | Opportunistic | Pending | The failure mode that actually bit us; unaddressed by the original plan. |

**Do not** execute these as a single “restructure config” PR.

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

**Guards added (`patch_audio.rs` tests, commit `e02dfad`):**
`deref_reads_reach_embedded_settings_and_are_not_shadowed` and
`into_request_defaults_measure_residual_off`.

### 6.2 ~~Why P3 is the next phase~~ — **RETRACTED, see §6.5**

> **This section's conclusion is wrong.** The experiment below ran a *subset* of the suite that
> excluded every production-path test. Re-run correctly (§6.5), production `fill_mode: Fit` **is**
> guarded. Kept for the audit trail; do not cite it.


Validating the P0 shadowing guard produced the strongest available argument for P3. A field
added to `PatchAudioRequest` that collides with a settings field silently wins at every
`request.<knob>` read site. Simulating exactly that — production `fill_mode` shadowed to
`Gate` while config says `Fit` — the **entire pre-existing suite still passes**:

| Suite | Result with production `fill_mode` forced to `Gate` |
|-------|------------------------------------------------------|
| lib (new guard excluded) | 381 passed, 0 failed |
| `golden_baseline_invariance` | passed |
| `gap_repair_spec_diff` | passed |
| `patch_audio_integration` | 26 passed, 0 failed |

The cause is M-HARNESS itself: `PatchTestOptions::default()` is *already* `fill_mode: Gate`,
so no test can distinguish "production runs `Fit`" from "production runs `Gate`". The drift is
not cosmetic — **it blinds the suite to a class of production regression.** That is why P3
outranks P1/P2 despite being listed later, and why it is worth doing without waiting for a
knob-touch trigger.

**Scope note.** P3 covers default *sources*, not test assertions. The
`query_reference_integration` literal joins P3 because it is another hand-maintained copy whose
`fill_mode: Gate` must become production `Fit` — a behavior-affecting alignment needing P3's
"update assertions in that PR" protocol, not a routine structural PR. Its **assertions** are
already sound (`Patched` status, `post_correlation > 0.85`, interior RMS) and are out of scope:
single-knob mutations there are silent because the chirp fixture is easy and the structure-trust
path bypasses the Pearson gate — confirmed by re-running with `disable_structure_trust: true`,
which makes `min_fill_correlation` bite immediately. Do not "fix" those assertions.

**P3 success criterion (checkable, mirrors P0's):** `PatchRequestSettings { … }` appears as a
struct literal only where a test deliberately overrides, and every such site is seeded from
`RepairConfig::default().patch_settings()`.

### 6.3 Measured P3 scope — 8 drifting knobs, not 3 (2026-07-23)

Two hand-rolled `PatchRequestSettings` literals survive P0: harness
`patch_request_with_options` (`crates/clip-sync-repair-harness/src/patch_audio.rs`) and
`crates/clip-sync-repair/tests/query_reference_integration.rs`. P0 only reshaped them; the
values are byte-identical to the pre-P0 `PatchAudioRequest` literals.

An early estimate put the drift at "3 of 31 shared knobs" by diffing `PatchTestOptions`
against production. **That measurement was structurally incapable of being complete**: a knob
absent from `PatchTestOptions` cannot appear in a `PatchTestOptions`-keyed comparison, yet it
still lands in the built request as a hardcoded literal. Re-measuring the right way — building
`patch_request_with_options(report, …, PatchTestOptions::default())` and diffing all 56 fields
of the resulting `.settings` against `RepairConfig::default().patch_settings()`:

| Field | Harness | Production | Notes |
|-------|---------|------------|-------|
| `fill_mode` | `Gate` | `Fit` | the §6.2 blind spot |
| `fill_fit_energy_nominal_bias_scale` | 1.0 | 0.25 | **absent from `PatchTestOptions`** — harness line mirrors `options.fill_fit_nominal_bias_scale`, so energy bias can never differ from nominal. Production deliberately splits them 4× (`config.rs`: *"0.25 recovers a 7 s-off nominal in the F4 EC-6 sweep"*). Only bites in `GapSignature::Energy` (`patch_region.rs:1523`). |
| `skip_equivalent_gaps` | `false` | `true` | absent from `PatchTestOptions` |
| `border_standoff_secs` | 0.0 | 0.35 | absent from `PatchTestOptions` |
| `absolute_silence_rms` | 0.0 | ~0.00101 | absent from `PatchTestOptions` |
| `residual_gate` | `Off` | `Veto` | absent from `PatchTestOptions` |
| `fill_border_search_secs` | 30.0 | 10.0 | |
| `max_fill_align_adjustment_secs` | 1.0 | 0.5 | |

Five of the eight are invisible to `PatchTestOptions` entirely. `query_reference_integration`
carries the same class of drift (e.g. it hardcodes `fill_fit_energy_nominal_bias_scale: 1.0`).

Consequences for P3:

- Seed from `RepairConfig::default().patch_settings()` and override explicitly — do **not**
  extend `PatchTestOptions` field-by-field to chase the gap; that is the mechanism that let five
  knobs drift unseen.
- Budget for behavior change on eight axes, not three; `residual_gate: Off → Veto` and
  `skip_equivalent_gaps: false → true` can change which gaps are patched at all, independent of
  the `fill_mode` flip.
- The energy-bias mirror means **no harness test has ever exercised production's
  nominal/energy split.** `crates/clip-sync-repair-fixtures/src/energy_signature_production.rs`
  sets the pair deliberately and is *not* part of this blind spot.

Reproduce by appending a `#[cfg(test)]` probe to the harness that builds the request from
`PatchTestOptions::default()` and `format!("{:?}")`-compares every field against
`patch_settings()`.

### 6.4 Plan re-examination after P0 (2026-07-23)

P0 removed more of the problem than this plan anticipated, and §6.3's measurement reframed
what remains. The bundle idea in §5 was the fix for *"four hand-maintained copies that
drift."* P0 collapsed the request layer into the settings layer **with zero read-site churn**,
which proved those two layers were pure duplication carrying no semantic difference. P3 step 1
removes the last independent default source. After both, a new knob has exactly one definition
and one seed, so it **cannot** drift. Grouping the 56 flat fields into policy bundles after
that buys tidiness, not correctness — so P1/P2/P4 must be re-argued on their own merits rather
than inheriting M-CFG's urgency. That re-argument follows.

#### 6.4.1 P3, revised — 3 steps

The earlier framing ("flip all 8 drifting knobs to production, one group per commit") was
wrong in two ways. First, it conflated two different goals: **step 1 is M-CFG** (remove a
duplicated default source), while flipping knobs is **M-HARNESS** (make tests exercise
production behavior). Second, and more importantly, it treated all 8 knobs in §6.3 as *drift
to be eliminated* — but the harness runs synthetic fixtures, so several are plausibly
**deliberate and correct** test overrides:

| Knob | Why the override may be legitimate (hypothesis — verify before dropping) |
|------|--------------------------------------------------------------------------|
| `absolute_silence_rms: 0.0` | Synthetic fixtures contain true digital silence; production's ~0.001 exists for a real-world noise floor. |
| `border_standoff_secs: 0.0` | No codec edge artifacts to stand off from in generated audio. |
| `skip_equivalent_gaps: false` | Tests likely *want* every gap processed rather than deduped. |
| `residual_gate: Off` | Tests want to observe the patch, not have it vetoed. |

Only `fill_mode` has a **proven** case for changing (§6.2: production runs `Fit`, harness runs
`Gate`, nothing in the suite can tell the difference). `fill_fit_energy_nominal_bias_scale`
rides along because it is inert until `Fit` is on (§6.4.4).

And that coverage is better bought **additively** than by mutating 37 existing tests' defaults:

1. **Seed the harness literal from `RepairConfig::default().patch_settings()`, overriding all
   8 back to today's values.** Structural, zero behavior change, and it permanently kills the
   "new knob drifts unseen" mechanism. Do this regardless of everything else.
2. ~~**Add a production-fidelity test**~~ — **DROPPED (§6.5).** `patch_request_from_repair`
   already runs unmodified `patch_settings()` across ~10 test files; the blind spot it was
   meant to close does not exist.
3. **Audit the 8 surviving overrides**: comment the justified ones, drop the unjustified.
   Expect several to stay. No deadline — and per §6.5 these are test-local settings rather than
   a coverage hole, so the stakes are low.

Step 1 is done. The abandoned "flip everything and see what breaks" sequence is explicitly
**not** the plan.

#### 6.4.2 P1, reframed — delete the twin, don't bundle into it — **done 2026-07-23**

`SeamGateConfig` carried **53 `pub` fields** against `PatchRequestSettings`' 56. That was not
the narrow derived projection §2 and Principle 5 assumed — it was a near-twin. P1 deleted it:
`SeamGateParams` now holds `&PatchRequestSettings` + owned `SeamGateDerived` (frames, rate/channels,
`silence_peak_fraction`, `measure_residual`, `anchor_matchability`). `from_repair` is frame math
only. Hot path was already behind `&` (copy-cost gate answered by types); this is duplication
cleanup.

#### 6.4.3 P2 demoted, P4 not recommended

- **P2** — post-P0/P3.1 there is no correctness argument left, only organization. Defer until a
  knob family becomes genuinely unwieldy on its own terms.
- **P4** — nesting TOML fights `repair_profile_field_mask_from_toml`, which does flat
  `repair_table.contains_key(...)` lookups, and would force alias keys for compatibility. Close
  to pure cost. Revisit only on a real user-facing TOML-ergonomics complaint.

#### 6.4.4 P5 (new) — the real unsolved problem is liveness, not grouping

Five `fill_fit_*` knobs are **dead under `fill_mode: Gate`**: the read at
`patch_region.rs:1523` sits in `gate_structure_align`, reachable only via
`evaluate_seam_gate_fit_candidate`, which `evaluate_seam_gate` (`patch_region.rs:215`) enters
only when `fill_mode == Fit`. Separately, `min_fill_correlation` is dead whenever
structure-trust fires (§6.2).

A flat 56-field struct cannot express "this knob only exists in this mode," and that is exactly
what let the energy-bias drift survive unnoticed: it was set wrong **and** inert, so nothing
could observe it. The original plan never addresses this, and it is the failure mode that
actually bit us — grouping knobs into bundles would not have caught it.

The type-correct fix is enum-carried params (`FillMode::Fit(FitParams)` / `FillMode::Gate`),
making the mistake unrepresentable. **Not recommended now:** it collides with flat TOML keys,
the field-mask walker, and the profile machinery, and the drift class it prevents is already
closed by P3 step 1. The cheap 80% is a test asserting that perturbing the five `fill_fit_*`
knobs leaves `Gate`-mode output byte-identical — it documents the coupling and fails loudly if
a fit knob later leaks into the `Gate` path. Escalate to the enum only if fit-mode knobs keep
multiplying.

### 6.5 §6.2 retracted — production config *is* covered (measured 2026-07-23)

The §6.2 experiment concluded that nothing in the suite could distinguish production `Fit` from
`Gate`. **That conclusion was an artifact of an incomplete test selection.** It ran the lib
tests, `golden_baseline_invariance`, `gap_repair_spec_diff`, and `patch_audio_integration` —
all of which either build requests from `PatchTestOptions` or never touch patch config at all
(the two goldens are scan/classification invariance and construct no patch config whatsoever).

There is a second, pre-existing construction path that §6.2 overlooked:
`patch_request_from_repair(report, &RepairConfig::default())` in
`clip-sync-repair-fixtures/src/energy_signature_production.rs:348`, which is
`repair.patch_settings().into_request(report)` — i.e. **unmodified production settings**. It is
used across ~10 test files (`anchor_seam_oracle`, `oracle_energy`, `seam_residual_oracle`,
`validate_residual_gate`, `integration_energy_smoke`, the gap corpus, …).

Re-running the shadow experiment — forcing `patch_settings()` to emit `fill_mode: Gate` — with
those tests included:

| Suite | Result with production `fill_mode` forced to `Gate` |
|-------|------------------------------------------------------|
| `anchor_seam_oracle` | **FAILED — 3 failed, 8 passed** (baseline: 11 passed) |
| `validate_patch_audio` | passed — but only because it sets `fill_mode: Fit` explicitly, so it never reads the production default |

**Production `fill_mode` is guarded.** By extension so are the other seven §6.3 values, since
`patch_request_from_repair` carries the whole production settings struct.

Consequences:

- **P3 step 2 (production-fidelity test) is redundant and was dropped.** The coverage it would
  have added already exists via `patch_request_from_repair`. The correct fix for §6.2 was better
  measurement, not more tests.
- **The 8 differences in §6.3 are not a coverage hole.** They are test-local settings for the
  `PatchTestOptions`-driven tests, sitting alongside a production path that is separately and
  genuinely covered. That substantially lowers the stakes of P3 step 3.
- **P3 reduces to step 1** (seed structurally), which is done.
- Methodological note: "the whole suite passes" is only meaningful with the selection stated.
  Both §6.2 and the original "3 of 31 knobs drift" figure (§6.3) failed the same way — a
  measurement whose *scope* silently excluded the evidence. Name the selection next time.

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
| ~~Harness tests assert old Gate/30s defaults~~ | Obsolete: revised P3 (§6.4.1) no longer flips shared defaults — it seeds structurally and buys coverage additively |
| Treating all 8 §6.3 differences as drift and dropping them → breaks fixtures that legitimately need synthetic-media settings | §6.4.1 step 3: justify-or-drop each, individually, with no deadline. Only `fill_mode` has a proven case for changing |
| Bundling (§5) pursued out of habit after its motivation is gone | §6.4 — re-argue P1/P2/P4 on their own merits; §5 is explicitly superseded |
| P3 scope underestimated from a `PatchTestOptions`-keyed diff — 5 of the 8 drifting knobs are hardcoded in the harness literal and absent from `PatchTestOptions`, so that comparison cannot see them (§6.3) | Diff the *built request's* `.settings` against `patch_settings()`, all 56 fields; seed from production rather than extending `PatchTestOptions` |
| Harness energy bias always mirrors nominal (no `fill_fit_energy_nominal_bias_scale` in `PatchTestOptions`), so production's deliberate 4× split is untested outside the fixtures crate | P3 seeds from production, restoring 0.25; expect `GapSignature::Energy` gaps to move |
| Harness `fill_mode: Gate` default blinds the suite to production `Fit` regressions (§6.2 — measured: a shadowed `fill_mode` passes 381 lib + golden + spec-diff + 26 patch tests) | P3; treat every assertion that changes when the default flips as a previously-untested behavior, not as a test to relax |
| P0 `Deref` shadowing: a field added to `PatchAudioRequest` colliding with a settings field silently wins at ~160 read sites | `deref_reads_reach_embedded_settings_and_are_not_shadowed` (verified to fail on a simulated collision) |
| Profile mask still targets flat fields | Update `RepairProfileFieldMask` / `apply_profile_bundle` to write into bundles |
| CLI help strings / default display | Keep reading defaults from the embedded bundle’s `Default` |

## 9. Success criteria

- Adding a knob means: define once on a bundle, wire CLI/TOML once, embed/propagate the
  bundle — **no new lines** in three parallel field lists.
- `PatchRequestSettings` and `PatchAudioRequest` no longer duplicate ~50 scalar fields.
- `SeamGateDerived::from_repair` derives frames only; policy is read from `&PatchRequestSettings`.
- Harness defaults for shared knobs come from `RepairConfig::default()` (P3).
- M-CFG closable after **P0 + P3 step 1 + P1** (one settings owner; gate has no policy twin).
  P2/P4 are not required to close it.

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-CFG** (this plan); **M-HARNESS** (P3)
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — orthogonal M-MOD slice
- `crates/clip-sync-repair/src/infrastructure/config.rs` — `RepairConfig`, `patch_settings`
- `crates/clip-sync-repair/src/application/patch_audio.rs` — `PatchAudioRequest`,
  `PatchRequestSettings`, `SeamGateDerived::from_repair`
- `crates/clip-sync-repair/src/application/patch_region.rs` — `SeamGateDerived`,
  `SeamGateParams` (settings borrow + derived + geom), existing
  `SeamGateParams` name (cfg+geom era — do not overload with a policy-bundle name)
- `crates/clip-sync-repair-harness/src/patch_audio.rs` — `PatchTestOptions`
- `crates/clip-sync/src/application/config.rs` — `AlignConfig` nesting precedent
- `crates/clip-sync/src/infrastructure/config/toml_keys.rs` — `unknown_toml_keys` / flatten caveat
