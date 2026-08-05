# TEMP — Equivalence margin band: gate-off experiment findings

**Status:** working notes, 2026-08-02. **DELETE when the band decision lands — do not archive.**
Anything worth keeping should be promoted into `docs/dev/gap-fingerprint.md`, `BACKLOG.md`, or the
threshold docs *before* this file is removed.

**Media hygiene:** corpus pairs are referred to by index only. No filenames, titles, or paths here or
in any artifact derived from this work. Raw dumps and logs stay under gitignored `/gap-files/`.

**Citation drift (audited 2026-08-04):** every named symbol cited below was verified to exist, but
**line numbers have drifted** — `seam_residual.rs:213/635/798`, `gap_equivalence.rs:441`,
`gap_fill_fit.rs:216`, `patch_region.rs:1132`, `config.rs:402`, `dual_fit.rs:45` and
`scan_gaps.rs:284` all now land near, not on, the thing they name. Grep the symbol, not the line.
One citation was wrong about the symbol itself and is corrected in place (§3.2 item 1).

### TL;DR — decision ledger (2026-08-04)

This file is a lab notebook. Read this block for the decision; promote it, then delete the rest.

**Band decision.** Do **not** ship a ±1.0 dB / ±1-block (or any revised) equivalence margin band.
The donor boundary is not fuzzy — it is fed a window measured at the wrong place (nominal map
off by 80–410 ms on the listen set; §2.5). No width fixes a misregistered measurement.

**Do not ship (killed with evidence).**
- `<=` donor tie-break — flips 6/12 listened correct drops to false keeps (§6.1).
- `interior_delta_db` as a dropout *classifier* — classes overlap by 58 dB over 39 pairs (§6.10.5).
- Widen / re-centre the residual floor probe — deliberate post-aligner design, not an oversight (§7.3).
- Local-context codec floor for the 35 dB margin — ~0.8 dB separation; needs a new threshold (§7.2).
- Bound / price the write overrun — already bounded / already priced; 33/17's placement path is unrecorded (§7.1, §7.1a).

**Shipped.** `DonorRegistrationMode::Observe` remains the enum/`DonorRegistrationParams` default
(callers that opt into registration without choosing a mode cannot silently move a decision).
**Items 1 and 3 built 2026-08-04 — see §7.4a.** Production scan sets `apply_donor_registration`
on by default (classifies the donor at the registered lag) and `measure_fill_level` records the
fill-vs-shoulder level on every patched gap. The §6.10.3 head/tail exclusion that was paired with
`Apply` in the recommendation is **not** implemented (§7.4a).

**Ship next (§7.4), ordered.**
1. **Fill-level sanity check** vs A shoulders — no level term in the fill path; ear damage tracks
   substitution magnitude (§6.10.11, §3.1a). **BUILT** (§7.4a).
2. **Name residual abstentions** and surface them in production output (`beyond_lag_reach` vs no
   energetic window vs non-finite) — dump already has `floor_source`; repair reporting does not (§7.3).
3. **Promote `DonorRegistrationMode::Apply`.** 39 pairs / 829 gaps / 782 registrations: 67.8 %
   nonzero lag (systematic per pair), 4.3 % abstain, **16 flips = 2.05 %**; touches none of the
   236 digital-zero-rail dropouts; net production effect is **3 patches stop** (12/8, 14/20, 38/4).
   All three heard: no hole, current patches are audible degradations — `Apply` removes real defects
   (§6.10.4, §6.10.11). Caveat: 2.05 % is a reconstruction (`--replay` cannot read `GapScanJson` yet).
   **BUILT** (§7.4a).
4. **`interior_delta_db` as recall widener only** — low risk, **zero measured benefit** on this corpus;
   do not ship before (1) (§6.10.6–§6.10.7a).

**Promote this ledger** into `BACKLOG.md` / `gap-fingerprint.md` / threshold docs, then delete this
file when the band item closes. Do not carry §2.3 band-yield numbers forward — they were measured
against the broken donor window.

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

1. **`−120.0` is a sentinel, not a deep cancellation.** A silent span floors at
   `SILENCE_FLOOR_DB = −120` rather than going to −∞ (`application/gap_equivalence.rs:57`, applied in
   `gap_interior_rms_db`). *(Corrected 2026-08-04: this cited `finite_db()` at `measure.rs:487`;
   there is no such function — the helper at that location maps a non-finite **correlation** to 0.0,
   a different clamp. The point stands, the citation was wrong.)* `floor_probe_informative` also
   requires `is_finite()`,
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
   writing 380 ms past the gap end. Seam/splice path, independent of the band. *(Of the three
   symptoms only the level is actionable today — §7.4 item 1. The overrun has no identified
   mechanism: see §7.1a.)*
3. **Re-examine the dropout margin against 33/17's mechanism** (§3.3 point 1). A fixed 35 dB margin
   below a *local* floor cannot express "A went to digital zero"; in a quiet passage the two are only
   34 dB apart. Whatever replaces it must not turn 33/17 into a keep.
   *(Items 2 and 3 are worked through in §7; the concrete proposals and their sequencing constraint
   are in §7.4.)*
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
follows it by two. Field-by-field across the 20 shared gaps: `donor_interior` /
`donor_interior_nominal` (the actual key — there is no `donor_nominal`) and `levels` differ on all
20, `residual` on 10; `splice`, `brackets`, `geometry`, `channels`, `splice_dualfit` are **identical**
on all 20.

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
(18/49 −48.5 → −77.4).

> **Which fraction this is — added 2026-08-04.** These numbers are
> `donor_interior_nominal` → `donor_interior` `silence_fraction`: a **50 ms downmix** measurement
> against its own `basis.floor_db`. They are **not** the `donor_silence_fraction` the gate decides on
> (100 ms interleaved scan blocks against `gap_floor_db`), which §6.8.2 reports for the same gaps —
> hence 18/49 reading 0.529 → 0.929 here and 0.571 → 1.000 there. Both are correct about their own
> instrument. On the **gate's** predicate the rise is **6 of 12**, not 9 (12/7, 14/13, 18/49, 25/1,
> 25/36, 34/1 rise; 10/11, 14/9, 25/8, 28/22, 33/17, 34/24 are unchanged) — the direction of §2.5's
> prediction holds on both, the count does not transfer between them. 33/17 still reports its genuine **+35.7 dB** interior delta and is not talked
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
- **`Apply` is still not enabled anywhere** (now carried as §7.4 item 3). The evidence for promoting it is now: negative controls
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

**Re-run 2026-08-04 as part of a full audit of this file: `20 gap(s) · 20 replayed · 0 registration
mismatch · 0 donor-fraction mismatch`, `Apply: 0 class flip(s), 1 abstain(s)`, exit 0.** §6.8.1 and
§6.8.2 are reproduced, not just recorded.

This is `equivalence-calibration --replay` (below), and it calls the production function rather than
a second implementation of it — so "reproduces" means identical, not close. It also means the replay
cannot silently drift from the gate the way a reimplementation would.

#### 6.8.2 `Apply`, answered from the dump alone

The re-count at the registered lag — the question §6.6 spent a whole corpus re-dump on — is now a
read over a directory:

| class | donor fraction @ nominal → @ registered |
|---|---|
| `repairable_dropout` (8) | 0.000 → 0.000, except 35/10 at 0.042 → 0.042 |
| `shared_silence` (10) | 0.500–0.625 → 0.500–1.000 (18/49 and 25/1 reach 1.000) |
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

> **SUPERSEDED 2026-08-04 — this table does not generalize.** Over 39 pairs the two populations
> overlap by 58 dB (dropouts from **−0.51**, non-dropouts to **+57.59**) and no threshold separates
> them. The clean air below is a property of a 20-gap boundary-selected sample. See §6.10.5, and
> §6.10.7 for the ear check that refutes the 33/17 reading. The paragraph that follows — "the delta
> separates the classes cleanly where the below-noise margin does not" — is **false corpus-wide**;
> what survives is the delta's *negative* direction (§6.10.9).

| class | `interior_delta_db` |
|---|---|
| `repairable_dropout` (8) | +49.7 … +75.8 |
| `shared_silence` (10) | −4.4 … +0.4 |
| `ambient_quiet` (2) | 33/17 **+35.7**, 34/24 **−0.1** |

**Strengthened 2026-08-04 — the margin misfires in both directions, and the delta catches both.**
§6.8.5a resolved 14/13: quiet program material present in both masters (`interior_delta_db` +0.09,
no ear-audible hole) that the characterize-side dropout test **flagged** as `repairable_dropout`,
because −42.9 dB below a shoulder-set −38.1 dB floor clears the 35 dB margin. So the fixed margin
produces a **false negative on 33/17** (a real digital-zero hole, missed by 0.6 dB) and a **false
positive on 14/13** (quiet program, flagged) — within the same 20 gaps. `interior_delta_db` gets both
right, +35.7 and +0.09, and it does so without a floor estimate, a codec identity, or a table.

So a dropout test that consulted the registered donor interior would classify 33/17 as
`repairable_dropout` — which by every other measurement it is — and would touch **no other gap in
this set**. That is a concrete candidate for item 3, not yet a proposal: it is 20 boundary-selected
gaps, the separation has never been measured on unselected material, and it moves a gap *out* of the
drop set, which is the direction that costs a patch attempt rather than a hole.

#### 6.8.3a A second, independent witness — the dual-fit trim

