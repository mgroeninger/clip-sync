# TEMP — B-stall step confirmation (short stalls)

**Status:** draft, rewritten 2026-09-14. A Change to the shipped A5 B-stall
warning: a **shoulder test** at each B silence, so the floor can drop from
10 s to 2 s. Scan-time only; changes no patch / skip decision.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Timeline integrity* →
*B-stall step confirmation*.

**Predecessor:** A5
([archive/TEMP-b-stall-warning-plan.md](archive/TEMP-b-stall-warning-plan.md)),
shipped at `stall_min_secs = 10.0`. Its § 5.1 census is the evidence base
here and its § 2 rules 0, 1, 3, 4, 5 stay as written unless § 2 below
says otherwise.

**Companions (do not merge):**

- *Stall-segmented map* (BACKLOG, same section) — consumes the cumulative
  offset. This plan changes what that offset is built from (measured
  shift, not silence length); it does not re-map gaps.
- A6 end-clip override warning
  ([TEMP-end-clip-override-warning-plan.md](TEMP-end-clip-override-warning-plan.md))
  — sees pair 50's step from alignment (end clip +2.92 s vs start +0.60 s)
  but cannot say **where** it is. This plan locates it.
- `gap_offset_agreement` — reads `agrees` on pair 50 across the real step
  because it compares inside the start-clip-clipped `start_overlap`.
  Evidence only; not in scope.
- Step-boundary rule
  ([archive/TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md)
  §4–§5) — the canonical **multi-gap level-change** classifier. This plan
  is not one: it fits two shoulders of a single event, the shape of
  seam-splice dual-fit. The sequence test therefore does not transfer;
  that plan's **dead-shoulder gate** does (§ 2 rule 2 and *Evaluable lag*), because without it
  pair 50 produced a staircase out of noise peaks.

**Media hygiene:** pair index + gap index only.

---

## 0. What this ships (normative)

**A B silence shorter than 10 s is reported as a B stall only when B's
audio after the silence lines up with A's audio from the start of the
mapped span (the timeline shifted), not from its end (the timeline held).**
Silences whose after-shoulder holds the map are B dropouts and are not
reported. Only reported stalls advance the cumulative offset, by the
**measured** shift.

At ≥ 10 s the shipped A5 report is unchanged (reported whatever the
shoulders say), so pair 54 cannot regress; the shoulder test still runs
there and its verdict is recorded (§ 2 rule 7).

---

## 1. Evidence and mechanism

### 1.1 A5 § 5.1 census (51 pairs, floor 2 s)

166 interior candidates; 66 passed A5 rule 3 (B silent, A has audio).

| Class | n | Later shared silences vs flat map | Registration, next ≤ 8 gaps |
|-------|---|-----------------------------------|-----------------------------|
| Dropout (no shift) | 64 | residual within ±0.41 s | 5/5 – 8/8 where measured |
| Pair 54, B 6152.8, 66.6 s | 1 | shifted, residual −0.85 s vs silence length | 0/1 |
| Pair 50, B 2249.3, 2.75 s | 1 | shifted, residual +0.05 s | 0/8 |

Pair 50 confirmed by ear 2026-09-14: B loses ≈ 2.5 s of program at
37:29. The archived WAV shoulder sweep measured its map step at ≈ +2.8 s.

Length does not separate the classes: 60 dropouts sit in 2.0–3.8 s, the
longest is 5.7 s (pair 48), and pair 50's stall is 2.75 s. At a 2 s floor
without a test, A5's cumulative offset is contaminated (pairs 18 and 25
reached +21.6 s from false reports; pair 54's read +4.59 instead of +2.06
from a 2.5 s dropout at B 4411.1).

### 1.2 The shoulder test

For a B silence `[b0, b1)` of length `dur` over which A has audio:

| | B pre-shoulder (ends at `b0`) matches A ending at | B post-shoulder (starts at `b1`) matches A starting at |
|---|---|---|
| **Dropout** (program replaced, timeline held) | `b0 − offset` | `b1 − offset` (end of the mapped span) |
| **Stall** (B paused, resumed) | `b0 − offset` | `b0 − offset` (start of the mapped span) |

