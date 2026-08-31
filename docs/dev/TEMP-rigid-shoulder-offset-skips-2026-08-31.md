# TEMP — Rigid shoulder-offset skips: does map-lag explain the missed fills? (2026-08-31)

**Status:** findings note, not a Change, 2026-08-31. **Startable.** Sizes the BACKLOG
class "95 skips at a rigid shoulder offset the bracket sweep never reaches" and probes
whether the **production remainder** would patch if the keep path saw a constant map
shift. **This note proposes no production change.**

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *95 skips at a rigid
shoulder offset the bracket sweep never reaches*.

**Companions:** the caution
[settled.md](settled.md) § E *A bracket sweep score cannot see a rigid map-lag error*;
11/g9 as abut false positive
[TEMP-early-shoulder-locks-collapse-skip-plan.md](archive/TEMP-early-shoulder-locks-collapse-skip-plan.md)
§ 4 / § 8.1; drain-not-patch
[TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
§ 1 (11/g9 must not drain; un-abut is that plan's step-agreement filter, not this probe).

**Media hygiene:** pair index + gap index only. `gap-files/` is gitignored; numbers below
are the durable record.

**Indices are 0-based**, matching dump filenames (`…_g001_full.json`) and the rest of `docs/`.
38/g1 is dump `g001`, not a 1-based display index.

---

## What this is

The class is: fingerprint **skip**-tier gaps whose 1 s `lag_decision` shoulders are both
live and rigid (`peak_r ≥ 0.94`, `|step| ≤ 25 ms`) **and** B holds content at the nominal
span (`donor_interior_nominal.continuous`, `silence_fraction = 0`). The bracket sweep
varies span and `move_frames` but never the donor map lag, so every bracket can score a
misregistered donor and land on `waveform_floor` looking like an absent donor. 11/g9 is
the worked case (~−70 ms common offset, `lag0_r` 0.046 / −0.018, all 23 brackets 0.05–0.27).

The candidate fix in BACKLOG is **register at the shoulder-agreed lag before the sweep**,
then a plain single-lag fill — not dual-fit; there is no step. Necessary is shown
(`settled.md`). Sufficiency is not.

Results 1–5 changed the object. The 90 are not 90 missed fills: 83 are dual-fit rescues,
and production has patched every one of those seen so far. The open set is **seven**
fingerprint `dual_fit_rescue == false` gaps, of which production still skips all seven.
Five of those seven fail dual-fit only because a constant shift already explains both
250 ms seams (`step_real`). This note's remaining measurement is whether the **keep path**
would patch those holes if it ran at that lag.

## What this is not

- Not a Change. Do not wire register-before-sweep from this doc. Do not hoist
  `post_seam_global_r` onto `SeamLocks` to drive the sweep from this doc.
- Not the collapse successor. Un-abutting 11/g9 so it can reach the keep path is
  [TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md),
  not a side effect of this probe.
- Not a 1 s `peak_r` census of "would convert." The class is *defined* by 1 s `peak_r ≥ 0.94`
  at the agreed lag, so that number converting 90/90 is a tautology.
- Not a conversion fraction over the 90, and not a stratified sample of the 83. Those
  are dual-fit fills, not unrepaired holes.
- Not the withdrawn Phase 1 instrument (250 ms Pearson at `b_mapped + mid_lag`, no
  sweep). That scores a placement the keep path may never choose (38/g1) and treats
  the 90 as one customer. § *Withdrawn*.

## Open questions

1. **Why do 38/g1's brackets fail?** Offset ≈ 0, so map-lag cannot be the reason.
   **Answered, Result 2.**
2. **Would the keep path patch the production remainder** if its B map were shifted by
   the constant-shift lag dual-fit already certified? Overlay `evaluate_seam_gate` after
   that shift; convert ⇔ `outcome.tier == "patch"` (structure + waveform + residual).
   **Open — Phase 1.**
3. **Is the class one mechanism?** Split by `|mid_lag|` and by `dual_fit_rescue`.
   **Answered for membership, Result 1 / 3 / 5: no.** The 83 are dual-fit fills; the
   seven remainder split further (Result 4). Q2 asks only whether the keep path can take
   the constant-shift slice.

## Class predicate

Over `gap-files/2026-08-29-complete-corpus/<pair>/corpus.json` (51 pairs / 1073 gaps —
**1049** `full`-tier plus 24 `summary`-tier. A `summary` gap carries no `outcome` at all, so
it is neither skip nor patch: the 644 skips are drawn from the 1049, and the skip rate must
not be read against the 1073):

```text
skip                       outcome.tier == "skip"          # 644
both 1 s shoulders         lag_decision pre_anchor[0] and post_anchor[0] present
live                       min(pre.peak_r, post.peak_r) ≥ 0.94
rigid                      |post.frac_lag_ms − pre.frac_lag_ms| ≤ 25
donor present              donor_interior_nominal.continuous
                           ∧ silence_fraction == 0.0
mid_lag                    (pre.frac_lag_ms + post.frac_lag_ms) / 2
```

BACKLOG wrote **95 / 644**. Re-derived here: **90 / 644** (Result 1). Use 90 as the ship
set of the *fingerprint class*. The production remainder is the seven in Result 3.

Of the 644 skips: 90 class, **78** no both-shoulder `lag_decision` (BACKLOG 78), **447**
donor silent or discontinuous at nominal (BACKLOG 436), **29** live-donor but weak or
stepped (BACKLOG 35). The 644 denominator and the 78 leg match exactly; the other three
moved.

**Where the five went (measured 2026-08-31, not speculation).** The delta is the
**donor-presence leg alone**: the earlier pass admitted 11 more gaps as donor-present
(447 → 436), and those 11 split 5 into the class and 6 into weak-or-stepped. Two causes
named in passing are **ruled out**:

- *Not an earlier dump.* The predicate above gives byte-identical 78 / 447 / 29 / **90**
  on `2026-08-23-`, `2026-08-28-` **and** `2026-08-29-complete-corpus`. The class has
  been 90 on every corpus dump in the tree.
- *Not the `r` floor on its own.* Holding donor strict: `r ≥ 0.92` → 91, `≥ 0.90` → 93,
  `≥ 0.85` → 95, `≥ 0.5` → 97, with donor pinned at 447 and weak at 24 throughout. So
  `r ≥ 0.85` reaches 95 but contradicts BACKLOG's 436 / 35. `r` is not the sharp lever;
  **`|step|` is** — `≤ 50 ms` gives 106.

No single donor loosening reproduces 436 / 35 / 95 jointly either (`silence_fraction ≤ ε`,
`longest_silence_ms ≤ t`, `continuous`-only, `continuous ∨ sf ≤ t`, and
`donor_interior_aligned` were all swept; `continuous ∨ sf ≤ 0.35` hits donor 436 but
yields 99 / 31). The reason: of the 447 donor-rejected skips that *have* shoulders, **419
would pass the live+rigid test**, so any donor loosening feeds ~9-in-11 into the class,
never a 5/6 split. The old pass therefore differed on the donor leg *and* somewhere in
live/rigid, via an ad hoc script that is not in the repo. **Do not hunt the five** —
90 is stable and is the ship set of the class.

88 of 90 skip `waveform_floor`; 2 skip `structure_floor` (2/g20, 36/g8).

---

## Result 1 — the class is 90, and it is not all far-offset

`|mid_lag|` is bimodal, not a single −70 ms story.

**Cumulative** cuts (these overlap — `< 5 ⊂ < 25`, `> 200 ⊂ > 50`):

| `|mid_lag|` | n (cumulative) | Reading |
|---|---:|---|
| < 5 ms | 14 | Already at nominal. Map-lag registration has nothing to apply. Includes **38/g1**. |
| < 25 ms | 34 | Inside a 50 ms bin; likely the same as < 5 for a 100 ms envelope. |
| > 50 ms | 49 | Offset large enough that `lag0_r` is typically dead. Includes **11/g9** (−70 ms). |
| > 200 ms | 20 | Far. Includes pair-31/g9 at **−1145 ms** (envelope dual-fit already fills this on the patch path). |

**Disjoint strata** — the partition `census-skip-buckets.ps1` prints. Membership census
only; Phase 1 no longer samples from these.

| stratum | n |
|---|---:|
| `|mid| < 5 ms` | 14 |
| 5–50 ms | 27 |
| 50–200 ms | 29 |
| > 200 ms | 20 |

p50 `|mid_lag|` = 57 ms. Full membership: **Appendix**.

Controls:

| gap | role | mid_ms | `lag0_r` | brackets (max seam) |
|---|---|---:|---|---|
| **11/g9** | positive — map-lag exhibit | −69.8 | 0.046 / −0.018 | 23 brackets, 0.05–0.27 |
| **38/g1** | negative — offset already 0 | −0.1 | **0.985 / 0.782** | 21 brackets, **0.130 / 0.130** |

## Result 2 — 38/g1 is named, and it is not an unrepaired hole

38/g1 fails the **bracket** seams at 0.130 while the 1 s pre lock at lag 0 reads 0.985.
(0.130 is a production keep-path number: bracket seams are `evaluate_seam_gate` at the
gate's own structure-chosen placement, not a probe-window score.)
`splice_dualfit` at the 250 ms seam **passes** (0.995 / 0.982, `gate_pass` true,
`dual_fit_rescue` true). On `gap-files/2026-08-30-tier-2-on` and on
`gap-files/2026-08-31-local-gain-acceptance` the gap is **patched**, `dual_fit_used`
true.

So:

- Map-lag is **not** why the brackets fail. There is no lag to apply.
- The keep-path sweep and the dual-fit seam are different instruments — same 250 ms
  scorer, different *placements* (sweep span/`move_frames` vs `seam_local_peak`). That
  is the other mechanism.
- 38/g1 does **not** show the class stays skipped in production. Dual-fit already
  fills it. It shows only that **register-before-sweep cannot be why this gap was
  a fingerprint skip**, and cannot be the fix for near-zero members.
- Scoring the keep path at `b_mapped` rather than at the structure-chosen start would
  mis-count 38/g1 as a convert. That is why Phase 1 no longer uses that instrument.

## Result 3 — most of the class is already a dual-fit rescue on paper

Fingerprint `dual_fit_rescue` (the "would `try_dual_fit` admit this skip" flag, not
the bracket outcome):

| `dual_fit_rescue` | n |
|---|---:|
| true | **83** |
| false | **7** |

Default production runs dual-fit. The 83 are predicted fills *without* touching the
bracket sweep. 31/g9 is in this 83 and is the gap the shoulder-locks Change newly
filled on Tier 2.

The seven that dual-fit does **not** predict (conjuncts named in Result 4):

| gap | mid_ms | 250 ms dual `r` | `gate_pass` |
|---|---:|---|---|
| **11/g9** | −69.8 | 0.989 / **0.500** | true |
| 1/g1 | 139.3 | 1.000 / 0.985 | true |
| 1/g15 | 158.8 | 0.999 / 0.983 | true |
| 20/g1 | −286.6 | 1.000 / 0.953 | true |
| 22/g5 | −249.8 | 0.998 / 0.982 | true |
| 30/g43 | 10.0 | 0.908 / 0.968 | true |
| **22/g13** | −247.9 | 0.996 / **0.271** | **false** |

## Result 4 — the seven remainder, named from the 08-29 dump

`dual_fit_rescue` is a six-term AND (`schema.rs` `dual_fit_rescue_flag`). All seven have
eligible brackets, aligned `continuous`, and `donor_interior_nominal.silence_fraction = 0`.
The failing term is dump-side on `2026-08-29-complete-corpus`. No new fingerprint run.

**Step-real** is not a lag delta. After both 250 ms locks exist, dual-fit scores the **post**
A template against B at `b_pre_seam + gap_frames` (step forced to 0). If that Pearson is
within `DUALFIT_STEP_REAL_MARGIN` (0.15) of the post's own-lag score, a single constant
shift already explains both seams and dual-fit *correctly* refuses
(`dual_fit.rs` `try_dual_fit`). That Pearson is `post_seam_global_r`. Production computes
it only inside `try_dual_fit` (after the keep path has already failed). The fingerprint
computes it next to the locks in `splice_dualfit_at`. `SeamLocks` does not carry it.
1 s `|step| ≤ 25 ms` is a different instrument and is not this test (11/g9).

| gap | `gate_pass` | post / global (Δ) | `bridge_frames` | `trim_frames` | Failing conjunct |
|---|---|---|---:|---:|---|
| **1/g1** | true | 0.985 / 0.984 (0.001) | 89872 | 1 | **`step_real`** |
| **1/g15** | true | 0.983 / 0.909 (0.074) | 93831 | −3 | **`step_real`** |
| **20/g1** | true | 0.953 / 0.882 (0.071) | 89568 | 9 | **`step_real`** |
| **22/g5** | true | 0.982 / 0.847 (**0.135**) | 29719 | −11 | **`step_real`** — 0.015 under the margin |
| **30/g43** | true | 0.968 / 0.946 (0.022) | 91489 | −1 | **`step_real`** |
| **11/g9** | true | 0.500 / −0.084 (0.585) | **−205** | −28980 | **`bridge ≤ 0`** (abut; post is the false peak one gap-length away) |
| **22/g13** | **false** | **0.271** / −0.052 (0.323) | 60170 | −381 | **`gate_pass`** — post dead even with envelope |

Five of seven are "a constant shift suffices" on the dual-fit 250 ms instrument. The lag
that test certified is `locks.pre.lag`, not the 1 s `mid_lag`. 30/g43 is why that
distinction is load-bearing: 1 s mid is +10 ms and `lag0_r` is 0.013 / −0.039 (250 ms at
`b_mapped` is dead), while seam-local 250 ms at the pre lock is 0.908 / 0.968. Applying
+10 ms cannot be the keep-path experiment for this gap.

11/g9's 0.500 post is not the −70 ms 1 s lock. A keep-path shift to −70 ms is still the
map-lag candidate there — and it cannot run in production while the gap is
`shoulders_abut`.

## Result 5 — production still skips exactly those seven

Join the 90 to `gap-files/2026-08-31-local-gain-acceptance/` (repair `--patch-only`,
`--normalize` on; gain is after keep/skip, so **disposition** is the join). Array index =
this note's 0-based gap index. Full 51-pair dump, 2026-08-31:

| | n |
|---|---:|
| Class members | **90** / 90 |
| **patched**, all `dual_fit_used` | **83** |
| patched, keep path (`dual_fit_used` false) | **0** |
| **skipped** | **7** |

The seven skipped are exactly Result 3's remainder:

| gap | production | keep-path seams |
|---|---|---|
| 11/g9 | `shoulders_abut` (pre −72 ms, post −676 ms, `bridge_frames` −205) | never reached |
| 1/g1, 1/g15, 20/g1, 22/g5, 22/g13, 30/g43 | `correlation_below_threshold` | ~0.02–0.18 |

38/g1 is patched dual-fit, matching Result 2. 31/g9 is patched dual-fit (0.972 / 0.904).

Over every fingerprint skip (644), not just the class: `dual_fit_rescue == true` is
**105 / 105** production dual-fit patches (zero skipped). The 83 were not a special
case. One fingerprint `rescue == false` still dual-fit patched (49/g3: no fingerprint
brackets, so the flag is inapplicable). Production skipped **20** gaps in all: the
seven remainder, 31/g7 (`shoulders_abut`, the real introduced-silence case; fingerprint
still `tier: patch` because the 08-29 dump does not run that skip), and **12** others
that fail the class predicate (donor silent/discontinuous, or 1 s shoulders weak —
20/g3 post `peak_r` 0.856 — or pair-50 dead seams). Those 12 are not this row's object.

`--patch-only` cannot name the dual-fit conjunct (no `post_seam_global_r`). Result 4
is the 08-29 fingerprint corpus.

---

## Withdrawn — 250 ms at `b_mapped + mid_lag` over a ~20-gap sample

The previous Q2 scored `fill_seam_correlations` at `b_mapped + mid_lag` (no search) on
the seven remainder + 38/g1 + three per `|mid_lag|` stratum of the 83. Do **not** run it.

- A pass at `b_mapped` is not the keep path. 38/g1 is the existence proof: 1 s at lag 0
  is 0.985, brackets 0.130 at the structure-chosen start. After a mid_lag shift,
  far-offset members become 38/g1-shaped. The probe would overstate production-after-fix.
- Near-zero "converts" on that instrument are a placement mechanism (`b_mapped` vs
  structure start), not map-lag. Pooling them with far-offset converts would look like
  one class.
- Sampling the 83 asks whether the keep path would have filled gaps dual-fit already
  fills. Result 5 shows it does not, and those are not missed fills.
- A null on a constant-shift-at-`b_mapped` score does not refute register-then-sweep.

The durable pieces of that method that still apply: fingerprint path so 11/g9 is
visible; `fill_seam_correlations` not the interleaved splice scorer; recipe floors, not
literals; `placement_in_bounds` as its own column (a shifted start can 0.0-sentinel).

---

## Phase 1 probe (still to run)

**Question 2.** On the production remainder, add a constant map lag to the B clock the
keep path already uses, then run `evaluate_seam_gate` (structure + waveform + residual).
Convert ⇔ fingerprint `outcome.tier == "patch"` (`any_ok`). Dual-fit is not in that
tier; a convert is a keep-path fill.

### Object

The six production `correlation_below_threshold` gaps. 11/g9 is a **separate
fingerprint-only row**: characterize returns `shoulders_abut` before the gate, so a
shifted keep path cannot patch it until the collapse plan un-abuts. The overlay still
oracles the gate.

| Gap | Primary shift | Role |
|---|---|---|
| 1/g1, 1/g15, 20/g1, 22/g5, 30/g43 | `locks.pre.lag` (250 ms; the lag `step_real` certified) | constant-shift remainder |
| 22/g13 | 1 s `mid_lag` (−248 ms) | post dead at 250 ms; different lag than dual-fit used |
| 11/g9 | 1 s `mid_lag` (−70 ms) | fingerprint only; 250 ms post is the false peak |

Record the unused lag beside each row (1 s `mid_lag` on the five, `locks.pre.lag` on
22/g13 and 11/g9) so a miss can be "wrong lag" rather than "keep path cannot use this
donor."

**30/g43 is not a no-op control.** Its 1 s mid is +10 ms; the experiment is the 250 ms
pre lock. 38/g1 remains the no-op (shift ≈ 0, already patched); do not spend a media run
on it unless the unshifted identity check needs a patched near-zero.

Unshifted control is `2026-08-29-complete-corpus` itself: these six are `waveform_floor`.

### Vehicle

Fingerprint overlay (`overlay.rs` → `oracle_score_fit_candidate` → `evaluate_seam_gate`),
not `--patch-only`. Production after a shift would still skip 11/g9 at abut, and a
keep-path miss could come back `patched` via dual-fit. `--fingerprint-gap` bounds the
oracle to the named gaps.

`--fingerprint-gap` is **1-based** (`composition.rs` `resolve_fingerprint_gap_select`).
1/g1 is `--fingerprint-gap 2`; 1/g15 is `16`; 11/g9 is `10`; 20/g1 is `2`; 22/g5 is `6`;
22/g13 is `14`; 30/g43 is `44`.

### Injection

Record-only. Do not change production skip or patch.

1. Measure `nominal_first_seam_locks` as the overlay already does (`splice_dualfit_at`).
   Do **not** re-derive `lag_decision`. Do not shift whole-file B.
2. Add the primary lag to the B times passed into `derive_seam_gate_geometry` (today
   `refined + gap_offset`). Widen the B extract by `|lag|` plus the existing search pad.
   Record `placement_in_bounds` per scored placement — a 0.0 seam is not "dead."
3. Leave `lag_decision` / `splice_dualfit` on the unshifted map. Those chose the lag;
   they are not the keep path.

Recipe floors from `source.gate_recipe`, never literals.

### Nested reads

1. **Throat** (`move_frames == 0`, nominal span) after the shift. Pass ⇒ a plain
   single-lag fill at the registered map would patch. That is the BACKLOG candidate as
   written.
2. **Full bracket list** after the shift. Throat fail + some other bracket pass ⇒
   register-**then-sweep**, not a plain fill. Both fail ⇒ this lag is not sufficient
   (wrong lag, 38/g1-shaped structure placement, or residual).

Convert ⇔ `outcome.tier == "patch"`. If it still skips, keep `skip_reason`
(`structure_floor` / `waveform_floor` / `residual`). Waveform-only was the withdrawn
instrument; residual can still refuse.

**Wiring identity (unshifted arm).** In the same process, the unshifted overlay must
match the 08-29 throat `seam_pre` / `seam_post` (and `failure_stage`) on these six
before the shifted arm is believed. Then only the B map offset changes.

### Limit

A throat pass is decisive for the plain-fill form. A full-list fail refutes "this lag
on the keep path" for that gap, not "any registration." Do not file a class-wide
`settled.md` refute from a null; rewrite (not delete) the BACKLOG row unless the
remainder is named closed.

If the five `step_real` misses convert at `locks.pre.lag` and 22/g13 does not at 1 s
mid, that is a split, not a pooled fraction. 11/g9 converting on the overlay is **not**
a production convert.

If they convert, a **separate** Change plan to wire register-before-sweep is in scope
(one deliverable). Hoisting `post_seam_global_r` onto `SeamLocks` so
`gate_pass && bridge > 0 && !step_real` can drive that shift belongs in that Change,
not here. 11/g9 still needs the collapse plan's un-abut before that Change can see it.

## 11/g9 sequencing

| Path | What happens to 11/g9 |
|---|---|
| Today (shoulder-locks shipped) | `shoulders_abut` skip, record-only. No fill. |
| This probe | Can oracle the gate at −70 ms on the fingerprint path. Must not change production. |
| Collapse successor | Must not drain. Step-agreement admission filter would drop it from the abut list, after which it is an ordinary skip again. |
| Register-before-sweep Change | Only then can the keep path offer a single-lag fill. |

Do not bundle those three.

## Non-goals

- Wiring the sweep, or hoisting step-real onto `SeamLocks` to drive it.
- Dual-fit policy changes (envelope already relocates far dead locks on that path;
  refusing `step_real` is correct).
- Treating `dual_fit_rescue == true` as "the gap is unrepaired."
- A conversion fraction over the 90 or over a stratified sample of the 83.
- Ear checks until Question 2 names a convert set.
- `--patch-only` as the primary read (abut + dual-fit confound). Confirmation later,
  reading `dual_fit_used`, is allowed.

## Data sources

- **`gap-files/2026-08-29-complete-corpus/`** — 51 pairs, 1073 gaps (1049 `full`-tier).
  Class membership, 38/g1, `dual_fit_rescue`, Result 4 conjuncts (`splice_dualfit`).
- **`gap-files/2026-08-23-` and `-08-28-complete-corpus/`** — same-predicate controls for
  the 95-vs-90 delta; both give 90.
- **`gap-files/2026-08-30-tier-2-on/38.json`** — 38/g1 production patched via dual-fit.
- **`gap-files/2026-08-31-local-gain-acceptance/`** — Result 5 production join
  (`--normalize` on; read disposition only). Complete 51-pair dump.

## Reproduction

`scripts/measure/census-skip-buckets.ps1 -DumpDir <corpus> -CsvOut <path>` — dump-only,
no media, one row per skipped gap with every predicate leg's raw input beside its
`bucket`. Prefers each pair's `corpus.json`; pair index = the containing directory name.
Per gap: `lag_decision.pre_anchor[0]` / `post_anchor[0]`,
`donor_interior_nominal`, `outcome.tier`. Predicate in § *Class predicate*. 38/g1
bracket maxima from `brackets[].seam_pre` / `seam_post`. Production join on patch-JSON
array index within a pair label (Result 5). Dual-fit conjuncts from
`splice_dualfit.gate_pass` / `bridge_frames` / `post_seam_r` vs `post_seam_global_r`
(Result 4).

## Exit

- Q1 named (this note, Result 2) → keep.
- Q3 membership split named (Results 1 / 3 / 4 / 5) → keep.
- Q2 states, **per remainder gap**, whether the shifted overlay patched (throat and/or
  full list), with `skip_reason` on a miss → convert on the constant-shift five ⇒
  promote a Change plan (one deliverable: register-before-sweep on the keep path);
  null on those five ⇒ rewrite (not delete) the BACKLOG row; the class is not one
  missed-fill bug and the keep-path candidate is the wrong shape for the holes that
  are actually open. 22/g13 and 11/g9 report separately.
- This note archives when every question is promoted or closed.

---

## Appendix — the 90

`mid_ms` = mean of the two 1 s `frac_lag_ms`. `lag0_r` is correlation at the
unshifted map (the number every bracket currently resembles).

| pair/gap | mid_ms | step_ms | 1 s r | lag0_r | skip | dual_fit_rescue |
|---|---:|---|---|---|---|---|
| 1/g1 | 139.3 | 0.0 | 1.000/0.999 | -0.137/-0.000 | waveform_floor | False |
| 1/g2 | 136.5 | 1.7 | 1.000/0.998 | 0.053/0.010 | waveform_floor | True |
| 1/g4 | 147.9 | -1.5 | 0.998/0.967 | 0.026/-0.006 | waveform_floor | True |
| 1/g5 | 179.8 | -1.9 | 0.995/1.000 | -0.035/-0.011 | waveform_floor | True |
| 1/g13 | 164.9 | 7.8 | 1.000/0.995 | -0.049/-0.002 | waveform_floor | True |
| 1/g15 | 158.8 | -0.1 | 0.999/0.994 | -0.058/0.065 | waveform_floor | False |
| 2/g18 | 127.0 | -10.2 | 0.989/0.993 | -0.067/0.005 | waveform_floor | True |
| 2/g20 | 128.5 | 6.5 | 0.983/0.997 | 0.016/0.146 | structure_floor | True |
| 3/g6 | -350.7 | 0.2 | 0.993/0.990 | 0.002/-0.027 | waveform_floor | True |
| 3/g9 | -353.6 | -1.6 | 0.983/0.974 | -0.007/-0.026 | waveform_floor | True |
| 3/g14 | -345.1 | 5.8 | 0.984/0.966 | 0.001/0.006 | waveform_floor | True |
| 7/g4 | 13.2 | -3.9 | 0.972/0.992 | -0.005/-0.029 | waveform_floor | True |
| 7/g6 | 2.6 | 5.1 | 0.965/0.957 | 0.846/-0.403 | waveform_floor | True |
| 8/g3 | 76.5 | -6.2 | 0.989/0.959 | -0.001/-0.004 | waveform_floor | True |
| 8/g7 | 36.4 | 0.3 | 0.993/0.990 | 0.060/0.128 | waveform_floor | True |
| 8/g10 | 45.9 | -5.9 | 0.988/0.974 | 0.047/-0.019 | waveform_floor | True |
| 9/g6 | -42.9 | 6.0 | 0.995/0.991 | 0.008/-0.302 | waveform_floor | True |
| 10/g1 | -4.5 | 15.2 | 0.994/0.984 | 0.088/-0.303 | waveform_floor | True |
| 10/g2 | 6.6 | 7.8 | 1.000/0.996 | -0.285/-0.149 | waveform_floor | True |
| 10/g3 | -22.7 | 13.3 | 0.999/0.996 | 0.042/0.122 | waveform_floor | True |
| 10/g5 | -0.6 | 2.5 | 0.993/0.993 | -0.083/-0.324 | waveform_floor | True |
| 11/g4 | -65.7 | -3.9 | 0.988/0.992 | 0.003/0.049 | waveform_floor | True |
| 11/g5 | -54.5 | 7.8 | 0.988/0.987 | 0.086/0.047 | waveform_floor | True |
| 11/g6 | -54.9 | -7.8 | 0.989/0.987 | -0.050/-0.024 | waveform_floor | True |
| 11/g7 | -56.9 | 5.8 | 0.981/0.991 | -0.012/-0.059 | waveform_floor | True |
| 11/g9 | -69.8 | 4.2 | 0.999/0.988 | 0.046/-0.018 | waveform_floor | False |
| 11/g11 | -40.0 | 1.7 | 0.987/0.972 | -0.132/0.000 | waveform_floor | True |
| 11/g13 | -63.6 | 22.8 | 0.993/0.984 | 0.035/0.027 | waveform_floor | True |
| 12/g1 | 141.0 | 2.1 | 0.990/0.992 | 0.121/-0.032 | waveform_floor | True |
| 12/g4 | 92.3 | 17.9 | 0.997/0.974 | 0.002/0.014 | waveform_floor | True |
| 12/g5 | 133.2 | 4.0 | 0.993/0.971 | -0.009/0.031 | waveform_floor | True |
| 13/g18 | 267.7 | -5.1 | 0.996/0.993 | 0.012/0.001 | waveform_floor | True |
| 17/g2 | -12.6 | 2.1 | 0.999/0.998 | -0.074/0.217 | waveform_floor | True |
| 17/g6 | -12.5 | 2.2 | 0.983/0.991 | -0.062/0.345 | waveform_floor | True |
| 17/g7 | 19.5 | -4.0 | 0.995/0.981 | 0.007/-0.060 | waveform_floor | True |
| 18/g26 | 316.3 | -4.0 | 0.991/0.955 | -0.122/0.032 | waveform_floor | True |
| 18/g27 | 364.3 | 3.9 | 1.000/0.999 | 0.011/0.002 | waveform_floor | True |
| 18/g33 | 334.4 | 0.2 | 0.989/0.991 | -0.010/-0.002 | waveform_floor | True |
| 18/g40 | 336.8 | 3.9 | 0.987/0.982 | 0.026/0.011 | waveform_floor | True |
| 19/g11 | 70.6 | 17.2 | 0.985/0.977 | -0.040/-0.011 | waveform_floor | True |
| 20/g1 | -286.6 | 0.2 | 1.000/0.988 | 0.042/0.006 | waveform_floor | False |
| 20/g2 | -291.5 | 2.3 | 1.000/1.000 | 0.022/0.001 | waveform_floor | True |
| 20/g6 | -293.5 | -5.8 | 0.999/0.998 | 0.001/-0.014 | waveform_floor | True |
| 22/g5 | -249.8 | -0.2 | 0.944/0.996 | 0.002/0.035 | waveform_floor | False |
| 22/g7 | -218.6 | -6.0 | 0.992/0.965 | -0.016/0.001 | waveform_floor | True |
| 22/g11 | -247.9 | -7.9 | 1.000/0.998 | -0.074/0.016 | waveform_floor | True |
| 22/g13 | -247.9 | -7.9 | 0.995/0.991 | 0.020/0.008 | waveform_floor | False |
| 23/g8 | -2.9 | 1.9 | 0.996/0.979 | -0.113/0.072 | waveform_floor | True |
| 23/g11 | -0.3 | -8.0 | 0.996/0.975 | 0.471/0.332 | waveform_floor | True |
| 23/g15 | 3.3 | -8.1 | 0.961/0.962 | -0.008/0.048 | waveform_floor | True |
| 24/g5 | -400.9 | 3.9 | 0.994/0.991 | -0.005/-0.002 | waveform_floor | True |
| 24/g31 | -395.4 | 2.2 | 0.985/0.995 | -0.004/0.022 | waveform_floor | True |
| 24/g46 | -385.7 | 0.8 | 0.998/0.977 | 0.001/-0.015 | waveform_floor | True |
| 25/g4 | 404.4 | 1.9 | 0.988/0.964 | 0.037/0.036 | waveform_floor | True |
| 27/g3 | 50.5 | -2.0 | 0.990/0.996 | 0.066/-0.015 | waveform_floor | True |
| 30/g2 | 8.2 | 18.1 | 0.988/0.973 | -0.253/0.014 | waveform_floor | True |
| 30/g6 | -4.5 | 23.8 | 0.998/0.989 | -0.348/-0.162 | waveform_floor | True |
| 30/g42 | -9.8 | 19.4 | 0.997/0.994 | 0.078/0.667 | waveform_floor | True |
| 30/g43 | 10.0 | -0.0 | 0.971/0.971 | 0.013/-0.039 | waveform_floor | False |
| 31/g9 | -1144.7 | -4.1 | 0.965/0.957 | — | waveform_floor | True |
| 32/g10 | 160.8 | 0.1 | 0.982/0.952 | 0.166/-0.075 | waveform_floor | True |
| 33/g22 | 9.3 | -1.9 | 0.991/0.994 | 0.077/-0.072 | waveform_floor | True |
| 34/g15 | 2.4 | -3.8 | 0.992/0.989 | -0.169/0.302 | waveform_floor | True |
| 35/g5 | 52.1 | 5.6 | 0.989/0.992 | 0.028/0.125 | waveform_floor | True |
| 35/g7 | 61.3 | 3.9 | 0.995/0.998 | 0.020/-0.195 | waveform_floor | True |
| 36/g4 | 13.6 | 8.7 | 0.997/0.992 | -0.207/0.004 | waveform_floor | True |
| 36/g5 | 24.6 | -5.9 | 0.969/0.961 | 0.015/0.011 | waveform_floor | True |
| 36/g8 | 1.4 | 6.1 | 0.976/0.988 | -0.020/0.229 | structure_floor | True |
| 38/g1 | -0.1 | -0.2 | 0.998/0.982 | 0.985/0.782 | waveform_floor | True |
| 39/g1 | 9.7 | 2.1 | 0.995/0.984 | -0.304/-0.026 | waveform_floor | True |
| 39/g3 | 5.1 | -3.5 | 0.965/0.990 | -0.038/-0.158 | waveform_floor | True |
| 39/g5 | 4.0 | -2.0 | 0.971/0.962 | -0.248/-0.238 | waveform_floor | True |
| 40/g2 | 13.4 | -3.9 | 0.989/0.990 | -0.059/-0.202 | waveform_floor | True |
| 40/g3 | 12.2 | 1.9 | 0.996/0.991 | 0.024/-0.110 | waveform_floor | True |
| 40/g6 | 15.2 | 4.0 | 0.985/0.988 | 0.256/-0.016 | waveform_floor | True |
| 43/g3 | -9.0 | -7.1 | 0.993/0.994 | -0.019/-0.311 | waveform_floor | True |
| 44/g7 | -7.1 | 6.0 | 0.999/0.997 | -0.139/-0.178 | waveform_floor | True |
| 44/g9 | 3.1 | -2.1 | 0.987/0.987 | 0.036/-0.394 | waveform_floor | True |
| 45/g3 | -4.4 | -5.8 | 0.996/0.999 | -0.106/0.177 | waveform_floor | True |
| 46/g2 | 90.3 | 3.9 | 0.981/0.986 | 0.001/-0.018 | waveform_floor | True |
| 46/g6 | 92.2 | -3.9 | 0.990/0.988 | 0.036/-0.009 | waveform_floor | True |
| 46/g7 | 93.2 | 5.7 | 0.993/0.973 | -0.010/0.002 | waveform_floor | True |
| 47/g9 | 172.1 | 3.8 | 0.997/0.964 | 0.046/-0.046 | waveform_floor | True |
| 47/g11 | 177.7 | -2.1 | 0.978/0.982 | 0.020/-0.064 | waveform_floor | True |
| 48/g1 | -36.4 | 0.2 | 0.978/0.977 | 0.183/0.072 | waveform_floor | True |
| 48/g2 | -37.6 | -2.0 | 0.970/0.976 | -0.024/-0.026 | waveform_floor | True |
| 48/g3 | -39.5 | 0.2 | 0.974/0.956 | 0.075/0.025 | waveform_floor | True |
| 48/g12 | -3.2 | 3.8 | 0.967/0.976 | 0.086/0.000 | waveform_floor | True |
| 48/g14 | -9.4 | 0.3 | 0.989/0.991 | 0.046/-0.004 | waveform_floor | True |
| 51/g1 | -183.3 | -5.8 | 0.997/0.994 | -0.020/-0.014 | waveform_floor | True |
