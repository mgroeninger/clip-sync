# Gap-type fixture corpus — implementation review

Status: **open** (2026-07-22). Companion to
[TEMP-gap-fixture-corpus-plan.md](TEMP-gap-fixture-corpus-plan.md).

Review of the Phase 0–5 implementation (loader, per-type assertions, curated golden, re-anchor
retirement, Phase-5 remaining cells). Goal: catch gaps, bugs, regressions, and smells before the plan
is archived.

**Verdict:** Phases 0–4 are solid. Phase 5’s synthetics are the problem area — PR tests are green, but
they freeze internally inconsistent / unreachable fingerprint states. Do not archive until the High
items are resolved.

---

## High

### 1. `11_residual_veto` is not a reachable fingerprint state

**Where:** `tests/gap_corpus/fingerprints/curated/11_residual_veto.json`, Phase-2 arm in
`tests/gap_cell_fixtures.rs`, frozen in `clip-sync-repair-harness/golden/curated.golden.json`.

**What was done:** Derived from `01_bracket_patch_clean` by flipping `outcome.tier` → `skip`, setting
`skip_reason` to residual headroom exceeded, and inflating residual (`informative: true`,
chosen/floor spaced for ~11 dB headroom).

**What was left unchanged:** The parent’s **10 brackets with `failure_stage: None`** (passing brackets),
`splice_dualfit.gate_pass: true`, and high seam peaks.

**Golden freeze:**

| Field | Value |
|-------|-------|
| `tier` | `skip` |
| `brackets_passing` | `10` |
| `bracket_exhausted` | `false` |
| `gate_pass` | `true` |
| `throat_residual_headroom_db` | `11.0` |

**Why unreachable:** The fingerprint dump path (`characterize_gaps_from_decode` →
`compute_region_measurements`) sets `outcome.tier` from **`any_ok` seam scoring** (Pearson brackets),
and measures residual **separately** at the throat for the diagnostic `residual` field. Production’s
G4 residual veto (`ResidualHeadroomExceeded`) can skip a placement that passed seams, but under
current dump semantics that does **not** produce `tier=skip` while leaving ten `failure_stage: None`
brackets.

So `residual_veto` is closer to `unfillable` than the plan currently claims: a production disposition
that the fingerprint schema does not honestly emit as a skip-tier cell.

**Phase-2 blind spot:** The arm only asserts `!patched()`, `residual_informative == Some(true)`, and
headroom > 6 dB. It never checks bracket consistency or the “seams passed” premise beyond residual
fields existing.

**Fix options (pick one):**

1. **Honest dump shape** — keep `tier=patch` (or whatever `any_ok` would emit) with informative
   residual + headroom > margin; assert on residual axes, not on skip-tier.
2. **Mark non-representable** — like `unfillable`: taxonomy entry, no curated fixture; cover via
   residual-gate / `GapRepairCell::ResidualVeto` / `GapPatchSkipReason::ResidualHeadroomExceeded`
   tests. Note: residual-catalog **C1b** (pipeline `ResidualHeadroomExceeded`) is still unproved /
   optional.
3. **If keeping skip-tier** — rewrite brackets so every formerly-passing row carries
   `failure_stage: "residual"` (or equivalent), `brackets_passing == 0`, and assert that shape. Only
   valid if a real dump path can produce it.

---

### 2. False confidence: green tests freeze bad fixtures

`each_fixture_matches_its_declared_cell`, `curated_golden_baseline_invariance`, and
`projection_preserves_curated_golden_baseline` all pass. That only proves the **weak assertions** and
the **self-hosting golden** agree with the committed (inconsistent) bytes — not that the synthetics
match vocabulary cells or reachable dump output.

Regenerating with `CURATED_GOLDEN_REGEN=1` after a bad synthetic edit would permanently encode the
bad shape.

---

## Medium

### 3. `10_decorrelated` is a partial mutation

**Where:** `tests/gap_corpus/fingerprints/curated/10_decorrelated.json` (derived from
`03_silence_splice_dualfit_target`).

**Mutated:** `baseline_lag` / `lag` peak_r → ~0.18, verdicts → `decorrelated`;
`splice_dualfit.gate_pass` → `false`; dualfit seam scores collapsed.

**Not mutated:** `splice.pre_peak_r` / `splice.post_peak_r` remain ~0.99 (silence-splice peaks).

**Analyzer consequence:** `gap_row` prefers `splice` for `peak_r_pre` / `peak_r_post`
(`gross_peak_r_*` in the golden). The committed golden for `10_decorrelated` still shows
gross peaks ≈ 0.99 while labeled decorrelated.

**Vocabulary mismatch:** [gap-vocabulary.md](gap-vocabulary.md) defines Decorrelated as seams that
do **not** recover at any lag (B has different content). Gross peaks that still look like a
silence-splice contradict that.

**Phase-2 blind spot:** Asserts `verdict == "decorrelated"`, donor occupied, `!dualfit_target()`.
`!dualfit_target()` holds mainly because `gate_pass` was flipped — the arm never pins
`dualfit_pass == Some(false)`, low `gross_peak_r_*`, or `!both_sides_recoverable()`. The comment’s
causal claim (“seams recover at no lag ⇒ not a dual-fit target”) does not match the gross-peak data.

**Fix:** Collapse **all** peak layers (`splice`, `baseline_lag`, dualfit seams) to
non-recoverable values; assert `gate_pass == false`, low peaks / `!both_sides_recoverable()`, and
keep the occupied-donor vs program-quiet distinction.

---

### 4. Phase-5 Phase-2 assertion arms are thin