`splice_dualfit` says the same thing from the other end of the pipeline, without consulting the
envelopes at all. **A materially nonzero `trim_frames` picks out the 8 `repairable_dropout`s — and
33/17.** The companion column moves with it: where dual-fit had to reconcile length,
`post_seam_global_r` collapses away from `post_seam_r` (−0.281 … +0.474 against seams of 0.92–0.98),
and where it did not, the two agree to three decimals (0.929 … 0.999).

| gap | class | `trim_frames` | `post_seam_r` | `post_seam_global_r` |
|---|---|---|---|---|
| 8 × dropouts | `repairable_dropout` | −1930 … +2002, all \|trim\| ≥ 290 | 0.916–0.979 | −0.281 … +0.474 |
| 33/17 | `ambient_quiet` | **+1461** | 0.973 | **0.030** |
| 34/24 | `ambient_quiet` | 0 | 0.929 | 0.929 |
| 10 × shared | `shared_silence` | 0, except 12/7 at **+2** | 0.988–1.000 | 0.989–0.999 |

**Corrected 2026-08-04.** This said "`trim_frames != 0` picks out *exactly*" and "all ten other gaps
trim zero frames"; 12/7 trims **2 frames**, so the predicate is not a clean zero test. The separation
is by magnitude — 290–2002 frames on one side, ≤ 2 on the other — which is still three orders of
magnitude but is a threshold, not a boolean, and would need one if it were ever used as a signal. The
`post_seam_r` floor on the shared row was also quoted as 0.993; the true minimum is **0.988** (25/1).

So two measurements that share no code and no inputs — the pre-decode envelope registration and the
post-decode dual-fit — both class 33/17 with the dropouts, while `a_below_noise_db` misses it by
0.6 dB. That is what makes §6.8.3 a candidate worth spending a corpus on rather than a coincidence
of one threshold.

33/17's `bridge_frames` exceed `gap_frames` by those 1461 frames — 30.4 ms at 48 kHz, ≈ the 36 ms
misplacement measured by ear in §3.3. **This is corroboration, not the mechanism**: `splice_dualfit`
is a diagnostic block, and 33/17's rendered patch did not come from dual-fit (`outcome.tier = patch`
⇒ the bracket gate patched it, and `dual_fit_used` is always `false` on a bracket). §7.1 traces the
overrun to its actual source. What the column does say is that the seam gate cannot see the problem
— `pre_seam_r` 0.998 / `post_seam_r` 0.973 both pass — and `post_seam_global_r` is the only field
that dissents.

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

#### 6.8.5a 14/13's two verdicts — found, then resolved: the scan side is right

Found 2026-08-04 while re-checking the tables. Every gap carries **two** equivalence blocks —
`scan_equivalence` (the scan verdict, copied forward by `characterize_gaps`) and `equivalence`
(characterize's own re-measurement). On 19 of 20 they agree. On **14/13 they do not**:

| block | class | `drop` | donor |
|---|---|---|---|
| `scan_equivalence` | `shared_silence` | **true** | 4/7 = 0.571 |
| `equivalence` | `repairable_dropout` | **false** | 3/7 = **0.429** |

**Resolved the same day, by ear and by envelope: there is no hole, and `shared_silence` is correct.**
The listener reports no drop. The envelopes agree — A's interior never approaches digital zero, and B
reproduces its contour bin for bin at the registered lag:

```
A:  -75.4  -76.5  -44.0  -45.6  -50.1  -84.2  -85.6  -86.4  -48.2
B:  -75.4  -78.5  -42.1  -47.9  -80.5  -84.5  -86.1  -87.2  -39.4   (registered, lag −1)
```

`a_gap_rms_db` −81.0, `gap_floor_db` −76.5, registered `a_interior_db` −48.1, `interior_delta_db`
**+0.09**. Every genuine dropout in this corpus reads **−101.5** on all three (9/9: −101.5 / −101.4 /
−101.5). This is mutual program quiet with internal structure, not a dropout.

Two ordinary mechanisms produced the false label, and both are already named in this file:

1. **The A-axis test fires on quiet-but-present content.** `a_below_noise_db` −42.9 clears the 35 dB
   margin against a −38.1 dB floor — but that floor is set by loud shoulders (−29…−36 dB), so a
   genuinely quiet −81 dB passage clears it without being empty.
2. **One donor block out of seven decides it.** 3/7 vs 4/7 straddles the 0.5 threshold on the strict
   `<` — §2.5's degeneracy verbatim. Registration moves it the right way: **0.571 → 0.714** at the
   registered lag, so `Apply` pushes 14/13 *deeper* into `shared_silence`.

§7.0 is therefore intact — no real dropout in this corpus is left unrepaired. §2.5's "the one gap the
gate kept, at 3/7 = 0.429" describes the older gate-off run; in this run it reads 4/7 and is dropped.

**What this gap is now evidence for: the fixed margin misfires in *both* directions.** 33/17 is a
real digital-zero hole the 35 dB margin **missed** by 0.6 dB; 14/13 is quiet program material the same
margin **flagged** as a dropout. One threshold, two opposite errors, on the same 20-gap set — and
`interior_delta_db` separates both correctly (33/17 **+35.7**, 14/13 **+0.09**) without knowing
anything about floors or codecs. See §7.4 item 4.

**Qualified 2026-08-04.** The 14/13 half stands and is the durable part: delta ≈ 0 on a gap the
margin labelled a dropout is a reliable "no hole here", and §6.10.9 finds 17 such gaps corpus-wide.
The 33/17 half does **not**: the ear check found the gap inaudible and the patch indistinguishable
(§6.10.7), so the delta's high reading there was not a rescue the margin missed. "Separates both
correctly" should be read as *"is correct in the negative direction"* — see §6.10.5.

#### 6.8.6 Still open

**ANSWERED 2026-08-04 — see §6.10.** 39 pairs, 829 gaps: `Apply` moves 2.05 % and touches none of the
236 rail-level dropouts (supports item 3); §6.8.3's separation does not survive (refutes item 4 as a
classifier). The paragraph below stands as the statement of what was open.

Unchanged by this run: **the corpus-wide rate**. These are the same 20 boundary-selected gaps —
12 nonzero lags, 1 abstain. Nothing here estimates behaviour on unselected material, and both
§6.8.2's "no flips" and §6.8.3's clean separation are claims about this sample. The cost of asking
the wider question has, however, collapsed: any envelope-bearing corpus answers both by `--replay`,
with no listen run and no re-dump.

### 6.9 The rate question is cheaper than §6.8.6 says (2026-08-04)

#### 6.9.1 It does not need a fingerprint corpus at all

§6.8.6 and §7.4 item 3 both scope the answer to "any envelope-bearing corpus … by `--replay`". That
means a **fingerprint** corpus — the expensive artifact. It is over-scoped.

Registration is computed at **scan** time, not during characterization. `scan_gaps.rs:245-261` builds
`GapEquivalenceParams` with `donor_registration: Some(DonorRegistrationParams::default())`
unconditionally, and `output.rs:740-796` serializes the resulting `gap_equivalence: Vec<…>` — verdict,
`donor_registration`, `envelopes`, `interior_delta_db` — on `GapScanJson` for **every** gap of a plain
`--format json` run. No decode, no per-bracket anchor oracle (~4.3–5.0 s/bracket × ~300 brackets/pair,
see `anchor-search-perf-baseline`), ~1 KB/gap of envelope.

So the rate question is answerable by a **scan-only run** — none of `--wav` / `--mux` /
`--repair-preview` / `--gap-fingerprints` set, so `PendingAfterScan::None`
(`composition.rs:352`). `scripts/scan-registration.ps1` drives this from the same manifest as
`measure-gap-fingerprints.ps1`, writing one JSON per pair to `gap-files/` with progress on stderr.

Corollary for anyone re-reading §6.6: the three read-offs (lag rate, would-flip rate, abstain rate) do
not require the dump either. They only ever needed the verdict.

#### 6.9.2 …but `--replay` cannot read that JSON yet

`equivalence_calibration.rs::collect_corpora` (`:717-746`) accepts only `corpus.json`, or a directory
containing one, and `replay_gap` reads `fp.scan_equivalence` off a loaded `GapCorpus`. The scan JSON
carries the same six fields `replay_gap` needs (`:876-951`) in a different envelope.

So today the cheap route answers items 1 and 3 by hand-parsing, **not** by the tool §7.4 item 3 names.
Teaching `--replay` the `GapScanJson` shape is a reader change with no measurement in it — the
`Apply` decision would then come from the production classifier rather than from a script. That is the
work item; it is small, and it is a prerequisite for quoting item 3's flip count as a *production*
number rather than a reconstructed one.

#### 6.9.3 `interior_delta_db` is written and read by nothing

§6.8.5a and §7.4 item 4 both reason from `interior_delta_db` as if it were available evidence. It is
available to a *reader of a dump*. In the code it is write-only: `measure.rs` records it, no decision
path consumes it, and §7.4 item 4 is the proposal to give it a first reader. Stated here because
"the delta separates the classes" is easy to misread as "the delta is doing work".

#### 6.9.4 First unselected-material data — one pair

The first pair scanned with `scan-registration.ps1` (corpus index 1; raw JSON in
`gap-files/2026-08-04-registration-rate/`, gitignored per `licensed-media-names-never-recorded`).
17 gaps, 17 verdicts, 16 registered with envelopes and `interior_delta_db`; gap 16 is
`not_evaluated` / `missing_signal`.

