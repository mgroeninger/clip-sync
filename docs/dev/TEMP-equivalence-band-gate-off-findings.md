# TEMP — Equivalence margin band: gate-off experiment findings

**Status:** working notes, 2026-08-02. **DELETE when the band decision lands — do not archive.**
Anything worth keeping should be promoted into `docs/dev/gap-fingerprint.md`, `BACKLOG.md`, or the
threshold docs *before* this file is removed.

**Media hygiene:** corpus pairs are referred to by index only. No filenames, titles, or paths here or
in any artifact derived from this work. Raw dumps and logs stay under gitignored `/gap-files/`.

---

## 1. What ran

Two corpora are in play:

| corpus | what it is |
|---|---|
| `gap-files/2026-07-31-...` (39 pairs) | full fingerprint run, gate **on**, binary **predates** `thresholds` |
| `gap-files/2026-08-01-v.0.5.1-band-test` (10 pairs, 21 gaps) | the gate-off experiment, binary **records** `thresholds` |
| `gap-files/2026-08-01-v.0.5.1-band-test-with-listen` (8 pairs, 12 gaps) | the §4 listen pass — same gaps re-run with `--gap-listen`, so each has A / B / patched WAVs alongside its dump. The evidence for §2.5, §3.1a and §3.3. |

The 21 gaps are the union of two independently-derived sets over the 39-pair corpus:

- **band set** — 16 gaps / 7 pairs: production drops that a ±1.0 dB dropout / ±1 donor-block margin
  would keep.
- **dangerous divergences** — 10 gaps / 8 pairs: scan gate drops, diagnostic front-end keeps.

Overlap is **5 gaps** (10/11, 14/9, 14/13, 25/36, 28/22) → 21 total. That two unrelated criteria
select overlapping gaps is itself corroboration; it was not designed in.

All 21 came back `plan_kind: fillable`, confirming `--no-skip-equivalent-gaps` was in force and the
experiment actually ran.

---

## 2. Findings

### 2.1 `thresholds` provenance works, and the earlier assumption was right

All 21 gaps record `{dropout_margin_db: 35.0, donor_silence_thresh: 0.5}`. Present on every decided
class, absent (`null`) on `not_evaluated`. That is the designed semantics — presence answers "was a
comparison made" — observed on real data for the first time.

`equivalence-calibration --band` reads these dumps and reproduces the 16-gap list **exactly** (same
pairs, same one-based tokens) as the list originally derived from a threshold-injected scratch copy.
The derivation loop is closed.

Pair 10's measurements are also **bit-identical** to the 2026-07-31 dump across all 13 comparable
gaps (`a_below_noise_db`, block counts, class, drop). The new binary perturbed no measurement.

### 2.2 The seam gate already catches most equivalence drops

Of the 21 gate-dropped gaps, with the equivalence gate off:

- **8 → patch**
- **13 → skip**, every one `skip_reason: correlation_below_threshold`, `dual_fit_rescue: false`

Turning the gate off does not mean 21 bad patches. It means 8 patches plus 13 declines the
downstream seam gate makes for free. This is the first direct measurement of the cost asymmetry the
band was designed around (false drop = shipped hole; false keep = one declined attempt) rather than
an assumption about it.

### 2.3 Band-specific yield

| set | size | patch | skip |
|---|---|---|---|
| band (16) | 16 | **5** | 11 |
| dangerous-only (5) | 5 | 3 | 2 |

Band patches: 10/11, 14/9, 18/49, 25/1, 33/17.

Adopting the band on this sample buys 5 repair attempts at the cost of 11 wasted ones.

### 2.4 Pair 25 distorts the yield

9 of the 16 band gaps are pair 25, and only 1 of those 9 patched. All nine share donor ≈ 0.5 and
`step_ms` 246–600 ms — the timing-offset signature (see `w5-timing-offset-gap-class`). This looks
like one systematic registration problem counted nine times, not nine independent boundary cases.

**Excluding pair 25 the band is 4 patches from 7 rescues**, a materially different proposition.
Any sizing of the band should report with and without pair 25.

### 2.5 The donor boundary is degenerate *and* the measurement feeding it is misregistered

**7 of the 21 gaps sit at donor fraction exactly 0.500**: 6/12, 2/4, 3/6, 5/10, 4/8, 7/14, 5/10.
Within the 12-gap listen subset it is **6 of 12** (10/11 = 6/12, 14/9 = 2/4, 25/1 = 3/6, 25/8 = 5/10,
28/22 = 5/10, 34/1 = 6/12).

Block counts are 4–14, so 0.5 is the most probable rational value the measurement can produce. The
strict `<` in `classify_gap_equivalence` (`gap_equivalence.rs:441`) decides a third of this sample by
itself, on an exact tie.

**Why so many land on the tie: the donor window is measured at the wrong place.** The 2026-08-02
listen run makes this checkable, because `--gap-listen` writes B's mapped span with the same context
as A's. Re-deriving the true A↔B lag from those WAVs (1 ms-step Pearson on the 2 s pre-gap mono
window, ±1 s) and re-measuring B *inside the gap* at that lag:

| gap | fitted lag | A in gap | B @ nominal | B @ fitted lag | Δ(A,B) |
|---|---|---|---|---|---|
| 10/11 | +28 ms | −65.5 | −65.5 | −65.5 | 0.0 |
| 12/7 | −81 ms | −51.5 | −38.0 | −51.5 | 0.0 |
| 14/9 | −119 ms | −70.7 | −38.5 | −70.7 | 0.0 |
| 14/13 | −122 ms | −58.3 | −53.3 | −58.3 | 0.0 |
| 18/49 | +341 ms | −85.8 | −48.5 | −85.1 | 0.7 |
| 25/1 | +332 ms | −88.8 | −69.1 | −88.8 | 0.0 |
| 25/8 | +410 ms | −85.4 | −66.1 | −85.4 | 0.0 |
| 25/36 | +347 ms | −89.0 | −52.6 | −89.0 | 0.0 |
| 28/22 | +47 ms | −50.3 | −50.3 | −50.3 | 0.0 |
| 33/17 | −19 ms | −108.4 | −72.4 | −72.2 | **36.2** |
| 34/1 | −115 ms | −55.7 | −52.7 | −55.7 | 0.0 |
| 34/24 | −7 ms | −66.3 | −65.1 | −66.4 | 0.1 |

