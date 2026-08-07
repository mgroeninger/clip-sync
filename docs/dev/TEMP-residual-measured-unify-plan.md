# TEMP — Unify residual “measured” semantics (mono ↔ multichannel)

**Status:** Phase 0–2 done 2026-08-06 (§5.1 toward MC); Phase 3+ pending. Working plan for
resolving the mono/multichannel disagreement on what counts as a *measured* residual floor —
the gate-facing leftover from
[archive/TEMP-residual-abstention-reporting-plan.md](archive/TEMP-residual-abstention-reporting-plan.md).

Companion: [BACKLOG.md](../../BACKLOG.md) § *Residual gate follow-ups* (row **Mono/multichannel
disagree on "measured"**), `domain/policies/seam_residual.rs`
(`residual_verdict_informative`, `side_floor_informative`, `gate_abstains`,
`uninformative_reason`), [gap-fingerprint.md](gap-fingerprint.md) § *Gate recipe*,
[`residual_gate_catalog/`](../../crates/clip-sync-repair/tests/residual_gate_catalog/).

**Deliverable:** one shared measuredness predicate and combine rule for
`SeamResidualVerdict::informative`, chosen only after a corpus census of the asymmetric cell.
Reporting vocabulary stays; the gate may move in one direction.

**Media hygiene:** unchanged. No filenames, titles, or paths; corpus pairs by index only.

---

## 0. Problem

A side whose floor probes are **sourced but non-finite** (reference window found, lag fit
failed → `ResidualUninformative::ProbeNonFinite`) produces opposite gate behaviour depending
on the constructor path:

| Path | “Measured” means | Sourced + non-finite side |
|------|------------------|---------------------------|
| Mono (`residual_verdict_informative` via `from_parts*`) | `source != None` | Counts as measured → fails → `informative: false` → `gate_abstains` → **veto lost** |
| Multichannel (`side_floor_informative` via `from_channel_residuals`) | `source != None && residual_db.is_finite()` | Dropped like unmeasured → other side may govern → **veto can fire** |

Same physical event, opposite decisions. Multichannel is the stricter reading (veto survives).
Even a one-channel `from_channel_residuals` can disagree with mono `from_parts` on the same
probes. The pin `uninformative_reason_is_exactly_the_gate_guard` only asserts
`reason.is_some() == gate_abstains()` *within* each path — not cross-path agreement.

### 0.1 What already shipped — do not re-open

From the abstention-reporting plan (archived 2026-08-05):

- **`gate_abstains()` is the authority.** Both gate readers
  (`apply_residual_to_confidence`, `classify_residual_band`) call it. Do not route decisions
  through `uninformative_reason().is_some()` again — that widened the guard past
  `!informative || beyond_lag_reach()` and suppressed a live `ResidualHeadroomExceeded` veto
  (headroom +10 dB vs 6 dB margin).
- **`uninformative_reason()` only *names* the guard.** Dependency runs one way: naming
  vocabulary cannot move a gate decision. Per-side `uninformative_pre` / `_post` remain
  diagnostic detail and may disagree with the combined value.
- **`FloorAboveOkDb` is already aligned** on both paths (measured failure). The only
  asymmetric cell is **exactly one side `ProbeNonFinite`, other side regime-OK**.
- **Do not widen lag reach, re-centre the floor, or rename abstention variants** as part of
  this work (§0.1 of the abstention plan still holds).

### 0.2 Why this is not a reporting change

`uninformative_reason` already names `ProbeNonFinite` on both paths. The backlog row was
filed because unifying *gate* semantics moves veto application one way or the other and
needs corpus counts first — same spirit as the donor-registration “count first, don’t change
the gate” row.

---

## 1. Goal / non-goals

**Goal**

- One definition of “measured” and one `combine_informative(pre, post)` used by both
  constructors.
- Cross-path agreement: same floor probes → same `informative`, `gate_abstains()`, and
  combined reason.
- Direction chosen from a written census + decision memo (§3–§4), not from preference alone.

**Non-goals**

- Changing `ResidualUninformative` variants or wire names.
- Routing the gate through reporting fields.
- M3 walk/OOB, G1 Pearson-skip residual, L6 walk step — related to *why* ProbeNonFinite
  appears, not to unifying measuredness. If the census shows ProbeNonFinite is mostly
  B-haystack OOB, fix M3 first and re-count before changing the gate (§6).
- Fingerprint schema churn beyond what the census already reads.

---

## 2. Root cause (one predicate)

```text
Mono measuredness:  source != None
MC measuredness:    source != None && residual_db.is_finite()
```

`side_uninformative` / `side_uninformative_channels` correctly *name* ProbeNonFinite on both
paths. `informative` then applies different filters:

- Mono: sourced-NaN is “measured but failed” → blocks informative when the other side is OK.
- MC: sourced-NaN never enters `side_floor_informative` → `None` → ignored via
  `unwrap_or(true)` → other side governs.

