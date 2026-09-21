# TEMP — Rescue branch: anchor the envelope on the capture it correlates

**Status:** § 5 Change **implemented** 2026-09-19 (unreleased). Deciding
measurement (§ 4) run and reproduced the same day. **Exits 1–6 are met** —
the targeted re-fingerprint of 2026-09-20 closed 4 and 5, the full-corpus
pass of 2026-09-21 closed 3 (2032 of 2046 shoulders bit-identical) and found
one unpredicted downstream move in `splice` on 31/g8 (§ 7). Only **exit 7**,
the ear pass, is outstanding.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* — the
31/g8 Narrow-capture item.

**Companions:** the refuted ladder and the deciding measurement that sent
31/g8 here
[archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md)
§ *Deciding measurements* 2; nominal-first ordering and the lazy wide
capture it introduced
[archive/TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md);
map coordinates and the `D` shift
[archive/TEMP-lag-decision-map-coordinates-plan.md](archive/TEMP-lag-decision-map-coordinates-plan.md)
§ Change point 1; durable field behaviour
[gap-fingerprint.md](gap-fingerprint.md) § *`lag_decision`*; the refused
default flip [settled.md](settled.md) § B.

**Media hygiene:** pair index + gap index only.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later
sentence disagrees with this one, this one wins.

**A correlation is anchored on the template it actually correlates. When the
rescue branch swaps in the lazily-built wide A capture, it must anchor on
*that* capture's border displacement, not on the shared narrow capture's.
Nothing else changes: the reported `border_displacement_ms`, the nominal
sweep, and every shoulder that never enters the rescue branch stay
bit-identical.**

One sentence: **the envelope search is given a wide template and a narrow
template's zero; give it its own zero.**

---

## 1. The defect

### 1.1 Where the two captures come from

`lag_decision_geometry` computes two sizes
(`crates/clip-sync-repair/src/domain/lag_decision.rs`):

- `narrow_need` = `lag_window_secs` + `lag_max_lag_ms` = 1.0 s + 600 ms = **1.6 s**
- `wide_need` = `lag_envelope_window_secs` + `lag_envelope_max_lag_ms` = 2.0 s + 2000 ms = **4.0 s**

Under the shipped `BorderCapture::Narrow` the shared capture is cut to
`narrow_need`, so `shared_covers_wide` is **false** at the defaults, and
`lag_pair` supplies `pre_wide` / `post_wide`: a lazy callback that re-cuts
the border at `wide_need`. `rescue_shoulder` calls it only for a shoulder
dead at nominal, substitutes the result as `a_border`, and hands that to
`envelope_shoulder`.

This path is live in **production**, not only in dumps —
`ShoulderLagKnobs::production` also pins `BorderCapture::Narrow`.

### 1.2 The trim is peak-relative, so the two captures have different edges

`border_templates_for_gap` walks silence off the gap-facing edge and then
applies `trim_low_energy_suffix` / `_prefix`, whose threshold is **12 % of
that buffer's own peak**. A 4.0 s capture reaching further from the gap can
contain louder material than a 1.6 s one, which raises the threshold and
trims further inward. `border_edge_displacement_frames` calls the distance
from the raw gap edge to the surviving template edge `D`.

So `D` is a function of capture length. On 31/g8's pre shoulder that
function is violent: `D_narrow` = **13.0 ms**, `D_wide` = **1983.8 ms**
(archived plan, deciding measurement 2). The wide template is not "the
narrow template plus context" — it is a *different two seconds of audio*,
sitting 2–4 s before the gap.

### 1.3 The anchor is never re-derived

`lag_pair` computes the haystack anchor once, from the shared capture:

```text
pre_side.anchor_frame = start_frame − D_narrow
post rescue anchor    = start_frame + gap_frames + D_narrow(post)
```

`rescue_shoulder` then builds `envelope_side` as
`LagSideSweep { a_border: wide_border, ..side }` — new template, **same
anchor**. `envelope_shoulder`'s pre branch aligns the template's *last*
sample with `anchor_frame` at lag 0 and searches
`[anchor − w − max_lag, anchor + max_lag]`.

Write `Δ` for the true local A→B offset (what `frac_lag_ms` reports; `D`
cancels out of it by design). The template's gap-facing edge is the A frame
`gap − D_wide`; the anchor asserts it is the A frame `gap − D_narrow`. The
peak therefore lands at