| quantity | this pair | the 20-gap set (§6.8) |
|---|---|---|
| nonzero `lag_blocks` | **16 / 16** (+1×9, +2×5, +4×1, −1×1) | 12 / 20 |
| `peak_r < 0.70` (abstain) | **2 / 16** (0.656, 0.350) | 1 / 20 |
| `interior_delta_db`, dropouts | +45.5 … +76.5 | +35.7 (33/17) |
| `interior_delta_db`, non-dropouts | −0.64 … +3.20 | +0.09 (14/13) |

Read carefully, because one pair is not a rate:

- ~~**The separation holds off-sample.**~~ ~~42 dB of clear air between the two populations on
  material nobody selected. This is the first evidence for §7.4 item 4 that is not from the boundary
  set.~~ **RETRACTED 2026-08-04.** One pair was not enough either: over 39 pairs the populations
  overlap by 58 dB (§6.10.5). The 42 dB read on this pair was luck of the draw, and it is exactly the
  error this bullet's own preamble warned about — "one pair is not a rate" applies to the separation
  as much as to the lag count.
- **The abstain rate is 2/16, not 1/20.** Still small, still a cost; the point is that §6.7.5's
  "1/20" is a boundary-set number and the wider figure is not yet known.
- **16/16 nonzero lag is not 16 independent registration failures.** §2.4 already makes this point
  about pair 25 — nine gaps, one systematic problem. Nine of these sixteen sit at exactly +1 bin.
  **The statistic that separates systematic offset from per-gap error is the per-pair modal lag and
  the spread about it, not the nonzero count**, and §6.6 item 1 asks for the flat count. Report both
  from the full scan.

#### 6.9.5 New: clipped, single-shoulder registrations

Gap 0 of this pair registered on `bins: 20` against a 99-bin A slice, with `core_bins: [0, 79]` and
`b_nominal_bin: 0` — the context window is clipped at the start of the media, so essentially one
shoulder survived. `MIN_REGISTRATION_BINS` is 8, so this registers and reports a `peak_r` like any
other gap.

Nothing in this file bounds how far `peak_r` can be trusted on a fifth of the intended window, and a
single-shoulder correlation cannot distinguish a genuine lag from a lag that only fits the material
on the side that survived. **Before `Apply` ships, the full scan should report the `bins` distribution
and the abstain rate conditioned on it.** If clipped registrations cluster near the `min_envelope_r`
boundary, the answer is a bins floor rather than a lower `r`.

**Answered in §6.10.3 — and the framing above was wrong about the cause.** Clipping is not a
start-of-media special case: `bins < 40` occurs on **31 gaps and is head-or-tail in 31/31**. The
condition to test is "first or last gap", not "clipped window".

### 6.10 The 39-pair scan — §6.8.6's rate question, answered (2026-08-04)

The scan-only route of §6.9.1, run to completion over the corpus: **39 pairs, 829 gap verdicts, 782
registered.** Replay per §6.8.5's `replay_gap` logic (reconstructed in a script, not the tool — §6.9.2
still stands). Pairs are named by corpus index only and the raw JSON is gitignored and **disposable**:
everything load-bearing is transcribed here, so the corpus can be deleted without losing the argument.
Nothing below requires re-reading it.

#### 6.10.1 Coverage

| class | n |
|---|---|
| `shared_silence` | 448 |
| `repairable_dropout` | 254 |
| `ambient_quiet` | 80 |
| `not_evaluated` | 47 |

All 47 unevaluated are `missing_signal`, and **all 47 are a pair's first (27) or last (20) gap** —
context windows that run off the end of scanned material. Zero occur mid-media. Every one of the
remaining 782 replayed successfully, so the flip counts below are complete, not sampled.

#### 6.10.2 Lag: the flat rate is misleading, the modal decomposition is not

**530 / 782 = 67.8 %** of registrations sit at a nonzero lag — far above the 20-gap set's 12/20, and
on unselected material. But §6.9.4's third bullet was right to ask for the decomposition:

- **23 of 39 pairs have a modal lag ≠ 0.** The offset is a property of the *pair* — a master-to-master
  timing difference — not a per-gap measurement failure.
- **Residual scatter about each pair's own mode is 102 / 782 = 13.0 %.**

Pair 25 is the archetype §2.4 described: 48 gaps, every one at +3 or +4 blocks (mean ≈ +371 ms),
matching §2.5's measured +332 / +410 / +347 ms. Counting those as 48 failures would be counting one
problem 48 times.

**This is the strongest support in the file for item 3.** Two thirds of all gaps are measured at the
wrong place, and the misregistration is systematic per pair rather than noise.

#### 6.10.3 Abstain rate, and §6.9.5 resolved

**34 / 782 = 4.3 %** fall below `min_envelope_r` 0.70 (`peak_r` min 0.200, p05 0.733, median 0.978).
By class: 29 `repairable_dropout`, 5 `shared_silence`. Abstain ⇒ keep ⇒ repair path, so these cost a
patch attempt, never a hole.

The `bins` distribution is bimodal with nothing between: **751 at 40, 31 at 20.** Conditioned:

| window | n | abstain | median `peak_r` |
|---|---|---|---|
| 40 (full) | 751 | 31 (4.1 %) | 0.976 |
| 20 (clipped) | 31 | 3 (9.7 %) | 0.995 |

Clipped registrations abstain at roughly twice the rate but their median `peak_r` is *higher*, so they
are bimodal rather than uniformly degraded — the fits are either very good or fail outright, which is
what a single surviving shoulder predicts. **And all 31 are head (12) or tail (19) gaps, none
mid-media**, which makes §6.9.5's proposed "bins floor" equivalent to a head/tail exclusion. Prefer
the latter: it is the actual condition, and it is checkable without reading the registration.
**(2026-08-04 ship note:** `Apply` landed without this exclusion — §7.4a.)

#### 6.10.4 What `Apply` would change: 2.05 %, and only 3 gaps that render

**16 flips / 782 = 2.05 %.** Drop set 528 → 534.

| direction | n | class | character |
|---|---|---|---|
| keep → drop | 11 | all `repairable_dropout` | all `\|interior_delta_db\| < 1` |
| drop → keep | 5 | all `shared_silence` | **all 5 are abstentions** |

The five drop→keep are `Apply` declining to decide, not `Apply` disagreeing. They cost a patch attempt.

The keep→drop eleven are the ones that matter, and they are far narrower than "11 dropouts stop being
repaired". Nested:

| | n |
|---|---|
| `repairable_dropout` | 254 |
| …with `a_interior_db` at the digital-zero rail (≤ −100 dB) | **236** |
| …not at the rail | 18 |
| …of those, `interior_delta_db < 5` (A and B carry the same content) | 17 |
| …of those, flipped by `Apply` | **11** |

**`Apply` touches none of the 236 genuine holes.** Every gap it would stop repairing is one where A is
40–80 dB above the rail and B reads within 1 dB of A at the registered lag — i.e. both masters carry
the same quiet material and there is nothing to fill.

Joining against the 2026-07-31 fingerprint corpus (829/829 keys matched, class agreed 802/802;
corpus-wide patch rate 372/802 = **46.4 %**, being 291 `tier: patch` plus 81 `dual_fit_rescue`):
**only 3 of the 16 flips reach a rendered patch at all** — 12/8, 14/20 and 38/4. The other 8 keep→drop
gaps are already declined downstream by the seam gate, and the 5 abstentions render nothing by
construction. So the entire production effect of promoting `Apply`, over 39 pairs, is **three patches
that stop being applied**.

Two of the three were examined bin by bin and are not dropouts. **12/8**: three ~−99 dB bins
*alternating* with −34.8 / −52.4 / −36.7 dB program — a stutter merged into one span by
`silence_hold_ms: 500` — with B reproducing the contour at core r = 0.9987 and delta −0.095.
**14/20**: quietest bin −85.8 dB, 16 dB above its pair's rail; B's quietest −86.0, core r = 0.932,
delta +0.016. Core bins are excluded from the registration fit, so those core correlations are
independent of the lag that produced them. **Heard 2026-08-04 (§6.10.11):** none is a dropout;
all three current patches are audible degradations.

#### 6.10.5 `interior_delta_db`: §6.8.3's separation does **not** survive off-sample — REFUTED

This is the finding that changes a recommendation. §6.8.3 tabulated clean separation on 20 gaps
(dropouts +49.7…+75.8, non-dropouts −4.4…+0.4) and §6.9.4 reported ~42 dB of clear air on one
unselected pair. Over 39 pairs the two populations **overlap by 58 dB**:

| class | n | min | median | max |
|---|---|---|---|---|
| `repairable_dropout` | 254 | **−0.51** | +62.62 | +78.32 |
| `shared_silence` | 448 | −7.13 | +0.00 | **+57.59** |
| `ambient_quiet` | 80 | −4.55 | +0.00 | +35.66 |

At a 5 dB threshold: 17 dropouts fall below it and 9 non-dropouts sit above it. There is no threshold
that separates them — 30 dB still leaves 18 below and 4 above. The clean tables in §6.8.3 and §6.9.4
were a property of samples too small to contain the overlap region, and both should now be read as
superseded.

#### 6.10.6 The rescue candidates are head-trim differences and room tone

All 9 non-dropouts with `interior_delta_db ≥ 5` — the set item 4 would reclassify as
`repairable_dropout`:

