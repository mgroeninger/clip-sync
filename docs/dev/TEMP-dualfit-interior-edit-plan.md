# TEMP — Dual-fit interior length edit: crossfaded jump at a quiet span

**Status:** draft 2026-10-02. One Change, behind a flag (default off).
§ 2 items 1, 2 and 4 built 2026-10-02 (uncommitted); item 3 deferred. § 3 measurement gate PASS 9/9 (2026-10-02). § 4 listen 2026-10-02: NOT FIXED by rule (new worse on 2 of 9; better on 3 of 5 blip gaps). 2nd sitting 2026-10-05: neither loss replicated (0 worse); (b)/(c) distributed edits refuted vs jump. 50/g7 blip = late gap start in A (separate defect). Next: decide.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement*, *Dual-fit
interior edit* row.

**Companions:**
- The defect was found and measured in
  [TEMP-dual-fit-registration-routing-plan.md](TEMP-dual-fit-registration-routing-plan.md)
  § 7, *The dual-fit blip is the interior length edit, not the seams*.
- That plan's step-kind routing bound (§ 6b: Adopt at 75 ms) is a
  cost/benefit crossover against this artifact. Its § 6c re-listen waits on
  this plan.
- Dual-fit mechanism: the `domain/dual_fit.rs` module doc.

**Media hygiene:** pair index + gap index only.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence
disagrees with this one, this one wins.

**Dual-fit's length edit becomes one crossfaded jump of `trim` frames, at the
quietest span that covers both of its sources. A cut jumps forward and a pad
jumps back, so a short bridge repeats real audio instead of holding one sample.**

Dual-fit fits each shoulder at its own lag, so the B bridge between the two
locks is `gap + step` frames long. A's hole is exactly `gap` frames, so `step`
frames of real programme must be removed or invented inside the fill. The step
is a timing discontinuity in the content (4–41 ms over 0.5–3.5 s in § 6b), not
clock drift, so no lag choice removes it. It can only be placed somewhere.

Today, `trim_at_lowest_energy_interior` places it two ways:

- **Bridge longer than gap:** it removes the quietest `trim`-frame interior
  window with a hard join.
- **Bridge shorter than gap:** it picks the quietest **single frame** and
  repeats that frame `pad` times. The result is a 29–35 ms near-DC flat line
  with hard edges, wherever one sample happens to be near zero. In § 6b this
  dropped the local level by 15–35 dB. It is the blip on 5 of the 6 ≥ 29 ms
  holds (routing plan § 7).

The new edit is one rule for both signs. Let `t = bridge − gap` (signed) and
`cf` = the edit crossfade in frames. The output plays the bridge up to `p`,
crossfades over `cf` frames into the bridge at `p + t`, and continues from
there:

```text
out[0 .. p]        = bridge[0 .. p]
out[p .. p+cf]     = fade_out · bridge[p .. p+cf]  +  fade_in · bridge[p+t .. p+t+cf]
out[p+cf .. gap]   = bridge[p+t+cf .. bridge_len]
```

Length is `p + cf + (bridge − p − t − cf) = gap` for either sign of `t`.

- `t > 0` (long bridge): skips `t` frames forward. This is today's cut, plus a
  crossfade.
- `t < 0` (short bridge): jumps `|t|` frames back, so `bridge[p+t .. p]` plays
  twice. That is a repeat of real (quiet) content, not a held sample.

**Position.** `p` minimizes the mean-square energy, across all channels, of the
span the edit touches: `[min(p, p+t), max(p, p+t) + cf)`. This is the same
search the cut branch does today, widened by `cf`, and now used for both signs.
The edge guard is unchanged (`seam_window_frames` from each end), so the
re-validation gate's border windows never see the edit. Where the bridge is too
short to keep the guard on both sides, the fallback is today's: the edit lands
at the guard-clamped position. It is never a decline, because a decline would
change which gaps dual-fit rescues, and that is the routing plan's question.

**Crossfade.** Equal-power (sin/cos). The two sources sit `|t|` frames apart,
which is many periods of almost any content, so they are uncorrelated and a
linear fade would dip by 3 dB. `cf` is `crossfade_ms` (default 10 ms), the same
knob as the seam crossfade, capped so that `|t| + cf` fits the interior room.

**Scope.** Only the `step_real` branch of `try_dual_fit` edits a bridge. The
`!step_real` branch reads exactly `gap` frames off the pre lock and has no
edit, so it is untouched. `--seam-extend-fill`'s seam margin is cut from the B
shoulders, not from the fill, so it is unaffected.