`asymmetric_multichannel_side_does_not_widen_the_gate_guard` encodes today’s MC behaviour on
purpose (do not let a ProbeNonFinite side reach the combined reason and kill a live veto).

---

## 3. Phase 0 — Freeze & name the asymmetry

**Gate does not move.** **Done 2026-08-06.**

1. Document the two measuredness definitions in one place (doc comment on
   `residual_verdict_informative` / `side_floor_informative`, pointing at this plan).
2. Add an explicit unit that **asserts the cross-path disagreement** on
   `(sourced-NaN, deep-floor)` so the asymmetry cannot be “fixed” by accident during
   unrelated cleanup. Keep existing pins:
   - `uninformative_reason_is_exactly_the_gate_guard`
   - `asymmetric_multichannel_side_does_not_widen_the_gate_guard`

**Exit:** disagreement is visible in tests and docs; no production behaviour change.
Pin: `mono_and_multichannel_disagree_on_sourced_nan_measuredness`.

---

## 4. Phase 1 — Corpus census (decision input)

**Question:** how often does the asymmetric cell appear, and when it does, was a live veto
at stake?

Prefer **dump-only** over existing fingerprint / repair JSON that already carries
`uninformative_pre` / `_post`, `informative`, `floor_*_db`, `chosen_*_db`, recipe
`residual_floor_ok_db`, and headroom margin. No re-decode unless dumps lack the fields
(pre-2026-08-05 dumps omit `uninformative_*` — exclude or re-measure those).

| Bucket | Definition |
|--------|------------|
| **A** | Exactly one side `probe_non_finite`, other side usable (`None`) |
| **A∩veto** | Bucket A and finite headroom would exceed the run’s margin |
| **B** | Both sides `probe_non_finite` |
| **C** | `probe_non_finite` + `floor_above_ok_db` |
| Split | Mono-downmix / analysis dumps vs true multichannel production pairs |

Script: [`scripts/census-residual-measured.ps1`](../../scripts/census-residual-measured.ps1)
over a dump directory (fill-level / patch-outcomes / fingerprint corpora). Emit pair-index +
gap-index counts only (media hygiene).

```powershell
./scripts/census-residual-measured.ps1 -DumpDir gap-files/2026-08-05-fill-level
./scripts/census-residual-measured.ps1 -DumpDir gap-files/2026-08-05-fill-level -CsvOut gap-files/census-residual-measured.csv
```

**Exit:** tabulated counts for A, A∩veto, B, C with channel-layout split. **No gate change.**
If A is empty across the available corpus, record that and keep the Phase 0 pin until a
broader dump says otherwise — do not “unify” on zero evidence.

---

## 5. Phase 2 — Written decision before code

| Census outcome | Direction |
|----------------|-----------|
| A rare; A∩veto mostly real echo / bad match | **Unify toward multichannel** — treat `ProbeNonFinite` like `NoReferenceWindow` (ignore; other side may govern). Production already does this; mono becomes stricter (more vetoes survive). |
| A∩veto often false / geometry-edge / B OOB (M3-adjacent) | **Unify toward mono** — sourced-non-finite is a measured failure; MC becomes softer (more abstentions). Safer when ProbeNonFinite means “don’t trust this seam.” |
| A material but mixed | Prefer still converging. If not, keep asymmetry **explicit** via a documented three-state policy enum — not two accidental filters — and say why in the decision memo. |

**Lean (confirmed by census):** multichannel semantics.

- Production repair residual is multichannel (`seam_chosen_and_floor_multichannel`).
- The abstention plan already refused to let ProbeNonFinite widen the guard and kill a live
  veto.
- A regime-OK side is real cancellation evidence; ignoring a failed fit on the other side
  matches how unmeasured sides are already ignored.

**Exit:** a short decision memo in this file (§5.1 when filled) naming the chosen direction
and the counts that justified it. Implementation does not start without it.

### 5.1 Decision memo

| Field | Value |
|-------|-------|
| Date | 2026-08-06 |
| Corpus / dump set | `gap-files/2026-08-05-fill-level` (39 pair JSON, `--patch-only`; census via `scripts/census-residual-measured.ps1`, margin 6 dB) |
| Counts (A / A∩veto / B / C) | **10 / 5 / 77 / 31** over **227** eligible gaps (A = 4.4%, A∩veto = 2.2%) |
| Chosen direction | **toward MC** |
| Rationale | A is uncommon, not empty. All five A∩veto rows are the non-dual-fit (MC) path: `informative: true`, finite headroom 16–18 dB ≫ margin — live veto stake on a regime-OK shoulder. The five A-without-veto rows are mostly `dual_fit_used` (mono `from_parts`): already abstaining with NaN headroom, so no veto to preserve either way. B is large (geometry / fit failure on both sides) but orthogonal to the asymmetric cell. Matches the first decision-table row and the prior lean: ignore sourced-NaN like unmeasured; do not let it kill a veto the other side supports. |

