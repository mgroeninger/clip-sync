# TEMP — Post-splice check: collapse introduced silence on skip gaps

**Status:** draft, 2026-08-27. One Change. **Startable the day**
[TEMP-early-shoulder-locks-collapse-skip-plan.md](TEMP-early-shoulder-locks-collapse-skip-plan.md)
ships capture + `visit_shoulders_abut` (this Change fills that visit; it does
not re-search and does not invent a second skip walk).

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Post-splice
introduced-silence collapse*.

**Companions:** skip identity, capture, and the post-splice visit hook
[TEMP-early-shoulder-locks-collapse-skip-plan.md](TEMP-early-shoulder-locks-collapse-skip-plan.md)
§ 0 / § 4; Hole 2 [gap-vocabulary.md](gap-vocabulary.md) § *Known holes*;
pipeline splice [pipeline.md](../pipeline.md) § 4 step 5; mux duration
[cli-output.md](../cli-output.md) § Timeline / duration warnings.

**Media hygiene:** pair index + gap index only.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence
disagrees with this one, this one wins.

**Fills complete first. The visit always checks. Drain is off by default:
confirm, warn, leave A’s length alone. `--collapse-introduced-silence`
is the opt-in that shortens PCM. Video is not cut. Mux is the path we
already have; it sees a shorter buffer only when the flag is on.**

The skip-gate already owns the loop. After splice it calls
`visit_shoulders_abut` once per captured abut skip, with A still on the
original clock. Today that visit is record-only. This Change **is** the
body of that visit: confirm the hole is introduced silence, then **warn**.
Cut only when the flag is on.

```text
characterize → keep / skip (shoulders_abut captured)
execute fills
anchored retry
splice_into_a                    // length-preserving; original A frame clock
for skip in captured shoulders_abut:
    visit_shoulders_abut         // check; warn; drain only if flag on
mux                              // current path; stdin = A PCM (shortened iff flag)
```

Fills replace silence with equal-length donor audio. Their destinations are
frame indices on the **scan / decode clock**. Shifting that clock before splice
would invalidate every later patch. So splice every keep, **then** visit the
captured skips. Drain, when requested, is last-to-first on that original
clock so earlier indices stay valid.

The hole is **audio-only**. Picture was not padded when the silence was
inserted, so A’s video already sits on the program clock that B’s audio
matches; A’s audio after the hole is late. Draining the silence pulls that
tail left and **re-aligns sound to picture**. Default does not do that —
it tells the operator the hole is there. `--mux` is unchanged: copy A’s
video (`-c:v copy`), replacement audio on stdin, `-shortest`. `--wav` is
the same PCM with no container.

`--repair-preview` never splices, so it never visits, never confirms, and
never warns on this axis (capture list only). Write mode, `--patch-only`,
and `--gap-listen` run the visit.

One sentence: **keep/skip on the original clock; splice every fill; then in
the skip-gate’s post-splice visit, check each captured abut skip for
introduced silence; warn; drain only under `--collapse-introduced-silence`;
hand whatever buffer that leaves to the mux we already have.**

---

## 1. What “introduced silence” is

The motivating case is 31/g7: A’s **audio** holds ~1.12 s of silence between
two program edges that are **adjacent in B**. A has the content plus extra
dead time; B has the content. Picture was not lengthened with that silence,
so the hole is an audio insertion against an unaltered video clock. Any
donor fill duplicates program (Hole 2). The honest repair is not a fill —
it is **delete the dead time**, which puts later audio back on the picture.

The skip-gate’s `shoulders_abut` row *is* that geometry: live locks,
`bridge_frames <= 0`, SilenceSplice cell. Vocab still refuses a causal story
for why the hole exists. Cutting A’s silence makes A’s remaining PCM match
B’s continuity **and** A’s own video. That is the intended action.

The visit list is the captured abut skips, not every skip reason. Other
skips are not candidates:

| Skip | Why it is not in the visit |
|------|----------------------------|
| `ProgramQuiet` | Shared pause. Cutting would make A shorter than B. |
| `CorrelationBelowThreshold` / Decorrelated | B occupied, no lock. Joining A’s edges is an unmeasured jump. |
| `ResidualHeadroomExceeded` | Seams passed and B ≠ A. |
| `BoundaryAlignmentFailed` / Unfillable | No placement, or geometry failed. |
| `NotApplied` | Gate approved a fill; splice failed. Still a bug, not a cut. |

Do not infer collapse from `bridge_ms ≈ 0` on other reasons — that arithmetic
is settled-dead ([settled.md](settled.md) § A). Admission is the skip-gate
capture. This Change does not walk `outcomes` looking for a second class.

