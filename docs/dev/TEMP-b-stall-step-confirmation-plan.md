# TEMP — B-stall step confirmation (short stalls)

**Status:** draft, 2026-09-14. A Change to the shipped A5 B-stall warning:
a step test so the floor can drop from 10 s to 2 s. Scan-time only; changes
no patch / skip decision.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Timeline integrity* →
*B-stall step confirmation*.

**Predecessor:** A5
([archive/TEMP-b-stall-warning-plan.md](archive/TEMP-b-stall-warning-plan.md)),
shipped at `stall_min_secs = 10.0`. Its § 5 census is the evidence base
here and its § 2 rules 0, 1, 3, 4, 5 stay as written unless § 2 below
says otherwise.

**Companions (do not merge):**

- *Stall-segmented map* (BACKLOG, same section) — consumes the cumulative
  offset. This plan changes what that offset is built from; it does not
  re-map gaps.
- `gap_offset_agreement` — on pair 50 it reads `agrees` (0.59 vs 0.60)
  across a real 2.75 s step, because it compares intervals inside the
  start-clip-clipped `start_overlap`. Noted here as evidence; not in scope.
- A6 end-clip override warning — pair 50's end clip (+2.92 s vs start
  +0.60 s) is the same step seen from alignment.

**Media hygiene:** pair index + gap index only.

---

## 0. What this ships (normative)

**A B silence shorter than 10 s is reported as a B stall only when the
timeline after it is measured to have shifted by about its length.**
Silences whose timeline stays flat are B dropouts and are not reported.
Only confirmed stalls advance the cumulative offset.

At ≥ 10 s the shipped A5 behaviour is unchanged (reported without
evidence), so pair 54 cannot regress.

---

## 1. Evidence (A5 § 5 census, 51 pairs, floor 2 s)

166 interior candidates; 66 passed A5 rule 3 (B silent, A has audio).

| Class | n | Shared silences after, residual vs flat map | Registration after (next ≤ 8 gaps, \|lag\| ≤ 500 ms ∧ r ≥ 0.7) |
|-------|---|------------------|------------------|
| Dropout (no shift) | 64 | within ±0.41 s | 5/5 – 8/8 where measured |
| Pair 54, B 6152.8, 66.6 s | 1 | step residual −0.85 s (flat +65.78) | 0/1 |
| Pair 50, B 2249.3, 2.75 s | 1 | step residual +0.05 s (flat +2.81) | 0/8 |

Pair 50 confirmed by ear 2026-09-14: B loses ≈ 2.5 s of program at
37:29. Gaps 3–7 register at lag 0 (r 0.84–0.96); gaps 8–18 are all
`not_evaluated` (r 0.17–0.50) — a 2.75 s step is past registration's
±2 s reach, so it shows as collapse, not as a lag.

Length does not separate the classes: 60 dropouts sit in 2.0–3.8 s, the
longest is 5.7 s (pair 48), and pair 50's stall is 2.75 s. At a 2 s floor
without a test, A5's rule 2 contaminates the cumulative offset (pairs 18
and 25 reached +21.6 s from false reports; pair 54's own offset read
+4.59 instead of +2.06 from a 2.5 s dropout at B 4411.1).

Every one of the 66 had at least one later shared silence; 4 had no
registration after them.

---

## 2. Rule (normative)

Candidates: A5 rules 0, 1 (floor now `stall_min_secs`, target 2.0), 3,
4 unchanged. Process candidates in B order; `offset` is the cumulative
offset from **confirmed** stalls only.

1. **Evidence window.** From the candidate's `b1` to the next
   candidate's `b0` (or the scanned end). Evidence past the next
   candidate belongs to it — pair 54's 4411.1 dropout reads flat +65.78
   only because its window was not bounded.
2. **Shared-silence residuals.** For each B silent interval in the
   window of length ≥ `step_probe_min_secs` (census-set), find the A
   silent run whose start is nearest `b0' − offset`; residual
   `r = (b0' − a0') − offset`. Discard if `|r| > dur + max_tol`
   (no counterpart). `flat_votes` = count with `|r| ≤ tol`;
   `step_votes` = count with `|r − dur| ≤ tol`.
