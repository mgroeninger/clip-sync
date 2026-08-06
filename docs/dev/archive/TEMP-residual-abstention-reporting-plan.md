# TEMP — Residual-abstention reporting

**Status:** **SHIPPED and archived 2026-08-05** — all three phases implemented; durable behaviour in
[json-output.md](../json-output.md) § *SeamResidualVerdict* / § *GapTags*,
[gap-repair-guide.md](../gap-repair-guide.md) § *Tag axes*, and
[gap-fingerprint.md](gap-fingerprint.md) (`residual` row + § *Gate recipe*).

**One prediction did not hold: §4.1's golden churn never materialized.** No committed golden moved —
the whole crate suite passed unchanged. The fingerprint JSONs under `tests/gap_corpus/fingerprints/`
are *inputs* to the differentials, not re-emitted output, and `tests/fixtures/full_surface_repair.json`
carries no `residual` block at all, so §1.4's correction had no committed multichannel side to move.
The one test that did change is `integration_residual_gate_smoke`'s gating-core comparison, which now
strips `residual_uninformative` exactly as it already stripped `residual_band` — a measure-only run
names its abstention where the pre-gate baseline has nothing to name. That is §1.5 holding, not a
neutrality failure.

**Post-ship review, same day — §1.5 needed a structural guarantee, not a promise.** A cleanup pass
had rewritten both gate readers to `uninformative_reason().is_some()`, which is *not* the guard:
`side_floor_informative` drops a multichannel side whose channels all found a window and then failed
the fit, so `informative` ignores it while the side still names `ProbeNonFinite`. On such a gap the
verdict is `informative: true` with a finite headroom, and routing the gate through the reason
suppressed a live `ResidualHeadroomExceeded` veto (measured: headroom +10 dB against a 6 dB margin,
`Err(HeadroomExceeded)` → `Ok(High)`). Fixed by making the guard an explicit named predicate —
`SeamResidualVerdict::gate_abstains()` — which both readers call and which
`uninformative_reason()` is now *defined in terms of*. The dependency runs one way, so no change to
the naming vocabulary can move a decision; §6's "nothing that decides anything reads it" is now
enforced by shape rather than by discipline. Pinned by
`uninformative_reason_is_exactly_the_gate_guard` (guard ≡ reason across every mono and multichannel
shape) and `asymmetric_multichannel_side_does_not_widen_the_gate_guard`. The underlying
mono/multichannel disagreement about what counts as *measured* is real, gate-facing, and out of this
plan's scope — filed in [BACKLOG.md](../../../BACKLOG.md) § *Residual gate follow-ups*.

Original status line: draft plan, 2026-08-05; self-review folded in the same day (§1.1 threshold note, §1.3
contract, §1.4 channel consistency, §3 promoted to required, §4/§4.1 fixture + golden mechanics,
§5 effort and churn counts); the two open decisions settled the same day against source (§1.1 record
the threshold on `CorpusGateRecipe`; §3 carry the real placement values). **No open decisions
remain.** Implements the **residual-abstention reporting** row in
[BACKLOG.md](../../BACKLOG.md) § *Donor registration leftovers*.

**Source:** residual probe reach / nominal floor anchor are deliberate (archived residual-gate
plans: post-aligner budget; headroom is chosen-vs-nominal). Reporting scope: name *which*
abstention fired and surface it outside the fingerprint schema — dump `floor_source` already
disambiguates absent vs measured floor.

**Deliverable:** a residual verdict that names *why* it carries no usable headroom reading, carried
into the repair path's own output — plus the `floor_source` correction that reporting depends on
(§1.4). **Reporting only — no gate-facing quantity moves.** Some serialized values *do* move, by
design; see §4.1.

**Media hygiene:** unchanged. No filenames, titles, or paths; corpus pairs by index only.

---

## 0. Problem

`SeamResidualVerdict.informative == false` is the only thing the repair path says when a residual
reading is unusable, and it covers four unrelated events:

1. the placement slid past the unified lag radius (`beyond_lag_reach()`),
2. no energetic, in-coverage A reference window existed within the walk horizon,
3. a window was found and the lag fit still produced nothing (non-finite probe),
4. the floor **was** measured on every measured side and simply sits above `floor_ok_db`.