---

## 2. The check (inside the visit, after splice, no re-search)

Admission is already done (the skip is on the captured list). The check is a
**post-splice PCM confirm** of the join the cut would produce, not a second
dual-fit.

On each visit, with A already containing every keep:

1. Read the lock payload already on the capture (`pre_lag_ms`, `post_lag_ms`,
   `bridge_frames`, `gap_frames`) and the refined A `[start, end)` frames.
   Do not call `seam_local_peak` again. The skip-gate’s in-range confirm
   stays; a destination that has become out of range is a no-op (same as
   record-only today).
2. Build the would-be join from **post-splice A**: a seam-local collar
   ending at `gap_start` concatenated to a same-length collar starting at
   `gap_end` (template length = existing 250 ms dual-fit seam window).
3. Pearson that concat against B at the abutting lock (`b_pre_seam` /
   `b_post_seam`, which already coincide). Same floor as a splice seam
   (`SPLICE_MIN_PEAK_R` / `min_fill_correlation` — whichever the skip-gate
   already treated as live). Neighbouring fills may sit in a collar; that
   is the repaired timeline, and that is what we want to score.
4. **Pass** → stamp the confirm on the row and **warn** (payload below).
   Drain `[gap_start, gap_end)` only when `collapse_introduced_silence` is
   on (see § 3). **Fail** → leave the skip, stamp
   `collapse_declined: join_dead`. Do not cut on admission alone; the
   confirm is the check. Do not drain on confirm alone; the flag is the
   cut.

The silent interior itself is not re-scanned. Scan already required it;
splice did not write it.

---

## 3. In-memory concatenate (flag on only)

Default: A PCM is not mutated. The visit’s pass list is a warning, not a
cut.

`--collapse-introduced-silence` (`repair.collapse_introduced_silence`,
default **false**; `--no-collapse-introduced-silence` overrides a TOML
true, same pair shape as `--normalize`): queue every confirm-pass, then
drain last-to-first (the visit collects; a second pass mutates so a later
skip in list order does not see a shifted clock):

```text
[ 0 , start )  +  optional join crossfade  +  [ end , len )
```

Drop `[start, end)` entirely. The remainder of the PCM shifts left by
`end - start` frames. Reuse `crossfade_ms` (default 10 ms) at the join —
the same helper family as `apply_seam_crossfade`, but A-to-A (no donor).
Negative `bridge_frames` (31/g7 ≈ −48) is a lock overlap of ~1 ms; it does
not change the A cut, which is still the full silent interior.

After those cuts, `a_pcm` is shorter. `pcm_secs` used by A3 / mux preflight
is this new length. Collapse should **close** audio-vs-container skew
(audio was the long side). If A3/B1 still fire, that is leftover real
skew, same as today.

Fills already in the buffer move with the tail. No patch is rewritten.
No later fill is re-placed.

---

## 4. Mux

No new mux path. `PatchAudio` already returns `MultiChannelPcm`;
`RepairVideos` already hands it to `mux_video_with_replaced_audio`. Default
PCM is still original length (silence left in). Flag on: that object is
just shorter. `--wav` / `--mux` do not care which.

B1’s 5 s preflight still compares PCM to **original** video duration.
31/g7’s ~1.12 s cut (flag on) lands inside that slack; a pair whose
collapsed cuts sum past 5 s is an honest mux refuse (use `--wav`), not a
reason to start cutting video.

---

## 5. Report identity

Default (flag off): the gap stays `Skipped { shoulders_abut }`. The visit
adds confirm numbers on that skip (or beside it on the capture list) so
the warning is grep-able without a new status:

```json
{
  "shoulders_abut": {
    "pre_lag_ms": -38.0,
    "post_lag_ms": -1160.3,
    "bridge_frames": -48,
    "gap_frames": 53702,
    "join_r": 0.91,
    "would_collapse": true
  }
}
```

Stdout warning (write / `--patch-only` / `--gap-listen`, once per
confirm-pass, same family as A1–A3):

`Warning: gap #8 looks like introduced silence (1.119 s, join r=0.91) — audio after this hole is late vs picture; pass --collapse-introduced-silence to drain it`

`join_dead` declines do **not** warn; they stay a quiet skip.

Flag on: restamp those rows `Collapsed { … }` (not `Skipped`, not
`Patched`). `skipped_count` does not include them. New
`collapsed_count` on `PatchSummary`. Human line:

`collapsed introduced silence (cut 1.119 s / 53702 frames, join r=0.91)`

