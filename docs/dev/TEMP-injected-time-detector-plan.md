# TEMP — Injected-time / shoulder-lag-disagreement detector

**Status:** draft, 2026-09-08. Design, not started. Touches production
(`region/characterize.rs`, `domain::GapTags`), not fingerprint-only.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Post-join defects (pairs 31
and 50)*.

**Predecessor (context, not superseded):**
[TEMP-constellation-fingerprint-plan.md](TEMP-constellation-fingerprint-plan.md)
§ 11 item D flagged, from existing dump data with no re-scan, that pair
50 gaps 9–17 show "repeated large pre/post disagreements (deltas 273.8–
1040.3 ms) — not yet investigated, flagged only." This plan is that
investigation's conclusion turned into a design: the disagreement is
real (confirmed against `lag_decision.pre_anchor`/`post_anchor`, the
Full-tier ±600 ms shoulder sweep), it is not what `--dual-fit`'s
`placement_slide` already measures (that instrument's confidence gate,
`peak_r >= ~0.35`, is `None` on exactly the gaps where the divergence
lives — pair 50 gaps 8–18 all read `gate_pass: false`), and it is not
what `anchored-retry`'s `PatchOffsetAnchor` carries (single scalar,
`Patched`-only, absent for skipped gaps).

**Companions:**
[TEMP-disjoint-offseam-lock-plan.md](TEMP-disjoint-offseam-lock-plan.md)
(50/g10, single-shoulder off-seam lock at a *disjoint* A slice — this
plan is on-seam, both shoulders, and about a *run* of gaps, not one
shoulder; complementary, not overlapping).

**Media hygiene:** pair index + gap index only.

---

## 0. What this tests (normative)

**Across a run of consecutive gaps, does the sign/magnitude pattern of
each gap's independent pre-shoulder vs. post-shoulder lag (measured the
same way `lag_at_placement` already measures it, but unconditionally —
no correlation-confidence gate) mark a bounded region of B whose content
is displaced in time relative to A, distinctly from an isolated
single-gap seam defect?**

That is the whole plan. Later sections only implement it. If a later
sentence disagrees with this one, this one wins.

"Independent pre/post lag" means `lag_pair`'s existing pair of bounded
(±600 ms nominal, ±2000 ms envelope-rescued) correlation sweeps, one
per shoulder, run against the already-decoded border template at
`characterize`'s baseline placement — the same measurement
`gap_fingerprint`'s Full-tier `lag_at_placement` already performs, nowhere
committed to production. "Unconditionally" means without
`registration_shoulder_residual_ms`'s confidence floor: the corpus
evidence (pair 50 gaps 9, 11–13, 17) is that the divergence is loudest
exactly where `peak_r` is too low for dual-fit's gate to read anything.
"A run of consecutive gaps" is the new part this plan adds: no existing
mechanism looks across gaps at all — `--dual-fit` and `anchored-retry`
both reason gap-by-gap.

This plan does **not** test: rewiring `--dual-fit`'s repair decision,
changing what `shoulders_abut` means for a single gap, building a
correlation-confidence gate for the new fields (they ship ungated,
diagnostic sequence-level flags only), or fixing pair 50's fill. It
tests whether a *warning* is constructible from data production would
already have on hand if this were wired — not whether the fill pipeline
should act on it.

---

## 1. Population (the customer)