Parameterize the post-shoulder position as A start `= b1 − offset − s`.
Dropout ⇒ `s ≈ 0`; stall ⇒ `s ≈ dur`; a stall that also lost program ⇒
`0 < s < dur`. Relative to the pre-shoulder's own lag, **`s` is the
measured shift.** Pair 50's ear note (≈ 2.5 s lost) and sweep (≈ +2.8 s)
predict `s` near 2.5–2.8 s, not exactly 2.75 s — the rule classifies by
range and reports the measured value.

### 1.3 Why not the earlier draft's evidence

The first draft of this plan voted on later shared silences and donor
registration. Both are dropped from the rule:

- **Registration cannot see these steps.** `max_lag_blocks = 10` at the
  100 ms block is ±1 s reach; every candidate ≥ 2 s is past it, so a stall
  reads as collapse — which decorrelated content also produces. It is
  also computed at the **pair** offset (`scan_gaps.rs`, per-gap loop), so
  after the first confirmed stall every later gap collapses regardless.
- **Later shared silences are non-local.** They need an evidence window,
  a counterpart search biased toward whichever hypothesis it centres on,
  and a sequence test (one vote is an excursion). They stay as the
  **census cross-check** in § 5, not in code.

---

## 2. Rule (normative)

Candidates: A5 rules 0, 1 (floor now `stall_min_secs`, target 2.0), 3,
4 unchanged. Process candidates in B order; `offset` is the cumulative
offset (pair offset + the rule-7 advance of every earlier **reported**
stall). The shoulder test runs only on candidates that survive A5 rule 3
(A not silent over the mapped span).

Inputs added to the detector: A and B block levels (`a_levels`,
`b_scan.levels` — both live at Step 6a; `BlockLevel { start_secs,
end_secs, rms_db, silent }`). Correlation is Pearson, as in
`register_donor_window`.

**Envelope.** `max(rms_db, ENVELOPE_FLOOR_DB)`, and B's silent shoulder
bins are **masked** out of the pairs (fixed per candidate, so every lag
correlates the same B bins). Digital-zero blocks sit at
`BLOCK_LEVEL_FLOOR_DB = −120`; unclamped, a handful of them dominate a
40-bin variance and the correlation aligns silence edges instead of
program. The clamp alone is not enough on B — a −80 dB bin still sits
tens of dB below program and carries the variance (found in
implementation) — so B-silent bins are dropped; A bins are only clamped,
which keeps a quiet A bed (−68 dB is on the corpus) distinct from
program.

**Windows by index.** Locate each base window once with `window_indices`,
then step lags by **block index** over equal-length slices (`n` = the B
shoulder's bin count). Re-selecting by block-centre time per lag drifts by
a bin and yields unequal lengths.

**Evaluable lag.** A lag is evaluable when its A slice lies wholly inside
`a_levels`, has ≥ `SHOULDER_MIN_LIVE_BINS` non-`silent` bins, and
`pearson` returns `Some`. Non-evaluable lags are **skipped**, not fatal:
one quiet A stretch inside pair 54's 66 s search must not void the other
~680 lags. A region's r is the max over its evaluable lags, or *none*
when it has none.

Constants below are **starting values**; § 5 pins them.

| Constant | Start | Meaning |
|----------|-------|---------|
| `SHOULDER_SECS` | 4.0 | Shoulder length each side |
| `PRE_SEARCH_SECS` | 1.0 | Pre-shoulder lag search, ± |
| `PRE_MAX_LAG_SECS` | 0.5 | Pre-shoulder must register within this of the map |
| `POST_SEARCH_PAD_SECS` | 1.0 | Post search range pad beyond `[0, dur]` |
| `FLAT_TOL_SECS` | 0.5 | `|shift| ≤` this is the flat region |
| `STEP_MIN_SECS` | 1.0 | `shift ≥` this is the step region |
| `SHOULDER_MIN_R` | 0.70 | Winning region's peak r floor (registration's `min_envelope_r`) |
| `SHOULDER_MARGIN_R` | 0.10 | Winning region must beat the other by this |
| `SHOULDER_MIN_LIVE_BINS` | 8 | Non-`silent` bins required in each B shoulder and each evaluable A slice (`MIN_REGISTRATION_BINS`) |
| `ENVELOPE_FLOOR_DB` | −80.0 | Envelope clamp (see above) |