**On 11 of 12, B matches A to ≤0.7 dB once registered** — these are mutual program silence, exactly
as the ear reports (§3.1a). The "half-occupied donor" the classifier sees is 13–37 dB of adjacent
program dragged into a 4–12 block window by a mapping error of 80–410 ms. Recomputing the donor
fraction at the fitted lag raises it on 11 of 12 (+0.08 to +0.57); **every** boundary case moves
*further into* `shared_silence`, and 14/13 — the one gap the gate kept, at 3/7 = 0.429 — reads 0.625
once registered.

Pair 25's three listened gaps give +332 / +410 / +347 ms, a consistent ~360 ms systematic error.
§2.4's "one registration problem counted nine times" is now measured, not inferred.

**This retires the tie-break proposal.** `<=` on the donor comparison would flip all 6 exact-0.500
ties in the listen set to *occupied*; all 6 are dropouts on the A axis, so all 6 become
`repairable_dropout` → keep. The ear says all 6 are mutual silence. The one-character change moves
6 of 12 correct drops to false keeps — do not ship it.

The same argument sinks the ±1-block band: one block is 8–25 % of a 4–12 block window, so ±1 flips
roughly 8 of the 10 `shared_silence` gaps here. The boundary is not fuzzy and it is not too tight;
it is being fed a number measured in the wrong window.

**The candidate that survives:** measure `donor_silence_fraction` at the **fitted** lag rather than
the nominal offset map. That moves every gap in this set away from the boundary in the correct
direction, would have caught 14/13, needs no new tunable, and makes the ±1-block question moot. It
is also the same defect class as `dualfit-revalidation-window-bug` (A7) — a correct comparison run
on the wrong window. Raising donor block resolution is still worth doing, but it is second-order: it
sharpens a boundary that is currently fed bad data.

---

## 3. Concerns / open

### 3.1 ~~Nothing has been heard~~ — RESOLVED 2026-08-03, see §3.1a

~~No audio was rendered — the fingerprint path never muxes.~~ A `--gap-listen` run over 12 of the 21
(`gap-files/2026-08-01-v.0.5.1-band-test-with-listen`) supplied the A / B / patched WAVs. The
concern below stands as written only for the 9 gaps that run did not cover.

The band decision should not be made on structure alone; the A3 dual-fit work set the precedent of
an ear check, and this is the same class of claim.

### 3.1a What the ear check found

**Verdict: none of the 12 is a legitimate dropout worth patching, and the patches are wrong.**

- On 11 of 12, A and B carry the same content in the gap to ≤0.7 dB once registered (§2.5). These
  are program silence present in both masters — the `shared_silence` cell, correctly identified.
- All 8 gate-off patches **raise** the level inside the silence, by 9 to 46 dB, and several rewrite
  seconds of surrounding A:

  | gap | A in gap | patched | Δ | frames the splice rewrote (rel. gap start) |
  |---|---|---|---|---|
  | 33/17 | −100.6 | −54.2 | **+46.3** | −0.01 … +0.98 s |
  | 25/1 | −80.1 | −44.1 | **+36.0** | −0.01 … +0.71 s |
  | 14/9 | −62.9 | −43.5 | +19.4 | −2.46 … +0.92 s |
  | 34/24 | −58.9 | −45.5 | +13.4 | −0.01 … +2.41 s |
  | 10/11 | −58.5 | −46.1 | +12.4 | −0.01 … +1.30 s |
  | 18/49 | −80.4 | −68.7 | +11.7 | −0.86 … +0.82 s |
  | 34/1 | −47.9 | −37.4 | +10.5 | −0.56 … +2.44 s |
  | 12/7 | −46.8 | −37.9 | +8.9 | −0.01 … +3.43 s |

The listener's report was independent of and prior to the measurement, and agreed with it: the pairs
sound identical, and on close listening the patch on 33/17 is audibly wrong.

### 3.2 The residual axis abstains — and *why* is itself evidence

`residual.informative` is `false` on **20 of 21** (34/24 is `true`), so the residual gate has no
opinion on almost all of these. But that is not missing data; read what it means before quoting it.

`residual_verdict_informative` (`domain/policies/seam_residual.rs:635`) is a **same-master regime
detector**, not a quality score: every *measured* side must have `floor_db ≤ floor_ok_db`
(`DEFAULT_RESIDUAL_FLOOR_OK_DB = −15.0`); unmeasured sides are ignored; false if no side was
measured. When false, `apply_residual_to_confidence` (`gap_fill_fit.rs:216`) returns the Pearson tier
unchanged — the residual never influences the seam decision.

Two traps when reading `residual` out of a dump:

1. **`−120.0` is a sentinel, not a deep cancellation.** `finite_db()` (`measure.rs:487`) maps any
   non-finite dB to `SILENCE_FLOOR_DB = −120`. `floor_probe_informative` also requires `is_finite()`,
   so a `−120` can never count as informative *even if it got there by cancelling to −inf*. In the
   dump `−120` is ambiguous between "no probe ran" and "cancelled perfectly".
2. **The finite floors here are −0.003 … −3.4 dB on 19 of 21** — essentially *no cancellation*. Only
   12/7 (−31.1) and 34/24 (−20.5) show a real same-master floor on any side.

**Do not read those near-zero floors as "B doesn't match A."** Window and lag reach were both
verified in source:

- **The probe window is on the shoulders, never in the gap.** `walk_reference_frames`
  (`seam_residual.rs:213`) starts one `standoff_frames` clear of the edge and walks *outward* in
  `step_frames` steps up to `max_walk_frames` (production `sample_rate * 3` = 3 s,
  `patch_region.rs:1132`), stopping at the first window whose A peak clears
  `absolute_silence_rms * SEAM_FLOOR_ENERGY_MARGIN` (4.0). So the floor is always measured on
  energetic A content. `source` records `Border` vs `Walked`.
