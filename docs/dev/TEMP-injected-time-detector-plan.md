# TEMP — Pre-patch timeline-mismatch warning

**Status:** draft, 2026-09-08. Design, not started. Production warning,
not fingerprint-only, not a per-gap diagnostic dump.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Post-join defects (pairs 31
and 50)*.

**Predecessor (context, not superseded):**
[archive/TEMP-constellation-fingerprint-plan.md](archive/TEMP-constellation-fingerprint-plan.md)
§ 11 item D showed, from existing dumps with no re-scan, that a *run*
of consecutive gaps can carry large ungated pre/post shoulder-lag
disagreements (the corpus examples are pair 50 gaps 9–17, deltas
273.8–1040.3 ms, and pair 31 gaps 7–8). That is not what `--dual-fit`'s
`placement_slide` already measures (its confidence gate, `peak_r >=
~0.35`, is `None` on exactly the gaps where the divergence is loudest)
and not what `anchored-retry`'s `PatchOffsetAnchor` carries (single
scalar, `Patched`-only). This plan is that observation turned into a
**file-level warning that fires before the patch loop**, not a
corpus-pair matcher and not a `GapTags` field dump.

**Companions:**
[TEMP-disjoint-offseam-lock-plan.md](TEMP-disjoint-offseam-lock-plan.md)
(one shoulder, one gap, off-seam lock — complementary, not this
warning).

**Media hygiene:** pair index + gap index only. Corpus pairs are
existence proofs and negative controls for (T, N). They are not the
user-facing identity of the warning.

---

## 0. What this tests (normative)

**Before the patch search runs, can a cheap ungated pre/post
shoulder-lag read at each gap's alignment-mapped (`b_mapped`)
position, followed by a cross-gap sequence pass, produce a single
file-level warning that this pair appears to have a timeline mismatch
which can make filling gaps significantly harder — distinctly from an
isolated single-gap seam defect, and without naming which file carries
the extra or missing time?**

That is the whole plan. Later sections only implement it. If a later
sentence disagrees with this one, this one wins.

"Cheap" means `lag_pair`'s existing pair of bounded (±600 ms nominal,
±2000 ms envelope-rescued) correlation sweeps, one per shoulder, over
already-decoded ~1 s borders — the same measurement Full-tier
`lag_at_placement` already performs at `b_mapped` (`build.rs`:
"Decision-seam lag at `b_mapped` nominal … not structure throat"), not
`place_on_b` and not `characterize_region`'s unified search.
"Unconditionally" means without `registration_shoulder_residual_ms`'s
confidence floor. "Sequence pass" is the product, not a follow-up:
one isolated spike is `--dual-fit`'s job when its gate can fire; the
warning's value is a *run*. "Before the patch search" means after A+B
PCM is in hand and before the characterize loop; a mid-run `phase()`
so the operator can abort, plus the same warning on the final stdout
report.

This plan does **not** test: rewiring `--dual-fit`, changing
`shoulders_abut`, aborting or skipping the patch loop automatically,
stamping per-gap lag fields onto `GapTags` / the v1 JSON gap rows,
fixing any fill, or identifying a named corpus pair. If the sequence
pass cannot be made honest at some (T, N), the plan is refuted — it
does not fall back to shipping per-gap diagnostics.

---

## 1. Population (the customer)

Every gap in the fill plan that has a mapped B span, once `PatchAudio`
has decoded A and B and before any region is characterized.
Disposition is unknown (the search has not run); the sequence pass
needs a lag reading on every planned gap or a real run cannot be told
apart from noise at its edges.

This is not the constellation plan's remaining-skip-shoulder
population, and it is not "gaps that `characterize_region` has already
placed." Scan-only (`dry_run` with no preview/write) is **out of
scope** for this Change: that path never holds A+B PCM. A later
scan-time retained-window pass could warn on dry-run too; it is not
this plan.

---

## 2. Design rules (normative)

1. The lag measurement is a pure extraction of existing code (`lag_pair`,
   `lag_side_sweep`, `LagSweepParams`, `summarize_lag_curve`,
   `rescue_shoulder`) from
   `application/gap_fingerprint/measure/lag.rs` into `domain/`,
   following `seam_local` / `lag_envelope` (same reason: one
   implementation for the fingerprint dump and this production
   pre-pass). No new correlation math.