Every gap in a run, patched or skipped, once `characterize`'s baseline
`place_on_b` placement has resolved a `start_frame` and decoded
`a_samples`/`b_mono`/`b_ch` are in hand — i.e., every gap that reaches
`region/characterize.rs`'s existing placement step. This is deliberately
**not** `TEMP-constellation-fingerprint-plan.md`'s § 1 "remaining-skip
shoulder" population (B present, Pearson lock absent, tier=skip only):
the sequence detector needs a lag reading on *every* gap in a run,
disposition-independent, or a real run can't be told apart from noise
at its edges (see pair 50 g8, g18 in § 6 — both `Patched`, both part of
the same anomalous span by timestamp, neither in the constellation
plan's population).

---

## 2. Design rules (normative)

1. The lag measurement itself is a pure extraction of existing code
   (`lag_pair`, `lag_side_sweep`, `LagSweepParams`, `summarize_lag_curve`,
   `rescue_shoulder`) from
   `application/gap_fingerprint/measure/lag.rs` into `domain/`, following
   the precedent already set for `seam_local`/`lag_envelope` (moved for
   the identical reason: "so the production patch path and this
   diagnostic scan use one implementation," `lag.rs:67-76`). No new
   correlation math.
2. The extracted call is **unconditional** — it runs at every gap
   `characterize` resolves a placement for, not gated behind
   `DetailTier::Full`, not gated behind `--dual-fit`, not gated behind a
   confidence floor. It is cheap: a bounded ±600 ms sweep over an
   already-decoded ~1 s border, the same cost class as `lag_decision`
   inside the Full-tier block today (`build.rs:258-268`), not the
   expensive per-bracket `constellation` sweep beside it.
3. New `GapTags` fields carry the raw per-shoulder read
   (`pre_frac_lag_ms`, `post_frac_lag_ms`, and `shoulder_lag_delta_ms =
   (post − pre).abs()`) plus each side's `peak_r`/`edge_pinned` so a
   consumer can judge confidence itself — this plan does not collapse
   that into a single verdict field. No new field feeds a repair
   decision in this plan; they are read-only diagnostics on the
   `GapReport`.
4. The cross-gap sequence pass is a **separate**, explicitly
   non-normative-until-fixture-tested step (§ 2.1) layered on top of the
   per-gap fields — it must not be conflated with, or block on, item 3
   shipping. Item 3 alone is useful (a human or a follow-up tool can
   already eyeball `shoulder_lag_delta_ms` across a run).
5. Nothing in this plan changes what `shoulders_abut` means for a single
   gap, changes `--dual-fit`'s gate, or adds a new CLI flag. Existing
   repair behavior and JSON output contract are unchanged; the new
   fields are additive to `GapReport`'s diagnostic surface only (exact
   placement — always-on internal field vs. a new opt-in report section
   — is a § 9 implementation decision, not decided here).
6. Sequence flagging (§ 2.1) needs two judgment calls this plan does not
   pre-decide and must resolve against fixture + corpus data before
   shipping: a per-gap delta threshold, and a minimum run length
   (isolated single-gap spikes are exactly what `--dual-fit`'s existing
   single-gap `placement_slide` already covers when its gate fires — the
   new signal's value is specifically the *sequence*, not a lone
   outlier).

### 2.1 Sequence pass (non-normative sketch, resolved in § 6/§ 7)

A run of ≥ N consecutive gaps (N TBD by § 6) where each gap's
`shoulder_lag_delta_ms` exceeds a threshold T (TBD by § 6, corpus
evidence starts the search near pair 50's observed 273.8–1040.3 ms
range) is flagged as a candidate injected-time / misregistration region.
Not: a single-gap threshold crossing (that is `--dual-fit`'s job when
its gate can fire); not a claim about *which* file (A or B) carries the
extra/missing time, only that the region exists.

---

## 3. Fixture (synthetic, before any corpus run)

Before touching `characterize.rs`, prove the extracted `lag_pair` (once
moved to `domain/`) still passes `lag.rs`'s existing unit tests
unchanged (pure move, § 2 rule 1) — no new fixture content needed for
that step.

New fixture content is needed only for § 2.1 (the sequence pass, not
yet existing anywhere):

- **Case 1 — real injected region.** A synthetic run of ≥ 5 gaps where a
  bounded span of B has a constant time offset relative to A (models
  pair 50's shape) and the rest of the run is at nominal registration.
  Expect: `shoulder_lag_delta_ms` elevated for exactly the gaps whose
  neighborhood straddles the offset region's edges, flat elsewhere.
- **Case 2 — single isolated seam defect, no region.** One gap with a
  real pre/post disagreement (bad splice) but neighbors on both sides at
  nominal registration. Expect: sequence pass does **not** flag (fails
  the minimum-run-length rule) — this is the case that must stay
  `--dual-fit`'s job, not this detector's.
- **Case 3 — noisy nominal run, no region.** A run at nominal
  registration with realistic per-gap correlation noise (low `peak_r`
  shoulders included, modeling pair 50's own low-`peak_r` gaps).
  Expect: `shoulder_lag_delta_ms` noise stays under threshold T at the
  chosen N, or § 6/§ 7 revisit T/N — this is the decoy-rate check for
  this plan (parallel to the constellation plan's redraw decoy).

---

## 4. Probe (production path, cost)

Per-gap: one extra `lag_pair` call at `characterize.rs`'s existing
`place_on_b` point, using data already decoded there. No new decode, no
new file read. Cost is the `lag_decision` cost class already paid today
under Full-tier fingerprinting (bounded ±600 ms correlation sweep over
~1 s of already-in-memory samples) — now paid on every repair run,
patched or skipped, not gated behind `--gap-fingerprints`.

The sequence pass (§ 2.1) is O(gaps in a run), pure post-processing over
already-collected `GapTags`, no additional decode or correlation work.

---

## 5. Dump / field contract (minimum)

On `GapTags` (or the analogous production-path struct — exact home is a
§ 9 decision):

- `pre_frac_lag_ms: Option<f64>`, `post_frac_lag_ms: Option<f64>` — raw
  `lag_pair` output, `None` only if the shoulder itself doesn't exist to
  sweep (not gated by confidence).
- `pre_peak_r: Option<f64>`, `post_peak_r: Option<f64>` — carried
  alongside so a consumer can judge confidence without a baked-in floor.
- `pre_edge_pinned: bool`, `post_edge_pinned: bool` — carried for the
  same reason `SeamLocalTags` carries them today.
- `shoulder_lag_delta_ms: Option<f64>` — `(post − pre).abs()` when both
  present, else `None`.

If § 2.1 ships in the same change: a run-level `injected_time_region:
Option<{start_gap_index, end_gap_index}>` per repair run, computed once
over the full gap sequence — not per-gap.

---

## 6. Corpus comparison methodology

No re-scan needed for the confirmatory half — pair 50 gaps 8–18's
`lag_decision.pre_anchor[0].frac_lag_ms` / `post_anchor[0].frac_lag_ms`
(already dumped, `--gap-fingerprints --features calibration,he-aac`,
Full tier) are read directly as ground truth for § 3 case 1's expected
shape and for picking T/N in § 2.1.

Negative controls, also from existing dumps, no re-scan: pairs 22 and
24 (flagged in the constellation plan's population but not part of any
known abutment/map-step case), plus the full 51-pair corpus's overall
`|pre − post|` distribution — the sequence pass's threshold T must sit
above the bulk of this distribution's noise floor, and N must be large
enough that pairs 22/24 (isolated single-gap disagreements, not runs)
do not flag.

A from-decode run is needed only to validate the moved `domain::lag_pair`
produces byte-identical output to today's `lag.rs` call sites (§ 2 rule
1, pure-move check) — not to re-derive any of the numbers above.

---

## 7. Decision

- If § 3's three fixture cases pass at some (T, N) and § 6's corpus read
  confirms pair 50 gaps 9–17 flag as one region while pairs 22/24 don't
  flag: item 3 (§ 9) ships as production `GapTags` fields, and § 2.1
  ships as a follow-up Change, tracked separately from this plan once
  T/N are pinned.
- If fixture case 3 (noisy nominal run) can't be kept under threshold at
  any N without also losing case 1's real region: § 2.1 is refuted as
  stated — the per-gap fields (§ 9 items 1–3) may still ship alone as
  diagnostics with no sequence verdict, and cross-gap flagging is
  deferred pending a better statistic (e.g. requiring monotonic-sign
  agreement within the run, not just magnitude).
- If the domain-move validation in § 6 finds the extracted `lag_pair`
  does not reproduce today's Full-tier numbers: fix the extraction
  before anything else proceeds; this is a bug, not a design question.

---

## 8. Done when

The § 3 fixture is green for the pure-move check and (if § 2.1 proceeds)
for the three sequence cases at a pinned (T, N); § 6 has been read
against pair 50 gaps 8–18 plus pairs 22/24 plus the full-corpus noise
floor; § 7 has picked a branch.

Production patch bytes unchanged — this plan adds diagnostic fields
only, no gate, no new repair decision.

**Not** done when: `shoulders_abut` or `--dual-fit`'s gate changes
behavior; a repair decision reads `shoulder_lag_delta_ms` or
`injected_time_region`; the sequence pass ships without a corpus-checked
(T, N).

---

## 9. Placement in code (checklist — execute in order)

Design is § 0–§ 2. This list is the Change.

- [ ] Move `lag_pair`/`lag_side_sweep`/`LagSweepParams`/
      `summarize_lag_curve`/`rescue_shoulder` from
      `application/gap_fingerprint/measure/lag.rs` into `domain/`,
      following the `seam_local`/`lag_envelope` precedent. Unit:
      existing `lag.rs` tests unchanged, now against the domain
      location; `lag_at_placement_envelope_recenter_is_per_shoulder`
      and `lag_at_placement_envelope_shared_far_unlocks_both` (the
      31/g7- and 31/g10-class regression tests) still pass.
- [ ] Call the moved `lag_pair` unconditionally from
      `region/characterize.rs` at the existing `place_on_b` baseline
      placement point. Unit: a gap with known pre/post lags gets the
      right `GapTags` values regardless of tier/flags.
- [ ] Add § 5's new ungated `GapTags` fields; wire them through to
      whatever surface exposes `GapTags` today (JSON output contract
      unaffected — confirm no field lands in the v1 JSON schema unless
      explicitly decided in a later Change).
- [ ] § 3 fixture (three cases) + § 2.1 sequence pass, if § 7 keeps it
      in scope.
- [ ] § 6 corpus read (pair 50 8–18, pairs 22/24, full-corpus
      distribution); § 7 decision recorded here or in `settled.md`.
