# Silence-floor / fillability findings ledger

**Opened:** 2026-07-29. **Status:** fixes landed 2026-07-29 (F1–F10/F12; F11 still open; R1–R4 review follow-ups).
Regression tests added 2026-07-29: production-floor scan E2E + F7 agreement, scanner→occupancy/donor
pipeline (digital silence + abs-floor dither), 6ch center-only mid-band occupancy.

Findings from diagnosing an `unfillable` mid-file gap on an uncatalogued licensed 5.1 pair
(A ≈ 6900 s, AAC-LC 48 kHz 5.1). Two production runs plus one direct `astats` measurement of
the donor window. Per the media-hygiene rule, the pair is referred to only by these properties;
timestamps are numeric, raw logs stay in gitignored `gap-files/`.

**Verification rule.** `file:line` references below were read on 2026-07-29 except where marked
`↻ re-verify` — those were read earlier in the same investigation and have not been re-checked
against current source. Re-read any reference before acting on it.

---

## 0. What triggered this

A production run reported 11 gaps: 1 repaired, 1 skipped, 9 `unfillable`. Two of the
`unfillable` gaps were mid-file, which normally only happens at file ends. Their JSON was
self-contradictory:

| A span (s) | `b_has_energy` | `equivalence` | `donor_silence_fraction` |
|---|---|---|---|
| 3164.172 – 3164.828 | `false` | `repairable_dropout` | `0.0` |
| 6729.746 – 6899.989 | `false` | `repairable_dropout` | `0.0` |

`b_has_energy: false` says the donor is silent; `repairable_dropout` with `donor_silence_fraction:
0.0` says the donor is fully occupied. Both are derived from the same B audio.

---

## 1. Evidence

### E1 — Direct measurement of the donor window (decisive)

`ffmpeg -ss 3163.26 -t 0.66 -af astats` on B, over the B-mapped span of the first gap:

| Channel | Peak (dBFS) | RMS (dBFS) | Crest | Max abs level |
|---|---|---|---|---|
| 1 | −84.97 | −96.30 | 3.68 | 0.000056 |
| 2 | −84.88 | −96.37 | 3.75 | 0.000057 |
| 3 | −100.40 | −101.11 | 1.09 | 0.000010 |
| 4 | −100.19 | −100.31 | 1.01 | −0.000009 |
| 5 | −99.90 | −100.95 | 1.13 | 0.000010 |
| 6 | −99.90 | −100.95 | 1.13 | 0.000010 |
| Overall | −84.88 | −98.77 | — | 0.000057 |

Readings:

- Max |sample| across all channels = `0.000057` full-scale = **1.9 LSB at 16-bit**. RMS −98.8 dBFS
  ≈ 0.4 LSB.
- Channels 3–6 have `min level ≈ max level ≈ 0.000009` with crest factor 1.01–1.13 and 0–1 zero
  crossings — a constant DC bias, not audio.
- Crest factors of 1.01–3.75 rule out impulsive content. This is continuous decoder dither /
  reconstruction noise around a digitally-silent source.

**Conclusion: B has no content in this window.** `b_has_energy: false` is correct, and
`unfillable` is the correct verdict — there is nothing to copy *and* nothing that needs copying.
Both sides are silent; this is a shared silent beat in the film.

### E2 — The absolute silence floor was active, not disabled

The human scan header printed `rms floor 0`, which reads as disabled (`0` is documented as
disabling). It is not. `scan_gaps.rs:74-75`:

```rust
if request.absolute_silence_rms > 0.0 {
    line.push_str(&format!(", rms floor {:.0}", request.absolute_silence_rms));
}
```

The value is a normalized amplitude; the default is `33.0 / 32767.0 = 0.001007`
(`config.rs:315-316`). `format!("{:.0}", 0.001007)` → `"0"`. The guard proves the value was
`> 0.0`, so the header is printing an **active** default floor as `0`.

With the floor at 0.001007 (−59.9 dBFS), `silence.rs:314` decides E1's window immediately:

```rust
if absolute_rms_floor > 0.0 && peak < absolute_rms_floor {
    return true;   // 0.000057 < 0.001007 on every channel
}
```

No other silence path is needed to explain `b_has_energy: false`.

### E3 — Two-run diff at `--silence-hold-ms 100` (vs. default 500)

Both problem gaps came back byte-identical at 1/5 the hold, refuting hold bridging as their
cause. But the same diff exposed an unrelated, more serious defect. Same gap start, **identical**
`noise_floor_db` = `−45.851409657193955` in both runs:

