# TEMP — B-stall warning (A5)

**Status:** draft, 2026-09-14. Not started. A Change (user-visible
warning + additive JSON field). Scan-time only; changes no patch / skip
decision.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Timeline integrity* →
*B-stall warning (A5)*.

**Companions (do not merge):**

- A4 injected-time warning
  ([archive/TEMP-injected-time-detector-plan.md](archive/TEMP-injected-time-detector-plan.md),
  [cli-output.md](../cli-output.md) § *Timeline / duration warnings*) —
  post-decode, lag-disagreement between ≥ 2 consecutive **locked** gaps.
  A5 is scan-time and reads silence, not lag. A4's numbers (T = 150 ms,
  N = 2) and its latched scan-time variant ([settled.md](settled.md) § C)
  are not touched.
- `gap_offset_agreement` (mutual-silence cross-check) — one pair-level
  boolean built from the same B intervals. It fired on pair 54
  (`agrees: false`, Δ 24.8 s) and said nothing locatable. A5 does not
  replace or re-tune it.
- BACKLOG *End-clip constrained search hides a timeline step* and
  *Stall-segmented map* (same § *Timeline integrity*) — separate
  deliverables. This plan is only the warning.

**Media hygiene:** pair index + gap index only.

---

## 0. What this ships (normative)

**When B has a long silent interval inside the mapped overlap and A is
not silent over the same mapped span, report it at scan time as a
possible B stall / corruption, and attribute later skipped gaps to it.**

That is the whole plan. If a later sentence disagrees with this one,
this one wins.

A long silence present in **both** files at the same mapped time is
program (fade-out, intermission, end silence) and is **never** reported.

---

## 1. Motivating case (pair 54)

Measured 2026-09-14, by ear and `ffmpeg silencedetect`:

| Event | Value |
|-------|-------|
| Map before the stall (g001–g011, locked throats at ~+0.15 s lag on the +2.06 s map) | ≈ +2.2 s |
| B digital silence (−91 dB flat) | B 1:42:32.7 – 1:43:39.4 (66.6 s) |
| A over the mapped span | program audio (not silent) |
| g012 (A 1:48:38.1) matching B audio, by ear | ≈ B 1:49:45 (+66.9 s) |
| End of soundtrack: A 2:25:12.4, B 2:26:20.2 | +67.9 s |
| B end silence 8780–8940 s | maps onto A's end silence → mutual, must **not** report |

Today: g012 skips as `boundary correlation below threshold (pre=0.18
post=0.32 min=0.55)` — true but unexplained; ear verification took a
multi-session investigation. A4 cannot fire (one gap after the stall,
and its throats do not lock). The B interval was already in the scan's
B silence map; nothing reported it.

With A5 the run would print one warning naming the stall and g012 would
carry "after B stall at 1:42:32.7 (66.6 s)".

---

## 2. Rule (normative)

Inputs, all already live at scan time: B silent intervals (the B
silence map built for per-gap energy lookup), A silent runs, the pair
alignment offset, the overlap.

1. **Candidate.** A B silent interval `[b0, b1)` with
   `b1 − b0 ≥ stall_min_secs` (default **10.0 s**, § 5) lying inside the
   mapped overlap. Clip to the overlap first; judge length after
   clipping.
2. **Map to A.** `a0 = b0 − offset`, `a1 = b1 − offset`, where `offset`
   is the **cumulative** offset at `b0`: the pair offset plus the lengths
   of every earlier reported stall on this pair. (A later stall must not
   be mapped through a map the first stall already broke.) Record the
   offset used.