The first three are abstentions — "we could not measure here". The fourth is a measurement, and its
answer is "B differs from A". A reader outside the fingerprint schema cannot tell them apart, and
33/17 plus 19 of the 21 gaps in §3.2 read `informative: false`.

### 0.1 What §7.3 already settled — do not re-open

- **Do not widen the residual lag reach.** It is a post-aligner budget; 600 ms is 60× the cost on a
  pipeline where gate search already dominates.
- **Do not re-centre the floor probe on the chosen placement.** Anchoring the floor at nominal is
  what makes headroom a measurement rather than a tautology (M6).
- **Do not touch the abstention itself.** `beyond_lag_reach()` is M5's safety valve against
  real-codec false vetoes. It stays exactly as it fires today.
- **"Abstained vs measured" is already answerable from a fingerprint dump** via `floor_source`. This
  plan is not that. It is naming *which* abstention, and getting it into production output.

### 0.2 Two things the source did not know

Found while reading the measurement path for this plan; both shape the design.

- **The multichannel summary destroys one of the three distinctions — and makes `floor_source` lie.**
  `side_worst_headroom_summary` returns `(NaN, NaN, SeamFloorSource::None)` when no channel has
  finite headroom. On multichannel media — the production path, via `from_channel_residuals` — a side
  that found reference windows and then failed the lag fit is indistinguishable from a side that
  never found a window. Cause (2) and cause (3) collapse into one. Worse, the collapse rewrites
  `floor_source` itself, so the field §7.3 relies on to explain an absent floor reports `none` for a
  side that *did* anchor one. Deriving the new reason before the collapse is necessary but not
  sufficient; §1.4 fixes the summary in the same pass.
- **Cause (4) is not an abstention, and today it is reported as one.**
  `classify_residual_band` maps `!informative || beyond_lag_reach()` to `ResidualBand::NoFloor`, so
  "we could not measure" and "we measured and B is not the same master" land on the same tag. Naming
  only the three abstentions would leave that fork unexplained one level up.

---

## 1. Design

### 1.1 The reason type

New in `domain/policies/seam_residual.rs`, next to `SeamFloorSource`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidualUninformative {
    BeyondLagReach,     // placement slide exceeded the lag radius (M5's abstention)
    NoReferenceWindow,  // no energetic in-coverage A window within `max_walk_frames`
    ProbeNonFinite,     // window found; the lag fit produced nothing
    FloorAboveOkDb,     // measured; the floor did not establish the same-master regime
}
```

**`FloorAboveOkDb` carries the same caveat as `DonorRelation::DiffCapture`, and its docstring must
say so.** The floor sitting above `floor_ok_db` means *the same-master regime was not established at
this seam* — not that B is proven a different master. A same-master pair yields it whenever it drifts
beyond the probe radius, or when the reference window is quiet enough that cancellation never gets
deep. Read it as "no cancellation evidence here", never as a provenance finding.

**`FloorAboveOkDb` is threshold-relative, and the threshold is a setting.** `residual_floor_ok_db` is
both TOML- and CLI-settable (`--residual-floor-ok-db`); `-15.0` is only the default, and
[gap-fill-modes.md](../gap-fill-modes.md) documents a `-50.0` variant. A stored `FloorAboveOkDb`
therefore means nothing to a later reader who does not know which threshold produced it — the same
class of defect this plan exists to fix.

**Settled 2026-08-05: record it.** `CorpusGateRecipe` — the dump's gate-provenance block, which
exists precisely to say what the `failure_stage` values were decided against — already carries
`residual_headroom_margin_db` and `residual_gate`, and does **not** carry `residual_floor_ok_db`.
The setting appears nowhere in the fingerprint schema, so today the threshold is unrecoverable from
a dump. Add it there, not per-gap on `ResidualInfo`: it is one value per run, exactly like the
margin beside it, and `PatchRequestSettings` already holds it so `from_settings` is a one-line
change.

Type it `Option<f64>` with `#[serde(default)]`, **not** a bare `f64` with a default-value function.
`CorpusGateRecipe`'s fields have no serde defaults today, so a bare field breaks every existing dump
that has a recipe — and defaulting to `-15.0` would silently assert the default was in force on runs
that may have used `-50.0`. `None` means "unrecorded, pre-dates this plan", which is the honest
reading and the same convention `floor_source_*` uses. The docstring still carries the caveat; the
recipe is what makes it checkable.