| pair/gap | delta | `a_interior_db` | `b_interior_db` | position |
|---|---|---|---|---|
| 26/0 | +57.59 | −101.02 | −43.43 | head |
| 3/0 | +44.90 | −101.52 | −56.62 | head |
| 33/17 | +35.66 | −101.46 | −65.80 | **interior** |
| 6/0 | +35.10 | −101.24 | −66.14 | head |
| 13/0 | +29.99 | −95.90 | −65.91 | head |
| 21/0 | +29.92 | −94.12 | −64.21 | head |
| 33/2 | +24.26 | −87.41 | −63.15 | **interior** |
| 32/0 | +22.83 | −100.86 | −78.03 | head |
| 34/0 | +14.27 | −101.47 | −87.20 | head |

**Seven of nine are the pair's first gap** (`a_span_secs` starting at 0.000) — head-trim differences
between masters, where one master begins with digital silence and the other with room tone. Those are
not defects and repairing them is not a repair.

**And every one of the nine has `b_interior_db` ≤ −43 dB.** Not one has a donor carrying anything near
program level. Whatever these gaps are, the material the rescue would import is inaudible.

#### 6.10.7 33/17 heard: the one rendering candidate is inaudible — item 4's supporting set is empty

33/17 is the only delta candidate that reaches a rendered patch, and the gate-off listen corpus
already contains its full triple (`_a_surround`, `_b_surround`, `_a_patched`) — the counterfactual
audio for exactly this recommendation, since production drops the gap and the WAV shows what item 4
would ship instead.

**Listened 2026-08-04: no dropout audible in A, and the patch is indistinguishable from both A and B.**

**This conflicts with §3.1a and the conflict is left open.** The 2026-08-03 ear check on the *same
WAV* reported *"on close listening the patch on 33/17 is audibly wrong"*, alongside a measurement of
A in gap −100.6 dB → patched −54.2 dB (**+46.3 dB**, the largest in that table) with the splice
rewriting −0.01…+0.98 s of surrounding A. The two reports split cleanly on one half: both agree there
is **no dropout in A**. They disagree only on whether the *patch* is audible. The +46.3 dB is
objective and not in dispute; what is in dispute is whether −54 dB material in a −69 dB bed is
noticeable, which is marginal and playback-level dependent. Recorded rather than resolved — it bears
on §7.4 item 1's motivation, though not on item 1's other evidence (12/7 rewrites **3.43 s** of
surrounding A, which is a correctness problem regardless of audibility).

The measurement was not wrong — `a_interior_db` −101.46 is genuine digital silence in A's interior.
What the ear adds is that the thing A is missing is B's −65.80 dB room tone, inside a passage already
classed `ambient_quiet`. A +35.66 dB delta between digital zero and *very faint* is perceptually
nothing.

**This is the flaw in the field as a positive test: `interior_delta_db` is a ratio with no absolute
anchor.** The same +35.66 dB between −60 and −24 dB would be a glaring hole. A bare delta threshold
cannot tell those apart, and §6.10.6 shows the corpus contains only the inaudible kind.

So item 4's supporting set is now empty: 7 head-trim artifacts, 1 interior case that never renders
(33/2), and 1 interior case refuted by ear. **The recommendation is dead *as a classifier*.** See
§6.10.7a, which rescues a weaker form of it.

#### 6.10.7a Forward-anyway: the gate is a cost filter, not a correctness gate

Raised 2026-08-04, and it survives scrutiny: the classification does not have to be right if it errs
toward *forwarding*, because the repair path has finer granularity than the gate and can decline.
`skip_equivalent_gaps` shipped to save scan work, not to protect audio, so its errors should be
biased toward the patch path. §6.10.6's nine candidates would then be a cheap **recall widener**
rather than a classification claim, and the ratio problem of §6.10.7 stops mattering — a false
positive costs a patch attempt, not a hole.

The corpus supports the premise. The repair path declines **511 of 802 gaps (64 %)** and
independently declined **8 of the 11** keep→drop flips (§6.10.4). Cost is negligible: 9 candidates
over 39 pairs, 7 of them head-of-media gaps that produce **zero brackets**.

**One measured counterexample, and it is the whole risk.** 33/17 is the only delta candidate that
reaches the repair path with brackets, and the repair path **accepted** it (`tier: patch`). The
empirical rejection rate for delta candidates is therefore **0 of 1**, not 8 of 11 — and the patch it
produced is §3.1a's +46.3 dB one. The reason is named in §7.4 item 1: Pearson is scale-invariant by
construction and there is **no level or loudness term anywhere in the fill path** (`patch_region.rs`,
`gap_fill_fit.rs`). The downstream can arbitrate on correlation and residual; it cannot see "this
fill is 46 dB louder than what it replaced." The granularity the argument relies on does not exist
yet — item 1 *is* that granularity.

So the forward-anyway framing is sound and item 4 becomes: **use `interior_delta_db` as a recall
widener, strictly after item 1 ships.** Two things stay true regardless — its measured benefit on
this corpus is **zero** (all nine candidates have `b_interior_db ≤ −43 dB`, so even a successful fill
imports inaudible material, and seven are head-trim differences between masters), and it should be
paired with the head/tail exclusion of §6.10.3, which removes seven of the nine for free.
**(That exclusion did not ship with `Apply` — §7.4a; item 4 would still need it if revived.)**

#### 6.10.8 …and the anchor that would fix it is not available

The obvious repair is to add an absolute floor on `b_interior_db` — "B has real program here", not
merely "B is louder than A". **That route is closed, and this file already closed it.**

- §7.2 measured exactly this and dropped it: deriving an achievable floor from the gap's own ±2 s
  context yields **0.8 dB of separation**, because shoulders are program material, not floor. It is in
  the Dropped list as *"Derive the achievable floor from the gap's local context"*.
- The scan retains no whole-file level statistic to anchor against. `scan_gaps.rs` streams and keeps
  only silence spans and `scanned_end_secs`; `GapScanJson` carries no global level, loudness, or
  program-reference field. There is nothing to read.
- An absolute dBFS number is not portable across masters anyway — −65 dB means different things in a
  quiet film and a loud one, so the anchor would have to be *relative to that title's own program
  level*.

The only way to get one is to **compute it dynamically from a much larger sample of the video** — an
integrated program level over the whole scan envelope, a new scan-side statistic with its own cost,
serialization and calibration. That is a project, not a threshold tweak, and there is no evidence here
that it would pay: §6.10.6's nine candidates would need it to reject all nine, which is the same as
not having the rule.

**So no anchored variant of item 4 is on the table.** Anyone re-proposing one is proposing the
program-level statistic first.

#### 6.10.9 What survives: the delta as a *negative* test

The field is not useless — its useful direction is the opposite of item 4's. **`interior_delta_db ≈ 0`
on a gap labelled `repairable_dropout` is a reliable "there is no hole here" signal**, and it needs no
anchor at all, because it compares A against B rather than against an absolute level. That
scale-freedom is the property §7.2 praised; item 4 spent it by using the field as a level test.

**Qualified 2026-08-04 — do not upgrade this to "A and B carry the same content."** The field is a
difference of **power means** over the eroded interior, so it goes to zero whenever both sides carry
the same total energy, however distributed across bins. The "no hole" reading is an aggregate-level
claim and is supported as one (12/8 and 14/20 both have delta ≈ 0 and were heard hole-free,
§6.10.11); the content-identity reading does not follow from this field and needs a direct core
comparison. Note also that the converse fails: **14/20's core r is 0.372 and it has no hole** — four
quiet bins make the correlation meaningless — so neither statistic decides a gap on its own.

> **Numbers retracted 2026-08-04 (§6.10.12).** This paragraph originally cited a bin-by-bin core
> comparison of the 6 residual gaps — core r 0.667–0.938, mean deviations 2.5–11.5 dB, "14/14 reads
> delta +0.01 with a mean bin deviation of 11.5 dB and one bin off by 35 dB" — as evidence that the
> aggregate overstates the match. **Those deviations were a lag-quantization artifact of the 100 ms
> scan grid**, not a property of the content. Measured on the WAVs at 10 ms with the waveform-derived
> lag, the same five gaps run **envelope r = 1.000, mean deviation 0.1–0.3 dB, max ≤ 2.0 dB**. The
> qualification above stands on its own logic — a difference of power means genuinely cannot establish content
> identity — but it is not supported by these gaps, which turned out to be as well matched as 12/8.

All 17 such gaps (delta < 5 dB on a labelled dropout), with A far above the rail and B within a
decibel of it:

| | |
|---|---|
| n | 17 of 254 dropouts |
| `a_interior_db` range | −39.46 … −82.16 (**none at the rail**) |
| `\|delta\|` | ≤ 0.84 dB |
| already flipped by `Apply` (item 3) | **11** |
| residual, caught by neither | **6** — 10/12, 14/8, 14/14, 14/17, 14/24, 16/23 |

Item 3 does most of this work already, via a different mechanism (the donor census at the registered
lag). The 6 residual gaps are the only thing a negative-delta test would add, and whether they render
is unmeasured. **Recorded as an observation, not proposed as a change** — it is the same corpus and
would need the same ear check the flips still need.

#### 6.10.10 `skip_reason` is a hardcoded placeholder, not a corpus defect