3. **A-silence test (the user's rule).** Let `a_silent_frac` = fraction
   of `[a0, a1)` covered by A silent runs (same scanner recipe as B).
   Report **only** if `a_silent_frac < 0.5`. At or above 0.5 the gap is
   in both files — suppress. Mapping error is bounded by the pre-stall
   drift (seconds at most), so a 0.5 cut does not flip on a ≥ 10 s
   interval. Every suppressed candidate is logged as one
   `tracing::debug` line carrying `b_start_secs`, `duration_secs`,
   `a_start_secs`, `offset_used_secs`, and `a_silent_fraction`, so the
   § 5 census can see where the 0.5 cut sits. Debug only: not in stdout,
   JSON, or `GapReport`. A suppressed candidate does not advance the
   cumulative offset (rule 2).
4. **Truncated B scan.** If B's scan did not reach `b1`, do not report
   a candidate that runs off the scanned end (a truncated scan is not
   evidence of silence).
5. **Attribution.** Every gap whose `a_start` is ≥ `a0` of a reported
   stall (and before the next reported stall) carries that stall's
   index. Informational only; reported on patched gaps too.
6. **No decisions.** A5 does not skip, stop, re-map, change the fill
   offset, or feed `collect_repair_warnings`. Re-mapping is the
   *Stall-segmented map* item, not this plan.

---

## 3. Output

### 3.1 Human (stdout, with A1–A4)

After the alignment block, one line per reported stall:

```text
Warning: A5 video B is silent for 66.6s at 1:42:32.7 (B) while A has audio at 1:42:30.5 (A) — possible B stall or corruption; gaps after this point (#13) may be offset by up to +66.6s and fail to fill
```

Also `tracing::warn` on stderr like A1–A3. Available in scan-only and
`dry_run` (no PCM needed).

In the gap table, a skipped gap with an attribution appends
`(after B stall #1)` to its reason. Patched gaps are unchanged in the
table.

### 3.2 JSON (additive, stays v1)

`scan.b_stalls` — optional, absent when empty:

| Field | Type | Meaning |
|-------|------|---------|
| `b_start_secs` / `b_end_secs` | f64 | B silent interval (clipped to overlap) |
| `duration_secs` | f64 | `b_end − b_start` |
| `a_start_secs` / `a_end_secs` | f64 | Mapped A span |
| `offset_used_secs` | f64 | Cumulative offset at `b_start` (§ 2 rule 2) |
| `a_silent_fraction` | f64 | § 2 rule 3 |

Per gap: optional `after_b_stall` (index into `scan.b_stalls`), absent
when no reported stall precedes the gap.

Echo `stall_min_secs` with the other scan-recipe keys. Follow the
[json-output.md](../json-output.md) revision procedure: optional-absent
additive fields, changelog line, regenerate goldens.

---

## 4. Tests

Unit (domain, pure — no decode):

1. **Stall reported.** B silent 60 s at B 100 s; A has no silence at
   A 98 s (offset +2). One stall, `a_silent_fraction = 0`,
   `offset_used = 2.0`.
2. **Mutual silence suppressed.** Same B interval; A silent over
   `[98, 158)`. Nothing reported.
3. **Partial A silence.** A silent over 40 % of the mapped span →
   reported; 60 % → suppressed. Boundary pinned at exactly 0.5 →
   suppressed.
4. **Short silence ignored.** 9.9 s B silence → nothing.
5. **Cumulative offset.** Two B stalls (60 s at B 100 s, 30 s at
   B 500 s). The second maps with offset `2 + 60`, not `2`; a fixture
   where A is silent at `B 500 − 62` but not at `B 500 − 2` must
   **suppress** the second (proves the cumulative map is used).
6. **Truncated B scan.** Interval touching the scanned end → nothing.
7. **Attribution.** Gaps before `a0` carry none; gaps after carry the
   right index; a gap after two stalls carries the later one.
8. **Overlap clip.** B silence straddling the overlap end is clipped
   before the length test.

Integration (`scan_gaps_integration`): synthetic A/B WAV pair with an
inserted B stall → human warning line, `scan.b_stalls`, and
`after_b_stall` on the following gap. Golden JSON regenerated only for
the new optional fields; existing goldens without a stall stay
byte-stable.

---

## 5. Threshold

`stall_min_secs = 10.0` is a starting value, not calibrated. Before
shipping, run a scan-only census on the corpus pairs (scan-only is
cheap; no fingerprint run) with a temporary floor of 2 s and record every
candidate: pair, B start, duration, `a_silent_fraction`, reported or
suppressed (suppressed rows come from the rule 3 `tracing::debug` line
— run with `RUST_LOG=warn,clip_sync_repair=debug`), and whether it is a
real stall by ear. Pair 54's 66.6 s must be reported; B's end silence
must appear as suppressed. If any real stall or program pause has
`a_silent_fraction` near 0.5 (0.3–0.7), revisit the cut before
shipping. Pick the default from that table (lowest value with no
false-positive in the census, rounded up) and record the census in the
archive note. If the census shows ordinary program pauses above 10 s
surviving rule 3, stop and revisit rule 3 before shipping.

---

## 6. Checklist

- [ ] Domain: pure `detect_b_stalls(b_intervals, a_runs, offset, overlap, b_scanned_end, stall_min_secs)` + attribution helper, § 4 unit tests; `tracing::debug` line for each rule 3 suppression (§ 2 rule 3).
- [ ] Scan: call it in `crates/clip-sync-repair/src/application/scan_gaps.rs` after the B silence map (Step 5) and A runs are both live; carry results on `GapReport`.
- [ ] Recipe/config: `stall_min_secs` on the scan recipe (TOML + echo); no CLI flag unless the census asks for one.
- [ ] Output: A5 line in `infrastructure/cli/output.rs`; `(after B stall #n)` on skipped gap reasons.
- [ ] JSON DTOs + [json-output.md](../json-output.md) additive entry + goldens.
- [ ] [cli-output.md](../cli-output.md) § *Timeline / duration warnings*: A5 row, "why", "what to do".
- [ ] § 5 census; set the default; pair 54 re-scan shows the warning and g012 attribution.

---

## 7. Done when

A5 ships on by default with the census-chosen floor, pair 54's scan
names the 1:42:32.7 stall and attributes g012, no mutual-silence
interval in the census is reported, and this plan is archived with a
[settled.md](settled.md) row plus removal of its BACKLOG row.
