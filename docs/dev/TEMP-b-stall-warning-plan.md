# TEMP — B-stall warning (A5)

**Status:** in progress, 2026-09-14. A Change (user-visible warning +
additive JSON field). Scan-time only; changes no patch / skip decision.
Spec pins from the 2026-09-14 review are folded in (§ 2 rules 0, 3, 4;
§ 3; § 5).

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
possible B stall / corruption, and attribute later gaps to it.**

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

With A5 the run would print one warning naming the stall, and g012
would carry the stall's index in JSON.

---

## 2. Rule (normative)

Inputs, all already live in `scan_gaps` once the per-gap loop has run:
B silent intervals (the B silence map built for per-gap energy lookup),
B `scanned_end_secs` and `truncated`, **every** A silent run (the
scanner's runs, i.e. each gap's `video_a_start_secs`–`video_a_end_secs`),
the pair alignment offset, and `start_overlap`.

0. **Preconditions.** No offset, no overlap, or B not scanned
   (`scanned_end_secs` absent) → nothing, and no candidates are logged.
   A5 is **not** gated on `scan_both`: the B scan itself is not, only the
   cross-check is.
1. **Candidate.** A B silent interval `[b0, b1)` with
   `b1 − b0 ≥ stall_min_secs` (default **10.0 s**, placeholder until § 5)
   lying inside the mapped overlap. Clip to the overlap first; judge
   length after clipping.
2. **Map to A.** `a0 = b0 − offset`, `a1 = b1 − offset`, where `offset`
   is the **cumulative** offset at `b0`: the pair offset plus the lengths
   of every earlier reported stall on this pair. (A later stall must not
   be mapped through a map the first stall already broke.) Record the
   offset used.