`Deserialize` is needed **only for `ResidualInfo`** (§3) — `SeamResidualVerdict` derives `Serialize`
alone. Deriving both on the enum is harmless and matches `SeamFloorSource`; it is called out here so
review does not churn on it.

**Named `ResidualUninformative`, not `ResidualAbstainReason`,** because the field must be *total* —
every `informative: false` verdict gets a reason, including the one that is not an abstention.
A field that explains three of four cases and is silent on the fourth reproduces the defect it was
written to fix. `FloorAboveOkDb` is the variant that says "this is not an abstention".

### 1.2 Per-side derivation, at measure time

The reason is a function of the probes already computed — no new measurement, no extra decode, no
extra lag search:

| Observed | Reason |
|---|---|
| `floor.source == None` | `NoReferenceWindow` |
| sourced, `floor.residual_db` non-finite | `ProbeNonFinite` |
| finite, `> floor_ok_db` | `FloorAboveOkDb` |
| finite, `≤ floor_ok_db` | `None` — the side is usable |

`BeyondLagReach` is **not** a side property; it belongs to the placement and is decided at the
verdict level (§1.3).

`SeamFloorProbe::none()` already collapses "no window" into `source: None` + `NaN`, and
`measure_a_win_at_delta` already preserves `source` when the fit fails — so the mono path can derive
all three from the probes it holds. **The multichannel path must derive the side reason from the
`&[SeamChannelResidual]` slice before `side_worst_headroom_summary` runs**, which is what fixes
§0.2's collapse. Reason follows the same channel `informative` does (best-cancelling / min-floor),
not the worst-headroom channel the scalars summarize — the field must explain `informative`, so it
must be read off the same channel.

### 1.3 Verdict shape