Every skip in every corpus on disk reads `correlation_below_threshold` — 511 in the large corpus, and
also 19 in the 2026-08-04 runs from the current binary, so this is not a fixed bug and **re-running
the corpus would return the same single value**. The source says so directly:
`measure.rs:2465-2474` constructs `GapPatchSkipReason::CorrelationBelowThreshold` with
`pre/post/min_correlation` all `0.0` on *every* skip, under a comment stating *"A placeholder
strategy/reason carries only the `patch`/`skip` distinction the reader's `tier` axis needs."*
`project.rs:350-358` can serialize all seven variants; the fingerprint measurement path constructs
one. The zeroed correlations are the tell.

This is a write-only-placeholder problem in the same family as §7.4 item 2, fixable by threading the
real reason through `compute_region_measurements`, and independent of any media.

#### 6.10.11 The three flips, heard — and the mechanism behind them

**Listened 2026-08-04, and the result is unambiguous: none of the three has a detectable hole.** What
they actually are:

| gap | what the listener heard | patch |
|---|---|---|
| 12/8 | a drum beat moving into music — periodic, slower than a stutter | **noticeably poor** |
| 14/20 | a quiet word spoken slowly, pause, loud exclamation, pause, another exclamation | barely noticeable — *"I likely wouldn't have noticed if I didn't know"* |
| 38/4 | a loud noise, pause, a clock ticking, pause, someone speaking | barely noticeable; the ticking is **slightly louder, dirty** |

So §6.10.4's envelope reading was right in substance and wrong in one detail: 12/8 is not a stutter,
it is a slower periodic beat. Nothing else changes.

**All three are the same failure, and the envelopes show it directly.** At the registered lag A and B
carry identical content bin for bin:

| | A core | B at registered lag |
|---|---|---|
| 12/8 | −99 −100 −35 −52 −100 | −99 −100 −35 −49 −100 |
| 14/20 | −84 −82 −73 −86 | −86 −73 −79 −86 |
| 38/4 | −84 −57 −89 −54 −90 −56 −89 −55 −91 −57 −89 −55 −90 −57 −91 −54 −91 −58 −80 | −81 −57 −89 −54 −90 −56 −87 −55 −90 −57 −88 −55 −91 −57 −90 −54 −91 −58 −91 |

38/4's 19 bins of −55/−90 alternation *are* the clock, matching between masters to 1–2 dB. There is
nothing missing from A in any of the three.

**Name the class: the periodic-transient false dropout.** At the *nominal* offset, 12/8's donor reads
−100 −35 −49 −100 −39 — B's loud bin lands on A's silent bin. That is exactly the dropout signature
("B has content where A has none"), and it is manufactured by **one 100 ms bin of registration error
on content that alternates loud/silent every 100 ms**. Drum beats, clock ticks and speech pauses all
have that structure. This is a third, ear-confirmed instance of §6.8.5a's mechanism, and the sharpest:
the misregistration does not merely mismeasure the donor, it *synthesises* the defect it then repairs.

**The patch damage is the size of the substitution**, which is why the three ranked as they did:

| gap | content | what the splice substitutes | heard |
|---|---|---|---|
| 12/8 | drum beats | −35 dB beat over −99 dB silence, **≈65 dB** | noticeably poor |
| 38/4 | clock ticks | −55 over −90, **≈35 dB** | slightly louder, dirty |
| 14/20 | speech pauses | −73 over −86, **≈13 dB** | barely noticeable |

Monotone in the substitution magnitude — and that magnitude is precisely what §7.4 item 1 measures.

**Consequence for item 3: `Apply` is not a cleanup, it is a fix.** §6.10.4 framed the three flips as
"three patches that stop being applied", the implication being three harmless no-ops removed. The ear
says otherwise: **all three current patches are audible degradations of undamaged audio**, one of them
plainly so. Promoting `Apply` removes three real defects. The last open risk on item 3 is discharged.

Extension, **verified 2026-08-04 for 5 of 6** — see §6.10.12: the low-delta dropouts `Apply` does
**not** catch (§6.10.9 — 10/12, 14/8, 14/14, 14/17, 14/24, 16/23) are the same class, reached by a
different route. 16/23 remains unmeasured (pair 16 was never run).

#### 6.10.12 The 6 residual gaps, measured at sample level — the donor test's *formulation* is the defect

The `--gap-listen` run for the six §6.10.9 residuals had already completed for pairs 10 and 14 (5 of
the 6; pair 16 was never run), so this needed no new media pass — only a read of WAVs already on
disk. It is the first sample-level A-vs-B interior evidence in this file, and it changes what §6.10.9
and §6.10.11's closing paragraph claim.

**Validation first, because it makes every number below comparable to the gate's own.** The
`_a_surround` WAV reproduces the scan envelope **exactly — all 49 bins of 10/12 to 0.1 dB** — when
read with interleaved reduction. The clip's frame 0 is `a_start_secs − gap_signature_context_secs`,
the mapping is confirmed, and mono downmix reads 3–8 dB quieter than the scan's interleaved reduction
exactly as `DonorInteriorBasis` documents. A WAV-side measurement can therefore be quoted against a
scan-side threshold without a basis caveat, provided the reduction matches.

**There is no hole, and nothing is near the digital-zero rail.** Over the silent core
(`a_span_secs`, eroded one 100 ms block per edge), counting frames where *every* channel is exactly
zero in the 16-bit clip:

| gap | A longest zero run | A zero frac | B longest zero run | B zero frac | quietest 10 ms bin A / B |
|---|---|---|---|---|---|
| 10/12 | 0.0 ms | 0.001 | 0.1 ms | 0.001 | −95.3 / −95.0 |
| 14/8 | 0.1 ms | 0.008 | 0.1 ms | 0.008 | −100.1 / −100.3 |
| 14/14 | 0.1 ms | 0.001 | 0.0 ms | 0.000 | −89.1 / −89.0 |
| 14/17 | 0.1 ms | 0.004 | 0.1 ms | 0.004 | −93.3 / −93.0 |
| 14/24 | 0.0 ms | 0.000 | 0.0 ms | 0.000 | −90.6 / −90.1 |

The longest all-channel-zero run anywhere is a single sample crossing. This closes the hedge recorded
against §6.10.9 — 100 ms RMS could not rule digital zero out, and at sample level it is ruled out.

**A and B carry the same content, at 10 ms.** Envelope Pearson is **+1.000 on all five**, mean bin
deviation **0.1–0.3 dB**, max deviation ≤ 2.0 dB, and the core level difference at the registered lag
is within ±0.1 dB. Waveform Pearson runs 0.51–0.89, which is the expected signature of two
independent encodes of one master — high enough to confirm the clips are not duplicates, low enough
to confirm they are not the same file.

**This retracts the deviation figures §6.10.9 carried.** Those came from the 100 ms scan grid at the
engine's bin-quantized lag. `listen-registration` puts the true lag at **−118 to −122 ms** on the four
pair-14 gaps where the engine registered −100 or −200 — 0.18 to 0.78 bins off, all within the
estimator's stated tolerance. At 10 ms with the true lag the deviations collapse to 0.1–0.3 dB. The
apparent mismatch was the sub-bin residual, the same quantity behind §6.10.11's mechanism, showing up
as inflated deviation instead of a class flip.

**The mechanism, and it is not misregistration.** These five are *correctly* registered and still
classified `repairable_dropout`:

| gap | class | A silent blocks | donor silent blocks | donor frac | `a_below_noise_db` | production |
|---|---|---|---|---|---|---|
| 10/12 | `repairable_dropout` | **4/9** | **4/9** | 0.444 | −40.0 | `tier: patch` |
| 14/8 | `repairable_dropout` | 3/8 | 2/8 | 0.250 | −36.9 | `tier: skip` |
| 14/14 | `repairable_dropout` | 3/8 | 2/8 | 0.250 | −45.7 | `tier: skip` |
| 14/17 | `repairable_dropout` | 3/8 | 1/8 | 0.125 | −44.1 | `tier: skip` |
| 14/24 | `repairable_dropout` | 2/6 | 0/6 | 0.000 | −42.7 | `tier: skip` |

**10/12 has identical silent-block counts on both sides — 4 of 9 and 4 of 9 — and the gate still calls
it a dropout.** It reads "A sits 40 dB below its noise floor ⇒ hole" and "the donor is only 44 %
silent ⇒ occupied" off the *same* alternating pattern present in *both* masters. Nothing is
mismeasured and nothing is misplaced; the two tests are simply asked independently. **The donor test
asks "is B non-silent?", never "is B non-silent *where A is silent*?"** — so any quiet periodic
passage satisfies both halves of the dropout definition in both files at once.

That is why `Apply` does not reach this set: it corrects *where* the donor is measured, and the
placement here is already right. Item 3's scope genuinely does not cover it, which §6.10.11's
extension paragraph had guessed at and this measures.

**Production blast radius is 1 of 5**, and the bracket gate is doing most of the work: only 10/12
reaches `tier: patch`; the four pair-14 gaps are `tier: skip` with `dual_fit_rescue: false`.

**10/12's patch, measured against A and then heard.** The write is bounded (1.0 s, ~0.1 s outside the
core each side — inside the §7.1 allowance), and every modified block is louder:

| clip time | A | patched | Δ |
|---|---|---|---|
| 6502.07 | −76.8 | −65.5 | **+11.3** |
| 6502.17 | −52.7 | −40.5 | **+12.1** |
| 6502.37 | −81.1 | −69.1 | **+12.0** |
| 6502.57 | −82.5 | −70.7 | **+11.8** |
| 6502.67 | −77.3 | −42.1 | **+35.2** |
| 6502.87 | −82.6 | −70.1 | **+12.4** |