3. **A-silence test (the user's rule).** Let `a_silent_frac` = fraction
   of `[a0, a1)` covered by the union of **all** A silent runs. Not
   `mutual_silence_intervals_from_gaps` — that is the SharedSilence /
   `!b_has_energy` subset the cross-check uses, sits beside the call
   site, and would under-count A silence and over-report stalls.
   Report **only** if `a_silent_frac < 0.5`. At or above 0.5 the gap is
   in both files — suppress. Mapping error is bounded by the pre-stall
   drift (seconds at most), so a 0.5 cut does not flip on a ≥ 10 s
   interval. Every suppressed candidate is logged as one
   `tracing::debug` line carrying `b_start_secs`, `duration_secs`,
   `a_start_secs`, `offset_used_secs`, and `a_silent_fraction`, so the
   § 5 census can see where the 0.5 cut sits. Debug only: not in stdout,
   JSON, or `GapReport`. A suppressed candidate does not advance the
   cumulative offset (rule 2).
4. **Truncated B scan.** When the B scan **truncated** (hard decode/seek
   error, `b_scan_truncated`), skip a candidate whose clipped `b1`
   reaches `scanned_end_secs` — a stopped scan is not evidence of
   silence. On a completed scan an interval may end exactly at the
   scanned end (pair 54's end silence must reach rule 3 and be
   suppressed there). A soft early stop (scan ended > 2 s before the
   declared duration without an error) is not truncation; the § 5
   census flags any reported stall that ends at the scanned end.
5. **Attribution.** Every gap whose `video_a_start_secs` is ≥ `a0` of a
   reported stall (and before the next reported stall's `a0`) carries
   that stall's index. Informational only; applies to patched gaps too.
   Computed from `gaps` + the reported stalls (pure helper), not stored
   per `Gap`.
6. **No decisions.** A5 does not skip, stop, re-map, change the fill
   offset, or feed `collect_repair_warnings`. Re-mapping is the
   *Stall-segmented map* item, not this plan.

---

## 3. Output

### 3.1 Human (stdout)

With the run-level warnings, one line per reported stall. No `A5`
prefix (A1–A4 strings carry no label). Clock times use the whole-second
`format_timestamp` with `{:.1}s` seconds beside them, as A1–A3 do:

```text
Warning: video B is silent for 66.6s at 1:42:33 (6152.7s) while video A has audio at 1:42:31 (6150.5s) — possible B stall or corruption; later gaps (from #13) may be offset by up to +66.6s and fail to fill
```

- `#n` is the **first** gap attributed to this stall, 1-based table
  number (0-based index + 1).
- When no gap is attributed to it, the tail reads `— possible B stall or
  corruption; no detected gaps follow it`. The warning still fires.

stderr: mirror the `not_applied_count` pattern in
`infrastructure/cli/output.rs` — format once, push to stdout, and
`tracing::warn!` the same string. A5 does not join
`collect_repair_warnings` (A4 precedent), so it gets no stderr for free;
unlike A4 it has no mid-run `phase()` line.

Available in scan-only and `dry_run` (no PCM needed). Where the output
has a per-gap skip-reason column, a skipped gap with an attribution
appends `(after B stall #k)` (k 1-based). Scan-only has no reason
column, so there the attribution is the JSON field only.

### 3.2 JSON (additive, stays v1)

`scan.b_stalls` — optional, absent when empty:

| Field | Type | Meaning |
|-------|------|---------|
| `b_start_secs` / `b_end_secs` | f64 | B silent interval (clipped to overlap) |
| `duration_secs` | f64 | `b_end − b_start` |
| `a_start_secs` / `a_end_secs` | f64 | Mapped A span |
| `offset_used_secs` | f64 | Cumulative offset at `b_start` (§ 2 rule 2) |
| `a_silent_fraction` | f64 | § 2 rule 3 |

Per gap: optional `after_b_stall`, a **0-based** index into
`scan.b_stalls` (human `#1` = JSON `0`); absent when no reported stall
precedes the gap.

`stall_min_secs` is **not** a `ScanRecipe` field: the recipe's contract
is `PartialEq` ⇔ same gap list, and a warning floor cannot move a gap
boundary. It follows the `local_pair_collar_secs` precedent — a
`RepairConfig` TOML key with no CLI flag, carried on `ScanGapsRequest`,
stamped on `GapReport` by every real scan (`None` on legacy/test
constructors), and echoed on the `scan` object with
`skip_serializing_if = "Option::is_none"`. Consequence: goldens built
from constructors that leave it `None` stay byte-stable;
`full_surface_repair.json` (which populates scan-stamped fields) is
regenerated once to pin its presence and a non-empty `b_stalls`.
Follow the [json-output.md](../json-output.md) revision procedure:
optional-absent additive fields, changelog line.

---

## 4. Tests

Unit (domain, pure — no decode):

1. **Stall reported.** B silent 60 s at B 100 s; A has no silence at
   A 98 s (offset +2). One stall, `a_silent_fraction = 0`,
   `offset_used = 2.0`.
2. **Mutual silence suppressed.** Same B interval; A silent over
   `[98, 158)`. Nothing reported; one suppressed candidate.
3. **Partial A silence.** A silent over 40 % of the mapped span →
   reported; 60 % → suppressed. Boundary pinned at exactly 0.5 →
   suppressed. Overlapping A runs count once.
4. **Short silence ignored.** 9.9 s B silence → nothing.
5. **Cumulative offset.** Two B stalls (60 s at B 100 s, 30 s at
   B 500 s). The second maps with offset `2 + 60`, not `2`; a fixture
   where A is silent at `B 500 − 62` but not at `B 500 − 2` must
   **suppress** the second (proves the cumulative map is used). A
   suppressed first candidate does not advance the offset.
6. **Truncation.** Truncated scan: an interval reaching `scanned_end` is
   skipped, an earlier one is still reported. Completed scan: the same
   interval ending exactly at `scanned_end` is a candidate. No scanned
   end → nothing.
7. **Attribution.** Gaps before `a0` carry none; gaps after carry the
   right index; a gap after two stalls carries the later one.
8. **Overlap clip.** B silence straddling the overlap end is clipped
   before the length test.

Scan wiring (`scan_gaps` unit tests with the fake reader): a stall is
stamped on the report; `stall_min_secs` is echoed; no offset → empty;
`scan_both = false` still detects.

Integration (`scan_gaps_integration`): synthetic A/B WAV pair with an
inserted B stall → human warning line, `scan.b_stalls`, and
`after_b_stall` on the following gap.

---

## 5. Threshold (ship gate, not code gate)

`stall_min_secs = 10.0` is a placeholder. Code may land at 10.0; the
user-visible default does **not** ship until this table exists.

Runner: `scripts/measure/scan-registration.ps1` (scan-only,
`--format json`) with `-ScanArgs "-c <census.toml>"` where the TOML sets
`stall_min_secs = 2.0`, plus `RUST_LOG=warn,clip_sync_repair=debug` so
rule 3 suppressions are captured. Record every candidate: pair, B
start, duration, `a_silent_fraction`, reported or suppressed, whether it
ends at the scanned end, and whether it is a real stall by ear.

Named checks: pair 54's 66.6 s is **reported**; pair 54's B end silence
appears as **suppressed**. Pick the default from the table (lowest value
with no false positive, rounded up) and record the census in the archive
note. If ordinary program pauses above 10 s survive rule 3, or any real
stall or program pause has `a_silent_fraction` in 0.3–0.7, stop and
revisit rule 3 before shipping.

---

## 6. Checklist

- [x] Domain: pure `detect_b_stalls(b_intervals, a_runs, offset, overlap, b_scanned_end, b_scan_truncated, stall_min_secs)` returning reported + suppressed, and `attribute_gaps_to_b_stalls`, in `crates/clip-sync-repair/src/domain/b_stall.rs`; § 4 unit tests 1–8.
- [x] Scan: call it in `crates/clip-sync-repair/src/application/scan_gaps.rs` after the per-gap loop (Step 6 neighbourhood; `a_runs` is consumed by that loop, so A runs come from `gaps`); `tracing::debug` each suppression; stamp `GapReport::b_stall`.
- [x] Config: `stall_min_secs` on `RepairConfig` (TOML, validated > 0, no CLI flag) → `ScanGapsRequest` → `GapReport`.
- [x] Output: stall line + `tracing::warn!` in `infrastructure/cli/output.rs` (§ 3.1); `(after B stall #k)` where a reason column exists.
- [ ] JSON DTOs (`scan.b_stalls`, `scan.stall_min_secs`, per-gap `after_b_stall`) + [json-output.md](../json-output.md) additive entry + regenerate `full_surface_repair.json`.
- [ ] [cli-output.md](../cli-output.md) § *Timeline / duration warnings*: A5 row, "why", "what to do".
- [ ] § 5 census; set the default; pair 54 re-scan shows the warning and g012's `after_b_stall`.

---

## 7. Done when

A5 ships on by default with the census-chosen floor, pair 54's scan
names the 1:42:33 stall and its JSON attributes g012, no mutual-silence
interval in the census is reported, and this plan is archived with a
[settled.md](settled.md) row plus removal of its BACKLOG row.