`SeamResidualVerdict` gains:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub uninformative_pre: Option<ResidualUninformative>,
#[serde(skip_serializing_if = "Option::is_none")]
pub uninformative_post: Option<ResidualUninformative>,
```

plus one method:

```rust
/// Verdict-level reason; `None` when the residual is usable.
pub fn uninformative_reason(&self) -> Option<ResidualUninformative>
```

**Combine rule.** The two sides can disagree — `FloorAboveOkDb` on one, `NoReferenceWindow` on the
other — so collapsing to one value needs an explicit priority. Both `uninformative_reason()` and
`GapTags.residual_uninformative` use it:

1. **`BeyondLagReach`** — a property of the placement, dominates both sides.
2. **The reason that actually failed `informative`, among the *measured* sides** —
   `FloorAboveOkDb` or `ProbeNonFinite`. When both sides failed with different reasons, prefer
   `ProbeNonFinite` (less measured).
3. **`NoReferenceWindow`** — only when *nothing* was measured.

Step 3 is last, not first, and that ordering is forced by `residual_verdict_informative`: unmeasured
sides are **ignored**, so a side with no reference window cannot by itself make a verdict
uninformative. If the other side was measured and failed, that failure is the reason; if the other
side was measured and passed, the verdict is informative and there is no reason at all.
`NoReferenceWindow` is only ever the answer when both sides went unmeasured — which is exactly the
case `residual_verdict_informative` returns `false` for on the "no side measured" branch.

A verdict with both sides usable returns `None`.

**`uninformative_reason()` is not the negation of `informative`, and the name hides that.** The gate
guard is a disjunction — `!informative || beyond_lag_reach()` — so a verdict can have
`informative == true` and still be unusable. Step 1 of the combine rule inherits that: the method
fires on beyond-reach verdicts whose `informative` is `true`. Fix the contract in the doc comment
rather than leaving the first reader to assume `is_some() == !informative`:

- The method answers **"why is there no usable headroom reading"** — it mirrors the *gate guard*, not
  the `informative` flag. That is the question every consumer in §2 is actually asking.
- The **combined value is authoritative for reporting**; the per-side fields are diagnostic detail
  and may legitimately disagree with it (a side reading `FloorAboveOkDb` under a combined
  `BeyondLagReach` is correct, not a bug).
- Consider `abstain_reason()` as the method name if `uninformative_reason()` still reads as the
  negation after the doc comment is written. The **field** names stay as in §1.1 — they are per-side
  and genuinely describe `informative`.

`skip_serializing_if` keeps clean measured gaps byte-identical on the wire, so existing goldens do
not churn.

**Unchanged, deliberately:** `informative`, `beyond_lag_reach()`, `worst_headroom_db()`,
`worst_chosen_db()`, `worst_floor_db()`, `residual_verdict_informative`, `floor_probe_informative`.
The hand-written `PartialEq` gains the two fields (they are `Option<enum>`, so no `NaN` handling).

### 1.4 Fix `floor_source` in the same pass — required, not optional

Deriving the reason before the collapse (§1.2) is **necessary but not sufficient**. If
`side_worst_headroom_summary` still rewrites sourced floors to `SeamFloorSource::None`, then
`floor_source_pre` / `_post` keep lying — in production output *and* in fingerprint dumps — on
precisely the case §7.3 trusts `floor_source` to explain. Shipping the new field while leaving that
in place would mean two fields disagreeing about the same side, with the older, more widely read one
wrong.

The collapse is also **broader than the multichannel non-finite case**. `side_worst_headroom_summary`
keys on `headroom = chosen − floor` being finite, so it returns `(NaN, NaN, None)` whenever no
channel has a finite *headroom* — which includes a channel with a **finite, sourced floor and a
non-finite chosen probe**. That is a side where the floor was genuinely measured, reported as though
no reference window was ever found.

**Fix:** when no channel yields finite headroom, fall back to a channel with a **sourced floor** and
report its `(floor_db, source)` rather than `(NaN, None)`. The fallback channel is the **min-floor**
channel — the same one the reason and `informative` follow — so all three agree. Only when no channel
has a sourced floor at all does the side report `None` — and then `NoReferenceWindow` is the honest
answer.

**All three summary values come from the fallback channel, `chosen_db` included.** The function
returns a triple that today is one channel's `(chosen, floor, source)`. If the fallback pulls
`(floor_db, source)` from the min-floor channel while `chosen_db` keeps the `NaN` the worst-headroom
path left behind, the emitted values straddle two channels — precisely the cross-channel mixing
`ResidualInfo`'s own docstring warns about for `informative` vs the scalars. In practice
`chosen_db` will usually still be non-finite (that is why the channel had no finite headroom), but it
must be *that channel's* chosen probe, not a leftover.

**Surfaces this moves** (all reporting; none gate-facing):

- `floor_source_pre` / `_post` on the verdict → production JSON and `ResidualInfo` dumps.
- `floor_pre_db` / `floor_post_db` → also `worst_floor_db()`, which feeds the `floor_db` scalar on
  patched/skip statuses in `patch_result.rs`.
- `worst_headroom_db()` is **unchanged**: headroom stays `NaN` on these sides either way, because the
  chosen probe is still non-finite and `worst_headroom_db` filters non-finite. The residual gate
  cannot see this change.
- `ResidualHeadroomExceeded`'s reported floors are unchanged: that path requires finite headroom, so
  the fallback never fires there.

### 1.5 Decision neutrality

Both readers of `informative` branch on the same expression:

- `apply_residual_to_confidence` — `!verdict.informative || verdict.beyond_lag_reach()` ⇒ return
  Pearson unchanged.
- `classify_residual_band` — the same guard ⇒ `ResidualBand::NoFloor`.

Neither expression changes. The new field is read by nothing that decides anything. `residual_band`
keeps its current three values — retagging `no_floor` into four bands would move a tag the goldens
and corpus history read, and §7.4 scoped this row to reporting.

**Neutrality is asserted on the gate-facing quantities, not on serialized output:** `informative`,
`beyond_lag_reach()`, `classify_residual_band`, and `apply_residual_to_confidence`'s
`FillConfidence` / `ResidualGateError`. Serialized output is **expected to change** under §1.4 — see
§4.1.

---

## 2. Production surfacing

This is the half §7.3 flagged as missing ("it never reaches production output").

| Surface | Change |
|---|---|
| `GapPatchOutcome.residual` | Free — it serializes the whole verdict |
| `GapTags` | New `residual_uninformative: Option<ResidualUninformative>`, set wherever `residual_band` is; makes a `no_floor` tag self-explaining |
| `format_gap_tags_verbose_line` | The "human residual line" is really the `-v` tag line (`residual_band=no_floor`), emitted via `log_gap_tags_verbose`. Append the reason **there** — there is no separate human residual line to patch |
| `log_residual_verdict_debug` | Add the field to the existing `tracing::debug!` |
| [json-output.md](../json-output.md) | **Add a `### SeamResidualVerdict` section** — `GapPatchOutcome.residual` links `#seamresidualverdict` today and the anchor does not exist, so the verdict's fields have never been documented. This writes that section (all fields, not just the new two), then adds the `GapTags` row and the enum's string values |
| [gap-repair-guide.md](../gap-repair-guide.md) | § Tag axes row; note that `residual_band` and the reason are read together |

