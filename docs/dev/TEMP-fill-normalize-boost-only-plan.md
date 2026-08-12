# TEMP — Fill normalize: boost-only (never attenuate)

**Status:** implemented 2026-08-12 (code + docs + tests landed; ear spot-check outstanding).
**One deliverable:** change A-border fill loudness matching so it may **raise** a quiet B fill
toward A's borders, but must **never attenuate** a louder fill.

Companion: [fill_level.rs](../../crates/clip-sync-repair/src/domain/fill_level.rs) module docs,
[json-output.md](../json-output.md) § FillLevelCheck (gunshot-in-conversation counter-example),
[pipeline.md](../pipeline.md) § Fill-level check, archive
[repair-write-path-plan.md](archive/repair-write-path-plan.md) § Normalization.

---

## 1. Goal / non-goals

**Goal**
- When `normalize_fill` is on, `compute_fill_gain(a_border_rms, b_segment_rms, max_db)` still
  computes the A-border / B-fill RMS ratio and still clamps the **upward** side with
  `max_fill_gain_db`, but **never returns a gain &lt; 1.0**.
- Quiet B fills keep today's boost behavior (subject to the existing ±dB clamp on the boost side).
- Loud correct content (e.g. a gunshot inside a talk scene) keeps its donor level instead of being
  dragged down to A shoulder RMS.
- Unit + integration coverage for both directions; durable docs note the policy.
- **Emit `normalize_gain` on the patch record** (folded in 2026-08-12, see § 1 sequencing). Without it
  the change is unauditable: the gain is on the spec (`gap_repair_spec.rs:197`) but serialized
  nowhere, so nothing can say which gaps this PR actually moved.