- **But the floor searches only ±10 ms.** It is measured at the *nominal* delta with the lag search
  centred on 0 (`chosen_and_floor_on_window:368`), radius `DEFAULT_RESIDUAL_LAG_SECS = 0.010`
  (`residual_gate.rs:44`). The `step_ms` on these gaps is **246–600 ms** — one to two orders of
  magnitude outside that window.

So the correct reading is *uncorrelated at the lags tried*, not *uncorrelated*. The probe never
reaches the alignment where cancellation would occur. That is consistent with the registration /
timing-offset story and is independently corroborated by `beyond_lag_reach()`
(`seam_residual.rs:798`), which abstains for the same reason on the placement axis — but it is
**not** evidence that the donor content is wrong, and must not be quoted as such.

### 3.3 The two internally-inconsistent patches — both resolved, and 33/17 is a defect

Both were "confirm rather than assume" items. The listen run confirmed them; neither is a metric
artifact.

**18/49 — explained by registration, not a seam bug.** `splice.post_peak_r` 0.136 with `step_ms`
−134.9 against `splice_dualfit`'s 0.982 / 0.993 was the widest internal disagreement in the set. The
WAV says the nominal donor window is off by **+341 ms**: B reads −48.5 dB there but −85.1 dB at the
fitted lag, against A's −85.8 dB. The shoulder that "does not correlate" is the one measured on
displaced content. Same root cause as §2.5; nothing seam-specific to fix here. (Caveat: the shoulder
fit itself is weak on this gap, r = 0.19 — the level agreement is the stronger evidence.)

**33/17 — a real dropout, perceptually nothing, and the repair is worse than the hole.** This is the
one gap in the set where B genuinely does *not* match A, and it is the most important finding in the
file.

The passage is extremely quiet: a −67…−69 dB bed of sparse clicks (peaks 40–90 LSB, occasional
250–1400), one voice event at the clip head (−47 dB) and one at −52 dB shortly before the gap.
A and B correlate at **r = 0.988 at +10.8 ms** over the post-gap shoulder — same rip. Inside the gap:

| 3.00–3.62 s | RMS | peak (16-bit LSB) |
|---|---|---|
| A | **−101.5 dB** | **1–2** |
| B | −65…−68 dB | 60–410 |
| patched | −54 dB | **1632** |

**A goes to hard digital zero for 620 ms while B continues the same click bed it carries everywhere
else in the clip.** So this is a true dropout — A's noise floor really does die — of material at
−68 dB, which is why it is inaudible and why the ear reports the pair as identical. "B should match
A" is perceptually true and technically false here, and the gate needs to survive that distinction.

Three things follow.

1. **The gate dropped it by luck, not by design.** `a_below_noise` is −34.4 dB, **0.6 dB** short of
   the 35 dB margin ⇒ `ambient_quiet` ⇒ drop. But that 34.4 dB is the distance from a −67.2 dB bed to
   digital zero; the number is small only because the passage is this quiet. The identical
   digital-silence hole in a normal-level passage reads 50–60 dB down and classifies
   `repairable_dropout` immediately. `ambient_quiet` is not protecting against this case — the local
   floor is.
2. **The patch it produces is a defect.** The fill sits at −60.4 dB with a 1632 LSB spike — ~8 dB
   above the bed it replaces and 4× B's peak there. It correlates with B at r = 0.958 but only at
   **+36 ms** (at lag 0, r = −0.048), so the donor content is placed 36 ms off.
3. **The damage runs past the gap.** 3.62–4.00 s is −69.5 dB in A and −69.4 dB in B but **−57.8 dB**
   in the patched file, including a 5534 LSB click at 3.90 s where both sources have ~1400 — ~12 dB
   of injected content and a 12 dB click overshoot, 380 ms beyond the gap end. Confirmed audible.

The `post_seam_r` 0.973 vs `post_seam_global_r` 0.030 disagreement and the lone nonzero
`trim_frames` (1461) are consistent with that 36 ms misplacement, so the A7-adjacent suspicion was
well founded — but the actionable defect is the patch itself, not the score. **Open item:** the
repair path produced a fill louder than its own surroundings and wrote outside the gap. That belongs
to the seam/splice path and is independent of the band decision.

### 3.4 A decline that may be a seam-gate miss, not an equivalence win

**28/22** is the deepest dropout in the set (−59.5 dB) with clean bracket seams (0.998 / 0.996) and a
half-occupied donor — and it was declined `correlation_below_threshold`. If A has an audible hole
there, the miss belongs to the seam gate, not the equivalence gate, and it is out of scope for the
band but in scope for the repair path.

### 3.5 Scope limits of the evidence

- 21 gaps, 10 pairs. Small, and deliberately selected for being near a boundary — **not** a random
  sample. Nothing here estimates the false-drop rate over the full 528.
- The band list was derived from the 39-pair corpus, which predates `thresholds`; `--band` correctly
  refuses it. Re-deriving the list corpus-wide still needs a re-dump. (Running *these* 21 did not
  need one — the experiment run was its own re-dump.)
- `dual_fit_rescue` was `false` on all 21; dual-fit is not a factor in any of these outcomes.

---

## 4. Listening list

A-timeline, ±5 s context so both seams are audible. **Gap numbers in this section are 1-based**
(what you type); §2 and §3 quote 0-based dump filenames, so they read one lower. Produce with

```
--gap-fingerprints <fresh-dir> --gap-listen --fingerprint-gap <n,m> --no-skip-equivalent-gaps
```

one run per *pair* (`--fingerprint-gap` takes comma lists). `--only-gaps` is **rejected** here —
`--gap-listen` takes its set from `--fingerprint-gap` and fans it out to both the corpus and the fill
plan. Without `--no-skip-equivalent-gaps` the gate drops the gap at plan time: A-only clip, no donor,
no patch. Use a fresh directory — a listen run writes a partial corpus, not the corpus of record.

**Tier 1 — the 5 band patches (the decision), riskiest first**

