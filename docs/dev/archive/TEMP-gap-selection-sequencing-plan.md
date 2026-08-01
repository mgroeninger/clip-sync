# Gap selection — sequencing (ARCHIVED)

Status: **archived 2026-07-29.** Meta doc (order + scope fence). Thin selection v1 and v1.5 ranges
shipped and archived ([TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md),
[TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md)). Recipe stays parked.
Feature semantics stay in those sibling plans — do not treat this file as current behavior.

**Why this existed.** Preparing [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) and the
selection siblings kept surfacing further adjacent defects (stale source claims *and* real nearby
bugs). That pattern is a sequencing smell: the program was settling the whole gap-identity /
provenance stack while the stated user need is a plan-time subset filter. This doc recorded the
chosen order and the hard scope rule so the implementation PR did not re-absorb that stack.

**Rejected alternative (do not reopen):** recipe-first (`ScanRecipe` on `GapReport` + JSON echo
before `--only-gaps`). That pays provenance / `PartialEq` cost for a deferred consumer
(`--gaps-from`). Revisit only by **unparking** [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md)
when a real same-recipe consumer exists — not by restoring a parallel “path” here.

**Siblings (one deliverable each; each owns a complete checklist):**
[TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md) (v1 — **archived**),
[TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md) (v1.5 — **archived**),
[TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) (recipe type — **parked**),
[TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md) (`--scan-window`, `--gaps-from`).

---

## 1. Decision

**Ship thin selection v1 next. Park `ScanRecipe` until a real consumer needs recipe equality.**

Rationale: v1 has **no code dependency** on the recipe type; blocking on it pays provenance design
cost for a deferred consumer (`--gaps-from`). Selection’s §8 checklist is complete for that
deliverable — recipe is a separate parked plan, not an unfinished slice of selection.

| If primary pain is… | Do |
|---------------------|----|
| “Patch 1,2,4,5; retry 3 with different flags” | **Selection v1** (done) |
| “Script must refuse a stale saved gap list” / building `--gaps-from` soon | Unpark **recipe**, then manifest |
| “`#` must survive recipe edits” | **v1.5 ranges** — **chosen next** (2026-07-29) |

---

## 2. After thin v1 lands (expected tree)

| Surface | Change |
|---------|--------|
| CLI / TOML | `--only-gaps` / `--skip-gaps` (and TOML peers); mutual exclusivity |
| Fill plan | `GapNotSelected` / `gap_not_selected`; filter note on stderr |
| JSON | New `plan_skip_reason` value only (plus selection-error stdout suppression) |
| `GapReport` / scan request | **Unchanged** — still flat `scan_block_ms` / `silence_peak_fraction`; missing knobs still absent from JSON |
| Corpus | `complete_recipe` / `from_report` back-fill **still present** |
| `format_scan_summary` RMS `{:.0}` | **Not fixed** unless spun out (see §4) |
| Docs | Operator workflow for subset patch; `#` remains run-scoped per [gap-vocabulary.md](../gap-vocabulary.md) |

Later work (each on its own plan’s checklist): v1.5 ranges after v1; recipe when a consumer needs
`PartialEq`; `--gaps-from` needs recipe (+ typically ranges). Same eventual feature set — different
time-to-operator-value, and the next PR must not absorb adjacent scan/provenance debt.

### Sibling status (at archive)

| Doc | Status claim |
|-----|--------------|
| [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) | **Parked until a consumer**; not a gate on selection |
| [TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md) | **v1 archived** (shipped + promoted) |
| [TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md) | **v1.5 archived** (shipped) |
| [TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md) | Unchanged; `--gaps-from` remains the recipe’s main consumer |

---

## 3. Concrete next steps

Ordered. Stop when the operator can subset-patch; do not start recipe work in the same PR.

### Step 0 — Commit the sequencing (docs) — done