**Accepted consequence (state it, don't discover it at the ear pass)**
- Today's attenuation also drags the fill's **edges** down toward A, so removing it raises the seam
  step at both boundaries for exactly the loud-fill case this protects. The crossfade smears that
  step, it does not remove it. This is accepted: the donor is the same master, so a real level step
  at the seam is program dynamics, not damage — and § 7's edge-shape axis, not whole-fill RMS, is
  where a future gate belongs.
- **No crossfade coupling in the implementation.** `crossfade_ms` is operator-configurable (default
  **10 ms**, `default_crossfade_ms`/`config.rs:433`; `--crossfade-ms`, TOML `crossfade_ms`;
  `--gap-listen` carries its own `listen.crossfade_ms`, `gap_listen/mod.rs:84`), but the gain is a
  scalar applied in `gained_fill` **before** the splice and the crossfade is applied **inside** it —
  orthogonal, no ordering to get right, and § 5's bit-equality test holds at any crossfade value.
  It matters only for reasoning: it is the mitigation dial that **already exists** for a seam step
  (lengthen the ramp), so do not propose a new knob. Note also that `fill_level` measures the fill
  with the crossfade **ignored** ([pipeline.md](../pipeline.md) § Fill-level check), so
  `edge_delta_db` systematically *overstates* the audible step — true today, larger after this change
  on the affected gaps.

**Non-goals (this plan)**
- Flipping default `normalize_fill` to `false`.
- Residual LSQ `g` (`a ≈ g·b`) as the production gain (needs verdict plumbing + corpus rules).
- B-shoulder / B-relative loudness matching.
- Applying normalize to SilenceSplice / dual-fit (still hard-coded `gain: 1.0`).
- Using `fill_level` / `edge_delta_db` as a veto or gain driver (stays report-only).
- Changing `normalize_window_secs`, border geometry, or crossfade.

**Sequencing against the fill-placement investigation (settled 2026-08-12)**

Ship this **before** [TEMP-fill-placement-registration-plan.md](TEMP-fill-placement-registration-plan.md)
§ 5 (Phase B / S4) renders any new ear-label set. The two plans touch disjoint quantities and the
placement axis is **gain-invariant** — the gain is computed at `region.rs:2056`, strictly after the
placement search and seam reconciliation, its only consumers are `gained_fill` and
`spec.normalize_gain`, and seam scores are Pearson (scale-invariant). So that plan's S1 / S2 / S2b /
S3 / S5 are unaffected and neither blocks the other.

Two things *are* affected, both confined to fills currently being **attenuated** (whole-fill RMS above
A-border RMS — boosted fills are untouched):

1. `peak_delta_db` / `edge_delta_db` comparability. `gap-files/2026-08-07-fill-level-shape/` becomes a
   different-policy baseline for that subset; re-run the `fill-level-rollup` after shipping.
2. **Ear labels** — the binding one. `--gap-listen` builds a real patch request and renders through the
   normal patch path, so `normalize_fill` applies to `_a_patched`. A loud-fill gap rendered after this
   change is *different audio* from one labelled before it. Existing `2026-08-09-drift-listen/` renders
   stay valid as files; a re-render will not match its own label.

Hence the ordering: Phase B is conditional and has not started, so shipping first puts the whole
forward label corpus under one policy instead of straddling it.

**The `normalize_gain` emit is folded into this PR** (2026-08-12; it was briefly filed as an S5 bullet
in the placement plan, which now points here). Rationale: the field is computed at the exact site this
PR already edits, and without it there is no way to say which gaps the policy moved — an unauditable
audible change. It ships **record-only**; no gate reads it, matching the fill-level precedent.

**Follow-ups (explicitly later, not this PR)**
- Residual seam `g` for true master-level mismatch without mid-gap RMS.
- B-relative matching as a possible full replacement for A-border RMS.
- Ear-pass whether boost-only leaves any quiet-donor regressions worth a second policy.

---

## 2. Why (failure mode)

Today:

```text
gain = clamp(a_border_rms / b_segment_rms, 10^(-max_db/20), 10^(+max_db/20))
```

`b_segment_rms` is the **whole assembled fill**. A loud mid-gap event inflates that RMS and the
scaler attenuates the entire patch toward talking-level A borders. That is the wrong model for
same-master restore: intentional dynamics inside the hole should survive.

Corpus ear labels already treat “loud event inside a quiet/talk gap, quiet at the seams” as often
**correct** ([json-output.md](../json-output.md) § FillLevelCheck — gunshot in a conversation).
A-border whole-fill normalize fights that case. The damaging direction is **gain &lt; 1**.

Boosting quiet fills toward A remains useful (existing
`patch_audio_normalizes_fill_loudness_to_a_border`); this plan keeps that half.

---

## 3. Decision

| Choice | Decision | Reason |
|--------|----------|--------|
| Policy | **Boost-only** | Smallest change that stops the gunshot-crush failure mode |
| Where | Domain `compute_fill_gain` | Single authority; Bracket characterize and any other callers stay consistent |
| Attenuation | Floor gain at **1.0** after the ratio; still apply `max_fill_gain_db` as an **upper** clamp only | Clear semantics: never quieter than the fill we assembled |
| Config | Keep `normalize_fill` default **true**; keep `max_fill_gain_db` meaning “max boost” | Avoid coupling “stop crushing” with “turn normalize off” |
| Dual-fit | Unchanged (unity) | Out of scope; already never attenuates via this path |
| Stacking | Do **not** also ship residual/`g` or default-off in the same change | One audible policy per deliverable |

Pseudo:

```text
if a_border_rms == 0 or b_segment_rms == 0 → 1.0
raw = a_border_rms / b_segment_rms
max_boost = 10^(max_gain_db/20)
gain = clamp(raw, 1.0, max_boost)   // was clamp(raw, 1/max_boost, max_boost)
```

---

## 4. Touch points

| Area | Change |
|------|--------|
| Domain | `policies::compute_fill_gain` (`silence.rs:362`) — replace `min_gain` with `1.0` in the clamp |
| Domain unit tests | **Add** a loud-B → exactly `1.0` case. Nothing to refresh: the only clamp test, `compute_fill_gain_clamps_to_max_db` (`silence.rs:737`), exercises the **boost** side and stays green unchanged; `compute_fill_gain_unity_when_rms_zero` is likewise unaffected |
| Characterize | Bracket path keeps calling `compute_fill_gain` unchanged. `region.rs:2065` is the **only** production caller — silence-splice and dual-fit hard-code `gain: 1.0` (`region.rs:1395`, `region.rs:2526`), so "all callers stay consistent" is already true |
| Invariant | `debug_assert!(gain >= 1.0)` where the gain enters the spec (`region.rs:2161`, `normalize_gain: gain`) — turns § 5's acceptance criterion into a checked invariant for free |
| Reporting | **Emit `normalize_gain`** (record-only). It exists on the spec (`gap_repair_spec.rs:197`) and as `RegionPatch.gain` (`region.rs:51`), but neither `RegionPatchOutcome::Patched` (`region.rs:81-95`) nor `GapPatchStatus::Patched` (`patch_result.rs:364`) carries it — add the `f32` to both and thread it at the conversion (`region.rs:239`). Serialize **unconditionally**, no `skip_serializing_if`: `1.0` must read as *unity*, not as *unavailable* — the exact error S5 of the placement plan fixed for `dual_fit_used` / `anchor_seam_used`. Document in [json-output.md](../json-output.md) alongside FillLevelCheck, since it is what makes `gained_fill`-based fields interpretable |
| Mixed-vintage dumps | **Corrected 2026-08-12 during implementation.** An earlier draft of this row called for `#[serde(default = …)]` returning `1.0` so old corpus JSON would still deserialize. `GapPatchStatus` is **`Serialize`-only** (`patch_result.rs:361`) — nothing reads a report back, and the `#[serde(default)]` attributes already on its sibling fields are inert. So no default is needed and none was added. The hazard is real but lands entirely **consumer-side**: gaps in `gap-files/2026-08-07-fill-level-shape/` and `2026-08-08-v0.5.2-full-corpus/` carry no `normalize_gain` at all, and absence means *pre-emit run*, never *unity measured* — a roll-up must hold those out rather than coerce them. Recorded on the field's doc comment |
| Integration | Keep quiet-B boost test green (`patch_audio_normalizes_fill_loudness_to_a_border`); add loud-B / would-have-attenuated case |
| Docs | `compute_fill_gain`'s own doc comment (`silence.rs:360-361`) says "clamped to ±`max_gain_db` dB" and becomes **actively false** — fix first. Then `config.rs:169` (`normalize_fill`) and `config.rs:183` (`max_fill_gain_db`), `README.md:184` (`--no-normalize` row) and the config block near `README.md:577`, and [pipeline.md](../pipeline.md). Archive write-path wording is historical — leave it |
| CLI | No new flag required for v1 (`--no-normalize` still disables entirely) |

Optional later (not required to ship): TOML comment that `max_fill_gain_db` is max **boost** only.

---

## 5. Verification

1. **Unit:** `compute_fill_gain` — quiet B → boost ≤ max; loud B → exactly `1.0`; zero RMS → `1.0`.
2. **Integration:** existing `patch_audio_normalizes_fill_loudness_to_a_border` still passes.
3. **Integration (new):** B fill louder than A borders → the written samples are **bit-identical** to
   the `--no-normalize` run. Assert equality, not a tolerance: gain is now exactly `1.0` on that
   path, and `gained_fill` (`region.rs:69`) multiplies by it, so there is no ε to allow for.
4. **`normalize_gain` emit:** a unit or integration assertion that the field reaches the patch record
   and reads `1.0` on the loud-B case and `> 1.0` on the quiet-B one — the same two fixtures as #1/#3,
   so no new setup. Confirm it serializes when `1.0` (that is the value most likely to be dropped).
5. **Golden check (confirm, don't assume):** the gain reaches `gained_fill`, which is also what
   `measure_fill_level` is handed (`mod.rs:474-505`) — so this shifts `peak_delta_db` /
   `edge_delta_db` for any previously-attenuated gap, and corpus fill-level numbers collected with
   normalize **on** are not comparable across this change. Committed goldens look safe because the
   fixtures set `normalize_fill: false` (`gap_corpus_fixtures.rs:844`,
   `energy_signature_production.rs:433`) — verify that rather than trusting it.
6. **Lib/clippy:** `cargo test -p clip-sync-repair --lib` + the touched integration test(s).
7. **Ear / corpus (manual, if media handy):** known gunshot-in-talk (or fill-level shape listen set under `gap-files/`) with `--patch-only` / listen — mid-gap event should no longer sound dragged down; spot-check that quiet-donor boosts still sound intentional, and listen at the **seams** for the step § 1 accepts.

Acceptance: no production path applies `normalize_gain < 1.0` when `normalize_fill` is on.

---

## 6. Implement checklist

- [x] Update `compute_fill_gain` to floor at 1.0 (`silence.rs:380`); add the loud-B → `1.0` unit test (`compute_fill_gain_never_attenuates_a_loud_fill`)
- [x] `debug_assert!(gain >= 1.0)` at the spec construction (`region.rs:2087`)
- [x] Thread `normalize_gain` onto `RegionPatchOutcome::Patched` → `GapPatchStatus::Patched`, serialized unconditionally; documented in json-output.md. No `#[serde(default)]` — see § 4 Mixed-vintage row
- [x] Add integration coverage for the would-attenuate (loud fill) case, asserting bit-equality with `--no-normalize`; keep quiet-boost test (`patch_audio_never_attenuates_a_fill_louder_than_a_border`)
- [x] Doc note: `compute_fill_gain` doc comment first (it becomes false), then config field docs, README, pipeline → boost-only
- [x] Run lib + touched integration tests; goldens confirmed unmoved — the fixtures do set `normalize_fill: false` (`gap_corpus_fixtures.rs:844`/`:1139`, `energy_signature_production.rs:433`), so the **only** golden change is the additive `"normalize_gain": 1.0` line in `tests/fixtures/full_surface_repair.json` (regenerated via `write_full_surface_repair_golden`; explicit contract revision). Full workspace green, `cargo fmt`/`clippy` clean
- [x] **Vacuity check:** temporarily restoring the old symmetric clamp makes the new integration test fail on the `debug_assert` (`got 0.57142836`), confirming the test is not passing by accident
- [ ] (Optional) Quick `--patch-only` / listen spot-check on a known loud-mid-gap patch
- [ ] Archive this TEMP when shipped; durable behaviour lives in pipeline / json-output companion notes

---

## 7. Out of scope reminders

Do not treat high `peak_delta_db` or whole-fill RMS vs A as “too loud → attenuate.” If a later plan
gates on level, use registration-conditioned **edge** shape ([json-output.md](../json-output.md)),
not this normalize path.
