# TEMP — Far-unique walk, then interpolate the hole

> **Archived 2026-09-14, unstarted — latched, not refuted.**
> [settled.md](../settled.md) § C. Dump-only map-source probe: do far unique
> 1 s phrases interpolate to the throat lag on locked throats? A yes would
> license a later ear TEMP (fill Decorrelated occupied skips at `b_pred`).
> The later-TEMP customer was 54/`#13`; that gap is a 66.6 s B stall, not a
> locatable lag on a locally-correct map, and every far stride still sits on
> the same wrong `+2.06 s` map. The remaining-skip population is the same
> step-shaped set constellation already measured (pairs 31, 50; now 54).
> **No code was written.** Reopen only if a remaining-skip is Decorrelated,
> occupied, and the true offset is inside ±600 ms of nominal `b_mapped`. Do
> not reopen to chase 50/g10 ([TEMP-disjoint-offseam-lock-plan.md](../TEMP-disjoint-offseam-lock-plan.md))
> or 31/g7–g8 (injected-time). 54/`#13` path:
> [TEMP-b-stall-warning-plan.md](../TEMP-b-stall-warning-plan.md) then
> stall-segmented map.

**Status:** draft, 2026-09-13; re-scoped 2026-09-14 (review: the throat
re-score cannot be a hit inside the fine band — § 0.1). Experiment, not a
Change. Fingerprint path only. Does **not** touch `domain/seam_locks.rs`,
keep-path search, dual-fit extract, or `fill_offset_mode`.

**Tracked as:** [settled.md](../settled.md) § C.

**Companions (do not merge):**

- [TEMP-disjoint-offseam-lock-plan.md](../TEMP-disjoint-offseam-lock-plan.md)
  — 1 s Pearson **on A disjoint from the production seam span**, vs an
  on-seam control. Latched ([settled.md](../settled.md) § C) on 50/g10.
  **Independent of this plan.** That plan's § 5 carry-back (confirm at
  the seam, on a 50/g10 lock) is a different question and is not opened
  by this TEMP. This plan shares its walk geometry only; it does not
  unlatch 50/g10 and does not change that probe's window / start /
  dual-fit-agreement rules.
- [TEMP-seam-anchor-relocation-plan.md](TEMP-seam-anchor-relocation-plan.md)
  — **do not re-run.** 10 s envelope starting 250 ms from the seam,
  take-first `r ≥ 0.35`, keep only lags within 2 ms of dual-fit. Refuted
  as a test of off-seam lock ([settled.md](../settled.md) § A). This plan
  reuses none of that funnel.
- [TEMP-anchor-seam-plan.md](TEMP-anchor-seam-plan.md) —
  energy/bool peaks inside ±3 s **choose a cut**. Shipped. Not a
  unique-phrase walk and not an interpolated B map.
- Dual-fit grow-post / grow-pre — longer template **glued to the join**.
  Post shipped; grow-pre refused ([settled.md](../settled.md) § A).
- Pair `fill_offset_mode = interpolated` / `anchored-retry` — clip-level
  start/end chromaprint, or *other gaps'* successful patches. Different
  anchors. Do not retarget those knobs.

**Media hygiene:** pair index + gap index only.

---

## 0. What this tests (normative)

**Where the throat itself locks, does a lag interpolated between two
self-unique, uniquely-locked 1 s A slices several seconds off the hole
predict the throat's own lag to seam scale?**

That is the whole plan. Later sections only implement it. If a later
sentence disagrees with this one, this one wins.

This is a test of **far phrases as a local map source**. A yes licenses a
*later* TEMP to ask whether a map-trusted fill at the interpolated `b_pred`
is acceptable **by ear** on gaps whose throat cannot lock. It does not
patch anything and does not claim the throat becomes recoverable.

“Walk seconds away until A is unique” is two predicates on the **same**
1 s slice, both required:

1. **Self-unique on A** — the slice's auto-lag curve (A vs A) has no
   strong rival outside a ±30 ms exclusion around lag 0. Periodic /
   bed-only audio is not a phrase.