+11 to +35 dB injected into material that already matched B to 0.1 dB. §6.10.11's substitution-magnitude
rule predicted "audible but mild — the 38/4 register" from the ≈35 dB peak. **Listened 2026-08-04: the
patch sounds worse than A.** Fourth ear-confirmed instance of a patch degrading undamaged audio, and
the first prediction this file made *before* the listening rather than after.

**What this adds that §6.10.11 did not.** The three flips were misregistration — a placement bug with
a placement fix. These five are the same *outcome* with correct placement, so the class survives item
3 entirely. Item 1 is the only recommendation on the table that catches them: a fill 11–35 dB above
the shoulders it replaces is exactly what a level check against the A shoulders sees, and it needs no
opinion about whether the gap was real.

#### 6.10.13 Still open after this scan
- **27 head/tail gaps carry no `outcome`** in the fingerprint corpus (summary tier), so their
  production disposition is unknown. They are disproportionately §6.10.6's candidates.
- **One corpus, one codec family.** Everything above is AAC-family material from a single collection.
- `--replay` still cannot read `GapScanJson` (§6.9.2), so the 16 flips are a reconstruction, not a
  production number.
- **16/23 is unmeasured** — pair 16 was never run for the §6.10.12 listen set. It is the one residual
  gap whose membership in the class is inferred rather than measured, and it is the only one of the
  six whose core minimum sits close to its pair's digital-zero rail (6.8 dB above, against 11.7–20.5
  for the other five). Cheapest remaining check in this file.
- **No dump can answer §6.10.12's question.** `b_levels`, `seam_probe`, `wide_envelope` and `lag` are
  all gated on `--fingerprint-diagnostics` and no corpus on disk used it; A's `levels.profile_db` is
  never emitted on the decode path at all (`project.rs:334-347` rebuilds the struct with an empty
  vector, pinned by a test at `measure.rs:4191-4195`), and `seam_probe` covers the seam *border*
  window rather than the interior. Sample-level A-vs-B interior comparison currently requires
  `--gap-listen` WAVs. Threading the already-computed `levels` into `FingerprintXSet` beside
  `b_levels`, under the same gate, would put both sides in one file on one basis.

---

## 7. Production-pipeline recommendations

Written 2026-08-04 in answer to three questions: can anything here improve production, can 33/17 be
identified and patched correctly, and were other gaps patched badly. **Four candidate
recommendations were checked against source and three of them died.** The dead ones are recorded
with their refutations, because each was plausible enough to be re-proposed.

### 7.0 The corpus, read as a production question

Two framing corrections first, both from re-reading the dump against the code.

**`outcome.tier` is the bracket gate only.** `GapRow::dual_fit_rescue` is documented as *"would
production's dual-fit rescue this bracket-gate skip?"* and `production_patched()` as the predicate a
"how many holes remain?" roll-up wants (`gap_fingerprint_corpus/schema.rs:256-285`). Reading `skip`
as "production left a hole" under-counts repairs by exactly the dual-fit rescues.

| gap | class | tier | `dual_fit_rescue` | what production does |
|---|---|---|---|---|
| 5/4, 7/3, 16/7, 19/13, 35/10, 36/9 | `repairable_dropout` | patch | — | patches |
| **8/4, 9/9** | `repairable_dropout` | skip | **true** | **patches via dual-fit** |
| 14/13, 25/8, 25/36, 28/22 | `shared_silence` | skip | false | drops at the equivalence gate anyway |
| the other 8 | `shared_silence`/`ambient_quiet` | patch | — | drops at the equivalence gate |

So **no real dropout in this corpus is left unrepaired**, and 9/9 — which the ear reports the patched
WAV fixes correctly — is a gap production repairs. (14/13's characterize-side block dissents and calls
it `repairable_dropout`; that was checked out in §6.8.5a and the scan verdict listed here is the
correct one — there is no hole.) (§3.5's "`dual_fit_rescue` was `false` on all 21"
described the older 21-gap run and does not generalize.)

**Only one gap is both real and badly patched.** §3.1a's eight gate-off patches look like eight
defects but seven are `shared_silence`/`ambient_quiet` with `drop: true` — gaps production never
renders, so they are evidence that the splice path misbehaves *when handed program silence*, not
shipped defects. 33/17 is the only gap where the hole is real and the patch is still wrong, which is
why §6.8.4's coupling matters: reclassifying it is what would make the defect reachable.

### 7.1 The write overrun is inside a configured bound — REFUTED as "unbounded"

The proposal was to bound the write extent to the gap span. Two reasons it does not stand:

1. **Dual-fit already does exactly that.** `DualFitResult.fill` is *"interleaved, exactly
   `gap_frames`"* (`dual_fit.rs:45`); `trim_frames` is the interior trim that reconciles the bridge
   back to gap length. The bound cannot "defeat dual-fit" — dual-fit is the path that already
   honours it.
2. **The boundary grid is bounded too, at 500 ms.** `evaluate_seam_gate_fit_joint`
   (`patch_region.rs:898-909`) sets `start_min = baseline.start − max_extend_frames` and
   `end_max = baseline.end + max_extend_frames` from `gap_end_extend_max_ms`, **default 500**
   (`config.rs:402`), step 20 ms, with `gap_start_extend_on_pre_seam_fail` /
   `gap_end_extend_on_post_seam_fail` both defaulting true. 33/17's 380 ms overrun is *inside* that
   allowance — a deliberate, configured extension doing what it was built to do.

   **Caveat, added 2026-08-04:** that bound is the *grid's*. The anchor-bracket path has its own
   feasibility budget, and the `brackets` array in the dump is the anchor enumeration
   (`measure.rs:1979`, `list_feasible_anchor_brackets`), not the grid — 33/17's rows reach
   `move_frames: 88799` (~1.85 s), well past 2 × 24 000. So "bounded at 500 ms" is true of the grid
   and **unverified for whatever path actually rendered 33/17**; see §7.1a.

The extension exists **because** the boundary is ambiguous in silence: it is the retry after a seam
fails. Tightening the cap would break the mechanism that rescues genuinely misdetected boundaries,
and a detector that could tell "this extension is wrong" from "this extension is a rescue" would
have to be better than the alignment already is on mostly-silent material.

~~**What survives is narrower.** … Nothing prefers a smaller extension to a larger one … Making
extension *cost* something is a comparator change.~~ **RETRACTED 2026-08-04 — see §7.1a. Extension is
already priced, heavily, and the mechanism this described is not the one that produced 33/17.**

### 7.1a The comparator already prices extension — the "make it cost something" proposal is dead

Three independent checks, each fatal on its own.

**1. The penalty exists.** `fit_candidate_ranking_score(min_waveform, boundary_move_frames)` is
`min_waveform − BOUNDARY_MOVE_PENALTY_PER_FRAME × boundary_move_frames`, the constant `2e-4`
(`gap_fill_fit.rs:232,247`), and `boundary_move` is start-move + end-move against the baseline
(`patch_region.rs:415,1700`). Anchor brackets pay that *plus*
`ANCHOR_CENTER_DRIFT_PENALTY_PER_FRAME` (`1.5e-4`) on center drift (`gap_fill_fit.rs:263,272`). The
tie-break is there too: `winner_cmp` orders by score then **smaller** `boundary_move`
(`fit_routing.rs:95`), pinned by `fit_candidate_ranking_prefers_less_boundary_move_at_equal_waveform`.
On the scale that matters, 33/17's 16 799-frame move costs **3.36** against a Pearson range of
[0, 1] — extension is not underpriced, it is priced so hard that no candidate carrying one can win
against *any* baseline that scores at all.

**2. The grid does not run in default production.** `RepairProfile::Default` sets
`fit_boundary_search: BaselineOnly` (`repair_profile.rs:132`; only `Full` sets `FullGrid`, `:144`),
and E5 returns the pool winner before the grid is ever enumerated (`patch_region.rs:1023`). A
grid-comparator change is a **no-op outside `--full`**.

**3. It would not have changed 33/17.** Its own bracket table says the small extensions were
*rejected*, not out-ranked:

| move_frames | end offset | `seam_pre` / `seam_post` | outcome |
|---|---|---|---|
| 0 (baseline) | — | 0.214 / 0.210 | `failure_stage: waveform_floor` |
| 7 200 | −150 ms start | 0.256 / 0.253 | `failure_stage: waveform_floor` |
| **16 799** | **+350 ms end** | **0.420 / 0.431** | placement produced |

The +350 ms bracket is the *smallest* move that scored at all. "Prefer the smallest |extension| among
candidates within noise" has nothing to choose from here — the candidates it would have preferred are
below the waveform floor. Whatever admitted this bracket, it was the **acceptance floor**, not the
comparator.

**What is not known, and blocks re-proposing anything here.** The rendered patch's seams (§3.3:
`pre_seam_r` 0.998 / `post_seam_r` 0.973) match **no row** in that table, whose scores top out at
0.43. The dump records the oracle enumeration, not the candidate production selected, so *which path
placed 33/17's fill is unrecorded*. Until that is instrumented, any proposal aimed at "the path that
over-extended" is aimed at a path that has not been identified. That is why §7.4 item 3 is an
investigation and not a change.

### 7.2 "The 35 dB margin was unreachable" — true, but not measurable without a codec table

33/17's context floor is −67.2 dB, 15 dB quieter than anything else in the corpus; the dropout test
asks for −102.2 dB and AAC bottoms out near −101.5 (already documented at
`application/gap_equivalence.rs:249`). The test was **arithmetically impossible** for that gap — not
0.6 dB too tight. That is the honest description of the failure and it belongs in §6.2 item 3's
framing.