```text
L_measured = Δ − (D_wide − D_narrow)
```

and the searched B span is displaced by the same `D_wide − D_narrow`. On
31/g8 pre that is **1971 ms**: the true Δ ≈ −1146 ms would have to appear at
L ≈ −3117 ms, which is outside the ±2000 ms envelope reach. It cannot be
found at any `min_envelope_r`, at any ladder rung, and by any statistic
computed on the curve. What the search returns instead is the best thing
inside a window pointed 1971 ms away from the template: **+800 ms, r 0.816**,
which fails the fine retry and is honestly `declined`, keeping +550.8 / 0.152.

Under `BorderCapture::Wide` the shared capture *is* the wide one, so
`D_narrow = D_wide`, the mismatch is zero, and the same shoulder keeps
−1146.1 / r 0.974 — agreeing with its own post (−1137.7) and with the
independent WAV sweep. **Wide is not better-reaching here; it is merely
self-consistent.** That reframes the archived reading: the arms do not
disagree about the curve, they disagree about where zero is.

### 1.4 The two-arm gap is 2000 ms, which is the tell

> **[2026-09-19] Settled by § 4's Result: not the same curve.** The fixture
> shows the narrow arm's peak is whatever its misdirected window happens to
> hold, not the true peak relabelled. 31/g8's exact 2000 ms arm gap is a
> content coincidence. The reasoning below is kept because the prediction it
> made — a mismatch of `D_wide − D_narrow` — is the part that held.

Narrow's peak (+800) and Wide's (−1200) differ by exactly 2000 ms, against a
predicted mismatch of 1971 ms and a 100 ms envelope bin. That is one bin's
worth of agreement. It is consistent with the two arms seeing the same
curve under two labellings, and it is also consistent with two different
peaks that happen to sit 2000 ms apart. **§ 4 is the measurement that tells
those apart**, and the Change does not depend on which is true — either way
the anchor is wrong.

---

## 2. Why this is not "flip the default"

[settled.md](settled.md) § B refuses `--border-capture wide` as the default
because it **re-baselines `border_displacement_ms` corpus-wide**: the shared
capture sizes every template, so 505 shoulders moved 161 displacements by
more than 1 ms.

This Change touches nothing outside the rescue branch:

- the shared capture, and therefore the reported `border_displacement_ms`,
  is unchanged;
- the nominal fine sweep is unchanged;
- the fine **retry** still uses the narrow template at the narrow anchor —
  only its *center* moves, and it moves to where the envelope meant;
- `EnvelopeRescue::NotConsulted` shoulders never call the wide callback, so
  they cannot move at all.

`--border-capture wide` stays exactly what it is today: a diagnostic arm.
After this Change it should also become a *control* arm — § 7 asks the two
arms to agree on the rescue population, which today they do not.

### 2.1 Which path this is (traced 2026-09-19)

Shared domain code, two callers, and **only one of them is production** —
where it is report-only:

| | Fingerprint | Production repair |
|---|---|---|
| Caller of the wide path | `measure/lag.rs` per-channel closures (`lag_pair` direct) | `detect_injected_time` → `shoulder_lags_at_b_mapped` (`patch_audio/injected_time.rs`) |
| Is the lazy wide path live? | yes | **yes** — `ShoulderLagKnobs::production` pins `Narrow`, so `shared_covers_wide` is false and the closures are supplied |
| Envelope enabled? | yes | yes — `lag_envelope_max_lag_ms` defaults to 2000, `lag_envelope_window_secs` to 2.0 |
| What consumes the result | every `lag_decision` field in the dump, and the verdict | `pre.frac_lag_ms` / `post.frac_lag_ms` only, fed to `find_injected_time_runs` |
| Effect of a changed value | reclassification (§ 4: `decorrelated` → `timing_offset`) | the injected-time **warning** and the `injected_time` key in the patch summary |

So § 5 edits production-path code, but it moves **no patch placement**.
`InjectedTimeReport` is stamped onto `PatchSummary` and printed as a progress
warning; nothing reads it back. The patch path's own envelope use — seam
locks (`domain/seam_locks.rs`, via `characterize.rs`) — calls
`envelope_shoulder` directly, never `rescue_shoulder`, builds no wide
capture, and is untouched.