| | hold 500 ms | hold 100 ms |
|---|---|---|
| span (s) | 2585.110 – 2586.254 (1.144) | 2585.110 – 2585.729 (0.618) |
| `a_gap_rms_db` | **−52.48** | **−101.48** |
| `a_below_noise_db` | −6.63 | −55.62 |
| class | `shared_silence` → **dropped** | `repairable_dropout` → **kept** |

A 49 dB swing in the A-side dropout depth from a scan knob, flipping a genuine deep dropout into
`shared_silence` and silently removing it from the fill plan.

The hold change also recomposed the gap list while the count coincidentally stayed at 11: one gap
split in two, one vanished, one shortened. Confirms the renumbering hazard the gap-selection plan
warns about (`archive/TEMP-gap-selection-plan.md` §2).

### E4 — Divergence in the opposite direction

New index 4 (2402.345 – 2405.379): `b_has_energy: true` with `donor_silence_fraction: 1.0`. The
two signals disagree in both directions, consistent with §2 F1.

---

## 2. Findings

### F1 — `donor_silence_fraction` ignores the absolute silence floor
**Severity: high. Status: FIXED 2026-07-29 (completed via R1/R2).**

Original pathology: B blocks counted silent only when `rms_db < gap_floor_db`. E1's A gap floor
≈ −101.5 with B dither ≈ −98.8 → donor `0.0` while absolute occupancy correctly said silent.
An abs-floor clamp (`max(gap_floor, abs_floor)`) fixed the floor-on case but left R1: with the
floor disabled, digitally silent blocks at `BLOCK_LEVEL_FLOOR_DB` (−120) fail `rms < −120`.

**Final fix:** donor silence is `BlockLevel::silent || rms_db < gap_floor` — the scanner's
peak/per-channel bit (abs floor baked in at scan time). Occupancy uses the same `silent` bit
(see R2).

### F2 — Hold bridging corrupts the A-side dropout depth, dropping real dropouts
**Severity: high. Status: FIXED 2026-07-29.** `BlockLevel.silent` retained at scan; A-gap RMS aggregates silent blocks only.

`held_count` resets on every silent block (`silence.rs:140`), so hold bridges compose without
bound and a `SilentRun` can span non-silent audio (the repo's own test
`silence_run_scanner_hold_bridges_single_noisy_block` proves the span behavior). The equivalence
gate then takes `aggregate_rms_db` over that span, which is an energy mean dominated by the
loudest included block — one bridged partial-signal block lifts a −101 dB dropout to −52 dB.

`core_*` does **not** guard this. `core_end` is assigned inside the silent branch
(`silence.rs:167`) and advances to the last fully-silent block's end, so bridged non-silent blocks
sit inside `[core_start, core_end]` exactly as they sit inside `[start_secs, end_secs]`. The two
pairs differ only by the sub-block frame walk at each edge. The core fields block *edge-refinement*
widening, which is not the widening that bit here.

**Fix direction:** derive the depth from the `BlockLevel` timeline, excluding blocks that failed
the silence test, rather than from an aggregate over the whole run. `BlockLevel` is pushed before
any silence classification (`silence.rs:126-132`), so it is the one uncontaminated product.

### F3 — `--absolute-silence-rms` unit mismatch
**Severity: high. Status: FIXED 2026-07-29.** CLI normalizes 0–32767 → amplitude; TOML rejects values ≥ 1.

The flag is documented as *"absolute RMS floor for silence detection (0–32767 scale; 0 disables)
[default: 33]"*, and the value is assigned straight through:

```rust
if let Some(rms) = args.absolute_silence_rms {
    config.repair.absolute_silence_rms = rms;
}
```

But the field's default is normalized (`33.0/32767.0`) and it is consumed in the normalized
`[-1, 1]` domain — `silence.rs:314` compares it directly to `peak`. So `--absolute-silence-rms 33`
sets the floor to `33.0` full-scale, i.e. **33× above maximum amplitude: every block silent, every
gap gone.** Any value ≥ 1 has the same effect.

`cli/mod.rs:370` asserts `config.repair.absolute_silence_rms == 25.0` after passing `25`, locking
the broken unit into a test.

**Decision needed:** normalize at the CLI boundary (divide by 32767, keeping the documented
operator-facing scale) or change the flag to accept normalized amplitude and fix the docs. The
first preserves the documented interface and the `[default: 33]` text.