It does **not** yield a usable rule without knowing the achievable floor, and measuring that floor
from the gap's own ±2 s context does not work. Taking each gap's context minimum as the measured
floor and asking whether `noise_floor_db − 35` falls below it:

| | shortfall below the observed context minimum |
|---|---|
| `shared_silence` / `ambient_quiet` | 0.8 (34/1), 0.9 (25/36), 3.2 (25/1), 4.6 (10/11), 7.4 (34/24) — five fire |
| `repairable_dropout` | 8.2 (35/10) … 31.6 (19/13); 16/7 does not fire at all |
| 33/17 | 28.3 |

Separation between the groups is **0.8 dB**, and it needs a new threshold — the thing the framing was
supposed to avoid. It only looks clean against a constant −101.5 codec floor, i.e. against a
per-codec table that would have to be threaded through the gap system.

The reason it fails is that shoulders are program material, not floor: 12/7 and 28/22 have context
minima near −100 dB while 33/17's never get below −73.9. **A local context cannot tell you what the
decode is capable of.** `interior_delta_db` remains the recommendation, and on this criterion it is
the better instrument anyway: it needs no floor knowledge, no codec identity, and no table — only
whether B's interior sits above A's at the registered lag.

### 7.3 The residual probe's nominal anchor is deliberate — REFUTED as an oversight

The proposal was to re-centre the residual floor probe on the placement actually used, and to widen
its ±10 ms reach. Both die, for different reasons, and the archived ledger has both:

- **Widening.** `residual-gate-wiring-plan.md:206` states the sizing rationale explicitly: *"Must
  exceed residual alignment error after the aligner; larger = O(lag·window) cost."* It is a
  **post-aligner** budget and correct as one. Widening to 600 ms is 60×: ~28 800 lags × ~12 000-frame
  windows × 2 sides per gap, seconds per gap, on a pipeline where gate search already dominates.
- **Re-centring.** The *chosen* probe is already lag-centred — `residual-gate-findings.md` **M5**,
  *"Real-codec reach false-veto. Lag-centered probe + `beyond_lag_reach()` abstention."* The
  **floor** is anchored at nominal on purpose, and **M6** records the degenerate case being hit and
  accepted: *"Production anchors the floor at nominal; bool lands on the decoy (nominal ≡ decoy) →
  headroom ≈ 0 → abstain, not veto."* Headroom is chosen-vs-floor; if the floor follows the chosen
  placement the two coincide and headroom is ≈ 0 by construction. **Anchoring at nominal is what
  makes headroom a measurement rather than a tautology.**

So 33/17's `informative: false` with `floor_source_pre`/`_post: "none"` is a *designed* abstention —
`beyond_lag_reach()` firing, the safety valve M5 added to stop real-codec false vetoes — not a probe
that failed. §3.2's reading ("uncorrelated at the lags tried, not uncorrelated") was right and
remains right.

**What survives is the reporting half — but less of it than first written.** The dump already
disambiguates: 33/17 carries `{"floor_source_pre":"none","floor_source_post":"none",
"informative":false}`, and `floor_source` exists precisely so an absent floor is not read as a
measured one (`gap_fingerprint/schema.rs:839-841`, pinned by
`floor_source_round_trips_and_disambiguates_an_absent_floor`). So "abstained" vs "measured and found
nothing" is **already answerable from a dump**.

Two things are still missing, and they are what the recommendation is now scoped to:

- **The abstention is unnamed.** `floor_source: "none"` says no floor was anchored; it does not say
  whether that was `beyond_lag_reach()` firing, no energetic window inside `max_walk_frames`, or a
  non-finite probe. Those are different events and one of them is a warning.
- **It never reaches production output.** The disambiguation is a fingerprint-schema field; the
  repair path's own reporting still surfaces the bare `informative: false`.

### 7.4 The recommendations

Ordered by confidence, with what each addresses. **Revised 2026-08-04** after the §7.1a source check:
former item 3 (price the extension) is downgraded from a change to an investigation, item 2 is
re-scoped to what is actually missing, and the `Apply` promotion — the largest production change in
this file — is lifted out of §6.7.5's prose and given a row.

| # | recommendation | addresses | risk |
|---|---|---|---|
| 1 | **Fill-level sanity check against the A shoulders.** RMS in dB over the assembled fill vs the border windows the seam gate already extracted. | 33/17's fill sits ~8 dB above the −69 dB bed it replaces (§3.3 point 2); §3.1a's eight patches raise level by 9–46 dB. **Strengthened 2026-08-04 by a graded ear result (§6.10.11):** three further patches, and the audible damage is **monotone in the substitution magnitude** — ≈65 dB → "noticeably poor" (12/8), ≈35 dB → "slightly louder, dirty" (38/4), ≈13 dB → "barely noticeable" (14/20). That is this check's own quantity predicting listener response across three gaps, which is the closest thing in this file to a calibration for its threshold. **Promoted to the top of the list 2026-08-04 by §6.10.12**, which supplies both a fourth confirming case and the first *prospective* use of the rule: 10/12's patch injects +11 to +35 dB into material already matching B to 0.1 dB, the ≈35 dB peak predicted "audible but mild", and the listener confirmed the patch sounds worse than A. More importantly, §6.10.12's five gaps are **correctly registered** — item 3 does not reach them — so this is now the *only* recommendation that addresses that class, and it does so without needing an opinion about whether the gap was real. Pearson is scale-invariant by construction, and there is **no level or loudness term anywhere in the fill path** (`patch_region.rs`, `gap_fill_fit.rs`), so no existing gate can see this. | Low. O(n) over `gap_frames + 2 × 250 ms` on buffers already in memory — no search, no second Pearson pass. Must compare against the **shoulders**, not the gap interior: on a true dropout the interior is silent and an interior comparison would flag every correct repair. |
| 2 | **Name the residual abstention, and surface it in production output.** Not "distinguish abstained from clean" — `floor_source` already does that in the dump (§7.3). Record *which* abstention fired (`beyond_lag_reach` vs no energetic window vs non-finite probe), and carry the distinction into the repair path's own reporting. | 33/17 and 19 of the 21 in §3.2 read `informative: false`; a reader outside the fingerprint schema still cannot tell an abstention from a clean bill of health. | Low — a reporting change, no decision moves. Does not touch M5's abstention, which stays. |
**Revised again 2026-08-04** after the 39-pair scan (§6.10): item 3's rate question is **answered and
the answer supports it**; item 4 is demoted from a classifier to a recall widener (§6.10.7a) and
reordered below 3.