`border_capture` itself is fingerprint-only ("Probe-only — production repair
never reads it", `infrastructure/config.rs`), so the Wide control arm of § 7
stays a dump-side instrument.

Practical consequence for § 7: a production-side regression check is cheap
(the injected-time warning must not appear or disappear on a pair whose
rescue population did not move), but the real exit remains the corpus
re-fingerprint.

---

## 3. Population (measured 2026-09-19)

Full-tier corpus dump `2026-09-15-corpus`, 52 pair directories, **2046
shoulders**:

| `envelope_rescue` | n |
|---|---|
| `not_consulted` | 2032 |
| `rescued` | 8 |
| `declined` | 6 |

**Only 14 shoulders corpus-wide can move.** All 14 carry
`envelope_window_ms = 2000.0`, which exceeds the 1.6 s narrow capture — an
independent confirmation that every one of them is correlating the lazy wide
template, i.e. is exposed to the mismatch.

The 14, by pair/gap index:

| Shoulder | verdict | `D_narrow` | `envelope_lag_ms` | `envelope_peak_r` | `frac_lag_ms` | `peak_r` |
|---|---|---|---|---|---|---|
| 25/g11 pre | declined | — | −1300 | 0.662 | 343.0 | 0.209 |
| 31/g7 post | rescued | 0.1 | −1200 | 0.944 | −1160.3 | 0.782 |
| **31/g8 pre** | **declined** | **13.0** | **+800** | **0.816** | **550.8** | **0.152** |
| 31/g8 post | rescued | 0.7 | −1100 | 0.943 | −1137.7 | 0.939 |
| 31/g9 pre | rescued | 52.2 | −1100 | 0.839 | −1142.6 | 0.965 |
| 31/g9 post | rescued | 0.2 | −1100 | 0.854 | −1146.7 | 0.957 |
| 31/g10 pre | rescued | 14.5 | −1200 | 0.968 | −1151.6 | 0.990 |
| 31/g10 post | rescued | 3.3 | −1100 | 0.958 | −1155.4 | 0.995 |
| 31/g11 pre | rescued | 13.7 | −1100 | 0.763 | −1158.4 | 0.984 |
| 31/g11 post | rescued | 1.2 | −1200 | 0.777 | −1158.4 | 0.972 |
| 32/g0 post | declined | 103.5 | +100 | 0.789 | 149.1 | 0.432 |
| 32/g1 post | declined | 1.1 | +100 | 0.670 | 147.7 | 0.409 |
| 49/g37 post | declined | 311.3 | 0 | 0.999 | −3.4 | 0.426 |
| 49/g38 pre | declined | — | −300 | 0.998 | −4.0 | 0.128 |

`D_wide` is **not** in the dump — that is the one number § 5 has to add.
But the eight `rescued` rows estimate it for free: a successful rescue means
the retry found Δ, so `frac_lag_ms − envelope_lag_ms ≈ −(D_wide − D_narrow)`
up to the 100 ms bin. Every rescued row reads between −6 and −58 ms, so on
all eight the two captures trim within a bin of each other and the mismatch
is invisible. **31/g8 pre is the corpus's only shoulder where the two
captures disagree by more than a bin** — n = 1, and a defect whose blast
radius is 14 shoulders.

That smallness cuts both ways and the plan says so out loud: this is a
correctness fix on a path that is nearly always degenerate, not a rescue
rate improvement. Do not size it as the latter.

---

## 4. Deciding measurement (before any Change)

**A synthetic shoulder where the two captures trim differently must
reproduce the mismatch, and `BorderCapture::Wide` must not.** If it does
not reproduce, § 1 is wrong and this plan stops.

Extend the fixture shape already used by
`border_capture_wide_matches_narrow_when_the_envelope_is_off`
(`crates/clip-sync-repair/src/application/gap_fingerprint/measure/lag.rs`).
That fixture cannot expose the defect because its pre shoulder is uniform:
both captures trim to 0 and `D_wide = D_narrow`. Add one thing — a **loud
passage 2–4 s before the gap and a quiet (but not silent) 2 s immediately
before it** — chosen so the 1.6 s capture's peak keeps its whole buffer
while the 4.0 s capture's peak puts the near 2 s under the 12 % floor.
Give the shoulder a true lag outside the ±600 ms fine sweep so the rescue
branch is entered.

Assertions, current binary:

1. `D_wide − D_narrow` is large (the fixture's whole point) — assert on the
   two `border_edge_displacement_frames` readings directly, not on a dump
   field that does not exist yet.
2. `BorderCapture::Narrow`: `envelope_lag_ms ≈ Δ − (D_wide − D_narrow)`, or
   the shoulder declines. **This assertion must fail after the Change** —
   it is the defect, written down.
3. `BorderCapture::Wide`: rescues at Δ.

Record the numbers here as a Result before touching `rescue_shoulder`.

### Result 2026-09-19 — reproduced, § 1 stands

`rescue_wide_capture_is_anchored_in_the_narrow_captures_frame`
(`measure/lag.rs`, beside the border-capture test). A 3 s shaped-loud
passage, then a flat 2 s at amplitude 0.03 running to the gap; true lag
−1160 ms; everything else at defaults.

| | `D` | `envelope_window_ms` | `envelope_lag_ms` | `envelope_peak_r` | edge-pinned | rescue | `frac_lag_ms` | `peak_r` | verdict |
|---|---|---|---|---|---|---|---|---|---|
| `Wide` | 2000.0 | 2000.0 | −1200 | 0.9997 | false | **rescued** | **−1160.0** | 1.000 | `timing_offset` |
| `Narrow` | 0.0 | 2000.0 | +2000 | **0.000** | **true** | declined | −419.9 | 0.019 | `decorrelated` |

The fixture's premise holds exactly: `D_narrow` = **0.0 ms**, `D_wide` =
**2000.0 ms**, from one capture-length change and nothing else.

Both arms correlate the *same* 2 s wide template — `envelope_window_ms` is
2000.0 on both, above the 1.6 s narrow capture. They differ only in where
zero is, and that alone is worth a correct rescue at r 1.000 against a
`decorrelated` verdict at r 0.019.

The `Narrow` row is the mismatch made visible in a single record: it reports
`border_displacement_ms` **0.0** and `envelope_window_ms` **2000.0** — a
template that cannot have come from the capture whose displacement is being
quoted. The envelope then runs off its own window (`+2000`, r 0.000,
`edge_pinned`), because the true peak is at −3160 ms and the search cannot
see it. On real content (31/g8 pre) the same displacement returns a
*confident* wrong peak instead, +800 / r 0.816; empty curve and false peak
are the same failure at different content.

This also answers § 1.4 the other way round, and the plan is corrected here
rather than above: the two arms are **not** the same curve relabelled. The
wide arm's peak is the true one and the narrow arm's is whatever its
misdirected window contains. 31/g8's 2000 ms arm gap is a coincidence of
that gap's content, not an identity.

Exit 1's "the Narrow assertion flips" is now a concrete thing: the test
asserts non-rescue with a `PRE-CHANGE:` message that names the flip.

---

## 5. Change

1. **The wide callback returns its displacement with its border.** `pre_wide`
   / `post_wide` become `Fn() -> Option<WideBorder>` where `WideBorder` is
   `{ samples: Vec<f64>, displacement_frames: usize }`. Both call sites
   (`shoulder_lags_at_b_mapped` in `domain/lag_decision.rs`, and the
   per-channel closures in `measure/lag.rs`) already build the `wide_need`
   spec — they call `border_edge_displacement_frames` on it as well, with
   the template length they just produced, and return the matching side's
   value. The two sites must not diverge; if that is awkward, hoist one
   helper rather than copying the call.
2. **`rescue_shoulder` re-anchors.** When a wide border is used, rebuild the
   envelope side with the anchor recomputed from `D_wide`:
   pre `gap_start − D_wide`, post `gap_end + D_wide`. The callers pass the
   anchor already reduced by `D_narrow`, so `rescue_shoulder` needs the raw
   gap edge, not a delta applied blind — add it to `LagSideSweep` rather
   than reconstructing it by adding `border_displacement_frames` back, so
   the arithmetic is stated once and a `None` displacement cannot silently
   read as zero.
3. **The fine retry is untouched.** With the anchor corrected the envelope's
   peak is already in the map frame, so `envelope_recenter_frames` feeds
   `side.anchor_frame + gross_lag_shift + env_frames` exactly as today. No
   translation term. Assert this in a test, because a reader's first instinct
   (and this plan's first draft) is that one is needed.
4. **The lead-in guard moves with the anchor.** `envelope_shoulder` refuses
   when `anchor_frame < w + max_lag`. A pre anchor moved *earlier* by
   `D_wide` makes that refusal slightly likelier near the head of B. Keep the
   guard; do not special-case it. Its two-refusals-one-observable problem is
   a separate, already-recorded item.
5. **Record `D_wide` in the dump.** Stamp the displacement of the capture the
   envelope actually correlated beside `envelope_window_ms`, which exists for
   precisely this reason — the recipe cannot express either adjustment.
   Proposed name: `envelope_border_displacement_ms`. `None` on the
   `shared_covers_wide` path and on `NotConsulted`, where it would be a
   duplicate of `border_displacement_ms`.

### Landed 2026-09-19 — all five, as written

`WideBorder { samples, displacement_frames }` and `WideBorderCut::side`, the
one helper both call sites now cut through (point 1's "hoist rather than
copy" was taken; the per-channel site supplies its own cut of the spec, the
spec and the displacement are derived once). `LagSideSweep` gained
`gap_edge_frame`, the **raw** edge, so `rescue_shoulder` re-derives the
anchor from `D_wide` instead of adding `D_narrow` back (point 2). The fine
retry is byte-unchanged, and the no-translation-term claim is now an
assertion, not a comment (point 3): the fixture requires coarse and fine to
agree to within one 100 ms bin. The lead-in guard was not touched (point 4).
`envelope_border_displacement_ms` is stamped, documented on `LagSummary`, in
[gap-fingerprint.md](gap-fingerprint.md) § *`lag_decision`* with its corpus
break, and the `lag_decision` contract's `placement` string now says the
envelope is anchored on its own capture (point 5).

The § 4 fixture, renamed `rescue_wide_capture_is_anchored_in_its_own_frame`,
**flipped exactly as predicted**: `Narrow` now rescues at −1160.0 ms,
`envelope_border_displacement_ms` 2000.0, `border_displacement_ms` still 0.0,
verdict `timing_offset`. `Wide` did not move. PR gate green (fmt, clippy,
manifest 41/41, 805 lib tests, `test-tier.ps1 -Tier pr` all 35 suites).

---

## 6. Contract implications

`envelope_lag_ms` today is documented as a coarse lag but is, on the lazy
wide path, a lag in a frame no other field shares. After the Change it is in
the same frame as `frac_lag_ms`, so `frac_lag_ms − envelope_lag_ms` becomes
the honest "how far did the fine sweep move from the coarse proposal"
quantity that § 3 could only approximate. Update the `lag_decision` contract
string and [gap-fingerprint.md](gap-fingerprint.md) § *`lag_decision`* in the
same commit — this is a **field-meaning** change on 14 corpus shoulders, and
it silently invalidates any pre-Change arithmetic on that difference.

---

## 7. Exit

1. § 4's fixture exists and its Narrow assertion **flips** — mismatch before,
   Δ after. Wide is unchanged throughout.
2. `border_capture_wide_matches_narrow_when_the_envelope_is_off` still passes
   unmodified, both contracts.
3. Corpus re-fingerprint (`--features calibration,he-aac`, one pair at a
   time; protocol in [gap-fingerprint.md](gap-fingerprint.md)) against
   `2026-09-15-corpus`: **no shoulder with `envelope_rescue: not_consulted`
   moves in any field.** That is 2032 of 2046 shoulders bit-identical, and it
   is the property § 2 is claiming.
4. 31/g8 pre reads Δ ≈ **−1150 ms** at `envelope_peak_r` ≈ 0.82, rescues, and
   the shoulder's verdict moves `decorrelated` → `timing_offset`, agreeing
   with its own post (−1137.7) and the WAV sweep (−1146.1). If it rescues to
   anything else, stop and write a Result — the mechanism is then only
   partly right.
5. The other 13 rescue-branch shoulders are inspected one by one and each
   change (or non-change) is explained by its own `D_wide − D_narrow`. § 3
   predicts all eight `rescued` rows stay put; a rescued row that moves is a
   finding, not a pass.

   **Refined 2026-09-20, from reading the landed code:** "stays put" is not
   "bit-identical" on those eight. Their `D` difference is −6…−58 ms, under
   the 100 ms bin — but a coarse peak whose corrected lag straddles a bin
   boundary can land one bin over, which moves `envelope_lag_ms` by 100 ms
   and with it the fine retry's *center*. The fine sweep is ±600 ms, so the
   same true peak is still inside it and `frac_lag_ms` / `peak_r` should not
   move; what can legitimately move is `envelope_lag_ms` (one bin) and
   `lag0_r`'s **presence**, which depends on whether the retry window still
   contains lag 0. A moved `frac_lag_ms` on those eight is still a finding.
6. Narrow and Wide now agree on 31/g8 pre. A standing diff of the two arms
   over the rescue population is the cheap regression probe afterwards.
7. Ear-pass 31/g8 under the new placement before calling the *defect* closed.
   Exit 4 closes the measurement; only ears close the patch.

### Measured 2026-09-20 — targeted re-fingerprint: exits 4 and 5 **met**

All 14 rescue-branch shoulders live in 9 gaps across 4 pairs, so the whole
population re-dumps under `--fingerprint-gap` in 23 min rather than hours
(25/g11; 31/g7–g11; 32/g0–g1; 49/g37–g38 — 0-based). Run:
`gap-files/2026-09-20-rescue-anchor`, `--features he-aac,ac3,calibration`,
diffed field-by-field against `2026-09-15-corpus`.

**Exit 4 — 31/g8 pre, exactly as predicted.**

| field | pre-Change | post-Change |
| --- | --- | --- |
| `envelope_lag_ms` | 800.0 | −1200.0 |
| `envelope_peak_r` | 0.816 | 0.821 |
| `envelope_rescue` | `declined` | `rescued` |
| `frac_lag_ms` | 550.8 | **−1146.1** |
| `peak_r` | 0.152 | 0.994 |
| `verdict` | `decorrelated` | `timing_offset` |

`envelope_border_displacement_ms` = 1983.8 against `border_displacement_ms`
= 13.0: **D_wide − D_narrow = 1970.75 ms**, the out-of-frame term § 2 named.
The shoulder now agrees with its own post (−1137.7) and the WAV sweep
(−1146.1), and `lag0_r` dropped out — the retry window no longer spans lag 0,
which exit 5's refinement already allows.

**Exit 5 — the other 13, each explained by its own D difference.** The new
field makes this mechanical: 12 of 14 shoulders have
`D_wide − D_narrow` under 7 ms, i.e. **inside one 100 ms bin**, and none of
them moved `frac_lag_ms`, `peak_r` or `verdict`. Two exceptions, both
one-sided and both the fix working:

- **31/g9 post** (Δ = 6.4 ms) moved `envelope_lag_ms` −1100 → −1200 and
  nothing else. This is precisely the bin-straddle the 2026-09-20 refinement
  predicted: sub-bin correction, one bin over, fine retry re-centred, same
  fine peak.
- **49/g38 pre** (Δ = **305.8 ms**) moved `envelope_lag_ms` −300 → **0.0**,
  cancelling the displacement to the millisecond. A second, independent
  confirmation of the mechanism on a different pair. It stays `declined`
  (`frac_lag_ms` −4.0 unchanged), so no decision moved.

No `rescued` row moved its `frac_lag_ms`. The remaining changes are
float-noise redraws of `envelope_peak_r` (largest 0.943 → 0.929 on 31/g8
post, Δ 4.5 ms) — the coarse curve is re-cut, so its peak height is not
expected bit-stable.

**Exit 3, partial.** The four `not_consulted` shoulders inside this run
(25/g11 post, 31/g7 pre, 32/g1 pre, 49/g37 pre) are bit-identical in every
field. The claim over all 2032 still needs the full-corpus pass.

### Measured 2026-09-21 — full corpus: exit 3 **met**, and one downstream find

`gap-files/2026-09-20-corpus`, all 52 pair directories, 1063 `full` + 24
`summary` gaps, ~9.0 h (the `2026-09-15-corpus` reference took 9.8 h). Clean
tree at `7062431` (`build.dirty: false`; the reference was dirty).

**Exit 3 — met, exactly.** 2046 shoulders, same census as the reference.
**2032 `not_consulted`, and 0 moved in any field.** The 14 that moved are
precisely the rescue-branch population § 3 predicted — no more, no fewer, and
no shoulder changed *which* branch it took except the one that was supposed
to (31/g8 pre, `declined` → `rescued`).

Both targeted findings reproduced bit-for-bit in the unnarrowed run: I
diffed every gap file the two runs share (pairs 25/31/32/49, 10 gaps),
excluding only `source` provenance — **0 differing fields.** So
`--fingerprint-gap` narrowing does not perturb the measurement, and the
23-minute targeted run is a sound stand-in for the 9-hour one on this axis.

Whole-file path census against the reference, all 52 pairs (file sets
identical, `scan_recipe` / `gap_count` / entry counts identical):

| path | gaps | expected? |
| --- | --- | --- |
| `lag_decision._contract.placement` | 1063 | yes — the reworded string |
| `source.build.{commit,dirty}` | 1063 | yes — new binary, clean tree |
| `lag_decision.*.envelope_border_displacement_ms` | 14 | yes — the new field, on exactly the rescue population |
| `lag_decision.*.envelope_{peak_r,lag_ms,rescue}` + the 31/g8 pre fine fields | ≤14 | yes — § 7's 2026-09-20 rows |
| **`splice.*` and `donor_interior_aligned`** | **2** | **not predicted — see below** |

**Downstream find: the Change moves `splice`, which the plan did not say.**
Two gaps, both in pair 31, and both follow from the corrected lag rather than
from any new code path:

- **31/g8** — `splice.pre_peak_r` 0.152 → **0.994**, `pre_peak_z` 5.32 →
  13.74, and `splice.step_ms` **−1688.4 → 8.4**. The −1688 ms "step" was an
  artifact of the garbage pre lag: with pre at +550.8 and post at −1137.7 the
  two shoulders disagreed by 1688 ms. Corrected, they agree to **8.4 ms**,
  which is the *shared far offset* reading, not a step. `donor_interior_aligned`
  also appears where it was `null` — the aligned bridge is now measurable, and
  it reads `rms_db` −37.3, `silence_fraction` 0.0, `continuous: true`: the
  donor is genuinely continuous across the span a fill would take.
- **31/g9** — `splice.post_peak_z` 42.05 → 41.53 (`post_peak_r` moves in the
  13th decimal). The one-bin `envelope_lag_ms` move re-centred the retry
  window, so the z-normalisation pool is drawn over a shifted sweep. Expected
  once the bin move is; no decision rides on it.

**No gap's `outcome` changed anywhere in the corpus**, so the production
patch/skip verdict is untouched — consistent with § 2.1's finding that
production reads `lag_decision` only through the report-only injected-time
warning.

This *strengthens* the `shared_far_offset` classification 31/g8 already
carried (see [settled.md](settled.md), the `collapsed_bridge` entry): the
pair's step was being read off a shoulder that had no valid lag.

**Still open:** exit 7, the ear pass. `2026-09-20-corpus` replaces
`2026-09-15-corpus` as the comparison baseline from here — `envelope_lag_ms`
and `_contract.placement` both changed meaning, so the older dump is no
longer field-comparable.

---

## 8. Non-goals

- **Do not flip `--border-capture`.** [settled.md](settled.md) § B, and § 2
  above. If this Change makes the arms agree on the rescue population, that
  is a reason to keep Narrow, not a reason to revisit the default.
- **Do not touch `min_envelope_r = 0.70`, the 2000 ms reach, or the 12 % trim
  threshold.** All three are calibration questions; this is a coordinate bug.
  The trim constant in particular is shared with seam splicing and is not
  this plan's to move.
- **Do not revive the adaptive widen ladder.** Refuted, trigger empty
  ([archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md)
  deciding measurement 1).
- **Do not make the envelope build its own capture.** The lazy wide cut is a
  deliberate cost decision from nominal-first; this Change keeps it and fixes
  the frame it is read in.
- **Do not size this as a rescue-rate improvement.** § 3: 14 shoulders,
  8 of them already correct.

---

## 9. Alternatives considered

- **Trim the wide capture with the narrow capture's threshold.** Makes
  `D_wide = D_narrow` by construction and needs no plumbing. Rejected: it
  would hand the envelope a template whose gap-facing 2 s is the quiet
  material the 12 % rule exists to exclude, and it makes one buffer's trim
  depend on another buffer's peak — a worse version of the coupling that
  caused this.
- **Correct the returned lag by `D_wide − D_narrow` instead of the anchor.**
  Same arithmetic for the retry center, but the *searched B span* stays
  displaced, so a true peak outside the shifted window — 31/g8's, exactly —
  is still unreachable. Rejected: it fixes the label, not the search.
- **Give the shared capture the wide cut only when the envelope is on.**
  That is `BorderCapture::Wide`. Refused as a default in § B.