---

## 3. Fingerprint parity — required, not optional

Add the same two fields to `ResidualInfo` in `application/gap_fingerprint/schema.rs` as
`Option` + `default`, so dumps written before this plan still deserialize.

**This was drafted as "optional, do last". It is not.** `application/gap_fingerprint/project.rs`
reconstructs a `SeamResidualVerdict` from a stored `ResidualInfo` on the replay path. Without the
new fields on `ResidualInfo`, every replayed verdict silently loses its reason — and replay is
exactly where §7.3 wants to read *why this abstained* after the fact. Two consequences:

- **The reason must be stored and read back, never recomputed from the reconstructed verdict.** That
  reconstruction hardcodes `placement_slide_frames: 0` and `max_lag_frames: 0`, so
  `beyond_lag_reach()` is **always `false` on the projection path**. Recomputing the combine rule
  there would skip step 1 and fall through to whatever measured-side reason ranks next — a
  plausible-looking wrong answer on the one abstention class §7.3 cares most about.
- **The hardcoded-zero placement fields are a pre-existing divergence, wider than this plan.** They
  also make `classify_residual_band` disagree between production and replay for beyond-reach gaps
  (production `NoFloor`; replay `Cancels` / `CorrelatesOnly` whenever `informative` is `true`).
  Naming it here rather than leaving it unsaid is the point — silence about exactly this kind of
  reconstruction gap is what let the §0.2 collapse survive.

**Settled 2026-08-05: carry the real placement values.** The zeros are not a measurement that was
never made — production has both values (`max_lag_frames` comes from
`params.derived.residual_max_lag_frames` where the verdict is built in `patch_region.rs`, and the
slide alongside it). They are dropped at the `ResidualInfo` boundary, because the write mapping in
`project.rs` never carried them. So this is a plumbing gap, not a reconstruction limit, and
documenting it as a known replay limitation would be documenting something we can simply fix.

**Copy the `floor_source_*` precedent exactly.** Those two fields were added to `ResidualInfo` on
2026-08-03 for the identical reason — the replay path was asserting a fixed value (`SeamFloorSource::None`)
on every gap because the dump didn't carry the real one — as `Option` + `default`, read back with
`unwrap_or(<old fixed value>)`, and commented at both ends to say `None` means the dump predates the
field. Add `placement_slide_frames: Option<i64>` and `max_lag_frames: Option<i64>` the same way,
with `unwrap_or(0)` on replay. Old dumps then behave exactly as they do today; new dumps replay
correctly. That also makes `beyond_lag_reach()` true on replay where it was true in production,
which is what combine-rule step 1 needs.

§3 stays a separate commit from §1–§2, and still lands after them.

---

## 4. Tests

| Test | Pins |
|---|---|
| One case per variant through `seam_chosen_and_floor` | The §1.2 table. The existing unit tests in `seam_residual.rs` already build every shape needed (quiet border, wrong-region B, oversized slide) |
| **Multichannel: windows found, B out of coverage** | Reports `ProbeNonFinite`, not `NoReferenceWindow`. **This fails on today's code** — it is the §0.2 regression. **New fixture, not a tweak** — see below |
| **Multichannel: sourced floor + non-finite chosen** | §1.4's broader case — the side reports its sourced `(floor_db, source)`, not `(NaN, none)`. Also fails today |
| Multichannel: reason follows the min-floor channel | §1.2's last paragraph — reason, `floor_source`, and `informative` describe the same channel |
| Combine rule | `BeyondLagReach` dominates; `FloorAboveOkDb` + `NoReferenceWindow` ⇒ `FloorAboveOkDb`; both-unmeasured ⇒ `NoReferenceWindow`; both-usable ⇒ `None` |
| Decision neutrality over a fixture set | The four gate-facing quantities in §1.5 — **not** serialized output |
| `worst_headroom_db()` invariance | Explicitly pinned across the §1.4 fix, since it is the one scalar the gate reads |
| Serde round-trip | Reasons round-trip snake_case; a clean measured gap's JSON is unchanged |