**A / A∩veto index list** (pair, gap_index):

| pair | gap | bucket | informative | headroom | dual_fit |
|------|-----|--------|-------------|----------|----------|
| 5 | 2 | A∩veto | true | 17.5 | — |
| 5 | 3 | A∩veto | true | 17.2 | — |
| 5 | 5 | A∩veto | true | 17.6 | — |
| 7 | 1 | A∩veto | true | 16.6 | — |
| 34 | 11 | A∩veto | true | 18.5 | — |
| 23 | 11 | A | false | NaN | true |
| 30 | 2 | A | false | NaN | true |
| 36 | 8 | A | false | NaN | true |
| 39 | 5 | A | false | NaN | true |
| 39 | 6 | A | true | NaN | — |

Split: all eligible gaps were 6-ch; A∩veto only on `dual_fit` absent; A-without-veto dominated by `dual_fit=true`.

---

## 6. Phase 3 — Shared architecture (only after §5.1)

Introduce one side-state helper used by both constructors:

```text
Unmeasured | ProbeFailed | RegimeFailed | RegimeOk
```

Derived from the same rules as today’s `side_uninformative` / channel min-floor read.

- **One** `combine_informative(pre, post)` implements the §5.1 policy.
- Mono `from_parts*` and MC `from_channel_residuals` both call it — delete the dual
  measuredness tests.
- Keep `gate_abstains()` → `uninformative_reason()` one-way.
- Per-side reasons still name `ProbeNonFinite` when the combined verdict stays informative
  (MC-style), if that is the chosen policy.

Suggested shape (names illustrative):

```rust
enum SideFloorState {
    Unmeasured,   // NoReferenceWindow
    ProbeFailed,  // ProbeNonFinite — policy decides ignore vs fail
    RegimeFailed, // FloorAboveOkDb
    RegimeOk,
}

fn side_floor_state(/* probe or min-floor channel */, floor_ok_db: f64) -> SideFloorState;

fn combine_informative(pre: SideFloorState, post: SideFloorState) -> bool;
```

`FloorAboveOkDb` / `RegimeFailed` stays a measured failure on both paths regardless of
direction.

**Exit:** both constructors share one combine; production behaviour matches §5.1.

---

## 7. Phase 4 — Pins & catalog

1. **Cross-path agreement:** same probes → same `informative` / `gate_abstains` / combined
   reason (replace the Phase 0 “assert disagreement” test).
2. **Retarget** `asymmetric_multichannel_side_does_not_widen_the_gate_guard` as the *shared*
   policy test (or its mono dual if §5.1 goes toward mono).
3. Keep `uninformative_reason_is_exactly_the_gate_guard` across every shape.
4. Optional RG catalog case only if a fixture reproduces A∩veto on real-ish geometry.
5. Close the BACKLOG row; archive this plan; durable note in
   [gap-fingerprint.md](gap-fingerprint.md) § *Gate recipe* (one sentence on measuredness).

**Exit:** suite green; backlog row closed; this file archived.

---

## 8. Related backlog — do not fold in

| Item | Relation |
|------|----------|
| **M3** — floor walk vs B haystack OOB | May *cause* ProbeNonFinite. If census attributes A mostly to OOB, fix M3 and re-count before unifying. |
| **G1** — residual on Pearson-only skips | Orthogonal coverage gap. |
| **L6** — coarse outward walk step | Floor geometry; recalibrate if touched. |
| **`finale_floor_nan_probe`** | M3-adjacent unit repro; useful census fixture, not this plan’s DoD. |

---

## 9. Effort & sequencing

| Phase | Effort | Gate moves? |
|-------|--------|-------------|
| 0 — freeze / pin disagreement | small | no |
| 1 — census script over dumps | small–med | no |
| 2 — decision memo | small | no |
| 3 — shared predicate + combine | med | **yes** (one direction) |
| 4 — pins / catalog / archive | small–med | no (locks the move) |

**Order is strict:** 0 → 1 → 2 → 3 → 4. Skipping the census to “just pick MC” is out of
scope for the same reason the backlog forbade a reporting-only “fix.”

---

## 10. Verification checklist *(execute when implementing)*

- [x] Phase 0: cross-path disagreement unit + doc comments
- [x] Phase 1: census table committed or attached to §5.1 (pair/gap indices only)
- [x] Phase 2: §5.1 filled; direction named
- [ ] Phase 3: single `SideFloorState` / `combine_informative`; both constructors call it
- [ ] Phase 4: cross-path agreement pin; retargeted asymmetric test; guard ≡ reason still holds
- [ ] `cargo test -p clip-sync-repair` (unit + residual gate smoke) green
- [ ] BACKLOG row closed; this plan archived; `gap-fingerprint.md` Gate recipe note

---

## 11. Open decisions

None — §5.1 chose **toward MC**. Phase 3 may proceed.
