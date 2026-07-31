# Silence-floor / fillability findings ledger

> # ARCHIVED 2026-07-30 — closed, do not update
>
> Every finding here is fixed, delegated, or refuted. Two findings that were open at archival
> time were **split into their own live ledger** rather than resolved here:
> [TEMP-equivalence-divergence-findings.md](TEMP-equivalence-divergence-findings.md) —
> **F15** (high, scan-time vs fill-time equivalence contradict each other on one gap) and **F14**
> (medium-high, the fingerprint corpus records `skip` where production patches via dual-fit).
> Their IDs are unchanged, so the cross-references below still resolve. That ledger has since been
> **closed and archived too** (2026-07-30, now a sibling here); its own remaining items moved on to
> [TEMP-equivalence-instrument-convergence.md](TEMP-equivalence-instrument-convergence.md).
> **F11** is delegated to
> [../TEMP-scan-recipe-plan.md](../TEMP-scan-recipe-plan.md).
>
> Kept for the rationale, which survives nowhere else: §3's refuted/withdrawn hypotheses
> (including two that were *re-reversed* on media), §6a's three-way differential showing the fixes
> are safe on previously-fingerprinted media, and §6c's refutation of the alignment-drift theory
> plus the failed-seek mechanism that probably produced it. Outbound doc links are relative to
> `docs/dev/archive/`.

**Opened:** 2026-07-29. **Status:** fixes landed 2026-07-29 (F1–F10/F12 fixed; F11 delegated to
[../TEMP-scan-recipe-plan.md](../TEMP-scan-recipe-plan.md); R1–R4 review follow-ups).
Media follow-ups ran 2026-07-30 (§6): the three-way fingerprint differential **passed**, R3 is
**closed**, §5 is **answered**, and two things reversed — **E1 is refuted** (it measured the wrong
window, so §0's original conclusion was wrong in the direction the fixes already corrected) and the
alignment-drift hypothesis **F13 is refuted** (§6c). Regression tests added 2026-07-29:
production-floor scan E2E + F7 agreement, scanner→occupancy/donor pipeline (digital silence +
abs-floor dither), 6ch center-only mid-band occupancy.

**On §0's premise.** It is closed for the instances in evidence, **not for the class** — F15 (now
in the split-out ledger) is a surviving path where two signals off the same B audio still
disagree.

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

### E1 — Direct measurement of the donor window (~~decisive~~ REFUTED 2026-07-30 — wrong window)