The row-1 claim that "the existing unit tests already build every shape needed" holds for the mono
shapes only. **The two multichannel rows need a fixture that does not exist yet:** the reference walk
must succeed while the B-side probe fails, which means a deliberately short `b_ch`. Every existing
test in `seam_residual.rs` builds B generously, so there is nothing to clone — budget a fresh builder
plus the usual iteration to get one channel non-finite and the other sourced. Treat these two rows,
not the enum, as the sharp end of Phase 1.

### 4.1 Expected golden changes

§1.4 is a correction, so some goldens **should** move and a diff there is the fix working:

- Multichannel gaps that recorded `floor_source: "none"` on a side that did find a reference window
  now record `border` / `walked`, with a finite `floor_*_db`.
- Anything derived from `worst_floor_db()` on those gaps — the `floor_db` scalar on patched/skip
  statuses — moves with it.

Review each moved golden against §1.4's rule before accepting: the side must have had a sourced
floor. A golden where `floor_source` changes on a side with **no** sourced floor is a bug in the
fallback, not a correction. Gaps with clean measured floors must not move at all.

**Which goldens, and how to re-bless them.** The affected sets are
`crates/clip-sync-repair/tests/gap_corpus/fingerprints/` (the `curated/` set plus `g003` and
`equivalence_divergence/band_donor.json`) and `crates/clip-sync-repair/tests/fixtures/full_surface_repair.json`.
The likeliest movers are the two donor-broken curated cases. Confirm the regeneration command from
[gap-fingerprint.md](gap-fingerprint.md) at execution time and record it in the commit message.

**Do not blanket-accept the regeneration.** Every moved `floor_source: "none" → "border" | "walked"`
gets eyeballed against the rule above, because the same regeneration that re-blesses the goldens is
what was supposed to prove the fix. A wholesale re-bless would rubber-stamp §1.4 with its own output.

---

## 5. Execution checklist

Fill `file:line` in at execution time, not before (dev README rule).

**Effort:** roughly a day for Phase 1 (most of it the two new multichannel fixtures, §4), half a day
for Phase 2, and about a day for Phase 3 — it now carries three schema additions (the reason, the
two placement fields, the recipe threshold), each needing a back-compat deserialize test, though all
three follow the `floor_source_*` pattern already in the file.

**Mechanical churn — small and bounded**, counted while reviewing this plan. Struct literals needing
a new field: **~6** `GapTags` sites (three in `domain/gap_tags.rs`, one in the anchor-retry path, two
in `gap_tags.rs` tests, one in `integration_residual_gate_smoke.rs`), **2** `SeamResidualVerdict`
literals (a test helper in `gap_fill_fit.rs`, and the §3 replay reconstruction), **5** `ResidualInfo`
literals (two in `measure.rs`, one in `project.rs`, two in `schema.rs`). Counts, not line numbers —
re-locate at execution time.

**Phase 2 is cheaper than its row count suggests.** `GapTagContext` already carries the verdict —
`residual_band_tag()` calls `classify_residual_band` on it directly — so the new tag is a sibling
method with no plumbing. Phase 2 is the *easy* phase; Phase 3 (§3) is the one with a real decision in
it, which is the opposite of how the original draft ordered the risk.

**Phase 1 — domain**