1. **Shoulders.** B pre = bins in `[b0 − SHOULDER_SECS, b0)`; B post =
   bins in `[b1, b1 + SHOULDER_SECS)`. Both must lie inside the scanned B
   extent and the overlap; otherwise `undetermined(no_shoulder)`. (The
   overlap is the pair-offset overlap A5 already computes, not re-derived
   at the cumulative offset — harmless for a warning; the stall-segmented
   map must not inherit it silently.)
2. **Dead B shoulder.** Fewer than `SHOULDER_MIN_LIVE_BINS` non-`silent`
   bins in B pre or B post, or a flat clamped envelope on either →
   `undetermined(dead_shoulder)`. This gate is on **B only**; A-side
   deadness is per lag (§ 2 *Evaluable lag*).
3. **Pre-shoulder registration.** Correlate B pre against the A slice
   ending at `b0 − offset + p` for `p ∈ [−PRE_SEARCH_SECS,
   +PRE_SEARCH_SECS]` (bin steps, evaluable lags only). No evaluable lag →
   `undetermined(dead_shoulder)`. Peak `(p*, r_pre)`. Require
   `r_pre ≥ SHOULDER_MIN_R` and `|p*| ≤ PRE_MAX_LAG_SECS`; otherwise
   `undetermined(pre_unregistered)` — the map is already wrong before the
   pause, so the post-shoulder cannot be read against it.
