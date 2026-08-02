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

### 2.5 The donor boundary is degenerate, not fuzzy — possibly the real finding

**7 of the 21 gaps sit at donor fraction exactly 0.500**: 6/12, 2/4, 3/6, 5/10, 4/8, 7/14, 5/10.

Block counts are 4–14, so 0.5 is the most probable rational value the measurement can produce. The
strict `<` in `classify_gap_equivalence` decides a third of this sample by itself, on an exact tie.

That reframes the problem. A ±1-block band is a blunt instrument aimed at a boundary that is not
uncertain but *quantized*. Two cheaper candidates to evaluate before shipping the band:

1. **Tie-breaks keep rather than drop** (`<=` on the donor comparison). One-character change,
   directly matches the cost asymmetry, and catches 10/11 — the gap that started this.
2. **Raise donor block resolution** so exact ties become rare, making the boundary meaningful.

Neither is a substitute for the band on the *dropout* axis, which is continuous and where a margin
does make sense.

---

## 3. Concerns / open

### 3.1 Nothing has been heard

No audio was rendered — the fingerprint path never muxes. **The 8 patches are structurally
plausible, not verified.** The band decision should not be made on structure alone; the A3 dual-fit
work set the precedent of an ear check, and this is the same class of claim.

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

### 3.3 Two patches with internally inconsistent metrics

- **18/49** — `splice.post_peak_r` 0.136 and `step_ms` −134.9 (one shoulder does not correlate),
  while `splice_dualfit` reports `pre_seam_r` 0.982 / `post_seam_r` 0.993. These are different
  measurements and the gate uses the bracket path, so this is not proof of a defect — but it is the
  widest internal disagreement in the set and it patched.
- **33/17** — `post_seam_r` 0.973 vs `post_seam_global_r` 0.030, and the only nonzero `trim_frames`
  (1461) in the set. It patched. Given `dualfit-revalidation-window-bug` (A7) was exactly a
  wrong-window seam-scoring defect in this area, this deserves confirmation rather than assumption.

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

A-timeline, ±5 s context so both seams are audible. Produce with
`--no-skip-equivalent-gaps --only-gaps <n> --wav <out>` (one gap per run); `--only-gaps` **is**
correct here — it drives the repair plan. Without `--no-skip-equivalent-gaps` the gate drops the gap
and the output is unmodified.

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

1. **`--only-gaps` is a silent no-op on `--gap-fingerprints` runs.** It filters the repair plan
   (`request.selection`); the fingerprint path never reads `selection`. Tokens are parsed and
   validated, nothing is filtered, exit 0. Cost us one full-corpus dump that was believed narrowed.
   *Proposed guard:* reject `--only-gaps`/`--skip-gaps` alongside `--gap-fingerprints` in
   `validate_fingerprint_flags` — "use `--fingerprint-gap` to narrow the dump". Not yet implemented.
2. **`--fingerprint-gap` is repeat-only.** `Vec<usize>`, no `value_delimiter`, so `2,9` is a parse
   error. `--only-gaps` *does* take comma lists (`value_delimiter = ','`) and also accepts ranges and
   timestamps. Two flags on the same 1-based axis, two grammars, and neither name says which stage it
   acts on.
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

## 6. What would change the conclusion

- **Tier 1 patches sound bad** → band does not ship at ±1.0/±1; revisit width, or drop the band in
  favour of the tie-break fix (§2.5).
- **Tier 1 patches sound good and 28/23 has an audible hole** → the equivalence gate is not the main
  source of shipped holes; priority moves to the seam gate.
- **Tie-break fix alone recovers most of the band set** → ship that instead; it is far smaller, has
  no width parameter to calibrate, and needs no new production concept.
- **Pair 25's nine gaps are one registration bug** → fix it there and the band's remaining case is
  7 gaps / 4 patches, which may not justify a production rule at all.