| 3 | **Promote `DonorRegistrationMode::Apply`** — measure the donor window at the registered lag and abstain below `min_envelope_r` (§6.4, §6.6). | §6.2 item 1, the file's primary finding: the donor fraction is measured 80–410 ms off (§2.5). `Observe` shipped first; `Apply` is now the production default (`apply_donor_registration`, built 2026-08-04, §7.4a). | **Low–medium; the blocking question is answered.** §6.10 ran the scan-only route over 39 pairs / 829 gaps / 782 registrations and every number lands in favour: **67.8 % nonzero lag, systematic per pair** (23/39 pairs have a modal lag ≠ 0; residual scatter about own mode only 13.0 %), abstain **4.3 %**, and `Apply` moves **16 gaps = 2.05 %**. It touches **none of the 236 dropouts at the digital-zero rail** — every keep→drop flip is a gap where A sits 40–80 dB above the rail and B reads within 1 dB of A (§6.10.4). Net production effect over 39 pairs: **3 patches stop being applied**. The 5 drop→keep flips are abstentions, which cost a patch attempt, never a hole. The recommendation paired this with the head/tail exclusion of §6.10.3; **that exclusion is not implemented** (§7.4a). `--replay` still cannot read `GapScanJson` (§6.9.2), so 2.05 % is a reconstruction, not a production number. **The audio risk is discharged 2026-08-04 (§6.10.11) and it resolved in item 3's favour, more strongly than expected**: all three gaps (12/8, 14/20, 38/4) are periodic program material (drum beats, clock ticks, speech pauses) with no detectable hole, A and B matching bin for bin at the registered lag, and **all three current patches are audible degradations** — 12/8 "noticeably poor". `Apply` removes three real defects rather than three no-ops. **Scope bounded 2026-08-04 (§6.10.12):** `Apply` corrects *where* the donor is measured, so it reaches only the misregistered cases. The five measured residuals are registered correctly (WAV lag within 0.78 bins of the engine's) and still classify as dropouts, because the donor test asks "is B non-silent?" rather than "is B non-silent *where A is silent*?" — 10/12 has **4/9 silent blocks on both sides** and is still `repairable_dropout`. Promote `Apply` on its own merits; do not expect it to close that class. |
| 4 | **`interior_delta_db` as a recall widener — NOT as a classifier.** Forward gaps with A's interior at digital zero and a registered donor interior well above it to the repair path, and let the repair path arbitrate (§6.10.7a). **Demoted 2026-08-04**; the original form ("put the delta in the dropout test", i.e. trust it to classify) is **refuted** — see below. | §6.2 item 3, but the evidence that motivated it did not survive the corpus. §6.8.3's clean separation was a small-sample artifact: over 39 pairs the classes **overlap by 58 dB** (dropouts from −0.51, non-dropouts to +57.59) and no threshold separates them (§6.10.5). Of the 9 gaps it would rescue, **7 are head-of-media trim differences** between masters and **all 9 have `b_interior_db` ≤ −43 dB** (§6.10.6). The one that renders, 33/17, was **listened to and is inaudible** — patch indistinguishable from A and B (§6.10.7, which also records a conflict with §3.1a). The field is a ratio with no absolute anchor, and §6.10.8 shows no anchor is obtainable: §7.2 already dropped local-context floors at 0.8 dB separation, and the scan retains no whole-file level statistic to anchor against. What survives *positively* is the **opposite** direction — delta ≈ 0 on a labelled dropout is a reliable "no hole here" (§6.10.9) — and item 3 already catches 11 of those 17. | **Low risk, but zero measured benefit.** As a recall widener the failure mode is a wasted patch attempt, and 9 gaps over 39 pairs is negligible. **Still do not ship before 1**, and the reason is now measured rather than assumed: 33/17 is the only candidate that reached the repair path with brackets and the repair path **accepted** it (0 of 1 rejected, not 8 of 11) — because there is no level term anywhere in the fill path. Item 1 *is* the granularity this recommendation delegates to. Pair with §6.10.3's head/tail exclusion, which removes 7 of the 9 for free (**not implemented** — §7.4a). Benefit on this corpus is **zero**: every candidate's donor is ≤ −43 dB, so even a successful fill imports inaudible material. |

### 7.4a Items 1 and 3, as built (2026-08-04)

Both shipped on by default, each with its own off switch, and neither invents a threshold.

**Item 3 — `apply_donor_registration` (on).** `RepairConfig::apply_donor_registration` drives
`DonorRegistrationMode` at the single production construction site (`scan_gaps.rs`), so the enum's
`#[default] Observe` is untouched: a caller that asks for registration without saying what for still
cannot silently move a decision. `--no-apply-donor-registration` classifies at the nominal map.
Both sides of the split are pinned by unit tests over one class-flipping fixture (B is the same
program 500 ms late, so the nominal window reads content and the registered window reads B's own
hole).

Abstention under `Apply` is **not** a fallback to the nominal window: `peak_r < min_envelope_r`
(or too few bins) yields `NotEvaluated` / `donor_registration_unreliable`, which keeps the gap.
Re-measuring at the nominal map would re-use the window already known to be wrong
(`gap_equivalence.rs`).

**Not implemented — head/tail exclusion (§6.10.3).** The recommendation said to pair `Apply` with
excluding first/last gaps of a pair (all 31 clipped `bins == 20` registrations in the 39-pair scan
were head or tail; they abstain at roughly twice the mid-media rate). That exclusion did **not**
ship with `apply_donor_registration`. Cost of leaving it out: more abstentions on head/tail gaps →
more patch attempts, never holes (fail open). A bins-floor would be equivalent on this corpus but
worse as a rule; prefer an explicit head/tail check if it is added later.

*Unlooked-for corroboration from the fixtures.* The F-series energy fixtures fail under `Apply`, and
the reason is the finding restated in test code. They build B as A delayed by half a gap **including
A's dropout**, so at the registered lag the donor window lands on B's own hole — and they only ever
satisfied the equivalence gate because of `ensure_nominal_b_occupied`, a helper whose comment reads
"F1 shifts the true donor so the nominal span can be mostly silent; add low-level occupant energy
without changing true_fill." That helper is a nominal-window artefact, written well before this
file measured the same artefact on media. Those fixtures exercise signature search and fit, not the
gate, so `scan_gaps_for_fixture` now pins the nominal-map donor and says why.

**Item 1 — `measure_fill_level` (on).** `domain/fill_level.rs`: interleaved RMS of the written fill
in 100 ms bins, the **loudest** bin against the **louder** of the two 1 s A shoulders (100 ms
standoff from each gap edge). Emitted as `fill_level` on `GapPatchOutcome` (JSON: §FillLevelCheck in
`docs/json-output.md`).

Four decisions worth recording, because each is a place the measurement could have been made
useless:

- **Per-bin peak, not a whole-fill aggregate.** The calibration points are peaks: ≈65 dB → poor,
  ≈35 dB → dirty, ≈13 dB → barely noticeable, and 10/12's single +35 dB bin inside an otherwise
  +11–12 dB fill. An average over the fill hides exactly the bin that does the damage.
- **Interleaved, not a mono downmix** — the two differ by 3–8 dB on 5.1 (§6.10.12).
- **The reference is the louder shoulder**, so a fill only looks bad if it beats *both* sides of its
  own neighbourhood. Conservative in the direction that matters for anything that later reads this
  as a threshold.
- **Measured after the anchored-retry pass and before the splice**, on the final patches with the
  gain applied and A still pristine. A shoulder read after a neighbouring splice would describe the
  repair rather than the program it is judged against.

**No threshold ships.** The four ear results bracket it at 15–30 dB, which is a bracket and not a
calibration; a veto's false positive is an unrepaired hole, which is worse than what is being
measured. Next step is the corpus sweep of `peak_delta_db` over patched gaps, against which a
threshold can be argued rather than asserted.

**Sweep helper:** `scripts/measure-fill-level.ps1 -Manifest pairs.csv` — same pair manifest as
`scan-registration.ps1` / `measure-gap-fingerprints.ps1`. Write-mode repair with throwaway `--wav`
(deleted unless `-KeepWav`), `--format json` to `<label>.json`, stdout/stderr split so the report
stays parseable, and `fill-level-rollup.csv` sorted by `peak_delta_db` descending. `-RollupOnly`
rebuilds the CSV from existing reports. Preview / scan-only cannot produce this field.

**New investigation, opened 2026-08-04 (§6.10.12) — make the donor test conditional**

- **Ask "is B non-silent *where A is silent*?" instead of "is B non-silent?"** The two halves of the
  dropout definition are currently evaluated independently — A's level against its own noise floor,
  the donor's occupancy against the gap floor — so quiet periodic material satisfies both in both
  masters at once. 10/12 carries **4/9 silent blocks on each side** and is still `repairable_dropout`.
  The conditional form (count donor occupancy only over the blocks A reports silent, at the registered
  lag) is a small change to `derive_gap_equivalence` and needs no new input: both block vectors are
  already in hand, and §6.10.12 shows they agree bin-for-bin once the lag is right.
  **Not yet a recommendation, for two reasons.** The rate is unmeasured — the corpus-wide count of
  gaps whose A-silent and donor-silent block sets *coincide* has never been computed, and the answer
  decides whether this is a 5-gap curiosity or a systematic reclassification. And it moves gaps out of
  `repairable_dropout`, which is the **dangerous** direction (§6.10.4's framing, and the reason the
  band died): a conditional test that is too eager drops real dropouts. Measure the rate on the
  existing 39-pair scan JSON first — it needs no media and no re-dump, the same route §6.10 used.
  Item 1 catches the observed damage regardless and does not carry this risk, so it ships first
  either way.

**Downgraded to an investigation — no longer a recommendation**

- **Bound or price the boundary extension.** Both shapes are dead as proposed. The comparator
  **already** penalises boundary move at `2e-4`/frame and tie-breaks toward the smaller move; the
  grid does not run under the default profile at all; and on 33/17 the smaller extensions failed the
  waveform floor rather than losing on score, so a "prefer the smallest within noise" rule would have
  changed nothing (§7.1a). What remains is a question, not a fix: **which path placed 33/17's fill?**
  The rendered seams (0.998 / 0.973) appear in no bracket row, so the dump does not say. Instrument
  the selected candidate first; only then is there a target. If anything is tuned afterwards the
  likely site is the **acceptance floor**, not the comparator.

Dropped, and why, so they are not re-proposed:

- **Bound the write to the gap span** — dual-fit already returns exactly `gap_frames`, and the
  boundary grid is bounded at 500 ms by configuration (§7.1; note the caveat there — that bound is
  the grid's, and the anchor enumeration in the dump exceeds it).
- **Gate on `post_seam_global_r`** — on a real dropout the shift it detects is legitimate, so it
  cannot discriminate without already trusting the classification it is meant to inform. Keep it as
  corroborating evidence (§6.8.3a), not as a gate.
- **Widen or re-centre the residual probe** — the reach is a deliberate post-aligner budget and the
  nominal anchor is what makes headroom meaningful (§7.3).
- **Derive the achievable floor from the gap's local context** — 0.8 dB of separation, and it needs a
  new threshold (§7.2).
- **Give `interior_delta_db` an absolute floor on `b_interior_db`** ("B has real program here", not
  merely "B is louder than A") — the natural fix for §6.10.7's ratio problem, and it is not
  obtainable. Local context yields 0.8 dB of separation (§7.2, above); the scan retains no whole-file
  level statistic to anchor against (`scan_gaps.rs` keeps only silence spans and `scanned_end_secs`;
  `GapScanJson` carries no global level or loudness field); and a bare dBFS number is not portable
  across masters anyway. The only route is an integrated program level computed dynamically from a
  much larger sample of the video — a new scan-side statistic with its own cost, serialization and
  calibration, and §6.10.6 gives no evidence it would pay, since it would have to reject all nine
  candidates, which is the same as not having the rule (§6.10.8). Re-proposing the anchored variant
  means proposing the program-level statistic first.
- **`interior_delta_db` as a dropout *classifier*** — §6.8.3's separation is a 20-gap artifact; the
  populations overlap by 58 dB over 39 pairs (§6.10.5). The recall-widener form survives as item 4.