2. The extracted call is **unconditional** at `b_mapped` for every
   planned gap. Not gated behind `DetailTier::Full`, `--dual-fit`, a
   confidence floor, or `characterize_region`. Cost class: Full-tier
   `lag_decision` (bounded ±600 ms over ~1 s in memory), paid once per
   gap in a loop that finishes **before** the characterize loop.
3. The user-facing deliverable is **one run-level warning**, not a
   per-gap field set. Per-gap `pre`/`post`/`delta`/`peak_r` exist only
   as internals the sequence pass consumes; they do not land on
   `GapTags` and they do not land on v1 JSON gap rows in this Change.
4. The sequence pass **is** this plan (not a later Change). It does
   not ship without a fixture- and corpus-pinned (T, N). Isolated
   single-gap spikes must not warn.
5. Nothing in this plan changes what `shoulders_abut` means, changes
   `--dual-fit`'s gate, adds a CLI flag, or stops the patch loop. Warn
   and continue. No new flag: after decode the extra cost is the
   `lag_decision` class, always on for preview/write.
6. (T, N) are not pre-decided. Conservative: prefer a miss over a
   false warning. Whole-file `offsets_consistent` / `offset_drift`
   already cover a step that lasts to the end clip; this warning's
   extra job is a *bounded island* (and localizing that the trouble
   sits on a run of holes). Do not duplicate the existing alignment-
   instability warning ([cli-output.md](../cli-output.md) § Alignment
   instability) with different words for the same fact.
7. User-facing copy names a **file flaw that can make filling gaps
   harder**. It does not name a corpus pair, a map-step size, or
   which file is wrong.

### 2.1 Sequence pass (resolved in § 6/§ 7 before any production wiring)

A run of ≥ N consecutive planned gaps where each gap's
`|post_frac_lag_ms − pre_frac_lag_ms|` exceeds T is one warning for
the pair. T and N come from § 6. Not a single-gap threshold; not a
claim about A vs B.

Expected interior signature when the mismatch is **outside** the
lag window (the corpus shape that justifies the warning): both
shoulders correlate against the wrong B content, so ungated delta
stays elevated **throughout** the mis-mapped span, not only at its
two edge gaps. An in-window constant offset that only lights the two
edges is case 2's cousin — not this warning.

---

## 3. Fixture (synthetic, before any production wiring)

Prove the extracted `lag_pair` still passes `lag.rs`'s existing unit
tests unchanged (pure move, § 2 rule 1).

New fixture content is the sequence pass:

- **Case 1 — real mismatch region.** A synthetic run of ≥ 5 gaps
  where a bounded span of B has a constant offset **outside** the
  ±600 ms / ±2000 ms envelope window, and the rest of the run is at
  nominal registration. Expect: delta elevated throughout that span;
  the sequence pass warns. (An in-window step that only spikes the
  two edge gaps is the wrong model — it would fail a minimum-run
  rule and is not the corpus shape.)
- **Case 2 — isolated seam defect, no region.** One gap with a real
  pre/post disagreement, neighbors nominal. Expect: no warning.
- **Case 3 — noisy nominal run, no region.** Nominal registration with
  realistic per-gap correlation noise, including low `peak_r`
  shoulders. Expect: no warning at the chosen (T, N), or § 6/§ 7
  refute.

---

## 4. Probe (production path, cost)

After `decode_ab` in `PatchAudio::run_with_decoded`, before the
characterize loop: for each planned region, `lag_pair` at `b_mapped`
from the already-decoded PCM. No extra file read. Then O(gaps)
sequence pass. If it flags, `phase()` on stderr immediately (operator
can abort) and record the same warning for the stdout report.

Not inside `characterize_region`. That loop *is* the cost this
warning exists to precede (`char_gate_search` is the dominant share
of repair time; this sweep is not).

Scan-time retained windows (warn on dry-run with no second full-file
decode) are a later Change.

---

## 5. Warning contract (minimum)