- [x] Land this document; update [README.md](../README.md) / [BACKLOG.md](../../../BACKLOG.md) active-plan rows to show **selection v1 next**, recipe **parked**
- [x] Recipe plan status: parked until consumer (`--gaps-from` or a script that needs same-recipe equality)
- [x] Selection plan §8: no scan-recipe prerequisite; §8 is complete for v1
- [x] Drop recipe-first as a live alternative in this file (short rejected note only)

### Step 1 — Implement thin v1 (one PR) — done

Implement [TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md) **§8 as written**. That checklist is
the full v1 deliverable; ticking it means the selection plan is done for v1 (promote per its §11).
Recipe is **not** an excepted leftover — it lives only on the parked recipe plan.

Summary (detail and tests in selection §8–§9):

- [x] Config + CLI flags (`Vec<String>` tokens; integers only at resolve)
- [x] `GapSelection` / `GapSelectionMode` / `resolve_gap_selection`
- [x] `GapFillSkipReason::GapNotSelected` + formatters / tags
- [x] `build_gap_fill_plan(..., selection)` + call-site updates
- [x] Wire resolve in `run_repair.rs`; selection on `PatchAudioRequest`
- [x] Empty-selection → exit 2; JSON stdout suppression on selection error (explicit signal)
- [x] Filter note; docs; integration test (`--only-gaps 2` leaves others on A)

**Hard scope rule for this PR:** if prep re-discovers an adjacent defect (RMS floor display, missing
JSON scan knobs, `limit_fill_to_mapped_region` on the report, corpus back-fill), **do not fold it in**.
File it under §4 or [BACKLOG.md](../../../BACKLOG.md) Open work and continue.

### Step 2 — After v1 ships — done (2026-07-29)

Executed as **audit residuals + status flip + archive meta** (most operator prose landed with Step 1):

- [x] Residual user-facing contract into [gap-repair-guide.md](../../gap-repair-guide.md) /
      [cli-output.md](../../cli-output.md) / [gap-vocabulary.md](../gap-vocabulary.md) per selection
      plan §11 (identity-not-count, empty asymmetry, precedence, filter note, flag/TOML table)
- [x] Re-ask which pain is next (§1 table): **v1.5 ranges** chosen
- [x] Archive **this** sequencing file (feature work continues only in sibling TEMP docs)

### Step 3 — Unpark recipe only with a consumer

When unparking, use [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) as written. Trigger examples:

- Implementing `--gaps-from`
- An external script that must compare saved scan knobs to the next run and cannot tolerate a
  hand-rolled five-field check drifting

Optional thinner interim (only if a script needs knobs *before* a type): add the three missing **flat**
JSON fields without introducing `ScanRecipe`. Prefer the type once equality matters.

---

## 4. Adjacent debt — park, do not absorb

Discovered during recipe/selection prep; **not** required for thin v1. Survives in
[BACKLOG.md](../../../BACKLOG.md) Open work (§ Gap-selection parked debt):

| Item | Note |
|------|------|
| `format_scan_summary` RMS floor `{:.0}` on normalized ≈ `0.001` → prints `0` | Real display bug; one-line fix + test rebase; own tiny PR or BACKLOG |
| JSON missing `min_gap_ms` / `silence_hold_ms` / `absolute_silence_rms` | Provenance symptom; recipe plan or flat-echo interim |
| Corpus `from_report` hardcodes `None` + `complete_recipe` back-fill | Deleted by recipe plan when unparked |
| `limit_fill_to_mapped_region` living on a scan report | Wrong home; recorded in recipe plan §5 “not in scope” |

Rule: audit findings become **separate** work items unless they block the current step’s checklist.

---

## 5. Done criteria for *this* document

- [x] Thin v1 chosen; recipe-first rejected (short note only)
- [x] Sibling status lines match (§2)
- [x] Selection v1 implementation PR landed (selection §8 complete)
- [x] This file archived; Active plans / README no longer list it as a live fork