**Flag.** `--dual-fit-crossfade-edit` (TOML `repair.dual_fit_crossfade_edit`,
`PatchAudioRequest.dual_fit_crossfade_edit`), default off. Off is today's
output, byte for byte. It is a flag only so one build can render old and new
arms for § 4. After § 4 passes, the default flips and the legacy branch is
deleted (old-vs-new differentials are migration scaffolding, not permanent
guards).

## 1. Evidence

From routing plan § 7 (2026-10-02, 19 gaps, waveforms):
- Every dual-fit seam continues at A's own lag to within 0.01 ms. The seams
  are not the blip.
- **Holds** (`t < 0`): 5/g8, 33/g14, 28/g13, 29/g20, 54/g10, 50/g7, and a
  3.9 ms one on 12/g2.
  - Blip notes on the dual-fit arm: t01, t03 and t08 (all losses), plus 29/g20
    in § 6. 50/g7 was *same*, with both arms blipped.
  - The hold position matches the note on t08 ("~5 s", hold at 4.79 s).
- **Cuts** (`t > 0`): 11 gaps, 2 losses.
  - 32/g13 (t02, "blip at pre"; the cut is 0.28 s after the pre seam).
  - 54/g9 (t16, no note).
  - Several cuts carry the write region's largest second difference, e.g.
    28/g18 at ×7.1 the context p99.9.
- Earlier: 13/g16's 37 ms forward jump, at −24 dBFS inside continuous speech,
  was ear-clean (BACKLOG Hole 1 row, 2026-09-22). So a cut need not be
  audible. That row's "jump placement at low energy stays unmotivated" is now
  motivated by the holds.

Measurement script: scratchpad `blip.py`, outside the repo. § 3 needs a copy
under `gap-files/`, and if it is reused, a home under `scripts/measure/`.

## 2. Change checklist

1. **Domain.** Add `jump_edit_to_gap(bridge, ch, gap_frames, edge_guard,
   cf_frames)` beside `trim_at_lowest_energy_interior` in
   `domain/gap_fill_fit.rs`, plus its span-energy position search. Unit tests,
   on synthetic audio:
   - the output is exactly `gap` frames for `t` > 0, < 0 and = 0;
   - no run of ≥ 16 identical frames appears that is not in the bridge;
   - a quiet span planted in the interior is chosen for both signs;
   - the output equals the bridge outside `[p, p+cf)` (prefix) and equals the
     shifted bridge after it (suffix);
   - equal-power: a constant-RMS uncorrelated source shows no level dip > 1 dB
     across the join;
   - a guard-clamped short bridge still returns `gap` frames;
   - `cf = 0` degenerates to a hard jump (a cut that matches today's on
     `t > 0`).
2. **Plumbing.** Add `DualFitParams.edit_crossfade_frames: Option<usize>`
   (`None` = legacy), and branch on it in `try_dual_fit`'s `step_real`
   assembly. The diagnostic `splice_dualfit` goes through the same function,
   so it cannot drift. Then wire the flag through CLI → TOML → `RepairConfig`
   → `PatchAudioRequest` → the params builder, and add it to the flag table in
   [gap-fill-modes.md](../gap-fill-modes.md).