2. **Unique A↔B lock** — the same slice vs B, Pearson in the dual-fit
   band (±600 ms around that slice's nominal `b_mapped`), is recoverable,
   prominent over its own lag curve, **and** stands out more than A's own
   repeats do (§ 2 rule 5).

“Interpolate” means: take the pre-side lock at its window centre
`(t_pre, L_pre)` and the post-side lock at `(t_post, L_post)`, form
`L(t)` linear in A time, evaluate it at the hole's start and end, and
compare against the throat `lag_decision` `frac_lag_ms` on the matching
shoulder.

### 0.1 Why the throat re-score is not a hit (normative)

The throat `lag_decision` already sweeps a 1.0 s window over ±600 ms
(`LAG_DECISION_WINDOW_SECS`, `LAG_DECISION_MAX_LAG_MS`) with the same
collar templates. The far walk searches the same ±600 ms band and rejects
edge-pinned locks, so every `L(t)` lies inside a band the throat has
already scored. Re-scoring the throat at `b_pred`:

- **narrowly** reads a value already on the existing curve (≤ the
  throat's `peak_r`) — it cannot become recoverable;
- **with ±600 ms reach** can only find something new *outside* the
  original band, which is the refused fine-window widening in disguise.

So the throat read at `b_pred` is a **reported column** (§ 4.3,
exact-lag reach only), never a pass condition. A Decorrelated throat
stays Decorrelated under this probe by construction; fixture 4b (§ 5)
pins that.

This plan does **not** test:

- Widening `lag_max_lag_ms` or the envelope (refused as the primary
  fix; already ran on the motivating gap and declined).
- A 10 s envelope, take-first `r ≥ 0.35`, or dual-fit ±2 ms agreement
  as pass/fail (settled § A).
- A closer start or longer window than the disjoint-offseam geometry
  (settled § C: do not reopen 50/g10 that way).
- Whether a fill at `b_pred` sounds right. That is the later TEMP, and
  only if § 7 says the map prediction is trustworthy.
- Carrying a lock into `seam_locks.rs`.

**Motivating case (reported, not the exit).** Pair 54, display `#13` /
`g012`: 0.60 s occupied donor, structure-floor skip (`pre=0.18 post=0.32
min=0.55`), dual-fit seams 0.23/0.23 `gate_pass=false`, both
`lag_decision` verdicts `decorrelated` (`peak_r` ≈ 0.10/0.11), envelope
rescue declined. Ten other holes on the same pair patch on the `+2.06 s`
map. § 1a records what the existing dump already settles about it.

**Not the customer.** 50/g10 is still
[TEMP-disjoint-offseam-lock-plan.md](../TEMP-disjoint-offseam-lock-plan.md).
If it appears in this dump, record it; do not treat it as a § 0 hit or
miss. Pair 31 map-step gaps (g7–g8) are injected-time, not missing
phrases.

---

## 1. Population

**Primary (the § 0 customer) — map-prediction set.** Gaps where both
throat `lag_decision` shoulders are **locked**: `peak_r ≥ 0.5`, not
`edge_pinned`, `envelope_rescue` ∈ {`not_consulted`, `rescued`}. Patched
or skipped does not matter; the throat supplies the reference lag.

**Reported — far-unique candidates.** Gaps where:

1. Production disposition is `skipped` (or fingerprint `outcome.tier =
   skip` with `dual_fit_rescue: false`).
2. Donor interior is **occupied** (`donor_interior_aligned.continuous`,
   not program-quiet).
3. Both throat `lag_decision` shoulders are `decorrelated` **or** dual-fit
   `gate_pass` is false with both `*_seam_r < 0.35`.
4. The skip is not a reasoned non-placement: not `shoulders_abut` /
   `bridge_frames ≤ 0`, not `not_fillable`, not
   `already_matches_reference`.

These are dumped so the later ear TEMP has `b_pred` and the far-lock
evidence ready. They do not decide § 7.

### 1a. Pre-check on the existing dump (done 2026-09-14, no code)

Before any implementation: can the motivating gap's throat lock anywhere
the fine sweep reaches? Read `gaps[0].lag_decision` on pair 54 `g012` in
the existing fingerprint run (fields only — the dump holds no curve):

| Shoulder | `peak_r` | `frac_lag_ms` | `edge_pinned` | `envelope_lag_ms` / `envelope_peak_r` | `envelope_rescue` |
|----------|---------|---------------|---------------|---------------------------------------|-------------------|
| pre | 0.102 | −175 | false | −800 / 0.712 | declined |
| post | 0.109 | +536 | false | +600 / 0.530 | declined |

Reading:

- **Both peaks are interior** (`edge_pinned: false`), so the ±600 ms
  maximum is not climbing into an edge. Every in-band lag is ≤ 0.11.
- **Pre** envelope cleared 0.70 and was not pinned, so the fine retry at
  `b_mapped − 800 ms` **ran** and was non-recoverable (`declined` after a
  proposed centre). Pre is below 0.5 over roughly [−1400, +600] ms.
- **Post** envelope was 0.53 < 0.70, so no retry ran; post is measured
  only over ±600 ms.

Consequence: no `L(t)` this walk can produce (inside ±600 ms) makes
54/`#13`'s throat recoverable. That is § 0.1 on the real gap, not a
reason to widen. 54/`#13` stays in the reported set; it is not a § 0
customer.

**Cause found (2026-09-14).** B has a 66.6 s digital-silence stall at
B 1:42:32.7 that A lacks; at g012 the true offset is ≈ +67–69 s, not the
+2.06 s map (end of soundtrack measured at +67.9 s). The gap is a
timeline step, not a Decorrelated hole — tracked in
[TEMP-b-stall-warning-plan.md](../TEMP-b-stall-warning-plan.md) and
BACKLOG § *Timeline integrity*. Every far stride on this gap (≤ 7.5 s)
off the hole) also sits after the stall and maps through the same wrong
map; do not read its walk as map evidence.

---

## 2. Design rules (normative)

These are the rules the 10 s funnel violated. Each is load-bearing.

1. **Window length is 1.0 s.** Same as grow-post's largest rung, the
   disjoint-offseam probe, and `LAG_DECISION_WINDOW_SECS`. A longer
   envelope cannot say where the matching energy sat.
2. **Seam and disjointness.** *Seam* is the refined gap edge (`a_start`
   pre, `a_end` post). The production span is the **displaced** throat
   template: pre `[a_start − d_pre − 1.0 s, a_start − d_pre]`, post
   `[a_end + d_post, a_end + d_post + 1.0 s]`, where `d_*` is that
   shoulder's `border_displacement_ms`. The first off-seam window sits
   0.5 s beyond that span (pre: ends at `a_start − d_pre − 1.5 s`; post:
   starts at `a_end + d_post + 1.5 s`). One-sided walks (post forward, pre
   backward). No window overlapping the displaced span is scored.
   `distance_ms` is still measured from the refined edge.
3. **Dump every stride.** 250 ms steps; near edge 1.5 s through 6.5 s
   beyond the displaced span (far edge 2.5–7.5 s); pre mirrored. Do not
   stop at the first lock. `first_unique_*` is a **derived** summary of
   the array, not the search.
4. **Self-unique is its own predicate.** A-vs-A over ±600 ms on the same
   1 s slice. Lag 0 is r = 1.0 and never edge-pinned by construction, so
   neither is part of the predicate. The rival is the tallest local
   maximum **outside ±30 ms of lag 0**
   (`secondary_peak_excluding`, § 9):
   `a_self_unique ⇔ a_self_rival_r ≤ 0.70`.
5. **A↔B lock is versus that stride's own lag curve.** Summarize with
   `summarize_lag_curve` (so `ab_prom` means what `lag_decision.prominence`
   means — plain `secondary_peak`, no exclusion). A lock is all of:
   `ab_r ≥ 0.5` (`LAG_DECISION_RECOVERABLE_R`), `ab_prom ≥ 0.10`, not
   `ab_edge_pinned`, **and** `a_self_rival_r < ab_r − 0.10` (the B lag
   stands out more than A's own repeats). `ab_z` is recorded for later
   threshold reads and is **not** admitted on. Do not use the landmark bar
   (`peak_r ≥ 0.8` ∧ `peak_z ≥ 12`) — that is a chain prior, not a phrase
   detector.
6. **B map is nominal `b_mapped` of that A slice.** Lag reach stays
   ±`SEAM_LOCAL_SEARCH_MS` (600 ms). Do not search the production fill
   offset, do not widen the fine window, do not re-run envelope rescue
   on far strides. An empty result means “no unique phrase on this map,
   off-seam,” not “no structure exists anywhere.”
7. **Neighbor clamp.** If the 1 s slice would overlap another gap's
   `[a_start, a_end)`, skip that stride and stamp `neighbor_bounded`.
   Dump it the same as scored strides.
8. **Interpolation requires a unique lock on both sides.** One-sided
   unique is stamped `rigid_one_sided` (apply that `L` to both hole
   edges) and does not enter § 7. Zero unique locks is `no_unique`.
9. **Throat read at `b_pred` is exact-lag only.** Pre collar = production
   `lag_decision` pre template; post mirrored. Score Pearson at the
   integer lag nearest `L(t)` at that edge with reach
   ±`LAG_EDGE_TOL_MS` — never ±600 ms (§ 0.1). Report it; do not gate on
   it. Residual at a silent core is uninformative; do not invent a
   residual pass.
10. **Do not use dual-fit agreement as pass/fail.** Record
    `far_lag − dualfit_lag` (ms) as a column. Agreement is a later
    production question.

Cheap A-only flatness (`energy_envelope_is_flat` on the **1 s** slice)
may skip a stride without touching B. Flat is a scored outcome
(`flat`), not silence in the dump.

Pad must cover `displacement + 1.5 + 5.0 + 1.0 + 0.6 s` from the seam in
the walk direction. The B haystack pad at defaults is
`gap_signature_context_secs + fill_border_search_secs +
fill_align_margin_secs` = 3 + 10 + 1 = 14 s (both `characterize.rs`
sites). Confirm the A-side capture reaches the same bound; name the site
if it is short.

---

## 3. Interpolation (normative)

`L` is gross lag in milliseconds relative to that slice's nominal
`b_mapped` (same convention as `lag_decision.frac_lag_ms`).

Let `t_pre` / `t_post` be the **centres** of the first unique pre / post
windows (derived summaries; the arrays still hold every stride).

```text
L(t) = L_pre + (L_post − L_pre) * (t − t_pre) / (t_post − t_pre)
```

Evaluate at `t = a_start` and `t = a_end` of the **scan hole** (refined
frames). Predicted B:

```text
b_pred(t) = b_mapped_nominal(t) + L(t)
```

where `b_mapped_nominal(t)` is the pair map already on the fingerprint
(`source.alignment.recommended_offset_secs` plus whatever per-gap
`fill_offset` the dump used). Do not add `L` twice.

**Prediction error.** `err_pre_ms = l_start_ms − lag_decision.pre.frac_lag_ms`;
`err_post_ms = l_end_ms − lag_decision.post.frac_lag_ms`. Present only
when that throat shoulder is locked (§ 1 primary predicate).

If `t_post == t_pre`, refuse interpolation and stamp `degenerate`.

**One-sided.** If only pre is unique, `L(t) = L_pre` for both edges
(`rigid_one_sided`). Post-only mirrored. Not in § 7.

**Disagreement.** If both sides unique and `|L_post − L_pre| > 100 ms`,
still interpolate and still report, but stamp `step_suspect`. A real
local map-step between the phrases is possible; treating it as smooth
drift would be the 31/g7 class of mistake. `step_suspect` gaps are
reported separately in § 6 and excluded from the § 7 p95.

---

## 4. Probe contract

### 4.1 Flag and recipe

CLI: `--fingerprint-far-unique` (requires `--gap-fingerprints`).
Independent of `--fingerprint-diagnostics`, the window ladders, and
`--fingerprint-constellation`. Flag off → field absent, default dumps
byte-stable.

Stamp on `source.probe_recipe` when the flag is on:

| Key | Value |
|-----|--------|
| `far_unique_window_ms` | `1000` |
| `far_unique_stride_ms` | `250` |
| `far_unique_first_offset_ms` | `1500` |
| `far_unique_last_near_ms` | `6500` |
| `far_unique_max_lag_ms` | `600` |
| `far_unique_lock_min_r` | `0.5` |
| `far_unique_lock_min_prom` | `0.10` |
| `far_unique_self_exclude_ms` | `30` |
| `far_unique_self_max_rival_r` | `0.70` |
| `far_unique_self_margin` | `0.10` |

`--no-dual-fit` is irrelevant: dump-only.

### 4.2 Per-stride fields (both seams)

| Field | Meaning |
|-------|---------|
| `distance_ms` | Near edge, always positive, from the refined gap edge |
| `a_start_secs` / `a_end_secs` | Slice on A |
| `reason` | `scored` / `flat` / `neighbor_bounded` / `out_of_range` |
| `a_self_rival_r` / `a_self_rival_lag_ms` / `a_self_unique` | Rule 4 |
| `ab_r` / `ab_prom` / `ab_z` / `ab_lag_ms` / `ab_edge_pinned` | A↔B Pearson, `summarize_lag_curve` |
| `ab_locked` | Rule 5 |
| `unique` | `a_self_unique ∧ ab_locked` |

### 4.3 Per-gap summaries

A reader can sort without expanding the arrays:

- `pre_n_scored` / `post_n_scored`
- `pre_n_unique` / `post_n_unique`
- `pre_first_unique_distance_ms` / `post_first_unique_distance_ms` (`None` if none)
- `pre_first_unique_lag_ms` / `post_first_unique_lag_ms`
- `interpolation`: `both_unique` / `rigid_one_sided` / `no_unique` / `degenerate`
- `l_pre_ms` / `l_post_ms` / `l_start_ms` / `l_end_ms` (`None` when `no_unique`)
- `step_suspect` (bool)
- `err_pre_ms` / `err_post_ms` (§ 3; `None` unless that throat shoulder is locked)
- `throat_r_at_pred`: pre / post Pearson at `b_pred`, exact-lag reach
  (rule 9; `None` when `no_unique`)

No field is an accept/reject against dual-fit. Existing
`lag_decision` / `splice_dualfit` columns stay the comparison.

Schema: `far_unique` is a `GapFingerprint` group; no production type
carries it; no gate reads it.

---

## 5. Fixture before corpus

No dump is a § 0 result until these are green. If a fixture fails,
change the probe; do not run media on a walk that cannot see a planted
phrase.

1. **On-seam energy stays on-seam.** A unique injected lag whose energy
   lives only in the displaced post span must **not** set `unique` on any
   off-seam stride.
2. **Off-seam phrase is found.** A unique injected lag whose energy
   lives only in `[seam + 3 s, seam + 4 s]` must set `unique` on a post
   stride whose `distance_ms` is ~3000, with `ab_lag_ms` within 10 ms of
   the injected lag.
3. **Self-unique rejects a period.** A 300 ms phrase repeated every
   400 ms (100 ms gaps) across a 6 s bed. Every stride inside the bed has
   `a_self_rival_r` near its ±400 ms rival and `a_self_unique == false`,
   even where `ab_r ≥ 0.5`.
4. **Interpolation predicts the throat.**
   - **4a (map prediction).** Shared content on A and B at lag `L`
     across the whole span including the throat, with distinct unique
     phrases at −3 s and +3 s. `interpolation == both_unique`;
     `l_start_ms`, `l_end_ms` within 10 ms of `L`; `|err_pre_ms|`,
     `|err_post_ms|` ≤ 10 ms.
   - **4a-step.** As 4a with `L_post = L_pre + 400 ms`: stamps
     `step_suspect` and still emits `l_*` / `err_*` / `throat_r_at_pred`.
   - **4b (Decorrelated honesty).** Same far phrases as 4a; throat
     collars are **independent** noise on A and B. `both_unique`, correct
     `L`, `err_*` `None`, and both `throat_r_at_pred < 0.3`. The probe
     must not manufacture a throat lock.
5. **Every off-seam window is disjoint** from the displaced production
   span (rule 2), including a case with non-zero `border_displacement_ms`.
   Assert in the unit test, not by inspection.

Do not inherit `energy_peak_candidates` or constellation onsets. This
instrument is dense Pearson on a 1 s slice.

---

## 6. Corpus comparison

One `--gap-fingerprints --fingerprint-far-unique` run with
`--features calibration,he-aac`. Start with pair 54 (it has ten locked
holes and the motivating skip), then widen pair by pair until the
map-prediction set reaches n ≥ 20 non-`step_suspect` `both_unique` gaps.
Do not require a 51-pair dump.

**Map-prediction set (decides § 7).**

| Read | Meaning |
|------|---------|
| `n` | `both_unique`, both throat shoulders locked, not `step_suspect` |
| p95 / max of `|err_pre_ms|`, `|err_post_ms|` | Does far `L(t)` land on the throat's own lag? |
| `rigid_one_sided` / `no_unique` rate | How often a gap has any far map at all |
| `step_suspect` count and its `err_*` | Reported separately; not in the p95 |
| `neighbor_bounded` rate | If dominant, spacing is wrong for this idea — a result, not a retry closer in |

**Reported set (54/`#13` and the Decorrelated occupied skips).**
`interpolation`, `l_start_ms` / `l_end_ms`, `step_suspect`,
`throat_r_at_pred`. Expected by § 0.1 / § 1a: `throat_r_at_pred` stays
low. Carry `b_pred` into the later ear TEMP only if § 7 is yes.

**Byproduct, not this exit.** The walk array is the same geometry
disjoint-offseam specified. Reading 50/g10 post off this dump is
allowed; it does not close or unlatch that plan. Do not change this
probe to chase a 50/g10 miss.

---

## 7. Decision

- **n ≥ 20 and p95 `|err|` ≤ 30 ms (max reported)** → § 0 is yes: far
  phrases are a trustworthy local map on this corpus. Open a **separate**
  TEMP asking whether a map-trusted fill at `b_pred` is acceptable by ear
  on the reported set. Do not promote into `seam_locks.rs` from this dump.
- **n ≥ 20 and p95 `|err|` > 30 ms** → close as **far-unique `L(t)` does
  not predict the throat lag on this map**. Do not lower the lock bar,
  lengthen the window, or start closer.
- **n < 20 after the pairs with locked holes are exhausted** → close as
  **insufficient map-prediction set** (report max `|err|` and the
  `no_unique` / `neighbor_bounded` rates that starved it).
- **Any outcome** → 54/`#13` is recorded as the § 1a result: throat
  non-recoverable across the fine band (pre also across the envelope
  retry); interpolation within the band cannot patch it.

---

## 8. Done when

The § 5 fixtures are green, then § 6 has been read against a real dump,
then this plan is archived with a [settled.md](../settled.md) row that
states **one** of the § 7 bullets plus the 54/`#13` line — not “far
context” as a slogan. **Latched 2026-09-14 without a dump:** the
later-TEMP customer collapsed (banner); the § C trigger is a remaining
skip whose failure is a locatable lag on a locally-correct map.

---

## 9. Implementation sketch (fingerprint path)

Dump-only, same shape as `--fingerprint-pre-window-ladder`:

1. Flag on `Args` / `RepairSettings` / `FingerprintConfig` (`calibration`
   feature), rejected without `--gap-fingerprints`.
2. `domain/lag_decision.rs`: add `secondary_peak_excluding(curve,
   peak_index, exclude_frames)` beside `secondary_peak`, with a unit test
   (a rival inside the exclusion is ignored; one outside is returned).
   `secondary_peak` itself is unchanged.
3. Measure in `application/gap_fingerprint/measure/` (new `far_unique`
   module). A↔B: `lag_correlation_curve_auto` + `summarize_lag_curve`.
   A↔A: `lag_correlation_curve_auto` + `secondary_peak_excluding`. Throat
   at `b_pred`: the `lag_decision` templates at exact-lag reach. Do not
   duplicate Pearson.
4. Wire group `far_unique: Option<FarUniqueSet>` on `GapFingerprint`,
   omitted when the flag is off. Overlay + projector: absent on the spec
   path, restored from decode like `constellation`.
5. `probe_recipe` stamps the frozen knobs in § 4.1.
6. Unit tests for § 5 live next to the probe, synthetic PCM, no media.
7. Default dumps remain byte-stable (fingerprint goldens unchanged when
   the flag is off).

Keep path, dual-fit, and `fill_seam_search_secs` do not change.