| pair | gap | listen | why |
|---|---|---|---|
| 18 | 50 | 2:17:53.4 – 2:18:04.2 | widest shoulder disagreement (§3.3) |
| 33 | 18 | 0:41:18.1 – 0:41:28.7 | global/local seam divergence, nonzero trim (§3.3) |
| 10 | 12 | 1:47:55.8 – 1:48:07.1 | the exact-0.500 tie; cleanest metrics — the exemplar |
| 25 | 2 | 0:00:12.3 – 0:00:22.9 | only pair-25 patch, `step_ms` 486 — tests the timing class |
| 14 | 10 | 0:12:29.8 – 0:12:40.3 | clean band patch, −59.1 dB — the easy case |

**Tier 2 — declines to sanity-check (listen to unpatched A; nothing changed)**

| pair | gap | listen | why |
|---|---|---|---|
| 28 | 23 | 1:36:27.1 – 1:36:38.1 | §3.4 — possible seam-gate miss |
| 14 | 14 | 0:29:21.6 – 0:29:32.5 | −42.9 dB, declined despite clean seams |

**Tier 3 — gate-off-only patches (not band; only relevant to gate-off wholesale)**

| pair | gap | listen |
|---|---|---|
| 12 | 8 | 1:31:04.5 – 1:31:15.3 |
| 34 | 2 | 0:00:43.8 – 0:00:55.1 |
| 34 | 25 | 1:28:12.4 – 1:28:23.6 |

⚠ **25/2 (0:00:17) and 34/2 (0:00:48)** sit in the first minute. If either is over leader, a logo, or
a studio sting, it is not a representative test.

---

## 5. Tooling footguns found (fix or document before the next run)

1. ~~**`--only-gaps` is a silent no-op on `--gap-fingerprints` runs.**~~ **FIXED.**
   `validate_dump_gap_selector` (`cli/mod.rs`) now rejects `--only-gaps`/`--skip-gaps` pre-scan on a
   **scan-only** dump run, naming `--fingerprint-gap` as the flag that narrows the dump. Scoped to
   scan-only deliberately: with `--wav`/`--mux`/`--repair-preview` the selection really does bound
   the repair half while the dump stays full-corpus, so that combination is still allowed. A TOML
   `only_gaps` key is caught too (the guard runs after `apply_cli_overrides`).
2. **`--fingerprint-gap` takes comma lists** (`value_delimiter = ','`), same as `--only-gaps`.
   ~~repeat-only~~. It does *not* accept the ranges or timestamps `--only-gaps` does. Two flags on
   the same 1-based axis, two grammars, and neither name says which stage it acts on.
3. **Dump filenames are 0-based** while both selection flags are 1-based (`--fingerprint-gap 12` →
   `g011`). Already warned in `args.rs`; locate files by timestamp, not by counting.
4. **`--gap-fingerprints` twice is a hard clap error.** `measure-gap-fingerprints.ps1` supplies it
   itself, so manifest `extra` must never include it. Fails loudly (exit ≠ 0, script throws) — but it
   silently cost a 9-pair run that had it in `extra`.
5. **`--fingerprint-gap` is not the free win it looks like.** Decode is ~65% of a run post-8g.6
   (`repair-perf.md:290`); narrowing only saves the characterize share. **Unmeasured on the current
   code** — the 88%/93% figures in `repair-perf.md` predate 8g.6's removal of the per-bracket oracle.
   Cheap experiment: run one pair with and without and diff the seconds the script already prints.

---

## 6. Where this leaves us

The §4 listening list was run and the branches below resolved. Recording both the outcome and the
predictions, since the predictions were written before the audio existed.

### 6.1 Resolved

- **"Tier 1 patches sound bad" → hit.** All 8 gate-off patches inject 9–46 dB into program silence
  (§3.1a). **The band does not ship at ±1.0 / ±1, and does not ship at a revised width either** —
  §2.5 shows the boundary is fed a misregistered measurement, so no width is the right width.
- **"Tie-break fix alone recovers most of the band set" → refuted, and inverted.** `<=` on the donor
  comparison flips 6 of the 12 listened gaps from correct drop to false keep. **Do not ship it.**
- **"Pair 25's nine gaps are one registration bug" → confirmed by measurement.** Its three listened
  gaps show a consistent ~360 ms nominal-mapping error. It is one bug, and it is not confined to
  pair 25 — the same error is 80–410 ms across seven other pairs.
- **28/22 (§3.4) → not a seam-gate miss.** A and B match at −50.3 dB at the fitted lag; there is no
  audible hole for the seam gate to have missed. The decline was correct.

### 6.2 What to do next, in order

1. **Register the donor window locally before measuring it** (§2.5, §6.4). Not "use the fitted lag" —
   that was the wrong shape and is corrected in §6.4. The gate runs **pre-decode**
   (`scan_gaps.rs:284`) with a single global `offset_secs` for the whole pair, and one constant
   cannot track 80–410 ms of local drift. Derive the lag from the block-level timelines the scanner
   already holds; see §6.4 for the validated method.
2. **Fix the 33/17 patch defect** (§3.3): a fill 8 dB above its own surroundings, placed 36 ms off,
   writing 380 ms past the gap end. Seam/splice path, independent of the band.
3. **Re-examine the dropout margin against 33/17's mechanism** (§3.3 point 1). A fixed 35 dB margin
   below a *local* floor cannot express "A went to digital zero"; in a quiet passage the two are only
   34 dB apart. Whatever replaces it must not turn 33/17 into a keep.
4. **Close the band item.** With (1) done there is no boundary-width question left to size. If the
   band is revisited it must be re-derived from post-fix dumps; the numbers in §2.3 are measured
   against the broken donor window and should not be carried forward.

### 6.3 What would change *this* conclusion

- **The 9 unheard gaps of the 21 behave differently.** The listen run covered 12; §3.1 still stands
  for the rest. A second listen pass is cheap now that the tooling footguns in §5 are documented.
- **The fitted-lag donor fix does not reproduce these results in-engine.** The lag here was derived
  offline from the listen WAVs with a simple Pearson search, not by the production fit. If the
  production fit lands elsewhere on these gaps, (1) needs rethinking before it ships.