3. **Dump fields (additive).** In `dual_fit_placement`, add `edit_frame`
   (fill-relative `p`), `edit_kind` (`skip` / `repeat` / `none`), and
   `edit_span_db` (the chosen span's level), recorded on both arms. This gives
   later listens a field to stratify by.
   *Deferred 2026-10-02:* the fingerprint probe (`splice_dualfit_at`) reads B
   mono only and never assembles a fill, so these fields need interleaved B in
   the probe plus a second edit run — not additive. `DualFitResult.edit`
   carries the placement, and `try_dual_fit` logs it at debug
   (`dual_fit: crossfaded interior edit`: `edit_frame`, `edit_kind`,
   `edit_span_db`). § 3 measures from the WAVs, so it does not need the dump.
4. **PR gate.** `check-repair-test-manifest.ps1`, `test-tier.ps1 -Tier pr`,
   fmt, clippy. Goldens must not move with the flag off.

## 3. Measurement gate (before any listening)

Re-render **only the dual-fit arm**, with the flag on, for the 6 ≥ 29 ms holds
(5/g8, 33/g14, 28/g13, 29/g20, 54/g10, 50/g7) and 3 cuts (32/g13, 54/g9,
28/g18). Use the same build flags as § 6b, plus `--dual-fit-crossfade-edit`.
Render one at a time (each run holds a whole pair in memory, ~14 GB). Re-run
the blip measurement. Pass:

- no held-sample run ≥ 16 frames inside any write;
- the level step across the edit is within ±6 dB (20 ms windows), the range
  the clean seams show;
- the lag track still reads pre lag → post lag, with the seams unchanged:
  every new fill equals the old one outside the old and new edit spans;
- the edit position is reported per channel as well as on the mixed level.
  That makes visible any gap where the mixed level picks a span that is loud
  on one channel, e.g. the center on speech.

A failure stops the plan here. Fix it before anyone listens.

**Result (2026-10-02): PASS, 9/9.** The build was the § 2 working tree on top of
248403a, with flags `--seam-extend-fill --dual-fit-crossfade-edit` and the
§ 6b floors. The renders ran one at a time. Each was compared against its
§ 6b dual arm; `a_surround` was byte-equal in both arms on every gap.

| Gap | Old arm | New edit (log) | Span | Fills differ | Clear of write start / end | Step (mix) | Per-channel step |
|-----|---------|----------------|------|--------------|----------------------------|-----------|------------------|
| 5/g8 | 28.9 ms hold | repeat −28.9 ms | −43.4 dB | 252 ms | 416 / 1908 ms | +0.1 dB | — |
| 33/g14 | 34.0 ms hold | repeat −34.0 ms | −46.6 dB | 226 ms | 2006 / 332 ms | +0.1 dB | — |
| 28/g13 | 34.6 ms hold | repeat −34.6 ms | −39.5 dB | 1024 ms | 814 / 763 ms | −0.8 dB | ≤ 1.2 dB |
| 29/g20 | 34.7 ms hold | repeat −34.7 ms | −59.4 dB | 35 ms | 1978 / 538 ms | +0.1 dB | ≤ 0.6 dB |
| 54/g10 | 30.6 ms hold | repeat −30.5 ms | −50.6 dB | 82 ms | 1879 / 1193 ms | +0.1 dB | ≤ 0.2 dB |
| 50/g7 | 32.1 ms hold | repeat −32.1 ms | −48.0 dB | 331 ms | 375 / 1868 ms | −0.3 dB | ≤ 0.5 dB |
| 32/g13 | 8.0 ms cut | skip +8.0 ms | −35.0 dB | 10 ms | 286 / 336 ms | +1.1 dB | ≤ 3.3 dB |
| 54/g9 | 36.2 ms cut | skip +36.2 ms | −35.9 dB | 10 ms | 266 / 344 ms | +0.2 dB | ≤ 1.3 dB |
| 28/g18 | 40.7 ms cut | skip +40.7 ms | −25.5 dB | 25 ms | 275 / 260 ms | −5.2 dB | ≤ 7.3 dB (LFE) |

- **No held-sample runs** of 16 frames or more remain in any new write. All 6
  holds of 29 ms or more are gone.
- **Seams are unchanged.** In every gap the two fills differ only between the
  old edit and the new one, and that span is at least 260 ms from either end
  of the write. This exceeds the 250 ms seam window.
- **Where the edit lands:**
  - On 29/g20 and the 3 cuts, the new edit lands at or within one crossfade
    of the old position, so the fills differ by little more than the
    crossfade.
  - On the other holds, the quietest span lies elsewhere in the bridge. Each
    fill plays the same audio offset by |t| between the two positions.
- **Weak point: short-write skips.** The 3 cuts (32/g13, 54/g9, 28/g18) are
  short writes of 0.62–0.63 s, so after the guards they had only about
  100–120 ms of placement room.
  - None found a quiet span. Each span sits at or above the write mean on at
    least one channel; 28/g18's is at −25.5 dB, on a bass pulse.
  - 28/g18's −5.2 dB mixed step (−7.3 dB on LFE) is the pulse's own decay,
    not the edit. Old and new 20 ms envelopes match everywhere except the two
    bins at the edit, where the new one is smoother (−24.0 / −25.2 / −26.1
    versus old −24.0 / −23.9 / −26.8 dB). The old hard cut stepped −2.9 dB
    mixed and −4.7 dB on LFE.
  - The crossfade removes the click, but these 3 still drop 8–41 ms of
    programme mid-content. If § 4 hears anything, expect it here; the
    parked shoulder-widening in § 5 is the lever for it.

## 4. Blind listen: does the fix remove the blip? (pre-registered here)

**Trials.**
- Old dual-fit vs new dual-fit on the 9 gaps from § 3.
- 2 **null** trials: the same new-dual file on both letters, on 2 gaps drawn
  from the 9. These measure how often a difference is heard where none exists.

**Build.** a/b assignment is balanced (5 on one letter, 4 on the other, among
the 9 real trials) and drawn with `secrets`, and the order is shuffled. The
key is held outside the listening folder. There is one sitting.

**Scoring.** a better / b better / same, with optional notes on where and what.

**Rule.**
- **k** = number of null trials not scored *same*.
- **Fixed** if, on the 5 blip-noted gaps (5/g8, 33/g14, 28/g13, 29/g20,
  32/g13), new is better on ≥ 3, and new is worse on ≤ 1 of the 9 real trials.
  Require k ≤ 1. With k = 2 the pass is inconclusive and is repeated once.
- **Not fixed** if new is worse on ≥ 2 real trials, or better on ≤ 1 of the
  5 blip-noted gaps.
- Otherwise **inconclusive**: record it, and decide on § 0's open points
  (crossfade length, per-channel position) before re-listening.

On **Fixed**: flip the default, delete the legacy branch and
`trim_at_lowest_energy_interior`'s pad arm, then hand off to routing plan
§ 6c.

**Result (2026-10-02): NOT FIXED under the rule. New was worse on 2 real
trials.**

The listen was one sitting of 11 trials. Real trials put new on a for 4 and
on b for 5.

| Trial | Gap | Kind | Score | Unblinded | Note |
|-------|-----|------|-------|-----------|------|
| t01 | 28/g18 | real, skip | same | same | |
| t02 | 54/g9 | real, skip | same | same | |
| t03 | 32/g13 | real, skip, blip | same | same | |
| t04 | 54/g10 | **null** | a? | difference heard (tentative) | |
| t05 | 54/g10 | real, repeat | a? | new better (tentative) | |
| t06 | 5/g8 | real, repeat, blip | b | **new better** | "a has a blip" (old) |
| t07 | 33/g14 | real, repeat, blip | b | **new better** | "a has a blip" (old) |
| t08 | 29/g20 | real, repeat, blip | b | **old better** | "a has a blip" (new) |
| t09 | 28/g13 | real, repeat, blip | b | **new better** | "a has a blip" (old) |
| t10 | 50/g7 | real, repeat | a | **old better** | |
| t11 | 33/g14 | **null** | same | — | |

How the rule applies:

- On the 5 blip-noted gaps, new was better on 3, worse on 1 and the same on
  1. That meets the ≥ 3 bar.
- New was worse on 2 of the 9 real trials. That trips **Not fixed**, which
  takes precedence.
- k = 1: one null was heard as different, on t04, and only tentatively.
  This is within the allowed k ≤ 1.
- None of the 3 skip-edit gaps was heard at all, including 32/g13, which was
  blip-noted.

Follow-up measurements, made after unblinding from the WAVs:

- **No onset in the repeated audio.** On all 6 repeats, the doubled
  material's loudest 2 ms window is only 1.6–5.4 dB over its mean. The two
  losses (29/g20 +4.0, 50/g7 +1.6) sit inside the winners' range
  (2.1–3.8).
- **29/g20 differs only over 35 ms.** Both arms make the same length edit at
  the same spot, about 15 ms before an onset that rises from −60 to
  −37 dB.
  - Old holds a sample that is near zero (−77 dB mixed) through the pause.
  - New repeats the pause's own −60 dB floor.
  - Neither arm has a sample step over the bridge's own −52 dB maximum.
  - The difference is a measured 35 ms near-silence versus floor. Nothing
    in the WAVs accounts for the reported blip.
- **50/g7 carries no note.** The new edit sits 375 ms into the write.
  Old's hold sits 300 ms later. The two arms play the same audio 32 ms apart
  between those points.
- **The span search picks quiet spans, not pauses.** On 5/g8, 28/g13 and
  50/g7 the chosen span is only 2–6 dB below its ±200 ms context. On 29/g20
  it is 20 dB below.
- **A spectral blip detector separates the arms.** It compares the 10 ms
  log spectra before and after each instant, against the unedited arm at the
  same instant.
  - Every old hold stands out: +3.4 to +18.8, at the 90th to 100th
    percentile of the write.
  - No new edit does: +0.2 or less, at or below the write's median.
  - This holds on 29/g20 and 50/g7 too.

**Second blind sitting (2026-10-05, same folder and key, not
pre-registered).** This was an unplanned re-listen of the § 4 files.

| Gap | Sitting 1 | Sitting 2 |
|-----|-----------|-----------|
| 5/g8 | new | new; blip noted on old, at its hold |
| 28/g13 | new | new; blip noted "after 4 s", on old (hold at 4.83 s) |
| 33/g14 | new | same |
| 54/g10 | new (tentative) | same |
| **29/g20** | **old** | same |
| **50/g7** | **old** | **new** |
| 32/g13, 54/g9, 28/g18 | same | same |
| nulls (2) | 1 heard (tentative) | 0 heard |

- Neither § 4 loss replicated: 29/g20 went to same, and 50/g7 flipped to new.
- Both blip notes in sitting 2 fall on old holds.
- Sitting 2 alone gives new better on 2 of 5 blip gaps, 0 worse, k = 0. Under
  the § 4 rule that reads **inconclusive**, not Fixed, because Fixed needs
  ≥ 3 of 5.
- Over both sittings (18 real trials): new better 6, old better 2, same 10.
  3 of 4 nulls were scored same.

**The 50/g7 blip is in A, not in any edit.** It was heard at about 2.8 s
("2–3 s") in both arms, in three sittings. The patched clip equals raw A
until the write starts at 2.990 s.

- Raw A's dropout begins at **2.974 s**, a −20 dB fall in 2 ms on every
  channel, to −51 to −65 dB.
- It then decays to digital silence (−104 dB) by 3.004 s.
- The gap boundary sits about 26 ms after the dropout begins, so 16 ms of
  partial dropout survives before the pre seam's crossfade. That 2.978 s
  transition is the largest spectral event in the clip, at the 100th
  percentile.
- This is a gap-start registration defect, separate from this plan. It is
  probably also the cause of sitting 1's "old better" on 50/g7.

**Distributed-edit prototypes (2026-10-05): rejected; the jump stays.**
The prototypes were built offline, for the 6 short-bridge gaps only.

- **Bridge recovery.** The unedited bridge was recovered by deleting the
  held run from the old arm. The recovered bridge equals the jump arm
  exactly, outside the jump's crossfade.
- **Where they edit.** Each prototype replaced only the interior: the
  bridge from 260 ms inside each end of the write.
- **(b) Band-limited resample.** A windowed-sinc resample with 100 ms
  raised-cosine rate ramps. It is −21 to −32 cents flat between the ramps.
- **(c) WSOLA.** 40 ms frames, ±10 ms alignment, with the end frames fixed.
  In effect it copies the bridge until the drift exceeds the tolerance. It
  then makes a few waveform-aligned 10 ms repeats, not a fine spread.

The blind sitting used 12 real trials against the jump plus 2 nulls.

| Comparison | Result | Notes |
|------------|--------|-------|
| (b) vs jump | jump better 3/6 (33/g14, 5/g8, 29/g20), same 3/6 | "music distortion" on the resample arm twice |
| (c) vs jump | same 6/6 | — |
| nulls | 2/2 same | — |

- The resample's pitch shift is audible as distortion, so it is rejected.
- WSOLA buys nothing over the single jump at more complexity, so it is
  rejected.
- Together with the § 5 non-goal: the distributed variants are now
  **measured and refuted** for this edit, not merely parked.

## 5. Non-goals

- **Widening the write into the A shoulders** to find a quieter edit spot.
  This is parked. Reopen it if routing plan § 6c still hears a dual-fit blip
  after this fix.
- **Time-stretch / WSOLA** of the whole bridge. A plain resample of 1–3 %
  shifts pitch by 17–50 cents. A pitch-preserving stretch is this plan's edit
  repeated many times.
- **Changing the edge guard**, the `!step_real` branch, or dual-fit's
  admission. Which gaps dual-fit rescues is the routing plan's business.
- **The routing bound itself.** It belongs to routing plan § 5 / § 6c.

## 6. Close

On Fixed:
1. Archive this plan.
2. Add a settled.md § A/B row recording the mechanism and the listen.
3. Delete the BACKLOG row.
4. Update the routing plan § 7 "Next" and its § 6c status.