- [x] Add `ResidualUninformative` to `crates/clip-sync-repair/src/domain/policies/seam_residual.rs`, next to `SeamFloorSource`; re-export from `domain/policies/mod.rs` if `SeamFloorSource` is.
- [x] Add a private `side_uninformative(floor: &SeamFloorProbe, floor_ok_db: f64) -> Option<ResidualUninformative>` implementing §1.2.
- [x] Add a `&[SeamChannelResidual]` variant that reads the min-floor channel, mirroring `side_floor_informative`.
- [x] **Fix `side_worst_headroom_summary` per §1.4** — fall back to a sourced-floor channel's `(floor_db, source)` when no channel has finite headroom.
- [x] Add the two fields to `SeamResidualVerdict`; populate in `from_parts_with_placement` and `from_channel_residuals`; extend the hand-written `PartialEq`.
- [x] Add `uninformative_reason()` with §1.3's combine rule, and the doc comment that says it mirrors the **gate guard**, not `!informative` (§1.3).
- [x] Docstring `FloorAboveOkDb` with the §1.1 caveat (mirror `DonorRelation::DiffCapture`'s wording) **and** the threshold-relativity note.
- [x] Unit tests per §4 rows 1–5; `worst_headroom_db()` invariance.

**Phase 2 — production output**

- [x] `GapTags.residual_uninformative` in `domain/gap_tags.rs`, populated at every `classify_residual_band` call site.
- [x] `format_gap_tags_verbose_line` appends the reason alongside `residual_band` (the `-v` line, via `log_gap_tags_verbose`).
- [x] `log_residual_verdict_debug` in `application/patch_region.rs` gains the field.
- [x] Decision-neutrality test on the four gate-facing quantities (§1.5); re-bless goldens per §4.1, checking each moved side had a sourced floor.
- [x] Write the missing `### SeamResidualVerdict` section in `docs/json-output.md` (the `#seamresidualverdict` link at `GapPatchOutcome.residual` is dangling today); add the `GapTags` row and enum values; update `docs/gap-repair-guide.md` § Tag axes.

**Phase 3 — fingerprint (separate commit, required)**

- [x] `ResidualInfo` fields as `Option` + `default`; back-compat deserialize test against a pre-2026-08-05 fixture.
- [x] Carry the reason through `project.rs`'s **write** mapping (verdict → `ResidualInfo`) **and** its **replay** mapping (`ResidualInfo` → `SeamResidualVerdict`) — read the stored value, never recompute it (§3).
- [x] `ResidualInfo.placement_slide_frames` / `max_lag_frames` as `Option<i64>` + `default`; populate from the verdict in the write mapping; `unwrap_or(0)` on replay, replacing the hardcoded zeros (§3). Mirror the `floor_source_*` comments at both ends.
- [x] Replay test: a from-decode dump whose production verdict was beyond-reach round-trips to `beyond_lag_reach() == true` (fails today), and a pre-field fixture still lands on `0` / today's behaviour.
- [x] `CorpusGateRecipe.residual_floor_ok_db: Option<f64>` + `#[serde(default)]`, populated in `from_settings`; back-compat test that an existing recipe without the key still deserializes to `None` (§1.1).

**Close-out**

- [x] Strike the BACKLOG row; note the durable behaviour in [json-output.md](../json-output.md).
- [x] Archive this plan per [README.md](README.md) § Plans.

---

## 6. Risks

| Risk | Handling |
|---|---|
| A future reader gates on the new field | Doc comment says report-only; nothing in `gap_fill_fit.rs` or `gap_tags.rs` reads it |
| Golden churn | `skip_serializing_if` ⇒ clean gaps unchanged. §1.4 moves goldens deliberately — §4.1 says which and how to review them; the neutrality test guards the quantities that must *not* move |
| §1.4's fallback masks a genuinely absent floor | The fallback requires a **sourced** floor; a side with none still reports `none`. Pinned by the both-unmeasured ⇒ `NoReferenceWindow` test |
| `FloorAboveOkDb` read as a provenance finding | Docstring carries the `DiffCapture` caveat (§1.1): it means the regime was not established, not that B is a different master |
| Reason, `floor_source`, and `informative` disagree on multichannel | §1.2 / §1.4 pin all three to the min-floor channel; one test pins it |
| `uninformative_reason()` read as `!informative` | §1.3 fixes the contract in the doc comment (it mirrors the gate guard); the name may change to `abstain_reason()` |
| Replay reports the wrong reason | §3: the reason is **stored**, never recomputed; and the placement fields it depends on are now carried instead of hardcoded to `0` |
| `FloorAboveOkDb` unreadable later | §1.1: `CorpusGateRecipe` gains `residual_floor_ok_db` as `Option<f64>`, so the threshold travels with the dump; `None` reads as "unrecorded", never as "the default was used" |
| Golden re-bless rubber-stamps §1.4 | §4.1: each moved `floor_source` is checked against the sourced-floor rule by hand, not accepted wholesale |
| Scope creep into the probe | §0.1 lists the three killed changes; none of them is touched here |