### F4 — Scan header misreports an active floor as disabled
**Severity: medium (diagnostic). Status: FIXED 2026-07-29.** Prints `rms floor 33 (at -60 dBFS)`.

`{:.0}` on a normalized amplitude prints `0`, and `0` is documented as "disables". This sent this
investigation down the wrong path for two rounds — every inference drawn from "the floor is off"
was invalid.

The unit test at `scan_gaps.rs:1146` expects `"rms floor 33"`, but passes only because the test
constructs the request with `absolute_silence_rms: 33.0` (raw sample units, `scan_gaps.rs:1131`)
rather than the config default. It validates the wrong unit and therefore hides the bug.

**Fix:** print enough precision to be unambiguous, or print dBFS. Fix the test to use the config
default. Interacts with F3 — resolve the units question first.

### F5 — Help-text default for the floor is wrong
**Severity: low. Status: FIXED 2026-07-29.** Help shows `[default: 33]` from i16-scale conversion.

Doc comment says `[default: 33]`; the actual default is `0.001007`, and the assertion renders it
`defaults.repair.absolute_silence_rms as u32` = `0`. The needle the test looks for is
`[default: 0]`, so the assertion is effectively vacuous for this flag. Same root cause as F3/F4.

### F6 — `unfillable` conflates two distinct causes
**Severity: medium (operator-facing). Status: FIXED 2026-07-29.** Per-gap labels: `both sides silent` vs `unmapped`.

```rust
pub fn is_fillable(&self) -> bool {
    self.video_b_start_secs.is_some() && self.b_has_energy
}
```

"No B mapping" and "no B energy" produce the same label. For E1's gap the truthful message is
*"both sides silent — nothing to do"*, which reads very differently from `unfillable` and would
not have looked anomalous to the operator. Splitting the label removes the false alarm that
started this investigation.

### F7 — `NotFillable` short-circuits equivalence
**Severity: low, but note the interaction. Status: FIXED 2026-07-29.** Precedence unchanged
(`NotFillable` still wins). `occupancy_agrees_with_donor_silence` asserts
`!b_has_energy ⇒ donor_silence_fraction ≥ donor_silence_thresh` when both are present
(`debug_assert` + `tracing::warn` in `scan_gaps`). Caught the incomplete F1/`rms < −120` case
on first outing. After R2 both signals share `BlockLevel::silent`, so F7 will not catch a
shared-domain regression in the −60…−35 dB band — keep the assert as a safety net for
rms-only donor regressions.

### F8 — Truncated B-side scan fails open, and the warning says the opposite
**Severity: medium. Status: FIXED (Phase B, 2026-07-29); R3/R4 notes below.** Fail-closed past
`b_scanned_end_secs`; truncation surfaced with timestamp. Mid-file fallback propagation is
recorded separately as R4 (not required for B fail-closed).

### F9 — `mutual_silence_intervals_from_gaps` inherits the fillability signal
**Severity: low. Status: FIXED 2026-07-29.** Cross-check intervals now come from
`GapEquivalenceClass::SharedSilence` (donor metric), not `!b_has_energy`. Fillability stays
local to plan-time occupancy. **Coverage caveat:** Prefer `SharedSilence`; when equivalence is `NotEvaluated` (e.g. no A
noise-floor context), fall back to mapped `!b_has_energy` only. Decided classes
(`RepairableDropout` / `AmbientQuiet`) never fall back to fillability.

### F10 — Degenerate and negative ranges
**Severity: low. Status: PARTIAL.** Degenerate occupancy ranges `debug_assert` + fail-closed.
Negative `b_start` is covered for occupancy via `b_range_fully_scanned`'s `start >= 0.0`
(fail-closed / unmapped). Unguarded coords elsewhere may remain.

### F11 — Recipe provenance missing from JSON
**Severity: medium (reproducibility). Status: CONFIRMED by E3 + code read.**

`min_gap_ms`, `silence_hold_ms`, and `absolute_silence_rms` do not appear in `--format json`. The
human header prints them; the machine-readable output does not, which is backwards. E3 shows why
it matters: the same pair produces different gap *composition* under different recipes, and the
JSON cannot say which recipe produced it. Already parked —
`archive/TEMP-gap-selection-sequencing-plan.md` §4, `BACKLOG.md` § Gap-selection parked debt.

### F12 — `SilentRun::core_*` doc comment overclaims
**Severity: low (documentation). Status: FIXED 2026-07-29.** Comment now states hold can place
non-silent blocks inside the core; equivalence must use `BlockLevel::silent`.