`--gap-listen`: default still has the silent window (skip-gate review
WAV). Flag on: that window is gone; v1 writes no `_a_patched.wav` for a
collapsed gap. Do not pretend a fill exists.

---

## 6. 31/g7 under this plan

After the skip-gate: `shoulders_abut`, captured, no −977 keep, A still
contains the 1.12 s hole through splice. After this Change (defaults): the visit concat-confirms, **warns**, A PCM
still contains the 1.12 s hole, mux is today’s argv. With
`--collapse-introduced-silence`: the hole is drained, PCM is ~1.12 s
shorter, mux is still today’s argv, audio after the hole sits on picture.
The stutter cannot exist under the flag — there is no fill and no residual
silence.

In-reach dual-fit fillable silence-splices never reach this visit (they
were patched). Program-quiet skips are never captured.

---

## 7. Non-goals

- Re-searching shoulders. Locks live on the capture.
- A second walk of skip outcomes. The skip-gate’s captured list is the set.
- Cutting during characterize / before splice / inside `splice_into_a`.
- Stretching or padding audio to keep the original (too-long) audio
  duration.
- Cutting or re-encoding video. Picture was not the padded side.
- Default-on drain. Vocab still has no causal story; n=1. Warn first.
- Pair-sequence / step-boundary classifier ([settled.md](settled.md) § D).
- `--gap-listen` join excerpts.
- Chunked PCM (full-file buffer is already resident).

---

## 8. Change shape

1. Domain helper `introduced_silence_join`: collars from post-splice A,
   Pearson vs B at the stored abut lock. `None` only when a collar is
   out of range.
2. Fill in `visit_shoulders_abut`: in-range + confirm pass → record
   (`gap_index`, start, end, `join_r`) and emit the warning. Do not
   mutate A inside the per-skip call.
3. After the visit loop, **if the flag is on**, drain last-to-first with
   A-to-A crossfade; restamp those outcomes `Collapsed { … }`;
   `PatchSummary.collapsed_count`. Flag off: skip status stays
   `shoulders_abut` with `would_collapse: true`. Capture list on
   `patch.shoulders_abut` unchanged (what was visited).
4. CLI + TOML: `collapse_introduced_silence` default false;
   `--collapse-introduced-silence` / `--no-collapse-introduced-silence`.
   No mux argv change. B1 still uses original video duration.
5. JSON + human warning + (flag on) collapsed status. Hole 2 detection
   sentence: skip-gate finds it; this Change warns; the flag closes A’s
   audio timeline.

---

## 9. Checklist

- [ ] `introduced_silence_join` unit tests: abut fixture passes; a dead
      A-A join declines; out-of-range collar is `None` (no panic).
- [ ] Splice-then-visit on the skip-gate’s write-mode fixture (one abut
      skip + one real fill), **flag off**: fills land at original indices,
      visit runs after splice, A length unchanged, warning fired,
      `would_collapse: true`, skip remains `shoulders_abut`.
- [ ] Same fixture **flag on**: skip span gone, PCM length decreases by
      exactly `gap_frames`, bytes on either side of the join match the
      pre-collapse shoulders, status `collapsed`.
- [ ] Last-to-first two-cut fixture (flag on): both interiors gone, no
      overlap corruption.
- [ ] `--repair-preview` does not shorten PCM, does not emit this
      warning; capture list still present.
- [ ] `--patch-only` flag off: warn, length unchanged. Flag on: shorten
      in memory; report `collapsed`.
- [ ] `--wav` / mux: flag off byte-identical length to today’s splice;
      flag on shorter PCM, same mux argv, `-c:v copy` unchanged.
- [ ] `Collapsed` + warning: serde, formatters, JSON + repair-guide +
      cli-output warning table.
- [ ] 31/g7 production, flag off: `shoulders_abut` + warning, hole still
      in the file. Flag on: `collapsed` (licensed listen WAVs already on
      disk, 2026-08-18 post-sweep stem). Depends on skip-gate already
      making it a skip.

---

## 10. Exit

- The skip-gate’s post-splice visit is where the check runs. Only
  captured `shoulders_abut` that pass the concat-confirm warn (and, with
  the flag, are cut).
- Default: A length unchanged; warning names the gap, the cut size, and
  `--collapse-introduced-silence`.
- Flag on: 31/g7’s silent interior is absent from the output PCM;
  duration drops by the cut; mux is the current path.
- Fills are never reindexed. Drain runs only on the post-splice buffer,
  after every keep is in.
- `--repair-preview` / `--wav` / `--patch-only` behave as § 0.
- In-reach dual-fit fills and program-quiet skips are unchanged.