3. **Registration.** Over the gaps in the window with a
   `donor_registration`: `reg_flat` = share with `|lag| ≤ 500 ms` and
   `peak_r ≥ 0.7`.
4. **Verdict.**
   - `step` — `step_votes ≥ 1`, `step_votes > flat_votes`, and
     `reg_flat < 0.5` (or no registration).
   - `flat` — `flat_votes > step_votes`, or `reg_flat ≥ 0.5`.
   - `undetermined` — no evidence in the window.
5. **Report.** Duration ≥ 10 s: reported regardless (A5). Below 10 s:
   reported only on `step`. Every non-reported candidate stays a
   `tracing::debug` line, now carrying its verdict and votes.
6. **Offset advance.** A reported stall advances `offset` by its
   measured shift: `dur + median(step residuals − dur)` when step votes
   exist, else `dur`. Record both.
7. **No decisions** (A5 rule 6).

`tol` must admit pair 54's −0.85 and reject every dropout (max 0.41):
start at 0.6 s with the "nearer to step than flat" condition doing the
separation; the census in § 5 pins it.

---

## 3. Output

- Human line unchanged; "by up to +Xs" uses the measured shift.
- JSON v1 additive on each `scan.b_stalls[]`: `shift_secs` (f64),
  `evidence` (`"step"` | `"length"` for ≥ 10 s without step votes),
  `step_votes`, `flat_votes` (u32). Changelog line per
  [json-output.md](../json-output.md).
- `stall_min_secs` default → 2.0 only after § 5 passes; `tol` and
  `step_probe_min_secs` are constants unless the census says otherwise.

---

## 4. Tests

Unit (domain, pure):

1. 3 s B silence, one later shared silence at `offset + 3` → reported,
   shift ≈ 3, offset advances.
2. Same, later shared silence at `offset` → not reported, offset does
   not advance.
3. Registration flat 6/8 with one step vote → not reported.
4. 3 s candidate with no evidence → not reported; 12 s candidate with no
   evidence → reported (`evidence: "length"`).
5. Two candidates: evidence after the second is not read by the first.
6. Pair 54 shape: 66 s stall, residual −0.85 → `step`.
7. Flat dropout before a real stall (pair 54's 4411.1) → dropout does
   not advance the offset; the stall maps at the pair offset.

Integration: extend `execute_reports_b_stall_and_attributes_later_gap`
with a 3 s B dropout (no shift) before the stall — not reported — and add
a synthetic 3 s B stall with a real timeline shift — reported.

---

## 5. Census (ship gate)

Re-run the A5 census data (51 pairs, floor 2 s, `RUST_LOG` debug) with
the new verdict logged. `gap-files/` is ephemeral: re-scan if the
2026-09-14 census directory is gone.

Pass: pairs 50 (B 2249.3) and 54 (B 6152.8) reported; zero of the 64
dropouts reported; pair 50's B 3241.3 candidate is `flat` against the
post-step map; no pair's cumulative offset moves except at a confirmed
stall. Record the verdict table in the archive note. Any `undetermined`
candidate < 10 s is listed for ear check.

---

## 6. Checklist

- [ ] Domain: verdict + measured shift in `crates/clip-sync-repair/src/domain/b_stall.rs`; § 4 unit tests.
- [ ] Scan: pass B silent intervals, A runs and per-gap registrations to the verdict in `crates/clip-sync-repair/src/application/scan_gaps.rs`; debug line carries verdict.
- [ ] Output + JSON DTOs (`shift_secs`, `evidence`, votes); [json-output.md](../json-output.md) + regenerate `full_surface_repair.json`.
- [ ] [cli-output.md](../cli-output.md) A5 row: floor and step-confirmation wording.
- [ ] § 5 census; set `stall_min_secs` default to 2.0.

---

## 7. Done when

The census passes § 5 at the 2.0 s default, pair 50's scan warns at
B 2249 s and pair 54's still warns at B 6152.8 s, and this plan is
archived with a [settled.md](settled.md) row plus removal of its BACKLOG
row.