| Cell | Current checks | Missing teeth |
|------|----------------|---------------|
| `Decorrelated` | skip; verdict; donor not quiet; `!dualfit_target()` | `dualfit_pass == Some(false)`; low recoverability / peaks |
| `ResidualVeto` | skip; informative residual; headroom > 6 | seams-passed premise; bracket / tier consistency; skip_reason |
| `TailGeometryMismatch` | `kind == GapKind::Tail` | `duration_secs >=` cutoff; unscored (`brackets_total == 0`); not a dual-fit target |

Contrast with `ProgramQuiet`, which correctly pins the footgun premise (`dualfit_pass == Some(true)`)
so “not a target” cannot pass on a trivially-bad gap.

---

### 5. Residual-veto / unfillable taxonomy asymmetry

The plan correctly concluded `unfillable` is not fingerprint-representable (plan/execution failure,
never characterized). The same dump-semantics argument applies to **residual-veto** under `any_ok`
outcome rules, but Phase 5 still shipped a curated skip-tier fixture for it.

Either both are non-representable fingerprint cells (covered elsewhere), or residual-veto needs an
honest representable shape (see High #1).

---

## Low / smells

### 6. Loader presence test still Phase-0-only

`clip-sync-repair-fixtures::gap_cell_fixtures` unit test `every_phase0_cell_is_present` only requires
the original eight real cells. Phase-5 types (`tail_geometry_mismatch`, `decorrelated`,
`residual_veto`) can vanish if removed from **both** manifest and disk without failing that test.

`manifest_and_on_disk_fixture_files_agree` catches orphans / manifest-only files, not “taxonomy
member missing entirely.”

**Fix:** Assert every **representable** `GapCellType` (all except `Unfillable`, or a dedicated
`REPRESENTABLE_CELLS` list) is present.

---

### 7. `Unfillable` in the enum + Phase-2 panic arm

`GapCellType::Unfillable` is retained for taxonomy completeness; Phase-2 panics if a manifest entry
declares it. Intentional guard, but a **loader-level reject** (`type: unfillable` in manifest → clear
load error) would fail earlier and more clearly than a match-arm panic.

Module comments still say synthetic-only cells “receive hand-built fixtures in Phase 5” — stale now
that Tail is real and Unfillable is unrepresentable.

---

### 8. Stale docs: `test-tiers.md` timing table

`docs/test-tiers.md` still has a repair-validation breakdown row:

> `golden_baseline_invariance` (`--ignored`) | seconds (needs local `gap-files/…`)

That contradicts the later Prerequisites note (and Phase 3) that the test is media-free `pr-repair`
on curated fixtures. Fix the stale row (and any similar mentions) before archive.

---

### 9. Equivalence cells do not assert seam orthogonality

`RepairableDropout` / `SharedSilence` / `AmbientQuiet` Phase-2 arms only re-run
`classify_gap_equivalence()` on recorded silence signals (correct given re-anchor vs equiv
diagnostic-tier mismatch).

Plan data note: `repairable_dropout·g1` is also a dual-fit target — equivalence class and seam
disposition are orthogonal. That fact is frozen in `curated.golden.json` only; Phase-2 does not pin
it. Optional follow-up: for `repairable_dropout`, also assert the live seam readout
(`dualfit_target()` or documented “may also be a seam cell”) so the orthogonality cannot silently
regress.

---

### 10. Plan / comment drift (doc smells)

- Phase-1 text still says the enum includes “the four synthetic-only cells”; Phase-5 finding was that
  Tail is real, Unfillable unrepresentable, only two synthetics shipped.
- Early taxonomy prose still mixes “#9 decorrelated / #10–12” numbering with the final table
  (#9 Tail, #10 Decorrelated, #11 ResidualVeto).
- `GapCellType` doc comments partially updated (Tail real, Unfillable not representable) but the
  module-level “synthetic-only … Phase 5” blurb was not.

Harmless once the plan is archived with a corrected note; fix if the plan stays live.

---

## Out of scope / not bugs (for this review)

- Growing beyond one-per-cell (deferred by plan).
- Changing `GapCorpus` schema or analyzer classification logic.
- Leaving `analyze_dirs` / `gap-fingerprint-stats` able to take `gap-files/` CLI args — intentional;
  no **test** should depend on those dirs (Phase 4 goal met for test wiring).
- `repairable_dropout` also being a dual-fit target — legitimate orthogonality, not a fixture bug.

---

## Suggested fix order before archive

1. **Decide residual-veto representation** (High #1) — honest dump shape, drop fixture, or
   residual-marked brackets; regenerate golden.
2. **Rebuild decorrelated** so every peak layer agrees with “no lag recoverability”; tighten
   Phase-2 (Medium #3–4).
3. **Tighten Tail / residual / decorrelated arms**; extend loader presence to all representable
   cells (Medium #4, Low #6).
4. **Docs cleanup** — `test-tiers.md` stale row; plan/enum comment drift (Low #8, #10).
5. Re-run `gap_cell_fixtures` + `golden_baseline_invariance` + `gap_repair_spec_diff`; only then mark
   the plan ready to archive.

---

## What looked good (context)

- Phases 0–3: curated dir + manifest, CWD-independent loader, live classifiers on committed bytes,
  self-hosting `curated.golden.json`, media-free pr-repair wiring.
- Phase-4: `assert_footguns` / re-anchor golden / smoke test retirement; `test-tier.ps1` includes
  `gap_cell_fixtures`, `golden_baseline_invariance`, `gap_repair_spec_diff`.
- Phase-5: extracting a **real** 363 s Tail member was the right call; documenting `unfillable` as
  non-representable matches dump reality.
- Footguns for silence-splice (is a target) and program-quiet (seams pass yet not a target) are
  properly pinned in Phase-2.