4. **Post-shoulder search.** Correlate B post against the A slice starting
   at `b1 − offset − s + p*` for `s ∈ [−POST_SEARCH_PAD_SECS, dur +
   POST_SEARCH_PAD_SECS]` (bin steps, evaluable lags only — lags near
   `−POST_SEARCH_PAD_SECS` can run past A's scanned end and are skipped).
   Define `shift = s`.
   - `r_flat` = max r over evaluable `|shift| ≤ FLAT_TOL_SECS`, or none
   - `r_step` = max r over evaluable `shift ≥ STEP_MIN_SECS`, or none;
     `shift*` its argmax
   Both none → `undetermined(dead_shoulder)`. Exactly one none → that
   region reads as −1 for rule 5 (it cannot win, and the other still has
   to clear `SHOULDER_MIN_R` on its own).
5. **Verdict** (total — every candidate gets exactly one):
   - `step` — `r_step ≥ SHOULDER_MIN_R` ∧ `r_step − r_flat ≥ SHOULDER_MARGIN_R`
   - `flat` — `r_flat ≥ SHOULDER_MIN_R` ∧ `r_flat − r_step ≥ SHOULDER_MARGIN_R`
   - `undetermined(reason)` — anything else, including rules 1–4:
     `no_match` when neither region clears `SHOULDER_MIN_R`; otherwise
     `ambiguous` (at least one clears the floor but the margin fails —
     both clearing is the periodic-material case, and one at 0.75 against
     the other at 0.68 is the same "cannot separate" answer).
   `STEP_MIN_SECS < dur` always holds at the 2 s floor, so the step region
   is non-empty. `FLAT_TOL_SECS < shift < STEP_MIN_SECS` is deliberately
   in neither region.
6. **Report.** Duration ≥ 10 s: reported regardless of verdict (A5).
   Below 10 s: reported only on `step`. Every non-reported candidate —
   A-silent suppressions (existing) and unconfirmed ones (new) — is one
   `tracing::debug` line carrying verdict, reason, `r_pre`, `p*`,
   `r_flat`, `r_step`, `shift*`, B pre / B post live-bin counts.
7. **Offset advance.** By verdict, for a **reported** stall:

   | Verdict | Advance | `evidence` |
   |---------|---------|------------|
   | `step` (any length) | `shift*` | `shoulder` |
   | `undetermined` (≥ 10 s) | `dur` | `length` |
   | `flat` (≥ 10 s) | `dur` | `length_contradicted` |

   **`shift*`, not `shift* − p*`.** Rules 3–4 put the true local offset
   after the stall at `offset − p* + shift*`. `p*` is bounded by
   `PRE_MAX_LAG_SECS` and is the same registration noise / slow drift the
   dropout path already ignores (A5's dropouts sit within ±0.41 s of the
   flat map); folding it into `offset` would random-walk the map by up to
   0.5 s per stall. The advance measures the **event**, and `p*` is
   recorded on the debug line and in JSON.

   **`flat` at ≥ 10 s keeps A5's `dur` advance** — deliberately, and
   visibly. The census has no ≥ 10 s `flat` candidate (pair 54 is the only
   one ≥ 10 s), so there is no evidence to change shipped behaviour on;
   `length_contradicted` puts the conflict in the report instead of
   silently corrupting or silently changing the map. Consumers of the
   cumulative offset (the stall-segmented map) must treat
   `length_contradicted` as "no shift" or refuse it; re-deciding the
   advance itself is latched on the first such candidate on real media
   (record in [settled.md](settled.md) § C at archive time).
8. **No decisions** (A5 rule 6).

Cost: pair 54's candidate is ≈ 690 post lags × 40 bins; the corpus's 166
candidates are negligible next to the scan.

---

## 3. Output

- Human line unchanged in shape; "by up to +Xs" uses the advance from
  rule 7.
- JSON v1 additive on each `scan.b_stalls[]`: `shift_secs` (f64, the
  rule-7 advance), `evidence` (`"shoulder"` | `"length"` |
  `"length_contradicted"`), `verdict_reason` (string, present on
  `evidence: "length"` — `no_shoulder` / `dead_shoulder` /
  `pre_unregistered` / `ambiguous` / `no_match`), `pre_lag_secs` (`p*`),
  `step_r`, `flat_r` (f64, optional — absent when the test did not reach
  the rule that produces them; a region with no evaluable lag omits its
  r). Optional fields use `skip_serializing_if`, since `BStallCandidate`
  serializes directly. Changelog line per
  [json-output.md](../json-output.md).
- `offset_used_secs` now means pair offset + earlier **advances**: update
  its doc comment (`b_stall.rs`) and [json-output.md](../json-output.md)
  § BStall. Values are identical to A5 on any pair whose reported stalls
  all carry `evidence` `"length"` or `"length_contradicted"`.
- `BStallScan` gains an `unconfirmed` bucket beside `suppressed` (A5
  A-silent rule) for the rule-6 debug lines; neither reaches `GapReport`.
- `stall_min_secs` default → 2.0 only after § 5 passes. All § 2 constants
  are `const`s in `b_stall.rs`, not config.

---

## 4. Tests

Unit (domain, pure — synthetic `BlockLevel` envelopes from a
deterministic non-periodic dB sequence; B derived from A by edit; silent
blocks at `BLOCK_LEVEL_FLOOR_DB` with `silent: true`, as the scanner
emits them):

1. **Stall.** B = A with 3 s of silence inserted → `step`, `shift*` ≈ 3.0,
   reported, `evidence: "shoulder"`, `offset` advances by `shift*`.
2. **Dropout.** B = A with 3 s replaced by silence → `flat`, not reported,
   `offset` unchanged.
3. **Stall that lost program.** 2.75 s silence inserted, 0.25 s of A
   removed after it → `step`, `shift*` ≈ 2.5 (measured, not `dur`).
4. **Long stall.** 66.6 s silence, resumes 0.8 s early (pair 54 shape) →
   `step`, `shift*` ≈ 65.8; search range covers it.
5. **Dead post-shoulder.** B silent/flat after the pause: 3 s →
   `undetermined(dead_shoulder)`, not reported; 12 s → reported,
   `evidence: "length"`, `verdict_reason: "dead_shoulder"`,
   `shift_secs` = `dur`.
6. **Pre unregistered.** Pre-shoulder lag 0.8 s off the map →
   `undetermined(pre_unregistered)`.
7. **Periodic material.** Envelope with period = `dur` → both regions
   clear the floor → `undetermined(ambiguous)`, not reported.
8. **Cumulative map, non-zero `p*`.** A pre-existing +0.3 s map error
   (inside `PRE_MAX_LAG_SECS`), then dropout, stall, dropout: `p*` ≈ 0.3 on
   each; only the stall advances `offset`, by `shift*` (≈ `dur`, **not**
   `dur − 0.3`); the second dropout reads `flat` through the post-stall
   offset (its A mapping and `a_silent_fraction` use it).
9. **Shoulder outside the scan.** Post-shoulder past `scanned_end` →
   `undetermined(no_shoulder)`.
10. **No levels.** Empty B levels → every short candidate
    `undetermined(no_shoulder)`; ≥ 10 s still reported, `evidence: "length"`.
11. **Quiet A stretch inside a long search.** 66.6 s stall (test 4) with a
    1 s A silence inside the post-search range → the lags reading it are
    skipped, verdict still `step`, `shift*` unchanged.
12. **Long `flat`.** 12 s B dropout with live shoulders → `flat`,
    reported, `evidence: "length_contradicted"`, advance = `dur`.
13. **A slice past A's end.** Candidate near A's scanned end so lags near
    `s = −POST_SEARCH_PAD_SECS` overrun `a_levels` → those lags skipped, no
    panic, verdict decided from the rest.
14. **B-silent masking.** Stall case with half of B pre at digital zero
    (still ≥ 8 live bins) → `step`; the test comment names the unmasked
    failure it guards.
15. **Region with no evaluable lag.** Flat region's A slices all
    below `SHOULDER_MIN_LIVE_BINS`, step region live and matching →
    `step` (flat reads −1); step region too → `undetermined(dead_shoulder)`.

Existing A5 unit tests stay; their candidates are ≥ 10 s with no levels
passed, and keep reporting with `evidence: "length"`.

Integration. The chirp fixture cannot exercise the shoulder test:
`write_offset_chirp_wav_pair` is constant-amplitude, so every clamped
envelope is near-flat and correlates at every lag (implementation
measured `ambiguous`: r_flat 1.0, r_step 0.93, rather than
`dead_shoulder`). So:

- **Existing** `execute_reports_b_stall_and_attributes_later_gap`: keep
  as is, and additionally assert its 20 s zeroed B segment reports
  `evidence: "length"` with a `verdict_reason` and `shift_secs =
  duration_secs` — this pins the chirp limitation rather than hiding it.
- **New fixture** (local helpers in `scan_gaps_integration.rs`, next to
  its existing `zero_wav_segment`): an amplitude-modulated
  chirp pair — the same 300–700 Hz sweep (keeps alignment behaviour close
  to the existing fixtures) multiplied by a deterministic non-periodic
  gain envelope (hashed gain step every 500 ms over [−24, 0) dB; 250 ms scan blocks) —
  plus two B edits: `zero_wav_segment` (dropout, length preserved) and a
  new `insert_silence` (stall: splice N s of zeros in, later
  samples shift, file grows by N s).
- **New test** `execute_confirms_short_b_stall_by_shoulders`
  (`scan_gaps_integration.rs`), `stall_min_secs = 2.0` explicit: a 3 s B
  dropout — not reported — then a 3 s inserted B stall — reported,
  `evidence: "shoulder"`, `shift_secs` within one 250 ms block of 3.0. Place both
  after the start clip window, and keep the end clip out of the
  assertions (it will see the step; that is A6's concern).

---

## 5. Census (ship gate)

Re-run the A5 census (51 pairs, floor 2 s, `RUST_LOG` debug) with the
rule-6 line logged for every candidate. `gap-files/` is ephemeral:
re-scan if the 2026-09-14 census directory is gone.

**5.1 Pre-check — go / tune / park.** Before pinning anything, tabulate
for the 66 A5-rule-3 survivors: verdict, reason, `r_pre`, `p*`, `r_flat`,
`r_step`, `shift*`, B pre / B post live-bin counts, skipped-lag counts per
region. Exactly one outcome applies, checked in order:

1. **Park** — pair 50 (B 2249.3) is `undetermined` with reason
   `dead_shoulder`, `no_shoulder`, or `pre_unregistered` (the shoulders
   carry no usable evidence; no constant in § 2 besides `SHOULDER_SECS`
   can change that). Try `SHOULDER_SECS` ∈ {2, 6} once; if still one of
   those reasons, record in [settled.md](settled.md) — the shoulder test
   cannot confirm the corpus's one short stall at scan time and A6 is the
   ceiling. Do **not** fall back to single-vote later-silence evidence;
   that is the excursion failure the step-boundary rule exists to prevent.
2. **Go** — all of: pair 50 `step` with `shift*` in 2.3–3.0 s; pair 54
   (B 6152.8) `step` with `shift*` within ±1 s of 65.8; zero of the 64
   dropouts `step`. Proceed to § 5.2.
3. **Tune** — anything else: pair 50 `ambiguous` / `no_match` / `flat`,
   or `step` with `shift*` outside 2.3–3.0 s; pair 54 not `step` or
   `shift*` out of range; any dropout `step`. List the failing rows, adjust
   in this order — `ENVELOPE_FLOOR_DB`, `SHOULDER_SECS`, then the
   region / margin constants — and re-run § 5.1. Each adjustment is
   recorded with the row it was meant to fix. If no setting reaches Go
   without a dropout reading `step`, **park** as in (1) with the tuning
   table as the evidence. Nothing ships from Tune.

**5.2 Pin constants.** Only the two positives are stalls, so pin with
headroom rather than at the boundary: the smallest stall margin
(`r_step − r_flat`) and the largest dropout anti-margin
(`r_step − r_flat` on a `flat`) must be separated by at least
`SHOULDER_MARGIN_R`, and every `flat` dropout's `r_flat` must clear
`SHOULDER_MIN_R` by ≥ 0.05. If they cannot be, stop and revisit
`SHOULDER_SECS` before shipping.

**5.3 Cross-check.** Every `step` / `flat` verdict agrees with the
later-shared-silence reading in A5 § 5.1 (64 flat, 2 shifted);
disagreements are listed and ear-checked. Pair 50's B 3241.3 candidate is
`flat` against the post-step map.

**5.4 Pass.** Pairs 50 and 54 reported with `evidence: "shoulder"`; zero
of the 64 dropouts reported; no pair's cumulative offset moves except at a
reported stall; every `undetermined` candidate < 10 s listed for ear
check; any ≥ 10 s `length_contradicted` row (none expected) listed and
ear-checked, triggering the rule-7 latch. Record the verdict table in the
archive note.

---

## 6. Checklist

- [x] Domain: envelope clamp, index-stepped windows, evaluable-lag skipping, shoulder test, verdict, rule-7 advance + `evidence`, `unconfirmed` bucket in `crates/clip-sync-repair/src/domain/b_stall.rs` (reuse `pearson` / `window_indices` from `gap_equivalence.rs` — lift to `pub(crate)` rather than copy); § 4 unit tests 1–15.
- [x] Scan: pass `a_levels` and `b_scan.levels` to the detector in `crates/clip-sync-repair/src/application/scan_gaps.rs` Step 6a; debug line per unconfirmed candidate.
- [x] Fixtures: amplitude-modulated chirp pair + `insert_silence` (local to `scan_gaps_integration.rs`); new `execute_confirms_short_b_stall_by_shoulders`; `length` assertion on the existing A5 integration test.
- [x] Output + JSON (`shift_secs`, `evidence`, `verdict_reason`, `pre_lag_secs`, `step_r`, `flat_r`); `offset_used_secs` wording; [json-output.md](../json-output.md) changelog + § BStall; regenerate `full_surface_repair.json`.
- [x] [cli-output.md](../cli-output.md) A5 row: floor, shoulder confirmation, cumulative offset by measured shift.
- [ ] § 5.1 pre-check (go / tune / park) → § 5.2 pin → § 5.3 cross-check → § 5.4; set `stall_min_secs` default to 2.0.
- [x] Stall-segmented map BACKLOG row: note that `length_contradicted` advances must not be consumed as shifts.

---

## 7. Done when

The census passes § 5.4 at the 2.0 s default, pair 50's scan warns at
B 2249 s with a measured shift near +2.5–2.8 s and pair 54's still warns
at B 6152.8 s, and this plan is archived with a [settled.md](settled.md)
row plus removal of its BACKLOG row. **Or** § 5.1 parks it, with the
settled.md row recording why.