- **A corpus-wide re-dump shows the nominal-map error is rare outside this selected set.** These 21
  were chosen for sitting near a boundary, which is exactly what a registration error would cause —
  so the sample cannot estimate how often it happens. It bounds the *mechanism*, not the *rate*.

### 6.4 The validated registration method

**Cheap enough for the pre-decode gate, and measured on all 12.** Cross-correlate A's and B's
100 ms `BlockLevel.rms_db` **dB envelopes** — the arrays the scanner already holds — over the gap
± `EQUIVALENCE_CONTEXT_SECS` (2.0 s), searching ±10 blocks. That is ~21 dot products over 40–70 bins
per gap, no decode, no fit. Prototyped in `domain/gap_equivalence.rs` as `register_donor_window`
behind `GapEquivalenceParams::donor_registration` (opt-in; `None` reproduces today byte for byte).

Three properties had to be got right, and two of them were only found by measuring:

1. **Exclude the gap core from the correlation.** Register on the *shoulders*. The core is the one
   stretch where A and B are expected to differ, and including it makes registration fail on exactly
   the gaps that most need placing: a deep A dropout against a live B is a run of −110 dB outliers
   that dominates the variance. Measured both ways on all 12 — identical lags, and the worst
   correlation rises from 0.447 to 0.883:

   | | lag (incl. core) | r | lag (excl. core) | r |
   |---|---|---|---|---|
   | 10/11 | +0 ms | 0.935 | +0 ms | 0.980 |
   | 12/7 | −100 | 0.985 | −100 | 0.990 |
   | 14/9 | −100 | 0.968 | −100 | 0.973 |
   | 14/13 | −100 | 0.948 | −100 | 0.918 |
   | 18/49 | +300 | 0.954 | +300 | 0.972 |
   | 25/1 | +300 | 0.997 | +300 | 0.992 |
   | 25/8 | +400 | 0.993 | +400 | 0.999 |
   | 25/36 | +300 | 0.957 | +300 | 0.883 |
   | 28/22 | +100 | 0.909 | +100 | 0.929 |
   | **33/17** | +0 | **0.447** | +0 | **0.970** |
   | 34/1 | −100 | 0.997 | −100 | 0.987 |
   | 34/24 | +0 | 0.972 | +0 | 0.994 |

   The 33/17 row is the point. Gap-inclusive, its low `r` was **conflating two different facts** —
   "the window can't be placed" and "the interior differs" — and it is the second one that is true.
   Registered on the shoulders it scores 0.970 like everything else, and its real signal moves to
   `interior_delta_db` (+35.3 dB), which is where a "B has content A lost" claim belongs.

2. **Erode one bin at each gap edge before comparing levels.** Without erosion the 100 ms grid
   quantization produced +25 dB (18/49) and −15 dB (25/8) artifacts. With it, 11 of 12 agree to
   ≤1.0 dB and the twelfth is 33/17's genuine +35.3 dB.

3. **`r` is a registration test, not an equivalence test.** Below `min_envelope_r` (0.70) the two
   timelines do not correspond at all and no statement about B's occupancy is defensible — so the
   gate **abstains**: `NotEvaluated` with `not_evaluated_reason: donor_registration_unreliable`,
   which keeps the gap. That is the "honest about why" half of the requirement. Every gap on this
   set registered at 0.883 or better, so 0.70 is a floor, not a tuned split.

**What the prototype's tests pin.** Beyond the flip case (nominal reads B's content ⇒ false keep;
registered lands on B's silence ⇒ correct drop), the negative controls are the ones that matter:
a **real dropout with a live donor still registers and still classifies `RepairableDropout`** —
registration may only move the window, never talk the gate out of a fill — and unrelated timelines
abstain and fail open. Flat envelopes are "cannot ask", not "does not match": no registration is
recorded and the nominal map stands, rather than an abstain that would keep every gap on quiet
material.

**Still unvalidated:** everything above is measured on the same 12 gaps that motivated it. The
out-of-set behaviour is only covered by synthetic fixtures — see §6.5. The registration is now
computed corpus-wide in `Observe` mode (§6.6), which will answer the *rate* question without
putting a verdict at risk; `Apply` stays off until it has.

### 6.5 The out-of-set gap is a materials problem

Testing this on gaps outside the 12 cannot be done offline from what exists today:

- No `--gap-listen` WAVs exist outside `2026-08-01-v.0.5.1-band-test-with-listen` (24 wavs / 12 dumps).
  The large corpus has 907 dumps and 0 WAVs.
- The dumps carry **no B-side level timeline**, and only **14 of 405** sampled gaps carry
  `levels.profile_db` at all — so the envelope correlation cannot be replayed from a dump either.

The negative controls that matter are the 132 `repairable_dropout` gaps in the 39-pair corpus
(against 229 `shared_silence`, 30 `ambient_quiet`, 14 unclassified): those are the gaps a
registration change must **not** flip. Two routes, neither blocking the prototype:

1. ~~**A small second `--gap-listen` run**~~ — **DONE 2026-08-03**, see §6.7. Eight new pairs, all
   `repairable_dropout`, none previously listened to.
2. ~~**Emit the registration on the next corpus dump**~~ — **DONE 2026-08-03**, see §6.6.

The second bullet above is also now stale: the dumps **do** carry both level timelines as of the
2026-08-03 envelope-capture change (§6.7.4), so the correlation is replayable from a dump.

### 6.6 Route 2 shipped: registration observed on the scan path

`DonorRegistrationParams` now carries a **mode**:

- `DonorRegistrationMode::Observe` — compute the registration, record it on the verdict, and
  classify at the **nominal** map exactly as before. Provenance only: same class, same
  `donor_silence_fraction`, same `donor_span_secs`, plus one `donor_registration` block per gap.
  No abstain — an abstain is a verdict change, and a run that started keeping gaps because of a
  mode flip would answer a different question than the one it was turned on to ask.
- `DonorRegistrationMode::Apply` — measure at the registered lag and abstain below
  `min_envelope_r`. The §6.4 fix. Not enabled anywhere.