Human, one `Warning:` line on the stdout report, in the existing
timeline/alignment-warning block ([cli-output.md](../cli-output.md)
§ Timeline / duration warnings). Same text via `phase()` on stderr
as soon as the pre-pass flags, before characterize. Suggested copy
(pin in `cli-output.md` when this ships):

`Warning: this pair looks like it has a timeline mismatch that can make filling gaps much harder — review before leaving a long repair running`

Do not mention a gap index range in the default line (that is a
corpus-style identity). Verbose may add the planned-gap span the
sequence pass used.

JSON: one additive optional run-level key on the repair report (not
per-gap `tags`), absent when the pass does not flag. Exact name and
shape are a `json-output.md` additive revision in the same Change —
boolean or a small object, not a `GapTags` dump. Human warning lines
are not duplicated as a JSON array (same rule as A1–A3).

---

## 6. Corpus comparison methodology

No re-scan for the confirmatory half. Read existing Full-tier
`lag_decision.pre_anchor[0].frac_lag_ms` / `post_anchor[0].frac_lag_ms`:

- Positive: pair 50 gaps 8–18 (a run) and pair 31 gaps 7–8 (a short
  run / straddle). These must warn at the chosen (T, N). They are
  calibration, not the warning's meaning.
- Negative: pairs 22 and 24 (isolated disagreements, not runs),
  plus the full 51-pair `|pre − post|` distribution. T sits above the
  noise floor; N is large enough that 22/24 do not warn.
- Do not warn solely because `offset_drift_secs` is large — that is
  the existing alignment warning. A pair that already carries that
  warning may also trip this one if a hole-run is present; that is
  allowed, not required.

A from-decode run is needed only to validate moved `domain::lag_pair`
matches today's `lag.rs` call sites — not to re-derive T/N.

---

## 7. Decision

- If § 3's three cases pass at some (T, N) and § 6's read flags the
  positive runs and not 22/24: this Change ships the pre-pass +
  warning. (T, N) are pinned in the code and in this file.
- If case 3 cannot sit under T at any N without also losing case 1:
  **this plan is refuted.** Record in `settled.md`. Do not ship
  per-gap fields as a consolation. A better statistic (e.g.
  monotonic-sign agreement inside the run) is a new TEMP, not a
  fallback in this one.
- If the domain-move check does not reproduce today's Full-tier
  numbers: fix the extraction before anything else; that is a bug.

---

## 8. Done when

§ 6 has been read; § 7 has picked a branch; if ship: § 3 is green at
the pinned (T, N), the warning fires from the pre-pass before
characterize on a case-1 fixture, case 2 and case 3 stay quiet,
production patch bytes and repair decisions are unchanged, and
`cli-output.md` / `json-output.md` record the additive warning.

**Not** done when: the warning is computed only after characterize;
`GapTags` grew lag fields; a repair decision or skip reads the flag;
(T, N) shipped without the corpus read; copy names a corpus pair.

---

## 9. Placement in code (checklist — execute in order)

Design is § 0–§ 2. This list is the Change. Do not start production
wiring until § 6/§ 7 have picked the ship branch.

- [ ] § 6 corpus read (pair 50 8–18, pair 31 7–8, pairs 22/24,
      full-corpus distribution); (T, N) candidate or refute recorded
      here.
- [ ] Move `lag_pair` / `lag_side_sweep` / `LagSweepParams` /
      `summarize_lag_curve` / `rescue_shoulder` from
      `application/gap_fingerprint/measure/lag.rs` into `domain/`.
      Unit: existing `lag.rs` tests unchanged at the new location;
      `lag_at_placement_envelope_recenter_is_per_shoulder` and
      `lag_at_placement_envelope_shared_far_unlocks_both` still pass.
- [ ] Pre-pass in `PatchAudio::run_with_decoded` after decode, before
      the characterize loop: `lag_pair` at `b_mapped` for every planned
      region; sequence pass at pinned (T, N); `phase()` if flagged.
- [ ] § 3 fixture (three cases) against that pre-pass.
- [ ] Human `Warning:` line + additive JSON run-level key; pin copy
      in `cli-output.md` and the key in `json-output.md`.