> **REFUTED 2026-07-30.** The `astats` numbers below are real, but they were taken over
> `[3163.26, 3163.92]`, which is **not** this gap's B-mapped span. A fingerprint of the same gap
> (table #10, `--fingerprint-gap 10 --fingerprint-diagnostics`) puts the mapped span at
> `[3162.550, 3163.206]` — disjoint from, and immediately preceding, the window measured here.
> At the *nominal* mapping B is fully occupied: `donor_interior_nominal` = **−43.77 dB,
> `silence_fraction` 0.0, `continuous` true**, and `b_levels.profile_db` runs −45 to −76 dB.
> Post-fix, the gap reports `b_has_energy: true` and **patches**. See §6.

`ffmpeg -ss 3163.26 -t 0.66 -af astats` on B, over what was believed to be the B-mapped span of the
first gap:

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

~~**Conclusion: B has no content in this window.** `b_has_energy: false` is correct, and
`unfillable` is the correct verdict — there is nothing to copy *and* nothing that needs copying.
Both sides are silent; this is a shared silent beat in the film.~~

**Withdrawn.** The conclusion held only for the window actually measured. B *does* have content at
the mapped span, `b_has_energy: false` was the wrong verdict, and the gap is repairable. What E1
measured is the ~0.7 s immediately before the donor — plausibly a genuine silent beat, which is why
the reading looked self-consistent and stopped further checking.

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
warns about (`TEMP-gap-selection-plan.md` §2, same folder).

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
**Severity: low. Status: FIXED 2026-07-29.** Occupancy was already covered by
`b_range_fully_scanned` (degenerate `start < end` + negative `start >= 0.0`, fail-closed). The
residue — "unguarded coords elsewhere" — was audited and closed by giving `Gap` one guarded
accessor instead of per-call-site clamps:

```rust
pub fn mapped_b_span(&self) -> Option<(f64, f64)>   // domain/gap.rs
```

Same predicate shape as `b_range_fully_scanned` (both `Some`, `start < end`, `start >= 0.0`),
minus the coverage limit. Two real defects it retires, both in the fingerprint path
(`gap_fingerprint/measure.rs`):

- **Mixed-timeline span.** `video_b_end_secs.unwrap_or(video_a_end_secs)` paired a B start with an
  **A** end on a half-mapped gap, so the extract window silently spanned two timelines.
- **Clamp/report disagreement.** `extract_start` was clamped with `.max(0.0)` while
  `gap_offset_secs` was derived from the *unclamped* `b_start`, so a negative mapped start produced
  an extract window and a reported offset that disagreed.

`mutual_silence_intervals_from_gaps` (`cross_check.rs`) also moved from "has a B start" to "has a
usable B span" — a deliberate tightening: half-mapped gaps no longer contribute cross-check
intervals. Covered by four unit tests in `domain/gap.rs`, one of which pins the accessor against
`b_range_fully_scanned` so the two predicates cannot drift.

### F11 — Recipe provenance missing from JSON
**Severity: medium (reproducibility). Status: CONFIRMED by E3 + code read.**

`min_gap_ms`, `silence_hold_ms`, and `absolute_silence_rms` do not appear in `--format json`. The
human header prints them; the machine-readable output does not, which is backwards. E3 shows why
it matters: the same pair produces different gap *composition* under different recipes, and the
JSON cannot say which recipe produced it.

**Delegated to [../TEMP-scan-recipe-plan.md](../TEMP-scan-recipe-plan.md)** (that plan's §1 "the visible
one" is this finding; its `GapScanJson` checklist item closes it). That plan was **unparked
2026-07-30** for script same-recipe equality; F11 lands with it. The flat-echo interim in
`TEMP-gap-selection-sequencing-plan.md` §3 (same folder) is no longer needed. Tracked in `BACKLOG.md`
§ Gap-selection parked debt until the recipe checklist ships.

### F12 — `SilentRun::core_*` doc comment overclaims
**Severity: low (documentation). Status: FIXED 2026-07-29.** Comment now states hold can place
non-silent blocks inside the core; equivalence must use `BlockLevel::silent`.

### F14, F15 — SPLIT OUT 2026-07-30
Both were opened 2026-07-30 from the §5 follow-up and were the only items still open when this
ledger was archived. They now live in
[TEMP-equivalence-divergence-findings.md](TEMP-equivalence-divergence-findings.md), IDs
unchanged:

- **F15** (high) — scan-time equivalence reports `donor_silence_fraction: 0.10` on a donor that
  three other measurements put at 0.58–0.87, admitting the §5 gap as `repairable_dropout` while
  fill-time says `shared_silence, drop: true`. F1's shape on a path R2's unification did not reach.
- **F14** (medium-high) — the fingerprint corpus records `outcome.tier: skip` for the 1050.82 s gap
  that production patches via dual-fit, from the same binary and flags, in a file that itself
  carries `gate_pass: true`. Corrupts the calibration oracle and the per-gap filenames.

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
  rule.** ~~REFUTED by E1~~ — **refutation withdrawn 2026-07-30**: E1 measured the wrong window, so
  its crest factors say nothing about the donor. Still believed false, but now on the stronger
  ground that the donor is continuous at the mapped span (`continuous: true`,
  `longest_silence_ms: 0.0`); the deciding path remains the absolute floor (E2), not the
  peak-fraction rule.
- **The absolute floor was disabled in these runs (`rms floor 0`).** REFUTED by E2 — the header is
  misformatting an active default. This is F4.
- **`b_has_energy: false` is the wrong signal for these gaps, and `donor_silence_fraction` is
  better founded.** ~~REVERSED by E1 — `b_has_energy` is correct; the donor metric is the broken
  one (F1).~~ **Re-reversed 2026-07-30**: with E1 refuted, the original claim was right on its first
  half. `b_has_energy: false` *was* wrong for gap #10 — the donor is occupied and the gap now
  patches. F1 is still a genuine defect in `donor_silence_fraction`; the two signals were simply
  both broken, which is why neither could arbitrate the other. This is the finding §0 was reaching
  for.
- **Assert the invariant `!b_has_energy ⇒ donor_silence_fraction ≥ donor_silence_thresh`.**
  Was withdrawn pre-F1 (incomparable thresholds). Restored and implemented as F7 after F1.

---

## 4. Order — EXECUTED 2026-07-29

Ran as planned; kept as the record of what shipped in which wave. Only F11 left the list (delegated,
see above) and F10 shipped partial. Source re-verified against this ledger 2026-07-29 (444 crate
unit tests green).

1. ~~**F3 + F4 + F5**~~ — done. Units normalized at the CLI boundary (the documented 0–32767 scale
   kept); header prints `rms floor 33 (at -60 dBFS)`; both tests that locked in the wrong unit
   rebased.
2. ~~**F1**~~ — done. Donor silence is `BlockLevel::silent || rms_db < gap_floor`.
3. ~~**F2**~~ — done. A-gap RMS aggregates silent blocks only, so hold-bridged non-silent blocks can
   no longer lift a −101 dB dropout to −52 dB.
4. ~~**F6**~~ — done. `unfillable_label()` splits `both sides silent` from `unmapped`.
5. ~~**F7 / F8**~~ — done (F7 caught R1 on its first outing). **F11** delegated to
   [../TEMP-scan-recipe-plan.md](../TEMP-scan-recipe-plan.md).
6. ~~**F9 / F10 / F12**~~ — done. F10 closed on a second pass via `Gap::mapped_b_span` after the
   audit found two live instances in the fingerprint path.

## 5. Open question — ANSWERED 2026-07-30

~~Whether gap #2's skip (`pre=0.04 post=0.04 min=0.12`, anchor ceiling 0.13 against a required
`min_fill_correlation` of 0.35) is a third, unrelated issue.~~

**First: the gap this section describes is not row #2.** The row numbers shifted with the gap list,
and the identifying figure is `min=0.12` — the *applied* threshold, distinct from the 0.35 named in
the same sentence. Under HEAD the only gap that still hard-skips is **2585.11–2586.25 s**, which
production's own operator log labels **gap #6**:

```
pre_correlation 0.0223  post_correlation 0.0213  min_correlation 0.12
best_attempt { pre 0.0507, post 0.0499, source: anchor }
```

Rows #2 and #3 were fingerprinted first on row-number reasoning and both turned out to **patch**, so
neither is this gap. (That detour is what produced §6c and F14, so it was not wasted.)

**Answer: not decorrelated, and not a third unrelated issue — the donor is partly silent.** The
`RUST_LOG=debug` production pass gives the decline reason directly:

```
dual_fit: seam-local peaks pre_r=0.9969 pre_lag=-2074 post_r=0.9998 post_lag=-53
dual_fit: declined — aligned donor bridge is not continuous
          silence_fraction=0.5833  longest_silence_ms=350.0
```

The shoulders match essentially perfectly (0.997 / 0.9998), so this is not decorrelation. The bridge
between them is **58% silent with a 350 ms contiguous silent stretch** — B has no usable content
across most of this gap, and dual-fit correctly refuses to splice a bridge that is mostly silence.
It is a partial-shared-silence gap, the same family as §0's premise, and the refusal is right.

**Identity confirmed by fingerprint** (`--fingerprint-gap 6`): the emitted
`..._g005_full_skip.json` has best bracket `seam_pre 0.05069 / seam_post 0.04987`, matching the
preview's `best_attempt { pre 0.0507, post 0.0499, source: anchor }` exactly. Supporting numbers:
`donor_interior_nominal` `silence_fraction 0.8696 / longest_silence_ms 650 / continuous false`,
`splice.step_ms` 323.5, 20 brackets all `waveform_floor`.

This closes §5 as a question about *mechanism*. It does **not** close the question of why the
scan-time gate calls this donor 10% silent when it is ~87% silent — that is **F15**, now tracked in
[TEMP-equivalence-divergence-findings.md](TEMP-equivalence-divergence-findings.md), and it is
the more serious of the two.

---

## 6. 2026-07-30 media follow-ups

Three runs on the HEAD binary (`--features calibration,he-aac`; see §7 for why the feature is
mandatory). Raw output in gitignored `gap-files/silence-floor/{fp,fp10}/` and
`gap-files/silence-floor-diff/`.

### 6a. Three-way fingerprint differential — PASSED

To check the F1–F12 fixes against media that *was* pinned under the old code, corpus pair 1 was
re-fingerprinted at two commits and compared to its 2026-07-26 reference dump:

- **A** = 2026-07-26 reference, **B** = `dcae0441` (last commit before this series), **C** = `HEAD`.
- **B − A: bit-identical** across all 17 gaps and all 15 sections. The 29 intervening commits and a
  rebuilt toolchain change nothing, so every delta below is attributable to these fixes alone.
- **C − B: confined entirely to `scan_equivalence`.** Zero movement on `geometry`, `levels`,
  `silence`, `contour`, `anchors`, `brackets`, `baseline_lag`, `residual`, `donor_interior`,
  `splice`, `splice_dualfit`, `outcome`, or the fill-time `equivalence` block. Per-gap filenames are
  byte-identical, outcomes included.

| gap | movement | reading |
|---|---|---|
| 0, 7, 9 | donor fraction only | F1/F2 — both B-silence signals now read the same `BlockLevel::silent` domain |
| 6, 8, 10 | `a_gap_rms_db` −0.8 to −3.1 dB, donor fraction | the F2/F3 floor fixes |
| 10 | class `shared_silence` → `ambient_quiet`, `drop` unchanged | reclassification, no decision consequence |
| 16 | class `shared_silence` → `not_evaluated`, **`drop` true → false** | F8 coverage fail-close |

Gap 16 is the one behavior change and it retires a fabricated verdict: its mapped B span ends at
8280.26 s while B is 8159.98 s long (`b_scanned_end_secs`, not truncated). The old code reported
`donor_silence_fraction: 0.999` / `shared_silence` over 120 s of audio that does not exist, and
**dropped the gap on that basis**. It is unfillable anyway (`b_has_energy: false`), so `outcome` is
unchanged — but the reasoning was invented. Visible in the run banner as "16 of 17 repairable"
(B) → "15 of 17" (C).

**Conclusion: the fixes are safe on previously-fingerprinted media.** No decision or geometry
regression; the single decision that moved, moved from a fabrication to a refusal.

### 6b. R3 — CLOSED

From `scan-postfix.json`: `b_scanned_end_secs` = 6899.989, `b_scan_truncated` absent ⇒ false, max
mapped B end = 6898.37. Nothing fail-closes on coverage for this pair.

### 6c. Alignment-drift hypothesis (would-be F13) — REFUTED

**Proposed and killed the same day; recorded so it is not re-proposed.** `scan-postfix.json` reports
start-clip offset −1.6216 and end-clip offset −4.2682 (`offsets_consistent: false`), while every gap
is mapped with the uniform start offset. Linear interpolation predicts ~0.27 s of donor
misregistration at t=1050 s and ~1.2 s at t=3164 s, which would have explained the whole
short-gap-contradiction pattern.

**Direct measurement refutes it at both timestamps:**

| gap | predicted error | measured |
|---|---|---|
| #2 @ 1050.82 s | ~0.27 s | `lag` peak at **−18 samples (−0.375 ms)**, ±600 ms search |
| #10 @ 3164.17 s | ~1.2 s | `baseline_lag` **`lag0_r` 0.9933 / 0.9960** |

`lag0_r` is correlation at exactly the nominal mapping; 0.993 is arithmetically impossible under a
1.2 s misregistration. The drift figures in the alignment block do not describe where the donor
actually sits, and the uniform-offset mapping is correct at both a mid-file and a late timestamp.

**Likely mechanism for the phantom drift.** Every fingerprint run logs, at `ERROR`, a failed seek on
B inside the *end-clip* alignment window:

```
failed to seek in media track=2 detail=… seek to 5994.732s on track 2 failed:
  malformed stream: mkv (ebml): the element is not an ancestor of the current element
```

The end clip is `[5999, 6899]`, so the −4.2682 end offset is produced through an error path on a
container B cannot seek cleanly. That would explain an end-clip offset that disagrees with the
start clip while the donor mapping is in fact correct throughout. Stated as a hypothesis — it has
not been confirmed that the fallback yields the bogus number — but it is the obvious next place to
look, and it means `offsets_consistent: false` on this pair should not be read as evidence of drift.
(Not visible under `RUST_LOG=clip_sync_repair=debug`: the error is emitted by the `clip_sync` crate
and that filter excludes it. Use `RUST_LOG=debug` to see it.)

*Caveat on the evidence:* `baseline_lag` entries carry `window_ms: 0, max_lag_ms: 0`, so
`peak_lag_samples: 0` is forced by the search radius and is **not** independent support. The
refutation rests on `lag0_r` alone (sufficient, but a single statistic). The ±600 ms `lag`
diagnostic is emitted only for gaps that go deep; #10 patched, so it was not computed there.

---

## 7. Reproducing these runs

Cost two failed runs to rediscover, so it is recorded here as well as in the run-protocol note:

- Build with **`--features calibration,he-aac`**. `calibration` gates the `--gap-fingerprints` /
  `--fingerprint-gap` / `--fingerprint-diagnostics` flags (otherwise
  `unexpected argument '--gap-fingerprints'`). `he-aac` is required **even for plain AAC-LC media**:
  `codec_registry.rs:34-47` registers *any* AAC decoder only under `he-aac`/`ac3`, so a `default =
  []` build fails with `alignment failed: no decodable audio tracks` — presenting as exit 0, an
  empty scan JSON, and zero fingerprints.
- **One pair at a time.** Peak RSS is ~15 GB for a ~2.3 h 5.1/48 kHz pair (characterization
  materializes the whole B track: `secs × rate × channels × 4 B`). Two concurrent runs OOM with
  `memory allocation of N bytes failed`.
- Recipe knobs need no flags — the defaults (`min_gap` 500 ms, block 100 ms, rms 33/32767, hold
  500 ms) already equal the 2026-07-26 reference `scan_recipe`. Pin `--silence-hold-ms 500`
  explicitly anyway, because the manifest's `scan_recipe` does not record it (that is F11 in the
  flesh).
- Match `--fingerprint-diagnostics` to the reference you intend to diff against. The 2026-07-26
  reference was produced **without** it; adding it there would break comparability. It is correct
  for a pair with no reference, where the Tier-3 fields (`lag`, `b_levels`, `seam_probe`,
  `wide_envelope`) are what answer the question.