`Observe` is the `Default`, so a caller that asks for registration without saying what for cannot
silently move a decision. `scan_gaps.rs` requests it unconditionally; the corpus dump picks it up
for free, because `characterize_gaps` copies scan's verdict onto every `GapFingerprint`
(`fp.scan_equivalence`). Nothing else changed — the whole default-feature suite passes unmodified,
goldens included.

**So the next corpus run answers the rate question directly.** Per gap the dump now carries
`lag_blocks` / `lag_ms` (how far off the nominal map was), `peak_r` vs `nominal_r` (what the
misregistration cost, and whether the window could be placed at all), `bins`, and the eroded
`a_interior_db` / `b_interior_db` / `interior_delta_db`. Three things to read off it:

1. **How often is `lag_blocks != 0`?** §6.3 says the 12-gap set cannot estimate this — it was
   selected for sitting near a boundary, which is what a registration error causes.
2. **How often would `Apply` have flipped a class?** Derivable per gap, since both windows are on
   the verdict. The 132 `repairable_dropout` gaps are the ones that must not move.
3. **How often is `peak_r < 0.70`?** That is the `Apply` abstain rate, and it is a cost: each one
   is a gap kept with no judgement made.

The end-to-end wiring is pinned by
`scan_gaps.rs::scan_records_donor_registration_without_moving_the_verdict`, on a fixture whose
registration is deliberately class-flipping (nominal reads B's content ⇒ `repairable_dropout`;
registered lands on B's hole ⇒ would be `shared_silence`), so "unchanged" is a claim with teeth.

### 6.7 The 2026-08-03 extended runs — what §6.6 was turned on to ask

Three directories landed on 2026-08-03 and none was reviewed against this file until now. Same
16 pairs / 20 gaps throughout: the 12 already listened to, **plus 8 pairs never seen before**
(5, 7, 8, 9, 16, 19, 35, 36), every one of them a `repairable_dropout` — the §6.5 negative controls.

| dir | ran | WAVs | `donor_registration` | what it is |
|---|---|---|---|---|
| `2026-08-03-...-extended` | 11:05 | **yes** (3/gap) | **no** | the §6.5 route-1 listen materials |
| `2026-08-03-...-extended-2` | 18:12 | no | **yes** | the §6.6 `Observe` run — **the corpus of record** |
| `2026-08-03-...-extended-3` | 20:34 | — | — | aborted; one empty pair directory |

#### 6.7.0 Read `extended-2`, not `extended` — the first run's detail fields are projected

`extended`'s `baseline_lag` records `window_ms: 0`, `max_lag_ms: 0`, `peak_lag_samples: 0` against a
confident-looking `lag0_r: 0.995` — a zero-width search window, i.e. the projection path, not a
measurement. `beb24f93` (16:24, *"incorporate measured details"*) introduced `MeasuredDetail` and the
`not_measured` marking for fabricated gaps; `extended` predates it by five hours and `extended-2`
follows it by two. Field-by-field across the 20 shared gaps: `donor_interior` / `_nominal` and
`levels` differ on all 20, `residual` on 10; `splice`, `brackets`, `geometry`, `channels`,
`splice_dualfit` are **identical** on all 20.

`extended` is still good for its WAVs — that audio is real, and it is the only listen material for
the eight new pairs. **Do not quote its `baseline_lag` or level fields.**

#### 6.7.1 The three §6.6 questions, answered

**1. How often is `lag_blocks != 0`? — 12 of 20 (60 %).** Mostly ±100 ms; pair 25 is +400 ms on all
three of its gaps, in-engine, confirming §2.4/§2.5's "one registration problem counted nine times".

**2. How often would `Apply` have flipped a class? — none of the 20, and the negative controls hold.**
All eight new `repairable_dropout` gaps read `a_interior_db` −101.5 (digital zero) against
`b_interior_db` −25.7…−49.1, so `interior_delta_db` **+49.7…+75.8 dB**, and the donor interior
`silence_fraction` is **0.0 at the nominal window and 0.0 at the registered one**:

| gap | lag | `peak_r` | `nominal_r` | A int. | B int. | Δ | nom. frac → reg. frac |
|---|---|---|---|---|---|---|---|
| 5/4 | +0 ms | 0.948 | 0.948 | −101.5 | −34.3 | +67.2 | 0.0 → 0.0 |
| 7/3 | +0 | 0.954 | 0.954 | −101.5 | −38.6 | +62.9 | 0.0 → 0.0 |
| 8/4 | −100 | 0.932 | 0.620 | −101.5 | −49.1 | +52.4 | 0.0 → 0.0 |
| 9/9 | +0 | 0.881 | 0.881 | −101.5 | −31.8 | +69.7 | 0.0 → 0.0 |
| 16/7 | +100 | 0.989 | 0.863 | −101.5 | −41.5 | +60.0 | 0.0 → 0.0 |
| 19/13 | +100 | 0.822 | 0.732 | −101.5 | −51.8 | +49.7 | 0.0 → 0.0 |
| 35/10 | +100 | 0.924 | 0.789 | −101.5 | −39.6 | +61.8 | 0.0 → 0.0 |
| 36/9 | +0 | **0.688** | 0.688 | −101.5 | −25.7 | +75.8 | 0.0 → 0.0 |

Registration moves the window and still says *fill it*. That is the property §6.4 said only synthetic
fixtures covered, now measured on eight pairs the method had never seen.

The `shared_silence` side moves the way §2.5 predicted: the registered donor interior fraction rises
on 9 of 12 (18/49 0.529 → 0.929, 14/13 0.778 → 0.957, 12/7 0.765 → 0.889) and donor RMS drops 13–29 dB
(18/49 −48.5 → −77.4). 33/17 still reports its genuine **+35.7 dB** interior delta and is not talked
into a keep. `nominal_r` shows what the misregistration was costing: 25/36 **−0.128** nominal against
0.829 registered, 25/1 0.245, 25/8 0.339.

**3. What is the `Apply` abstain rate? — 1 of 20 (36/9, `peak_r` 0.688).** It is a
`repairable_dropout`, so the abstain keeps a gap that was being kept anyway. Cost nil on this sample.

**Caveat that has not moved:** these 20 were selected for sitting near a boundary. This bounds the
mechanism and the negative controls; it still does not estimate the corpus-wide rate.

#### 6.7.2 The production path reproduces the offline lags

§6.3 required this before `Apply` could ship: *"if the production fit lands elsewhere on these gaps,
(1) needs rethinking."* The §2.5 lags were derived by hand from the listen WAVs (1 ms Pearson) and
§6.4's by an offline prototype; `extended-2`'s come from the shipped scan path. No shared code.

| gap | §2.5 offline, WAV | §6.4 offline envelope | `extended-2` in-engine | |
|---|---|---|---|---|
| 10/11 | +28 ms | +0 | **+0** | ✓ |
| 12/7 | −81 | −100 | **−100** | ✓ |
| 14/9 | −119 | −100 | **−100** | ✓ |
| 14/13 | −122 | −100 | **−100** | ✓ |
| 18/49 | +341 | +300 | **+300** | ✓ |
| 25/1 | +332 | +300 | **+400** | one bin |
| 25/8 | +410 | +400 | **+400** | ✓ |
| 25/36 | +347 | +300 | **+400** | one bin |
| 28/22 | +47 | +100 | **+0** | one bin |
| 33/17 | −19 | +0 | **+0** | ✓ |
| 34/1 | −115 | −100 | **−100** | ✓ |
| 34/24 | −7 | +0 | **+0** | ✓ |

**9 of 12 exact; the other 3 differ by exactly one bin, and all three sit within ~50 ms of a bin
boundary** (+332, +347, +47). That is grid quantization, not disagreement — every in-engine lag is
within one bin of the WAV-derived truth. §6.3's first failure condition is discharged.

It is worth being clear about what the quantization costs, because it is the same arithmetic that
sank the ±1-block band: one bin is 8–25 % of a 4–12 bin donor window (§2.5), so a one-bin lag error
is decision-relevant at the boundary. The registration is not therefore fuzzy — the three disagreeing
gaps are `shared_silence` by a wide margin at either lag — but "within one bin" is the accuracy
claim, not "exact".

#### 6.7.3 `listen-registration` — the cross-check, extended to all 20 gaps and made repeatable

The table above stops at the 12 gaps §2.5 had measured by hand. The eight new pairs had listen WAVs
too (in `extended`), so the same question was askable of them — and the hand method is now a dev
binary, `crates/clip-sync-repair/src/bin/listen_registration.rs`, `[[bin]] listen-registration` under
`required-features = ["calibration"]`:

```text
cargo run --features calibration --bin listen-registration -- \
    gap-files/<listen-run> --observe-dir gap-files/<observe-run>
```

`--observe-dir` exists because of the split in §6.7.0: the WAVs are in `extended` and the
`donor_registration` is in `extended-2`, matched by pair directory and file stem. It reads nothing
from the dump but the gap's geometry, so a corpus whose detail fields came from the projection path
is still cross-checkable. It recovers the export context from the clip length rather than assuming
3.0 s, then Pearsons the 2 s pre-gap A shoulder against B at 1 ms steps over ±1 s — lag 0 being the
nominal map, by the cut geometry in §6.7.2.

It reproduces all 12 rows above exactly, which is the evidence that it is the same instrument. Over
all 20 gaps:

- **Every gap agrees with the engine within one 100 ms bin.** Worst is 9/9 at 0.83 bins (+83 ms WAV
  vs +0 engine) — and that row carries the weakest waveform correlation in the set, r = 0.251, so it
  is a statement about the WAV estimate on a quiet shoulder, not about the registration. 9/9 is a
  hard gap that the patched WAV **fixes correctly** by ear; nothing here is outstanding on it. The
  eight new pairs land at −8 / −18 / −138 / +83 / +73 / +54 / +47 / +4 ms.
- **The independently measured interiors match the engine's to ~3 dB**, so §6.7.1's table is not an
  artifact of the level path it was measured with.
- **The safety result, from the `B−A` column.** On the dropout class B sits 49.8–79.0 dB above A, and
  moving B from nominal to the registered lag changes that by **≤1.9 dB**. No lag error the search can
  reach moves a dropout toward `shared_silence`. On `shared_silence` the same column stays within
  ±0.9 dB of zero at either lag — including 18/49, where B at nominal reads −49.0 dB and B registered
  −86.0, i.e. the nominal window was measuring the wrong content by 37 dB.

What is *not* cross-checkable: `extended` against `extended-2` on the registration field itself, which
is **absent on all 20 of `extended`'s gaps** — the `scan_gaps` wiring landed at 12:53 (`9e7aa1f6`),
after that run. The check above is the stronger one anyway; it compares against an independent
derivation rather than a second run of the same code.

#### 6.7.4 Envelope capture: the dumps are now replay-complete

Answering §6.6's question 2 above required a full corpus re-dump, because the dump recorded the
registration's *outputs* and not its *inputs* — and `donor_blocks` reproduces the fraction only at the
one window that was measured. `DonorRegistration.envelopes` now records both dB envelopes: A's over
the gap ± context with the core marked (not omitted, so the exclusion is a policy a replay can vary),
B's padded by `±max_lag_blocks` so every lag the search tried can be re-asked, plus `bin_ms`,
`b_nominal_bin` and per-side `silent_bins`. With `gap_floor_db` already on the verdict, the donor
count at *any* lag falls out — no second block vector needed.

Two tests pin it: the recorded envelopes reproduce the registration exactly, and re-counting them at
`lag_blocks` reproduces `Apply`'s fraction on a class-flipping fixture. The first one earned its
keep — the bins were `f32` on the first cut and the replay test failed. The reason generalizes: the
donor count is `rms_db < gap_floor_db`, so a bin sitting *on* the floor can flip under rounding, and
that is precisely the comparison a replay exists to re-ask. `f64`, ~2 KB per gap.

The consequence for §6.5: from the next corpus dump on, the 132 `repairable_dropout` negative
controls are answerable by script over dumps already on disk, with no listen run and no re-dump.

#### 6.7.5 Still open after these runs

- **The rate question over unselected gaps.** Everything above is 20 boundary-selected gaps.
- **`Apply` is still not enabled anywhere.** The evidence for promoting it is now: negative controls
  hold out-of-set (§6.7.1), production reproduces the offline lags on all 20 gaps within one bin
  (§6.7.2–6.7.3), the dropout margin moves ≤1.9 dB under registration, abstain rate 1/20.
- **§6.2 items 2 and 3 are untouched** — the 33/17 patch defect and the fixed 35 dB dropout margin.
  Neither is affected by any of this.

### 6.8 The envelope-bearing run — replay-completeness proved, and a candidate for §6.2 item 3

`2026-08-03-...-without-listen-envelope-bearing` (21:37–23:05) is the first corpus dumped by a binary
that records `donor_registration.envelopes`. Same 16 pairs / 20 gaps, no listen WAVs, ~21 KB per gap
(~840 KB total). It is a **measurement** run — `baseline_lag` carries `window_ms: 1000`,
`max_lag_ms: 600` and fractional peaks, not the `window_ms: 0` projection signature that made
`extended` unquotable (§6.7.0) — and its `lag_blocks` are identical to `extended-2`'s on all 20 gaps,
so the scan is deterministic and dropping `--gap-listen` does not perturb it.

#### 6.8.1 §6.7.4's claim, discharged on real media

Every one of the 20 gaps carries envelopes, and every one replays:

- **Registration: 20/20 exact.** `lag_blocks`, `peak_r`, `nominal_r` and `bins` (40 on every gap)
  reproduce field-for-field when `register_donor_window` is re-run on levels rebuilt from the record.
- **Donor fraction: 20/20 exact** at the nominal window, against the recorded
  `donor_silence_fraction`.

This is `equivalence-calibration --replay` (below), and it calls the production function rather than
a second implementation of it — so "reproduces" means identical, not close. It also means the replay
cannot silently drift from the gate the way a reimplementation would.

#### 6.8.2 `Apply`, answered from the dump alone

The re-count at the registered lag — the question §6.6 spent a whole corpus re-dump on — is now a
read over a directory:

| class | donor fraction @ nominal → @ registered |
|---|---|
| `repairable_dropout` (7) | 0.000 → 0.000, except 35/10 at 0.042 → 0.042 |
| `shared_silence` (9) | 0.500–0.625 → 0.500–1.000 (18/49 and 25/1 reach 1.000) |
| `ambient_quiet` (2) | 0.000 and 0.364, both unchanged |

**No class flips, on any of the 20.** Registration only pushes `shared_silence` deeper into
`shared_silence`; it never walks a dropout toward silence. That is §6.7.3's safety result again, now
measured with the scan-block donor predicate the gate actually decides on rather than from the WAVs,
and the abstain count is unchanged at 1/20 (36/9, `peak_r` 0.688).

#### 6.8.3 A candidate for §6.2 item 3 — the fixed 35 dB margin

This is what the run adds that no earlier one could. **33/17's `a_below_noise_db` is −34.4 dB: it
missed `repairable_dropout` by 0.6 dB** and landed in `ambient_quiet`. Its registration says the
opposite — A at −101.5 (digital zero) against a donor at −65.8, `interior_delta_db` **+35.7 dB**. The
other `ambient_quiet`, 34/24, sits at a nearly identical −33.3 dB below floor but has interior delta
**−0.1 dB**: genuinely quiet on both sides. The two signals disagree on exactly one gap in the set,
and the delta separates the classes cleanly where the below-noise margin does not:

| class | `interior_delta_db` |
|---|---|
| `repairable_dropout` (7) | +49.7 … +75.8 |
| `shared_silence` (9) | −4.4 … +0.4 |
| `ambient_quiet` (2) | 33/17 **+35.7**, 34/24 **−0.1** |

So a dropout test that consulted the registered donor interior would classify 33/17 as
`repairable_dropout` — which by every other measurement it is — and would touch **no other gap in
this set**. That is a concrete candidate for item 3, not yet a proposal: it is 20 boundary-selected
gaps, the separation has never been measured on unselected material, and it moves a gap *out* of the
drop set, which is the direction that costs a patch attempt rather than a hole.

#### 6.8.4 …which reframes §6.2 item 2, the 33/17 patch defect

33/17 reads `drop: true` here and yet `outcome.tier = patch`: this run has the equivalence skip off.
**With the gate on, production skips 33/17 and the defective patch never renders.** The defect is
real, but it is reachable only with the gate disabled — *or* via §6.8.3, which would reclassify 33/17
to `repairable_dropout` and hand it to the patch path for real. The two items are therefore coupled:
fixing the margin without fixing the splice would ship the defect rather than expose it.

#### 6.8.5 `equivalence-calibration --replay`

```text
cargo run --features calibration --bin equivalence-calibration -- <corpus-or-parent> --replay
```

Takes the same three input shapes as the other modes (a `corpus.json`, a directory holding one, or a
parent of numbered pair directories). Per gap it prints the class, the recorded lag and `peak_r`, the
donor fraction at nominal and at the registered lag, what `Apply` would decide (with `FLIP` and
`abst` markers), and the replay status. **Exit code 1 on any mismatch** — a dump whose recorded
inputs do not reproduce its own outputs is not evidence about anything else it says, so this is a
gate and not a report. Gaps with no envelopes are counted and named, never assumed;
`--replay-min-envelope-r` prices the abstain rate against a corpus.

The four tests that pin it live in the bin: the registration replays, the donor re-count at the
registered lag differs from the nominal one on a class-flipping fixture (a replay returning the same
number twice would look healthy while measuring nothing), an abstain is a keep and is not reported as
a rescue, and an envelope-less verdict declines rather than assumes.

#### 6.8.6 Still open

Unchanged by this run: **the corpus-wide rate**. These are the same 20 boundary-selected gaps —
12 nonzero lags, 1 abstain. Nothing here estimates behaviour on unselected material, and both
§6.8.2's "no flips" and §6.8.3's clean separation are claims about this sample. The cost of asking
the wider question has, however, collapsed: any envelope-bearing corpus answers both by `--replay`,
with no listen run and no re-dump.