---

## 2b. Review follow-ups (R1–R4)

### R1 — F1 incomplete with floor disabled
**Status: FIXED 2026-07-29.** Digitally silent blocks at −120 failed `rms < gap_floor` when
`absolute_silence_rms = 0`. Resolved by counting `BlockLevel::silent` in the donor fraction.
F7's assertion exposed this via failing `scan_both_*` fixtures.

### R2 — Occupancy/donor switched to interleaved RMS re-threshold
**Status: FIXED 2026-07-29.** Both B-silence signals now read `BlockLevel::silent` (scanner
predicate: peak-domain, per-channel, abs floor at scan time). Occupancy:
`any(!silent)` over the window. Donor: `silent || rms < gap_floor`. Restores the original
measurement domain and avoids center-channel dilution false-unfillable in the −60…−35 dB band.

### R3 — `incomplete_prefix` truncation heuristic
**Status: MITIGATED 2026-07-29; media re-check still useful.** Occupancy fail-closes on
`scanned_end_secs` (last PCM fed), not the `b_scan_truncated` flag. A duration over-report of
a few ms (typical audio vs video) does not trip the 2 s belt. The belt previously set
`b_scan_truncated` without a hard error, which could mislabel a complete walk as truncated.
Incomplete-prefix now warns only; `b_scan_truncated` is set only on scan error. Still worth
confirming the live pair's `b_scanned_end_secs` vs gap #11 end (6899.989).

### R4 — Mid-file fallback propagation bundled with F8
**Status: RECORDED.** `media_scan.rs` seek-loop fallbacks now propagate mid-file
`DecodeFailed`/`SeekFailed` (near-end soft EOF kept). Affects A-side and alignment fallbacks,
not only B. B fail-closed occupancy already works via `b_scanned_end_secs` alone. Primary
callers of these fallbacks today are test fakes; production Symphonia sequential scans use a
different path. Kept as an intentional fail-loud policy with its own note — not a silent F8
side effect.

---

## 3. Refuted / withdrawn during this investigation

Kept so they are not re-proposed.

- **Hold bridging causes the mid-file `unfillable` gaps.** REFUTED by E3 — both gaps identical at
  hold 100 ms. Bridging is real (F2) but acts on A's classification, not B's occupancy.
- **`core_*` fixes hold bridging.** REFUTED by code read: `core_end` advances across bridges too
  (see F2).
- **B contains sparse transients (high peak, negligible RMS) tripping the `rms < peak × fraction`
  rule.** REFUTED by E1 — crest factors 1.01–3.75 mean continuous content; the deciding path is
  the absolute floor (E2), not the peak-fraction rule.
- **The absolute floor was disabled in these runs (`rms floor 0`).** REFUTED by E2 — the header is
  misformatting an active default. This is F4.
- **`b_has_energy: false` is the wrong signal for these gaps, and `donor_silence_fraction` is
  better founded.** REVERSED by E1 — `b_has_energy` is correct; the donor metric is the broken one
  (F1).
- **Assert the invariant `!b_has_energy ⇒ donor_silence_fraction ≥ donor_silence_thresh`.**
  Was withdrawn pre-F1 (incomparable thresholds). Restored and implemented as F7 after F1.

---

## 4. Suggested order

1. **F3 + F4 + F5** together — one units decision, then the display and the two tests that lock in
   the wrong unit. Cheapest, and F3 is a live foot-gun for anyone who passes the flag as
   documented.
2. **F1** — the defect that produced the misleading output. Small, and the floor it needs already
   exists in `cross_check.rs`.
3. **F2** — largest blast radius (changes which gaps get repaired on real media), so it wants a
   fixture and a re-run of the pair before and after.
4. **F6** — operator-facing label split; removes the false alarm.
5. **F7 / F8 / F11** — after F1 and F2 settle, since each depends on their outcome.
6. **F9 / F10 / F12** — cleanup.

## 5. Open question

Whether gap #2's skip (`pre=0.04 post=0.04 min=0.12`, anchor ceiling 0.13 against a required
`min_fill_correlation` of 0.35) is a third, unrelated issue. It is not an ambient-quiet near-miss —
equivalence says `repairable_dropout` with donor `0.0`, which per F1 means the equivalence signal
cannot be trusted for it either. A gap fingerprint (`--fingerprint-gap 2 --fingerprint-diagnostics`)
would settle whether it is the decorrelated regime. Not yet run.
