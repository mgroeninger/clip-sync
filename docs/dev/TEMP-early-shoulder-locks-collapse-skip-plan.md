# TEMP — Early shoulder locks on the patch path (collapse → skip, before keep)

**Status:** draft, 2026-08-27. One Change. **Startable.**

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Early shoulder locks / collapse skip*.

**Companions:** [gap-fingerprint.md](gap-fingerprint.md) § *`lag_decision`* / § *`splice_dualfit`*;
Hole 2 [gap-vocabulary.md](gap-vocabulary.md) § *Known holes*;
shipped rescue ordering
[archive/TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md);
pair-sequence skip classifier remains [settled.md](settled.md) § D (not this Change).

**Media hygiene:** pair index + gap index only.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence
disagrees with this one, this one wins.

**Once, early** (after B extract, **before** `evaluate_seam_gate` and **before**
`try_dual_fit`): run **nominal-first rescue on the seam-local templates dual-fit
already uses**. Fine ±600 is `seam_local_peak` on `mono(refined ± fill_seam_search_secs)`.
If that shoulder is dead (`peak_r` < 0.5), coarse envelope at the existing
`--lag-envelope-max-lag-ms` (default 2000, 100 ms bins, interior + r ≥ 0.70);
if usable, another `seam_local_peak` ±600 **recentered on that peak**. Envelope
may lazy-fetch a wider A buffer (same as today’s fingerprint rescue); the fine
template stays the seam window. That produces `b_pre_seam` / `b_post_seam` and
thus `bridge_frames`.

**Keep reads that:** if the locks are **qualified** (`min(pre_r, post_r)` clears
both `min_fill_correlation` and `fill_absolute_floor` — `try_dual_fit`'s
`gate_pass`, same numbers, same NaN polarity) **and** they abut
(`b_post_seam <= b_pre_seam`) → **skip** with reason `shoulders_abut` (payload
below), never search a fill. **Capture** the skip (`gap_index`, A frames, lock
payload) and hold it.

**Unqualified locks decide nothing.** Two dead shoulders produce noise lags, and
noise lags cross about half the time; an unqualified abut would preempt a
legitimate bracket keep on every decorrelated gap. So the abut test is
`gate_pass ∧ collapse`, in that order — exactly today's order inside
`try_dual_fit` (`domain/dual_fit.rs:139` then `:155`), just hoisted. Locks that
fail `gate_pass` fall through to `evaluate_seam_gate` and the run behaves as it
does today, byte for byte.

### Why the gate is not hoisted with it

The alternative — run `evaluate_seam_gate` first and let abut veto a keep — is
wrong on three counts, all readable in `region/characterize.rs`:

- **Geometry.** `dual_fit_input` is deliberately built from the **baseline**
  `refined`, *"before the gate can mutate `refined` via boundary-extension
  retry"* (`characterize.rs:381`). The gate does mutate it —
  `retry_waveform_seam_extensions(&mut refined, …)` and `SeamGateOutcome.refined`
  — so locks measured after the gate would sit on a different `gap_frames` and a
  different `b_post_nominal` than the ones dual-fit uses. Pre-gate locks are the
  same locks; post-gate locks are a new sensor, and §1's byte-parity claim dies
  with them.
- **Cost.** `char_gate_search` is the dominant per-gap cost (see
  [repair-perf.md](repair-perf.md)). Hoisting it makes every abut gap pay the
  bracket search this Change exists to skip.
- **Nothing flows the other way.** The lock scan reads only `a_pcm`,
  `b_samples`, and nominal `b_mapped`; it consumes no gate output. There is no
  ordering constraint forcing the gate first.

Cheap sensor first, expensive search only when the cheap sensor does not decide.

**`try_dual_fit` no longer calls `seam_local_peak`, and no longer computes
`gate_pass`.** Both move into the lock helper (one place, same numbers). It
takes the locks and does the rest: step-real, donor, trim, assemble — with the
collapse test kept only as a caller guard. If the early scan already collapsed,
you never call it.

**After splice** (length-preserving fills are in; A still original clock): loop
the captured list and `visit_shoulders_abut`. This Change’s visit is
record-only.
[TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
warns in that same loop (drain only under `--collapse-introduced-silence`).

Cheap search lives in **one** place. Dual-fit stays “accept and build a fill
from locks we already have.”

**Stamp lookalike tags on the skip; do not split the reason.** `bridge ≤ 0`
is one number — Hole 2 and the lookalikes in §4 share it. This Change still
skips. Clamp, fine-edge pin, and program-quiet are exact; uniqueness and
envelope-at-`−gap` are not. The successor reads the tags before it drains.

One sentence: **characterize computes the seam locks first; keep may skip on
collapse; dual-fit and `splice_dualfit` consume the same locks and do not
search; abut skips are captured (with lookalike tags) and visited after splice.**

---

## 1. Sensor (locked)

The early scan is **seam-local**, with the shipped **nominal-first envelope
ordering**. It is not a 1 s `lag_pair` whose `frac_lag_ms` is then treated as a
seam. Dual-fit and `splice_dualfit` place on 250 ms raw templates; a 1 s peak
and a seam peak are different placements (F14). In-reach dual-fit fills stay
byte-identical because the fine window and template are the ones `try_dual_fit`
already searched.

Shoulders are independent around nominal `b_mapped` (production dual-fit today).
Do not add `lag_pair`’s landmark chain to this helper.

Envelope knobs are the existing repair-config fields (`lag_envelope_max_lag_ms`,
`lag_envelope_window_secs`, `infrastructure/config.rs:193–213`). This Change
reads them on the patch path; they stop being fingerprint-only. Default
2000 / 2.0 is enough for 31/g7’s post (−1160 ms). `0` disables envelope
(fine ±600 only). B extract already includes `fill_border_search_secs`
(default 10 s), so the ±2000 ms haystack is in the decoded window.

Three things followed from those fields being fingerprint-only when this plan was
written; each was a step in §7, not an assumption, and **all three are now done
(step 4, 2026-08-28)**. Kept as written-and-struck so the reasoning stays readable:

- They **were** config-file settable but CLI-unreachable on a repair run:
  `--lag-envelope-max-lag-ms` / `--lag-envelope-window-secs` existed only under
  the `calibration` feature and were rejected without `--gap-fingerprints DIR`.
  ~~This Change does **not** relax that (§6)~~ — **relaxed 2026-08-28, on
  request, in step 4.** Both flags are now on every build and on every run mode;
  only the window's positivity check remains in `validate_fingerprint_flags`, and
  the calibration-gated set is `--gap-fingerprints` / `--fingerprint-gap` /
  `--fingerprint-diagnostics` / `--border-capture` / `--gap-listen`. The exit
  criteria below are still written against config values, which is unaffected —
  a flag is now just a second way to set the same field.
- `RepairConfig::validate` had **no arm** for `lag_envelope_window_secs`; the
  positivity guard lived only in the CLI path a config-file run never reaches, so
  a non-positive value would have silently disabled every rescue once production
  read the field. **Added in step 4** (2026-08-28), alongside the finite check —
  the field joined `validate`'s existing finite tuple, which is what catches NaN
  (`NaN <= 0.0` is false, so the positivity arm alone would let it through).
- Both doc comments said “Fingerprint-only … production repair never reads it.”
  **Rewritten in step 4**; both now say the knob is read by both paths and name
  the shared `EnvelopeCfg::from_knobs`.

---

## 2. Fingerprint readers

| Field | This Change |
|-------|-------------|
| **`splice_dualfit`** | Calls the same domain helper. Production-mirror: locks, `gate_pass`, `bridge_frames`, step-real inputs. 31/g7 becomes rescued post + `bridge_frames` ≤ 0 ⇒ `dual_fit_rescue: false` (that flag already requires `bridge_frames > 0`). |
| **`lag_decision`** | Stays the 1 s census at `b_mapped`. Calls the **same envelope primitive** once it lives in domain. Not the skip predicate. |
| **`shoulder-sweep`** | Unchanged. Independent check; shares no correlation code with the helper. |

Repair and a fingerprint dump are usually different processes. Each invokes the
helper. Same algorithm, two runs. When both run in one process, stamp
`splice_dualfit` from the locks rather than sweep twice.

---

## 3. Call graph

**Today**

```text
characterize
  B extract
  evaluate_seam_gate  → High keep → Patch(Bracket)   [31/g7 stops here]
                      → skip      → try_dual_fit
                                      seam_local_peak ±600 × 2
                                      gate / collapse / assemble
execute / anchored retry / splice / summarize
```

**This Change**

```text
characterize
  B extract
  nominal_first_seam_locks → (pre, post, b_pre_seam, b_post_seam, bridge_frames, gate_pass)
       │
       ├─ gate_pass ∧ abut? → Skip { reason: shoulders_abut, cell: SilenceSplice }
       │            capture ShouldersAbutSkip. No evaluate_seam_gate. No try_dual_fit.
       │
       ├─ otherwise (incl. !gate_pass, incl. locks None) ─┐   // today's path, unchanged
       │                                                  │
       ├─ evaluate_seam_gate → keep → Patch(Bracket) ◄────┘
       │
       └─ skip → try_dual_fit(locks)   // no seam_local_peak
                    gate_pass / collapse / step-real / donor / assemble

execute patches
anchored retry          // may rewrite fills; does not retry abut skips
splice RegionPatches    // length-preserving; A clock unchanged
for skip in captured shoulders_abut:
    visit_shoulders_abut(skip, &a_pcm)   // this Change: record-only
summarize
```

`--no-dual-fit` does **not** skip the early scan. Collapse-before-keep is not a
dual-fit feature. Dual-fit input buffers can stay gated on the flag; the lock
helper runs from `a_pcm` + `b_samples` already in characterize.

`--repair-preview` collects the list onto the summary and does **not** run the
visit (no splice, A unpatched). Write mode and `--patch-only` run the visit.

---

## 4. `shoulders_abut` — skip identity, capture, post-splice visit

Cell: **SilenceSplice** (Hole 2: degenerate splice, zero-length donor bridge).
Not `ProgramQuiet`, not no-placement, not a new cell.

New `GapPatchSkipReason` object (per-gap status). 31/g7:

```json
{
  "shoulders_abut": {
    "pre_lag_ms": -38.0,
    "post_lag_ms": -1160.3,
    "bridge_frames": -48,
    "gap_frames": 53702,
    "pre_z": 14.1,
    "post_z": 8.2,
    "post_envelope_rescue": "rescued",
    "clamped": false,
    "edge_pinned": false,
    "program_quiet": false
  }
}
```

Bools omitted when false is acceptable (`skip_serializing_if`); `pre_z` /
`post_z` are `seam_local_peak`'s already-returned uniqueness; envelope rescue
is stamped only on a shoulder that actually retried. `cell_for_skip_reason` /
`reason_admits_cell` / `skip_cell_from_tags`: this reason is SilenceSplice
only (wildcard-free match arms). Do not add a second variant for lookalikes.

Human line (stdout / `-v` / `--gap-listen` manifest) stays the four geometry
numbers; tags live on the object:

`shoulders abut (bridge=−48 frames, gap=53702, pre=−38.0 ms, post=−1160.3 ms)`

### Lookalikes (stamp, do not split)

Abut means both A edges landed on the same (or overlapping) B instant. Hole 2
is that number *and* the two edges are adjacent program. Four other paths
qualify and collapse. This Change still skips them as `shoulders_abut`. It
does not gate on `peak_z` (settled § B: do not wire `dualfit_confident` into
production dual-fit — stamping `z` on a skip already decided is a different
use). It does not withhold capture. The collapse Change filters drain from
these tags
([TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
§ 1).

| Path | vs Hole 2 | Stamp |
|------|-----------|-------|
| **Clamp** — unclamped `nominal + lag` does not abut; `.max(0)` makes both seams frame 0 | Exact. Near the start of B, not a join. | `clamped` — compare before `.max(0)` |
| **Fine-edge pin** — accepted peak sits on `±max_lag` of the search that produced it (nominal ±600, or the recentered ±600 after rescue). Short gap + silence/period can put locks at opposite corners | Exact. 31/g7 is **not** this: post is dead *inside* ±600, then rescued at −1160, interior to the recentered window. | `edge_pinned` (either shoulder) |
| **Program-quiet** — B silent at nominal `b_mapped` | Exact for the shared-pause leak. Scan `shared_silence` already dropped most; this is `not_evaluated` / flag off. | `program_quiet` from `program_quiet_at_nominal` on the `b_mono` already in characterize — run it at skip, do not wait for `try_dual_fit` |
| **Envelope at ≈ `−gap − pre_lag`** | Same `bridge ≤ 0` as 31/g7. Discriminator is uniqueness of *that* peak, not that envelope ran. | `*_envelope_rescue` + `*_z`. Not a different reason |
| **Same B event** (`post_lag ≈ pre_lag − gap`) | Same number as Hole 2. `step-real` will not separate them. A-pre vs A-post / concat vs B is the successor confirm | `*_z` only. Concat stays in the visit |

> **`edge_pinned` is exact for what it says, not for what it is often read to mean
> (noted 2026-08-28, implementing step 2a).** It fires when the accepted peak sits on the boundary of the
> lags *actually scored* — search exhausted. It does **not** detect "the true peak lies beyond reach":
> out-of-reach content produces a flat noise curve that peaks somewhere in the middle, so the guard reads
> `false`. That is already on the record — pair 50 reads `edge_pinned: false` on all four shoulders with the
> content ~2.8 s away
> ([archive/TEMP-31g7-model-vs-findings.md](archive/TEMP-31g7-model-vs-findings.md) §7.2). So the tag's
> "Exact" rating above is exact **for the pin**, and a `false` on an abut row certifies nothing about reach.
> On the patch path this is narrow — the abut only fires on `gate_pass`, so a flat noise curve is not in
> play — but the successor must not read `!edge_pinned` as "in reach".
>
> **Determination: no new reach detector in this Change (decided 2026-08-28, implementing step 5).**
> The question asked was whether `edge_pinned` should be replaced by a better test of "did the search
> actually reach the true peak". Answer: no, and not because it is hard — because the axis already
> exists and `edge_pinned` is not on it.
>
> - **`edge_pinned` keeps its narrow meaning** — *the accepted peak sits on the boundary of the lags
>   scored*. It is a property of the **search**, exactly measurable from `SeamPeak`'s post-clamp
>   `lag_lo`/`lag_hi` (step 2a), and it answers "would widening the window have changed this reading?"
>   Widening it into a reach verdict would make one field answer two questions and be wrong at both.
> - **Reach is already carried by the pair `(r, z)`**, both on `SeamPeak` and on the skip payload.
>   Low `r` + low `z` is *no opinion* — nothing was found, in reach or out of it. High `r` + low `z` is
>   *periodic* — found in many places, so the lag is not identifying. High `r` + high `z` is a unique
>   registration. A "true peak beyond reach" reading is the first of those, and the honest report of it
>   is the low `r`, not a boolean.
> - **The abut cannot be reached by the ambiguous case anyway.** `abut()` checks `gate_pass` *first*,
>   so both shoulders cleared `min_fill_correlation` and `fill_absolute_floor` before the collapse test
>   is even asked. A flat noise curve — the exact shape "out of reach" produces — never gets to be an
>   abut row. The gap the note above warns about is real for the *fingerprint* corpus, where locks are
>   reported without a gate, and narrow-to-empty here.
> - **A "not found within reach" verdict is a vocabulary change, not a field.** It belongs with the
>   gap-vocabulary redesign's re-rooting on registration, alongside the definitional fix already filed
>   in [archive/TEMP-31g7-model-vs-findings.md](archive/TEMP-31g7-model-vs-findings.md) §7.2. Inventing
>   a second, differently-defined `edge_pinned`-shaped boolean here would give the successor two
>   almost-synonyms to reconcile.
>
> What this Change owes the successor instead: the successor's drain filter must not read
> `!edge_pinned` as "the search was wide enough". It should read the payload's `pre_z`/`post_z` for
> that, and the `edge_pinned` field only as "this row's lags are a floor, not a measurement".

`program_quiet: true` on this reason is still SilenceSplice, not a
`ProgramQuiet` skip. Scan-time `shared_silence` remains the live drop for
shared pauses that never reach characterize. This tag is the leak
(`not_evaluated` / flag off) so the drain visit can refuse the cut.

**Capture** (final dispositions after anchored retry): a `ShouldersAbutSkip` per
abut — `gap_index`, `a_start_frame`, `a_end_frame`, and the lock payload
(lags, bridge, gap, `z`, envelope rescue, `clamped`, `edge_pinned`,
`program_quiet`). Hold that vec through splice. Do not cut, and do not visit,
during characterize or inside the splice loop. `splice_into_a` is
length-preserving, so those A frames still name the hole after neighbouring
fills are in.

**Visit** (after the splice loop in `PatchAudio::run_with_decoded`, A still
original length):

```text
for skip in shoulders_abut {
    visit_shoulders_abut(skip, &mut a_pcm)
}
```

This Change’s `visit_shoulders_abut` is record-only: confirm the destination
frames are still in range, stamp the same vec onto `PatchSummary.shoulders_abut`
(run-level JSON, omitted when empty). `--gap-listen` may also slice that
post-splice A window (neighbours filled, hole still silent) as the review WAV;
pre-splice `_a_surround` stays the before.

The successor
[TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
puts the concat-confirm in that loop body: **warn by default**, drain A
PCM only under `--collapse-introduced-silence` **and** only for rows that
are not `clamped` / `edge_pinned` / `program_quiet`; mux is the current path
(video was not padded). This Change ships the capture, the post-splice
visit, and the summary list. It does not shorten A.

---

## 5. 31/g7 under this plan

A is silent ~1.12 s; B’s match to pre and B’s match to post abut. At the post
seam, ±600 is dead (`peak_r` ~0.12) so envelope consults; the shipped 1 s
census already reports **−1160 ms** with envelope on. Fine retry is the 250 ms
post seam recentered there.

**It clears the §0 qualification with room.** At each shoulder's own lag the
seams read **0.83 / 0.78** ([archive/TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md)
§9.2; post 0.7820 at −1160.25 confirmed by the 2026-08-24 corpus run), against
`min_fill_correlation` 0.35 and `fill_absolute_floor` 0.12. `gate_pass` is true
by ~2×, so the qualification does not cost us the motivating case — it only
excludes the noise-lag gaps (`0.966 / 0.123` at nominal is what the *unrescued*
post reads, and that pair would not have qualified). Those published numbers
are 1 s-census / wide-sweep readings; the checklist's 31/g7 run is what confirms
the same margin on the 250 ms seam template (F14 — different placement).

After this Change: early locks → `gate_pass` ∧ `bridge_frames` ≤ 0 →
`shoulders_abut`, captured. Unified-fit ±10 s never runs. No −977 keep. After
splice, the visit sees the still-silent hole on original-clock A (other gaps
already filled).

---

## 6. Non-goals

- Closing A’s timeline (cut/concatenate). This Change captures and visits;
  [TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
  warns in that visit and drains only under `--collapse-introduced-silence`.
- Settled § D pair staircase / `--steps`.
- Lowering `SPLICE_MIN_PEAK_R`.
- Changing plan / `b_has_energy`.
- Envelope-first, the widen ladder, or dense `seam_local_peak` over ±2000.
- Using 1 s `lag_pair` positions as dual-fit locks.
- New CLI. Envelope knobs already exist as config fields. ~~Relaxing
  `validate_fingerprint_flags` so `--lag-envelope-*` works without
  `--gap-fingerprints DIR` is a separate Change (§1).~~ **Retired 2026-08-28, on
  request: the relaxation was done inside step 4** — no new flag, the two
  existing ones simply stopped being calibration-gated and dump-dir-gated once
  production started reading the same fields. Still a non-goal: any *new* flag.
  Preview does not splice and does not run the visit.
- Unqualified abut. A collapse read off sub-floor locks decides nothing (§0).
- Splitting `shoulders_abut` into lookalike reasons, gating the skip on
  `peak_z`, or withholding capture for clamp / edge-pin / quiet. Stamp
  those on the object (§4); the collapse Change is who refuses the drain.

---

## 7. Change shape

The measurement pieces are already **triplicated** — `build_dual_fit_input` +
`try_dual_fit` (production), `splice_dualfit_at` (fingerprint), and the envelope
inside `rescue_shoulder` — with comments at each site asserting they match
(`probes.rs`: *“Same range guard as `build_dual_fit_input`”*, *“Production
`try_dual_fit` uses the same nominal centers”*). Steps 1–3 collapse that into
one domain module; the rest consume it. The precedent is exact: `lag.rs:70–75`
already re-exports `seam_local_peak` from `domain::seam_local` *“so the
production dual-fit repair (A3) and this diagnostic scan use one
implementation (no drift)”*. Do that again, one layer up.

**1 — `domain::lag_envelope` (move, no behavior change).** Move
`EnvelopePeak`, `EnvelopeCfg`, `envelope_peak_at`, `envelope_shoulder`,
`envelope_recenter_frames` (`measure/lag.rs:488–568`) and `fine_rms_envelope`
(`measure/pcm.rs:262`) into `domain::lag_envelope`. They already depend on
nothing fingerprint-shaped: only `&[f64]` buffers, `sample_rate`,
`lag_correlation_curve` (already domain), and
`DonorRegistrationParams::default().min_envelope_r` (already domain).

`envelope_shoulder` currently takes `LagSideSweep`, but reads only four of its
six fields — give the domain version a narrow `EnvelopeSide { a_border,
b_signal, anchor_frame, pre_shoulder }` and let `lag.rs` build it from its
`LagSideSweep` at the one call site. `gross_lag_shift` /
`border_displacement_frames` stay fingerprint-side.

`LagSummary`, `LagSideSweep`, `rescue_shoulder`, `stamp_rescue`,
`EnvelopeRescue` **stay** in `measure/lag.rs` — they are schema-shaped
(`envelope_*` wire fields). Re-export the moved items from `lag.rs` so existing
paths and tests are untouched.

Also move the `0.5` triage line to a domain const
`LAG_DECISION_RECOVERABLE_R` (named `SEAM_LOCK_RECOVERABLE_R` when this step was
written; **renamed 2026-08-29** once it became clear the seam locks are the one
caller that does *not* use it) and have `is_recoverable(&LagSummary)`
(`lag.rs:275`) read it. That const is the **1 s `lag_decision` census's**
recoverable line and the helper's *default* — **not** a number either seam-lock
caller inherits; see step 2. (As written this said "the fingerprint's" line;
corrected 2026-08-29 — see the note under step 2.)

**The triage threshold is a parameter, not a shared const (corrected
2026-08-28).** Triage (`is this reading trustworthy enough not to need help?`,
0.5) and the gate (`is this good enough to build a fill from?`,
`min_fill_correlation` 0.35 / `fill_absolute_floor` 0.12) are different
questions, and the plan already keeps them separate. The defect is that 0.5 sits
**above** 0.35, opening a `[0.35, 0.5)` band: a shoulder production accepts today
at its nominal lag would be judged "needs rescue" by the new helper, and the
envelope could relocate a lock that currently builds a fill. **Measured on
`2026-08-23-complete-corpus`: 4 gaps have a shoulder in that band and 3 of them
have `gate_pass` today** — 8/g4 (post 0.390), 20/g3 (post 0.443), 25/g11 (pre
0.410). Tier 1.5's five movers did not include them (that query required a
`lag_decision` rescue; these three were never asked), so this is a **fourth,
separate** way step 4 could change production bytes.

So `nominal_first_seam_locks` takes the triage threshold as an argument:

- **fingerprint** passes `LAG_DECISION_RECOVERABLE_R` (0.5) — `lag_decision` and
  `is_recoverable` keep today's behavior exactly;
  **⚠ superseded — see the correction below;**
- **patch path** passes `min_fill_correlation`, which makes the band empty *by
  construction* and buys a much stronger invariant than §1 currently claims:
  **a lock that passes the gate at its nominal lag is never relocated.** The
  envelope is consulted only on shoulders that would fail the gate anyway, so
  rescue can only ever *add* a fill that is declined today — it can never move or
  alter an existing one. Byte-parity by construction, not by corpus luck.

Retry acceptance uses the same passed threshold, symmetrically: keep the
recentered lock only if it now clears it. For production that reads "rescue only
if the rescued lock would actually pass the gate."

31/g7 (post 0.123) and 31/g9 (0.143 / 0.128) are far below 0.35, so Tier 1.5's
findings and §5 are unaffected by this correction.

**2 — `domain::seam_locks` (the new shared unit).**

```text
SeamLockTemplates::build(a_samples, channels, refined, b_mono, b_mapped_start,
                         seam_window_frames) -> Option<SeamLockTemplates>
```

carries the range guard (`refined.start_frame < w || refined.end_frame + w >
a_frames || gap_frames == 0`) and the raw `mono(refined ± w)` templates —
today duplicated verbatim at `dual_fit.rs:88` and `probes.rs:267`. Both call
sites become this constructor. **Not** `border_templates_for_gap` (silence-skip
+ energy trim moves the pre template — `probes.rs` documents why).

> **Both callers pass `min_fill_correlation` (corrected 2026-08-29).** The split above
> — fingerprint at 0.5, patch path at the gate floor — is **not what was built**. `probes.rs`
> passes `cfg.min_fill_correlation` and `characterize.rs` passes `df.params.min_fill_correlation`;
> the fingerprint was moved onto production's line during step 6 so the dump would predict the
> repair (recorded in the step-6 "as built" note), and this design text was never updated to match.
> It is corrected here because reading it the other way makes Tier 1 look like an over-estimate of
> the production abut set on the triage axis, which it is not — the two paths triage identically.
>
> `triage_r` nonetheless stays an **argument**, and for a stronger reason than the one given above:
> the correct value is not a literal. `min_fill_correlation` is a recipe field, so hardcoding `0.35`
> would hold the invariant *a lock that passes the gate at its nominal lag is never relocated* only
> at the default recipe. Tying triage to whatever the gate floor is makes it hold at every recipe.
> `LAG_DECISION_RECOVERABLE_R` keeps exactly one live consumer — `is_recoverable` on the 1 s census,
> a different instrument — plus the unit test that pins the parameter as a parameter.

```text
nominal_first_seam_locks(&templates, max_lag_frames, envelope: Option<EnvelopeCfg>,
                         wide_a: Option<&WideA>, floors, triage_r) -> Option<SeamLocks>
```

Per shoulder: `seam_local_peak` at nominal → if `peak_r < triage_r` and envelope
on, coarse envelope → `seam_local_peak` recentered → keep the retry only if it
now clears `triage_r`, else the honest nominal. `triage_r` is
`min_fill_correlation` on **both** paths (step 1, as corrected). Returns pre/post `(r, lag_frames, z)`, `b_pre_seam`, `b_post_seam`,
`bridge_frames`, and `gate_pass` (`min(pre_r, post_r)` vs both floors).

> **The NaN polarity this plan asked for does not exist (corrected 2026-08-28,
> during implementation).** The plan specified `!(smin < floor)` "so NaN passes",
> copying the shipped `try_dual_fit` / `probes.rs` form. But `seam_local_peak`
> returns `finite(peak_r)` — **NaN → 0.0** — so `smin` is finite by construction
> and a digital-silence shoulder arrives as `0.0`, which *fails* the gate. The
> two forms are equivalent on finite input; `seam_locks` uses the plain
> `smin >= floor`. Two shipped comments asserted the opposite and are now
> corrected in place: `probes.rs`'s "Keep raw correlations — including NaN …
> gate_pass would fail where `NaN < floor` does not", and F14's
> "(NaN-aware: matches production's `smin < floor` form)" in `schema.rs`.
> **The NaN caution on `post_seam_global_r` is real and unchanged** — that value
> comes straight from `normalized_correlation`, which does not clamp. No behavior
> changed; this was wrong reasoning reaching a right answer.
`abut()` is `gate_pass && b_post_seam <= b_pre_seam`. Also carry, for the
skip payload: `edge_pinned` (accepted peak on `±max_lag` of the producing
search), whether each accepted lock came from envelope retry, and unclamped
`nominal + lag` so characterize can set `clamped`. `None` only when a seam
window is out of range.

> **Step 2 shipped without two thirds of that payload (recorded 2026-08-28).**
> The refactor half is complete and verified; of the payload half, only
> `rescued: bool` exists. `edge_pinned` was **deferred** because it is not
> readable at the call site (step 2a), and the unclamped `nominal + lag` values
> are computed and thrown away at `seam_locks.rs:305–306`. The step 1–3 tripwire
> could not catch either — it is a parity check, and these fields have no
> consumer until step 5. Both are prerequisites for step 5's *payload*, not for
> its abut *decision*; the successor collapse Change refuses without them
> (it drains A's PCM only for rows that are not `clamped` / `edge_pinned` /
> `program_quiet`), so they are not optional.

`wide_a`: on the patch path A is fully decoded in `a_pcm`, so the wider
envelope window is a plain slice — no lazy-fetch closure needed. The parameter
exists for the fingerprint’s `wide_a_border` closure, which cannot slice.

**`SeamLockTemplates` enforces its own invariant (added during implementation).**
`a_pre.len() == a_post.len() == seam_window_frames`, `gap_frames > 0`, checked in
the only two constructors (`build` from interleaved A, `from_mono` for callers
that already cut the borders); the fields are private with `pub` getters. Without
this the struct is a live trap: the old `try_dual_fit` sliced `&a_pre[len-w..]`
and `&a_post[..w]` from possibly-longer inputs, whereas the locks now measure
whatever length they are handed — so an over-long template would have been
measured at one length and spliced at another. `try_dual_fit` also now takes
`w`/`gap_frames` **from the templates, not from `DualFitParams`**, and
`debug_assert`s the two agree.

> **Step 4 as built (2026-08-28).** `ENVELOPE_BIN_MS` + `EnvelopeCfg::geometry` /
> `EnvelopeCfg::from_knobs` live in `domain::lag_envelope`;
> `measure/pcm.rs`'s `WIDE_ENV_BIN_MS` is now a re-export of `ENVELOPE_BIN_MS`, so
> the rescue path and the Tier-3 `wide_envelope` diagnostic cannot drift apart on
> the bin. `measure/lag.rs` builds `envelope_cfg` from `from_knobs` and takes
> `env_window`/`env_lag` from `geometry` (the second, duplicated `EnvelopeCfg { … }`
> literal is deleted). `PatchRequestSettings` carries both knobs, filled from
> `patch_settings()`; the two "Fingerprint-only / production repair never reads it"
> doc comments are rewritten; `validate` gained the positivity arm (the finite check
> came from adding the field to the existing finite tuple). Tests:
> `rejects_non_positive_lag_envelope_window`, `patch_settings_carries_lag_envelope_knobs`.
> **Nothing reads the new settings fields yet** — that is step 5.
>
> **CLI gating relaxed (2026-08-28, on request — §6's non-goal is retired).** Both
> `--lag-envelope-*` flags lost their `#[cfg(feature = "calibration")]` and their
> "requires `--gap-fingerprints DIR`" rejection; `apply_cli_overrides` applies them
> unconditionally. `validate_fingerprint_flags` keeps only the window's
> positive-and-finite check, hoisted above the calibration block (it used to sit
> inside, ahead of the dump-dir early return). Tests
> `lag_envelope_max_lag_ms_without_dump_dir_is_rejected` and
> `lag_envelope_window_secs_without_dump_dir_is_rejected` are deleted and replaced by
> `lag_envelope_flags_apply_without_a_dump_dir`, which is itself ungated;
> `non_positive_lag_envelope_window_secs_is_rejected` is ungated too and no longer
> passes a dump dir. `--border-capture` stays gated — production does not read it.
> `docs/dev/development.md`'s feature table is corrected. Verified: lib suite green
> both with `--features calibration,he-aac` (718) and with no features (686), clippy
> clean on both.

**2a — widen `seam_local_peak` (own change, no behavior change).** Step 2's
`edge_pinned` is not free. `seam_local_peak` (`domain/seam_local.rs:183`) returns
`Option<(f64, i64, Option<f64>)>` and **clamps its own reach**
(`max_lag.min(anchor_start).min(b_mono.len() - (anchor_start + n))`), so the
caller cannot tell where the search actually ended — and a naive
`lag.abs() == max_lag` test is wrong precisely on the B-edge-proximate gaps this
plan cares about.

Return a named `SeamPeak { r, lag, z, lag_lo, lag_hi }` rather than a wider
tuple: `characterize.rs:651` annotates a closure parameter as
`Option<(f64, i64, Option<f64>)>`, and a 5-tuple there is unreadable. `lag_lo` /
`lag_hi` are the **post-clamp** bounds, read the way `measure/lag.rs:133–135`
reads them off the curve's first/last entries. Add
`SeamPeak::edge_pinned(sample_rate)` using `lag.rs:130–136`'s exact definition,
and promote `LAG_EDGE_TOL_MS` (`lag.rs:15`, today
`pub(in crate::application::gap_fingerprint::measure)`) into
`domain::seam_local`, re-exported from `lag.rs` — the same move step 1 made for
`LAG_DECISION_RECOVERABLE_R`, and for the same reason: one definition of "pinned",
not two.

Six call sites: `probes.rs:292`, `:294`, `characterize.rs:637`, `:644` (plus the
`:651` annotation), `seam_locks.rs:215`, `:250`. Tests: peak at the search edge
→ pinned; interior peak → not; **a search whose `max_lag` was clamped by B-edge
proximity reports the clamped bound**. Verification is that every existing
number is unchanged.

Then finish step 2's payload: `SeamLock.edge_pinned` (from the *accepted*
search — the recentered one when a rescue fired, not the nominal probe), and
`SeamLocks.b_pre_seam_unclamped` / `b_post_seam_unclamped` with
`clamped()` = "unclamped seams do not abut but clamped ones do", plus the two
§8.3 unit tests. Nothing reads these yet, so again no existing value moves.

### Step 6 is not a drop-in — three gaps found while implementing 1–3

Reviewed `nominal_first_seam_locks` against the shipped `rescue_shoulder`
(`measure/lag.rs:309`) it is meant to replace. Steps 1–3 do not touch the
fingerprint, so none of these is a live defect — but step 6 cannot be a
mechanical swap, and sizing them now is cheaper than discovering them mid-step.

1. **`SeamLock` cannot carry the fingerprint's wire fields.** `rescue_shoulder`
   stamps five diagnostics onto its `LagSummary` — `envelope_rescue`
   (`NotConsulted` / `Declined` / `Rescued`), `envelope_window_ms`,
   `envelope_lag_ms`, `envelope_peak_r`, `envelope_edge_pinned` — all of them
   serialized. `SeamLock` has a single `rescued: bool`, which collapses
   `NotConsulted` and `Declined` into the same value and drops the other four.
   Step 6 must widen `SeamLock` (returning the `EnvelopePeak` and a three-state
   outcome), or every corpus dump loses those columns.
2. **The `gross_lag_shift` mismatch is real but benign.** `rescue_shoulder`
   returns the retry's lag *relative to the recentered anchor* and adds
   `env_frames` to `gross_lag_shift`, leaving the consumer to sum them;
   `lock_shoulder` folds the shift in (`lag = shift + lag2`) so `b_*_seam`
   arithmetic is placement-independent. Same B frame either way, different field
   split — the adapter must not add the shift twice.
3. **`envelope_window_ms` is a clamped, recorded value.** `rescue_shoulder`
   records `env_cfg.window.min(a_border.len())` because `envelope_shoulder`
   clamps internally and the recipe cannot express the adjustment.
   `lock_shoulder` performs the same clamp (inside `envelope_shoulder`) but does
   not surface it. Folded into gap 1.

Also confirmed clean: the recenter arithmetic itself (`anchor + shift`, clamped
at 0), the retry-acceptance predicate (`r2 >= triage_r` ≡ `is_recoverable`), and
`envelope_shoulder`'s `anchor_frame` convention — it wants the **seam point**
(end of the pre window), which is what `b_mapped_start` is.

**3 — `try_dual_fit` consumes locks.** Delete its two `seam_local_peak` calls
and its `gate_pass` / collapse blocks (`dual_fit.rs:106–160`); take `SeamLocks`
instead. Keep the collapse test as a `debug_assert`-grade guard — the caller
should never invoke it on an abut. Its remaining body (step-real, donor,
trim, assemble) is unchanged, so in-reach fills stay byte-identical.

**Where the two lock-level rejections went (implementation detail, 2026-08-28).**
They are spelled out in exactly one place, `dual_fit::dual_fit_at_nominal`
(locks → `gate_pass` → `bridge_frames > 0` → `try_dual_fit`), which the
production caller and every fixture go through; `try_dual_fit` re-asserts both
as `debug_assert`s. Step 4 replaces the production caller's use of that wrapper
with the early, pre-gate-search form — at which point the wrapper's remaining
users are the fixtures. Not two copies of the predicate, one.
`build_dual_fit_input` keeps building `DualFitRepairInput` but sources its
templates from `SeamLockTemplates`.

**4 — Plumb the two knobs to production.** *(Done 2026-08-28.)* Add
`lag_envelope_max_lag_ms: u32` and `lag_envelope_window_secs: f64` to
`PatchRequestSettings` (`patch_audio/request.rs:57`), populated in
`RepairConfig::patch_settings()` (`config.rs:763`) alongside
`normalize_window_secs`. Read in `characterize_region` as
`request.lag_envelope_max_lag_ms` via the existing `Deref` — no new parameter
threading, no second config path. Add the missing `RepairConfig::validate` arm
for `lag_envelope_window_secs > 0 && finite` (§1), mirroring the CLI's message,
and rewrite both “Fingerprint-only / production repair never reads it” doc
comments (`config.rs:193–213`).

The `EnvelopeCfg` construction (`env_on` / `env_window` / `env_lag` /
`env_bin = 100 ms`, `lag.rs:592–595`) moves to a domain
`EnvelopeCfg::from_knobs(max_lag_ms, window_secs, sample_rate) -> Option<_>`
returning `None` when `max_lag_ms == 0`. Both callers use it; the 100 ms bin
stays a single const.

**5 — `characterize_region`.** After B extract and before
`char_gate_search`, build `SeamLockTemplates` + `nominal_first_seam_locks` in
a `char_seam_locks` span (Level-B parity with the neighbouring spans). If
`locks.abut()` → `shoulders_abut` skip via `skip_region_spec`, stamp the §4
tags (`clamped` from unclamped seams, `edge_pinned` from the producing
search, `program_quiet_at_nominal` on `b_mono` already in hand), capture,
return — no `evaluate_seam_gate`, no `try_dual_fit`. Otherwise carry the locks into
`skip_or_dual_fit` → `try_dual_fit`. The locks are computed from the **baseline**
`refined`, in the same place and for the same reason `dual_fit_input` already is
(`characterize.rs:381`) — the gate may mutate `refined` afterwards.

Under `--no-dual-fit` the lock scan still runs (§3); only
`build_dual_fit_input`’s heavier buffers stay flag-gated. Three of them are
**not** heavy and must hoist out of the `request.dual_fit.then(…)` gate at
`characterize.rs:383` — `templates`, `b_mono`, `a_gap_floor_db` — because the
locks, the `clamped` tag, and `program_quiet_at_nominal` all need them on every
run. Consequence to record rather than discover: a `--no-dual-fit` run newly
pays one B mono conversion plus two ±600 ms searches per gap. Narrow, since
`dual_fit` defaults to `true`.

**Steps 5 and 7 are one compile unit (2026-08-28).** They cannot land
separately: step 5's `skip_region_spec` call needs
`GapPatchSkipReason::ShouldersAbut` to exist, and the moment it exists the four
wildcard-free matches (§8.2) refuse to compile without their arms. Do them as
one change; the checklist keeps them as two entries only because the
verification differs.

> **Steps 5 + 7 as built (2026-08-28).** Landed together as planned, and the wiring
> came out slightly wider than §7 sketched:
>
> - **`dual_fit_input` is now built unconditionally** (the `request.dual_fit.then(…)` gate at
>   `characterize.rs:381` is gone entirely, not just three fields hoisted out of it). Splitting the
>   struct would have meant two constructors and two range guards for one set of buffers; consumption
>   is still flag-gated at `dual_fit_eligible`, and `BExtractWindow` is not `Serialize`, so a
>   `--no-dual-fit` run's newly-populated `b_mapped_start_frame` is internal-only. The recorded cost
>   on that path is unchanged: one B mono conversion plus two ±600 ms searches per gap.
> - **`domain::dual_fit::dual_fit_from_locks`** is new — the two lock-level rejections plus
>   `try_dual_fit`, taking locks the caller already measured. `dual_fit_at_nominal` is now
>   `nominal_seam_locks` + a delegation to it, so the rejections are still written once, and
>   `skip_or_dual_fit` takes `locks: Option<&SeamLocks>` and passes characterize's own locks through.
>   Without this the gap would have paid the per-shoulder search twice and the skip decision and the
>   fit could have disagreed about a lag.
> - **The envelope is live on the patch path from this step**, via
>   `EnvelopeCfg::from_knobs(request.lag_envelope_max_lag_ms, …)` — default 2000 ms, i.e. **on**. The
>   `wide_a` closure slices mono A out of `a_pcm` around `refined` (the doc comment on `WideA` said the
>   patch path would pass `None`; that would have handed the envelope a 250 ms border and a quietly
>   narrowed search, so it passes a slicing closure instead). `triage_r` is `min_fill_correlation`, so
>   §7's invariant holds: a lock that passes the gate at nominal is never relocated. This is the
>   Tier 1.5 blast radius, and it is now real rather than pending.
> - **Two more wildcard-free matches than §8.2 listed** — `clip-sync-repair-harness`'s
>   `energy_matrix.rs:146` and `residual_gate.rs:92` label formatters. Both refused to compile without
>   an arm; both got one. §8.2's count is corrected below.
> - **The §9 byte-parity invariant is now a test, not an argument** —
>   `a_gate_passing_nominal_lock_is_never_relocated`. Turning the envelope on in
>   production without it would have left the whole safety case resting on reading the
>   `triage_r` plumbing correctly.
> - **Tests**: `tests/abut_skip_integration.rs` (new target), three cases on a synthetic
>   introduced-silence noise pair — abut skips with a non-positive bridge and a post lag of
>   `≈ −gap`; the skip survives `--no-dual-fit`; and the contrast fixture (silence *replacing* B
>   content) is not abut and still patches through the ordinary gate. Plus
>   `cell_for_skip_reason_is_exhaustive_and_correct`'s eighth case and
>   `is_retryable_patch_skip_excludes_shoulders_abut`.

**6 — `splice_dualfit_at`.** Replace its template build + two
`seam_local_peak`s with `SeamLockTemplates` + `nominal_first_seam_locks`;
keep the prominence measurements (`pre_seam_prom` / `post_seam_prom`) and
`post_seam_global_r`, which are fingerprint-only. `bridge_frames`,
`trim_frames`, `gate_pass` come from the locks. `lag_decision` keeps its 1 s
census and now calls the moved envelope primitive.

> **Step 6 as built (2026-08-28).** The swap landed, and the three gaps sized in advance came out
> smaller than feared — because `lag_decision` keeps `rescue_shoulder`.
>
> - **`SeamLock` did not need widening.** Gap 1 ("`SeamLock` cannot carry the fingerprint's wire
>   fields") was sized against replacing `rescue_shoulder`, which stamps the five `envelope_*` columns
>   onto `LagSummary`. Step 6 does not touch it — the 1 s census is unchanged, so **no existing column
>   moves or disappears**. Gaps 2 (`gross_lag_shift` split) and 3 (clamped `envelope_window_ms`) are
>   properties of that same adapter and likewise never arose.
> - **What `splice_dualfit_at` lost**: its own range guard, its own `interleaved_to_mono` border cuts,
>   both `seam_local_peak` calls, its own `b_*_seam` arithmetic and its own restatement of the gate.
>   All of it is now `SeamLockTemplates::build` + `nominal_first_seam_locks` — production's call, with
>   `triage_r = min_fill_correlation` (production's line), not the census's `LAG_DECISION_RECOVERABLE_R`.
>   Kept, because production computes neither: `post_seam_global_r` and the two `*_seam_prom`.
> - **The envelope is on here too**, from `cfg.lag_envelope_*` (default 2000 ms / 2.0 s). This is the
>   fix the divergence demanded: a dump previously read 31/g7's post shoulder dead at ±600 ms and wrote
>   `gate_pass: false` while production rescued it, gated it, and skipped the gap as `shoulders_abut`.
> - **`wide_a_slicer` is now shared** (`domain::seam_locks`). Both callers decode A whole, so both
>   widen by slicing; written once because the two paths are compared axis-by-axis and a fingerprint
>   that widened differently would mispredict exactly the shoulders the envelope exists to rescue.
>   `WideA`'s doc comment — still claiming the patch path passes `None` — is corrected with it.
> - **Five new `SpliceDualfit` columns**, all `Option`, all `skip_serializing_if`:
>   `pre_seam_rescued` / `post_seam_rescued`, `pre_seam_edge_pinned` / `post_seam_edge_pinned`,
>   `seams_clamped`. Together they let a dump reproduce production's `ShouldersAbut` payload, which is
>   what the successor collapse census will read. `None` means **"this dump cannot say"** — pre-2026-08-28
>   dumps, and the projection path, which reconstructs from `SeamLocalTags` (accepted lags, no
>   provenance). Deliberately not `Some(false)`: that would let a reader conclude "nothing was rescued"
>   from a dump that never ran a search.
> - **Test**: `splice_dualfit_rescues_a_far_post_shoulder_and_reads_the_abut` — introduced silence with a
>   −1200 ms post shoulder, asserting both arms. Envelope on ⇒ `post_seam_rescued: Some(true)`,
>   `gate_pass`, `bridge_frames ≈ 0`, `seams_clamped: Some(false)`. Envelope off ⇒ `post_seam_r < 0.35`,
>   `!gate_pass` — the old reading, pinned as *envelope-off* rather than left able to come back. The
>   third assertion is the invariant: `pre_seam_r` is bit-identical across the two arms.

**7 — Wire the reason.** `GapPatchSkipReason::ShouldersAbut { … }`
(`domain/patch_result.rs:127`) carries lags, bridge, gap, and the §4
lookalike tags (`pre_z` / `post_z`, envelope rescue, `clamped`,
`edge_pinned`, `program_quiet`). Formatters, `cell_for_skip_reason` /
`reason_admits_cell` / `skip_cell_from_tags`, JSON contract
([json-output.md](../json-output.md), [gap-repair-guide.md](../gap-repair-guide.md)),
Hole 2 detection sentence in [gap-vocabulary.md](gap-vocabulary.md).
Human line stays the four geometry numbers.
Add `ShouldersAbut => false` to `is_retryable_patch_skip`
(`anchor_retry.rs:88` selects retries by that predicate) — otherwise anchored
retry re-characterizes every abut gap and re-abuts it, which is §3’s “does not
retry abut skips” by luck rather than by construction.

**8 — Capture + visit.** Collect `Vec<ShouldersAbutSkip>` from the final skip
specs (after the anchored-retry pass at `patch_audio/mod.rs:463`, where
`region_results` settles). **Source the A frames from those specs, not from the
outcomes** — `GapPatchOutcome` (`domain/patch_result.rs:306–322`) carries
`a_start_secs` / `a_end_secs` in *seconds* and no gap index, so §4's
`a_start_frame` / `a_end_frame` cannot be read off it without a lossy round-trip
through the sample rate. The visit goes between the `patch_splice` loop
(closing ~`mod.rs:556`) and `PatchSummary::from_outcomes` (`:560`), where `a_pcm`
is still owned and mutable; the summary field arrives via a
`.with_shoulders_abut(…)` builder alongside the existing `.with_fill_geometry`
/ `.with_program_gain`. After
the splice loop in `run_with_decoded`, call `visit_shoulders_abut` for each.
Stamp `PatchSummary.shoulders_abut`. Preview stamps the list, skips the visit.


> **Step 8 as built (2026-08-28).** Capture, hold, visit, stamp — and one refactor the plan did not
> ask for but the capture forced.
>
> - **`GapPatchSkipReason::ShouldersAbut` became a newtype** over a named `ShouldersAbutLocks`.
>   The run-level row republishes the identical payload, and two copies of eleven fields would have
>   drifted the moment the successor added one. The wire shape is unchanged — a newtype variant still
>   serializes as `{"shoulders_abut": {…}}` — so no golden moved. Six match sites and three literals
>   were rewritten (`cell_for_skip_reason`, `reason_admits_cell`, `skip_cell_from_tags`, the tags
>   tier/seam arm, and both harness formatters).
> - **`ShouldersAbutSkip`** = `gap_index` + `a_start_frame` / `a_end_frame` + `#[serde(flatten)]`
>   locks. Frames come from `spec.refined` via `shoulders_abut_row(&spec, region.gap_index)`, called
>   in **both** the preview loop and the write loop before the spec is consumed — never from
>   `GapPatchOutcome`, which carries seconds and no gap identity.
> - **Settled, not pass-1.** After the anchored-retry pass the vec is `retain`ed against
>   `region_results`, keeping only rows whose final outcome is still an abut skip. Today that
>   discards nothing (`is_retryable_patch_skip` excludes `ShouldersAbut`, and
>   `should_apply_anchored_retry_outcome` never installs a new skip) — both are decisions that could
>   be revisited, and the list must describe the run, not pass 1.
> - **`visit_shoulders_abut`** runs between the splice loop and `PatchSummary::from_outcomes`, once
>   per row, record-only: it confirms `a_start_frame <= a_end_frame <= a_pcm.frames()` and warns
>   otherwise. It takes A by **shared** reference, because nothing here writes; the successor widens
>   the signature when it drains. A row that fails the range check is still reported — dropping the
>   evidence of a geometry defect is the wrong repair for it.
> - **`PatchSummary::shoulders_abut`** (`Option<Vec<_>>`, `skip_serializing_if`) via
>   `with_shoulders_abut`, alongside `with_fill_geometry` / `with_program_gain`. Empty ⇒ `None`, so a
>   run without an abut is byte-identical to one written before the field existed. Preview stamps the
>   list and runs no visit (nothing spliced).
> - **Tests**: `abut_capture_survives_splice_and_reaches_the_summary` — a two-gap fixture (a real
>   dropout at 5 s that the gate fills, an inserted 400 ms silence at 13 s that it refuses), asserting
>   the row's identity and frames, that the refused span is *still silent* after the splice, that A was
>   not shortened, and that the neighbouring fill still happened. `preview_reports_the_abut_without_pcm`
>   pins the preview half. The length assert allows ±1 frame — the WAV decoder's edge, three orders of
>   magnitude below the ~17 640-frame drain it is guarding against.
> - `run_patch_preview` was added to the harness beside `run_patch`.

**9 — Close out.** Docs first: the skip reason's row in
[json-output.md](../json-output.md) and
[gap-repair-guide.md](../gap-repair-guide.md), the run-level
`patch.shoulders_abut` shape, and Hole 2's detection sentence in
[gap-vocabulary.md](gap-vocabulary.md) (§7 step 7 lists these; they are not
optional trailing work). Then the three-step BACKLOG move: (1) mark the
[BACKLOG.md](../../BACKLOG.md) entry done, (2) file the verdicts this plan
settled — the `[0.35, 0.5)` band correction, the refuted NaN polarity, Tier 0's
n=1 — into [settled.md](settled.md) under their letters, (3) **delete the
BACKLOG text**. Step 3 is the one that gets skipped. Finally delete this
`TEMP-` document or archive it under `docs/dev/archive/`, per whichever the
successor collapse plan still needs to cite.

> **Step 9 as built (2026-08-28), docs half.** [json-output.md](../json-output.md) gained the
> `{"shoulders_abut": {…}}` skip-reason form with its eleven keys, two paragraphs on what the refusal
> means (`bridge_frames <= 0` is the decision, it costs no gate search, it does not depend on
> `--dual-fit`, cell `silence_splice`) and on the lookalike tags — including the `edge_pinned` caveat
> that it means *the peak sat on a searched boundary*, not *the true peak is out of reach*, for which
> the reader is pointed at `pre_z` / `post_z`; a `PatchSummary` row; a **ShouldersAbutSkip** section
> saying why it is frames and not seconds, and that nothing acts on it yet; and the v1
> additive-revisions line. [gap-repair-guide.md](../gap-repair-guide.md) lists `shoulders_abut` in the
> `patch_skip_reason` tag row, and [cli-output.md](../cli-output.md) carries the human skip string.
> [gap-vocabulary.md](gap-vocabulary.md) Hole 2's detection sentence is replaced and the superseded
> sentence quoted in place, marked true-when-written — a reader who acts on it would hunt for a
> detector that now exists.
>
> Three verdicts filed in [settled.md](settled.md): the NaN polarity under **A** (there is no NaN —
> `seam_local_peak` clamps to finite, so a dead shoulder arrives as 0.0; the caution on
> `post_seam_global_r` is a different value and still real), and under **E** both the `[0.35, 0.5)`
> band (why `triage_r` is an argument and must not be collapsed back to one const) and Tier 0's n=1
> (a `lag_decision` reconstruction is a bounded candidate list, not a census — 1 s template at
> `b_mapped` vs production's 250 ms at the seam).
>
> **Left open deliberately**: the BACKLOG close and this document's deletion. Both assert validation
> that has not run — §8.1's Tier 1 re-fingerprint, 31/g7's production confirmation, and the 31/g9 ear
> check all need licensed media.
>
> **Status 2026-08-29.** Tier 1 has since **run** (n = 2, `{11/g9, 31/g7}`) and the provenance the
> dump was silently dropping is now on the wire; both are recorded in §8.1. What still gates the
> close is **Tier 2** — its outcome differ is unbuilt, the two-arm run has not happened, and two ear
> checks remain (31/g7's *current* fill, which this Change removes, and 31/g9's new one). The
> `[0.35, 0.5)` verdict filed under **E** was itself corrected on 2026-08-29: both callers pass
> `min_fill_correlation`, there is no fingerprint/patch divergence, and the row now turns on the
> recipe-field rationale instead. `SEAM_LOCK_RECOVERABLE_R` was renamed
> `LAG_DECISION_RECOVERABLE_R` the same day — the seam locks are the one caller that does not use
> it, and the old prefix is how the wrong belief spread.

---

## 8. Verification and checklist

### 8.1 Corpus verification (three tiers)

**Tier 0 — offline, before writing any code. MEASURED 2026-08-28: n=1.**
Reconstruct the bridge from **`lag_decision`**, the only field on the corpus wire
whose search can reach a far post shoulder — it runs the shipped nominal-first
ordering at the 2000 ms envelope, so a rescued lock is recorded in place with
`envelope_rescue: "rescued"`. Per gap:

```text
bridge_ms  = gap_ms + post_anchor.frac_lag_ms − pre_anchor.frac_lag_ms
candidate ⇔ min(pre_anchor.peak_r, post_anchor.peak_r) ≥ min_fill_correlation
             ∧ bridge_ms ≤ 0
```

Over `gap-files/2026-08-23-complete-corpus` (51 pairs / 1073 gaps) — **971
evaluated, exactly one candidate**; 102 gaps carry no `lag_decision` or only one
shoulder:

| pair/gap | gap_ms | `bridge_ms` | pre_r | post_r | post rescue |
|---|---|---|---|---|---|
| **31/g7** | 1118.8 | **−3.4** | 0.829 | 0.782 | `rescued` (envelope −1200 ms, env r 0.944) |

**The `-3000` sibling arm returns the same single candidate** (971 evaluated,
1 hit). So pair 50's +2.8 s cluster rescuing at the wider radius adds **nothing**
— and structurally never could: `bridge = gap + step` with `gap > 0`, so a
**positive** map step can never reach `≤ 0` at any duration
([archive/TEMP-collapsed-bridge-census-2026-08-20.md](archive/TEMP-collapsed-bridge-census-2026-08-20.md)
Result 7). This retires the earlier "expect 1, possibly 2–3 once pair 50
rescues" guess: **it is 1, at both envelope radii, by measurement.**

That also reproduces the collapsed-bridge census's n=1 from committed dump bytes
with no media run, on the same 971-gap denominator that census used.

**Do not run this split on `splice_dualfit.gate_pass ∧ bridge_frames ≤ 0`.** An
earlier draft of this section did, and called the census "exact precedent" for
it. It is neither. The census never reads those fields — `splice_dualfit`,
`gate_pass` and `bridge_frames` appear nowhere in it; it found 31/g7 with the
±2000 ms wide sweep plus `lag_decision`. And the predicate is **structurally
blind** to this class: `splice_dualfit` searches ±600 ms with **no** envelope
rescue, so 31/g7's post lock at −1160 ms is out of window and the dump records
the nominal noise reading instead — `post_seam_r 0.123`, `gate_pass false`,
`bridge_frames` **+57375** (byte-identical in the 08-19, 08-23 and 08-23-3000
dumps). That split returns **0** corpus-wide and would return 0 however many
abutting gaps existed, because an out-of-reach lock is the *defining property* of
the class. Not a lower bound — a blind spot shaped like the thing being counted.

> **This paragraph is about the committed dumps, and stays true of them — but step 6
> (2026-08-28) removed the cause.** `splice_dualfit_at` now calls
> `nominal_first_seam_locks` with the envelope on, so from that commit forward the probe
> reaches a −1160 ms lock and its `gate_pass` / `bridge_frames` agree with production's
> `shoulders_abut` decision. The `gate_pass ∧ bridge_frames ≤ 0` split is therefore
> *not* blind on dumps written after step 6 — it is the natural Tier 1 roll-up. Every
> number quoted above (`post_seam_r 0.123`, `bridge_frames +57375`) is a **pre-step-6
> byte**, and re-running the corpus will move it; that re-run is Tier 1, and until it
> exists these remain the durable record.

**What Tier 0 is, and is not.** It shares production's *ordering* (nominal fine
→ envelope → recentered fine) but not its *placement*: `lag_decision` is a 1 s
template at `b_mapped`; `nominal_first_seam_locks` is a **250 ms** template at
the seam (F14). A 1 s window straddling a seam can lock where a 250 ms one will
not, and the reverse. So Tier 0 delivers a **bounded candidate list and a
falsifier before any code is written** — not the ship set.

There is **no candidate queue today**: Tier 0's only hit, 31/g7, was itself
established by `shoulder-sweep` (`cargo run --features calibration --bin
shoulder-sweep`) — the −1160.3 ms / r 0.782 lock, and the reclassification of
31/g8 *out* of this population as `shared_far_offset`. It is already confirmed by
the instrument, so nothing is pending arbitration.

The instrument's remaining job is **prospective and narrow**: if Tier 1 surfaces
a member Tier 0 did not have, `shoulder-sweep` is what decides whether it is a
real abut or an artifact of the 1 s → 250 ms placement change. It needs listen
WAVs (~38 gaps of 1073 have them), so it can arbitrate a *named* gap and can
never produce a base rate — if a Tier 1 newcomer has no WAVs, this is not the
tool that settles it, and §9's "unaccounted for" clause applies.

One caveat still bounds the count from below: the delta population is sized in
[archive/TEMP-envelope-3000-corpus-2026-08-24.md](archive/TEMP-envelope-3000-corpus-2026-08-24.md)
— 24 of 2020 populated shoulders differ between the 2000 and 3000 arms, 22
dead→rescued across 11 gaps (all pair 50), **zero outcome changes across all
1073 gaps**. That is the entire set of gaps whose locks a wider envelope can
move, and the run above already covers both arms.

**Premise this Change ships on:** `shoulders_abut` fires on **one** corpus gap
(31/g7). That is a stated premise, not a post-merge discovery — which was the
point of running Tier 0 first.

**Reproduction.** Both arms, ad hoc over `gap-files/`, no committed script:
per gap take `lag_decision.pre_anchor[0]` / `post_anchor[0]`, `gap_frames` from
`splice_dualfit.gap_frames` (falling back to `geometry.gap_frames`), and
`sample_rate`; apply the two lines above. `jq` or a few lines of Python; the
numbers in the table are the durable record if `gap-files/` is cleaned.

~~Tooling gap: the harness reads `gate_pass` but **never reads
`bridge_frames`** — no roll-up prints either predicate today.~~ **Closed
2026-08-28**: `gap-fingerprint-stats` now prints a `=== shoulders abut ===`
section (`report.rs::shoulders_abut_text`, predicate
`GapRow::shoulders_abut`), so Tier 1 is one command. It reports its **own
denominator** — a dump with no `bridge_frames`, and equally a pre-step-6 dump
whose bridge was measured on the blind ±600 ms search, prints as *unmeasurable*
rather than as zero abuts, which is the one misreading this section exists to
prevent. Each abutting gap prints bridge frames and ms (ms derived from the
row's own `gap_frames` vs `duration_secs`), gap length, both seam `r`s, and the
lookalike tags (`clamped`, `pre`/`post-rescued`, `edge-pinned`,
`program-quiet`).

**Tier 1 — re-fingerprint after implementing.** Step 6 routes `splice_dualfit`
through `nominal_first_seam_locks`, so the new dump's `bridge_frames` /
`gate_pass` *are* the production predicate, not a mirror of it. Re-run
`--gap-fingerprints` over the 51 pairs (`--features calibration,he-aac`, one
pair at a time, ~15 GB RSS) and diff the abut set against Tier 0. Only here does
the exact set become knowable.

> **Tier 1 RUN 2026-08-29 — n = 2, `{11/g9, 31/g7}`.** Corpus
> `2026-08-28-complete-corpus`: 51 pairs, 1073 gaps, 1175 JSON files, every pair's parsed
> gap total equal to its `corpus.json` `source.gap_count`, zero unparseable files. Roll-up
> (`gap-fingerprint-stats`, one command):
>
> ```
> measurable: 971/997 matched gaps carry both gate_pass and bridge_frames
> abutting: 2
>   11·g9 |  -205 f / -4.3 ms |  599.5 ms gap | 0.989 / 0.500
>   31·g7 |  -153 f / -3.2 ms | 1118.8 ms gap | 0.966 / 0.874
> ```
>
> **Tier 0's 31/g7 survives** (§5 not falsified; pre-step-6 the same gap dumped
> `gate_pass false`, `bridge_frames +57375`). **11/g9 is the one newcomer**, and §8.2's rule
> requires it be explained by the placement change Tier 0 cannot model — it is: its
> `lag_decision` shoulders sit at ≈ −72/−68 ms with `step_ms +4.24` (⇒ a *positive* bridge),
> while the seam locks are 603.75 ms apart. A 1 s template at `b_mapped` and a 250 ms one at
> the seam are not looking at the same thing here, which is exactly the modelled gap. Nothing
> is unaccounted for; the merge is not blocked on this.
>
> **The predicate is not knife-edge.** Across all 957 `gate_pass` gaps the bridge distribution
> is −205, −153, then **+17851 f (+371.9 ms)** and a bulk at ≥ +24247 f where bridge ≈ gap
> (step ≈ 0). A 372 ms void separates the abut set from everything else, so `bridge ≤ 0` could
> be `bridge ≤ 300 ms` without moving the ship set. Bridge measurement noise is **not** the
> risk here; wrong lock *placement* is.
>
> **Weak locks are rare and already catalogued.** Only 6 of 957 `gate_pass` gaps have
> `min(pre_r, post_r) < 0.60` (p05 = 0.902, p50 = 0.982). The four weakest are 8/g4 (0.390),
> 25/g11 (0.410), 20/g3 (0.443) — the three band gaps §7 step 1 measured — and 11/g9 (0.500).
>
> **Two false leads, so they are not re-derived.** Low seam prominence and negative
> `post_seam_global_r` are *not* false-peak signatures: 95 of 957 `gate_pass` gaps have
> `post_seam_prom < 0.10` (down to 0.0004) and 164 have negative `post_seam_global_r` (down to
> −0.837), and they fill normally. Do not build a drain gate on either.

> **Provenance was missing from the wire, and is now fixed (2026-08-29).** `probes.rs` populated
> the five lookalike tags, but the from-decode fingerprint path rebuilds `splice_dualfit` from
> `SeamLocalTags`, which did not carry them — so **zero of the corpus's dumped gaps recorded
> `rescued` / `edge_pinned` / `clamped`**, and the roll-up prints `provenance UNRECORDED` for
> that corpus. Fixed by carrying the tags on `SeamLocalTags` and populating them from the locks
> on the production dual-fit path. Pairs 11 and 31 were re-fingerprinted at the fixed binary
> (`gap-files/2026-08-29-early-shoulder-tier-1-repop`, 17/17 and 13/13 gaps, complete):
>
> | | 11·g9 | 31·g7 |
> |---|---|---|
> | `pre_seam_rescued` / `post_seam_rescued` | false / **true** | false / **true** |
> | `pre_seam_edge_pinned` / `post_seam_edge_pinned` | false / false | false / false |
> | `seams_clamped` | false | false |
> | retry `r`, `prom`, `z`, `global_r` | 0.500, 0.0716, 10.96, −0.084 | 0.874, 0.4252, 25.96, +0.006 |
> | step (`trim_frames`) | −28980 f = −603.75 ms | −53857 f = −1122 ms |
>
> Every number else is bit-identical to the 2026-08-28 corpus, so the fix moved no measurement.
>
> **Both abuts are envelope rescues** — so a drain gated on `!rescued` would drain nothing, and
> that idea is dead. 31/g7's rescue is independently corroborated (the 2026-08-18 wide sweep put
> its post shoulder at −1160 ms, beyond ±600 ms reach; the step here is −1122 ms) and is exactly
> the case step 6 exists to catch. The discriminator between the two is **rescue quality**, not
> rescue existence, and it separates them on every column above.
>
> **`edge_pinned` fires 0 times on 0 of 52 locks** across all 26 gaps of both pairs — the first
> measurement of this tag that has ever existed, since no dump predating the fix carried it. It
> would have vetoed neither abut, and in particular does not flag 11/g9, whose weak lock is a
> **mid-sweep false peak** rather than a boundary artifact. That is the "not-pinned ⇏ trustworthy"
> asymmetry landing on the one gap it mattered for. Treat `edge_pinned` as a veto with a
> so-far-empty positive set; **do not** promote it to a corrector on this evidence. Should a later
> corpus show it firing on weak locks, the correct form is not a second mechanism but a narrower
> trigger on the existing one — rescue on `r < triage_r || edge_pinned`, keeping the retry only
> if `retry.r > r` (**strictly better**), so a trustworthy lock can still move but only onto a
> more trustworthy reading. Two pairs is a small sample; the base rate over the full corpus is
> unmeasured and needs the next full re-fingerprint, not a dedicated run.

**Tier 1.5 — step 4's blast radius, measured offline 2026-08-28.** Step 4 turns
envelope rescue on in production for the first time, which touches far more than
the abut gap. Sized from the same corpus — gaps with a **dead** seam lock
(`splice_dualfit` r < `LAG_DECISION_RECOVERABLE_R`) **and** a `lag_decision` shoulder
marked `rescued`. Note (2026-08-29) that 0.5 is **not** the line the shipped helper
consults the envelope at — that is `min_fill_correlation` (0.35) — so this filter is
looser than the real trigger and the counts below are an **upper bound** on the blast
radius, not an estimate of it:

| | count |
|---|---|
| comparable gaps | 971 |
| no dead seam lock — envelope never consulted, path provably unchanged | **948** |
| ≥1 dead seam lock | 23 |
| dead seam **and** a rescued shoulder — locks can move | **5** |

All five are **pair 31**, and **none has `gate_pass` today** (0 of 5), so step 4
cannot alter an existing dual-fit fill — it can only newly *admit* a gap that is
declined today. Resolving the five:

| pair/gap | today | rescued locks | effect of step 4 |
|---|---|---|---|
| 31/g7 | skip | post 0.939 @ −1137.7 (pre live) | **the abut skip** — this Change's target |
| 31/g8 | patch | pre `declined` (+800 false peak) | inert — pre stays dead ⇒ no `gate_pass` |
| 31/g10 | patch | 0.990 / 0.995 @ ≈−1153 | inert — gate keeps, `try_dual_fit` never reached |
| 31/g11 | patch | 0.984 / 0.972 @ −1158.4 | inert — same |
| **31/g9** | **skip** (`waveform_floor`) | **0.965 / 0.957 @ ≈−1144** | **NEW dual-fit fill** — locks clear `gate_pass` by ~3×, `bridge` ≈ +637 ms ≈ `gap_ms` (far-offset, so no abut), so `try_dual_fit` is reached with strong locks on a gap production declines today |

**31/g9 is the one unintended outcome change this Change carries, and it is named
in advance.** It is not a regression on its face — both shoulders agree within
4.1 ms at pair 31's map step, the `shared_far_offset` signature, so B genuinely
holds that material and the fill would be *correct* placement. But it is an
unreviewed new fill in the same pair and same class as **31/g10, the census's one
unexplained audible blip** (Question 11, no mechanism). Treat it as a required
ear check, not a bonus.

**Tier 2 — production truth.** `scripts/measure/measure-patch-outcomes.ps1`
with `--patch-only` over the 51 pairs (`gap-files/extended-pairs.csv`). Two things only
Tier 2 catches — the envelope knobs now firing on the production path can shift **non-abut**
locks and change dual-fit placements (§7 step 4), and `is_retryable_patch_skip` changes the
anchored-retry population (§7 step 7). It pays no anchor-oracle cost; that per-gap sweep is a
Full-tier dump cost, not a production one. **Not yet run** (2026-08-29).

> **What the existing baseline already settles (2026-08-29, no new run).** Both Tier-1 abut gaps
> were looked up in `gap-files/2026-08-13-repeat-terms`:
>
> | gap | outcome today | after this Change |
> |---|---|---|
> | **11·g9** (`a_start` 3788.963, dur 0.599) | **skipped** — `correlation_below_threshold`, pre 0.043 / post 0.043, best anchor attempt 0.268 / 0.213 | `shoulders_abut` |
> | **31·g7** (`a_start` 1725.697, dur 1.119) | **patched** — pre 0.437 / post 0.437, `align_adjustment_secs` −0.977, confidence **high**, `normalize_gain` **2.128**, `dual_fit_used` false | `shoulders_abut` |
>
> **11/g9 cannot cost a fill** — it is already refused, so the Change alters only the *reason*.
> Whatever its rescue is worth (retry `r` 0.500, `prom` 0.0716), it has no production
> consequence, and it needs **no ear check**.
>
> **31/g7 is the entire substance of Tier 2.** Production patches it today at
> `confidence: high` with a **2.13× gain**; this Change refuses it. If the silence-splice
> reading is right, today's fill fabricates 1.12 s of amplified content over silence that was
> never missing from B. That **inverts the ear-check priority below**: the listen that carries
> the most information is 31/g7 *as it sounds today*, because a confirmed artifact there is
> positive evidence the refusal is correct — 31/g9's new fill is the secondary check.

> **Use a same-binary two-arm control, not the 2026-08-13 baseline as primary (2026-08-29).**
> That baseline is two weeks and several merges old (`program_gain` landed 2026-08-26 among
> others), so a diff against it mixes this Change with everything since — and the acceptance
> bar is *zero* outcome changes on non-abut gaps, which such confounds would trip constantly.
> Run both arms from the same HEAD binary:
>
> - **arm A** — default, `-OutDir gap-files/<date>-tier-2-on`
> - **arm B** — `-RepairArgs "--lag-envelope-max-lag-ms 0"`, `-OutDir …-tier-2-off`
>
> `0` is a documented disable and the two `--lag-envelope-*` flags are deliberately **ungated on
> production runs** (`cli/mod.rs:306`, from step 4). With the envelope off the seam locks never
> relocate, so arm B reproduces pre-Change placement and the A↔B diff isolates steps 4 + 6. Arm B
> is a control only — never a shipping configuration. Keep the 2026-08-13 diff as a **secondary**
> check for the one thing arm B cannot neutralize: step 7's change to the anchored-retry
> population.

> **Tooling gap — the outcome differ does not exist (open 2026-08-29).** Nothing in
> `scripts/measure/` diffs two `measure-patch-outcomes` runs per gap.
> `diff-patch-adjustments.ps1` answers a different question (the `border_standoff_secs` sweep),
> but it donates the rule that matters: **join arm × arm on `patch.gaps[*].a_start_secs`,
> rounded to 1 ms, within a pair label — NEVER on gap index**, because the arms do not agree on
> which gaps get patched, so index *i* is not the same gap in two arms. A Tier 2 diff joined on
> index is garbage. What is needed: classify each joined gap as unchanged / status flip /
> skip-reason change / patched-with-different-numbers, and report unjoined gaps on **both**
> sides rather than dropping them. This is the same shape of gap as the Tier 1 roll-up
> (closed 2026-08-28) and, like it, needs no media to build.

**Tier 2 acceptance (revised 2026-08-29):**

- 31/g7 `patched` → `shoulders_abut`, and 11/g9 `correlation_below_threshold` → `shoulders_abut`
- 31/g9 may newly gain a dual-fit fill (Tier 1.5) → ear check before accepting
- **31/g7's current fill** → ear check (new; the strongest confirmation available)
- zero outcome changes on every other gap
- a run that does **not** flip 31/g7 falsifies §5 and blocks the merge

### 8.2 Test blast radius

**Compile-forced (6 sites — corrected 2026-08-28, implementing step 7).**
`cell_for_skip_reason` (`domain/gap_repair_spec.rs:300`), `reason_admits_cell`
(`:320`), `skip_cell_from_tags` (`:344`) are wildcard-free by design (C4b — the
compile-time vocabulary guarantee), plus `format_gap_patch_skip_reason`
(`domain/patch_result.rs:205`). All four take `ShouldersAbut => SilenceSplice`.

Two more live **outside** `clip-sync-repair` and were missing from this list:
`clip-sync-repair-harness`'s `energy_matrix.rs:146` (`format_skip_reason`) and
`residual_gate.rs:92` (`skip_reason_label`). Both are wildcard-free label
formatters and both refused to compile; each got a `shoulders_abut(bridge=…)`
arm. The lesson for the next vocabulary change is that C4b's reach is the
workspace, not the domain crate — a good thing, and worth knowing before the
build tells you.

**Compile-silent — the real risk (3 sites).**

* `is_retryable_patch_skip` (`domain/patch_anchor.rs:340`) is a `matches!`
  allowlist, so a new variant silently defaults to `false`. That *is* what this
  plan wants, but nothing enforces it: add an explicit assertion or the intent
  is uncaptured. **Done** — `is_retryable_patch_skip_excludes_shoulders_abut`.
* `cell_for_skip_reason_is_exhaustive_and_correct`
  (`domain/gap_repair_spec.rs:423`) iterates a **fixed 7-case array**. Despite
  its name and its “C4b backstop” comment it cannot detect an unlisted variant —
  the wildcard-free match is the only real guarantee. Add the 8th case; do not
  rely on it to remind you. **Done** — the 8th case, plus `reason_admits_cell`
  assertions that the abut admits `SilenceSplice` and nothing else (the lookalike
  tags, `program_quiet` included, must not move its cell) and a `skip_cell_from_tags`
  agreement check.
* `gap_tags.rs:462` ends in `_ => (PatchTier::NotApplicable,
  SeamShape::NotApplicable)` (found 2026-08-28, missing from this list when it
  said 2 sites). A new variant lands there silently. `NotApplicable` is the right
  answer for an abut skip — no tier, no seam shape — but decide it explicitly
  with a comment naming `ShouldersAbut`, or the next reader cannot tell the
  default from a decision.

**Rewrites (~6 functions).** Step 3 changes `try_dual_fit`'s signature to take
`SeamLocks`. Test callers: `domain/dual_fit.rs:304`, `:382`, `:483`, `:524`,
and `application/patch_audio/region/dual_fit.rs:829`, `:995`
(`dual_fit_registration_residual_derives_from_try_dual_fit_lags_only`). Each
builds locks first — the largest mechanical edit in the test surface. Also
re-read `gap_fingerprint/schema.rs:1760`
(`dual_fit_rescue_requires_every_try_dual_fit_condition`): it documents the
conjunction, and `gate_pass` just left `try_dual_fit`.

**Fixtures and goldens — untouched as planned.**
`tests/gap_cell_fixtures.rs:269` requires every fingerprint-representable cell
to have a curated fixture; `SilenceSplice` already has
`03_silence_splice_dualfit_target.json`, so no 13th fixture (a *new* cell would
have demanded one — a further argument for §4's cell choice).
`curated_fixtures_carry_derived_fields` (`tests/curated_fixture_backfill.rs:115`)
fires only if step 6 adds a new **derived** `splice_dualfit` field — one whose
value the committed bytes already imply. **Step 6 added five columns and the
tripwire stayed silent, correctly** (verified 2026-08-28): `pre_seam_rescued` /
`post_seam_rescued`, `pre_seam_edge_pinned` / `post_seam_edge_pinned` and
`seams_clamped` are *measured* provenance, not derivable from any bytes on disk,
which is exactly why they are `Option` and read `None` on an older dump. The test
still checks only `dual_fit_rescue`. If a genuinely derived field is ever added:
`CURATED_FIXTURE_BACKFILL=1`, then re-freeze
`harness/golden/curated.golden.json` with `CURATED_GOLDEN_REGEN=1`.
`GapPatchSkipReason` derives `Serialize` only — no in-repo deserializer breaks,
so the JSON-contract docs are the only downstream consumer.

**The refactor's own tripwire.** Every existing in-reach dual-fit numeric test
must pass with *identical values* after steps 1–3, and `measure/lag.rs`'s
envelope tests must compile and pass unedited through the re-exports. Steps 1
and 3 both claim “move, no behavior change”; a shifted value falsifies that
claim and is the signal to stop.

### 8.3 Checklist

- [x] **Tier 0 first** — **done 2026-08-28, n=1 (31/g7)**, from
      `lag_decision` over `2026-08-23-complete-corpus` *and* its `-3000` arm
      (971 evaluated each). Count, gap list and the refuted
      `splice_dualfit` split are recorded in §8.1.
- [x] `domain::lag_envelope` move: fingerprint `rescue_shoulder` uses the moved
      primitive (no second envelope), `lag.rs` re-exports, existing lag tests
      pass unedited. One `LAG_DECISION_RECOVERABLE_R`; one 100 ms bin const.
      **Verified 2026-08-29** — `lag_envelope.rs:28` and `:50` (`ENVELOPE_BIN_MS`)
      are the only definitions; `measure/lag.rs:72,78` re-export from
      `domain::seam_local` and `domain::lag_envelope`.
- [ ] `domain::seam_locks`: `SeamLockTemplates::build` is the only range guard
      (both old copies deleted); `nominal_first_seam_locks` unit tests —
      ~~`gate_pass` NaN polarity matches `try_dual_fit`~~ (**superseded** — §7 step 2's
      NaN was refuted; there is none to test, see [settled.md](settled.md) § A),
      envelope not consulted when nominal is recoverable, **rescued-but-unrecoverable
      falls back to the honest nominal**, `edge_pinned` is true iff the accepted peak
      is on `±max_lag` of the producing search, `clamped` is true iff unclamped
      seams do not abut and `.max(0)` makes them abut.
      **Audited 2026-08-29 — the range guard is confirmed sole; four of five tests
      landed, one is missing.** `SeamLockTemplates::build` (`seam_locks.rs:92`) holds
      the only seam-window range check; grepping `region/` for the old guards' text
      leaves just `pcm.rs:186` (`splice_into_a: region out of range`), which guards the
      **write** offset, not a shoulder window, and must stay. `seam_locks.rs`
      holds exactly three tests: `a_gate_passing_nominal_lock_is_never_relocated`
      (envelope not consulted), `clamped_marks_an_abut_the_clamp_manufactured`
      (clamped), `unqualified_locks_never_abut`; `edge_pinned` is covered in
      `domain/seam_local.rs` (`…_flags_a_peak_on_the_search_boundary`,
      `…_reports_the_clamped_search_bounds`). **No test covers the fallback branch at
      `seam_locks.rs:298`** — `retry.r < triage_r ⇒ keep the honest nominal` — which is
      live behavior on the production path and reachable with a synthetic fixture (no
      media). Write it before the merge.
- [x] `seam_local_peak` widening (step 2a): returns `SeamPeak` with the
      post-clamp `lag_lo` / `lag_hi`; one `LAG_EDGE_TOL_MS`, in `domain`,
      re-exported from `measure/lag.rs`; all six call sites updated; **every
      existing value unchanged**. Unit tests: edge peak pinned, interior peak
      not, and a B-edge-clamped search reports the *clamped* bound (the case
      `lag.abs() == max_lag` gets wrong). **Verified 2026-08-29** —
      `seam_local_peak_recovers_offset_seam`, `…_flags_a_peak_on_the_search_boundary`,
      `…_reports_the_clamped_search_bounds` in `domain/seam_local.rs`; `LAG_EDGE_TOL_MS`
      lives in `domain::seam_local` and `measure/lag.rs:70` documents the move.
- [x] Step 2's deferred payload: `SeamLock.edge_pinned` from the accepted
      search, `SeamLocks.b_*_seam_unclamped` retained rather than discarded at
      `seam_locks.rs:305–306`. **Verified 2026-08-29** — `seam_locks.rs:133`,
      `:153–154`, and `clamped()` at `:170` reads the unclamped pair.
- [x] **Unqualified locks decide nothing** (`unqualified_locks_never_abut`,
      verified 2026-08-29): dead-shoulder fixture
      (`min(pre_r, post_r)` below floor) whose noise lags cross produces **no**
      abut skip and reaches `evaluate_seam_gate` — same outcome as today.
- [x] `try_dual_fit` consumes locks; unit tests: abut never assembles; in-reach
      two-lag fixture matches today’s fill bytes. **Verified 2026-08-29** —
      `dual_fit.rs:162` takes `locks: &SeamLocks`; the in-reach fixture asserts
      `trim_frames == step`, exact fill length, and interior (non-pinned) locks.
- [x] Plumbing: `patch_settings()` carries both knobs; `characterize_region`
      reads them through `Deref`; `RepairConfig::validate` rejects a
      non-positive `lag_envelope_window_secs`; both doc comments no longer say
      “production repair never reads it”. **Verified 2026-08-29** —
      `patch_audio/request.rs:72,75`, read at `characterize.rs:430–431`;
      `config.rs:971` mirrors the CLI's message. The phrase survives only on
      `border_capture` (`measure/config.rs:75`, `infrastructure/config.rs:211`),
      where it is **true** — that field really is probe-only. Do not "fix" those.
- [x] `characterize_region` skip-before-keep; `--no-dual-fit` still abut-skips;
      `is_retryable_patch_skip(ShouldersAbut) == false` (anchored retry does not
      re-characterize an abut gap). This one is **compile-silent** — the
      `matches!` allowlist defaults a new variant to `false`, so the assertion
      is the only thing that captures the intent (§8.2). `gap_tags.rs:462`'s
      `_ =>` arm is the third silent site — decide `NotApplicable` explicitly.
      **Done 2026-08-28**: `tests/abut_skip_integration.rs` covers the first two
      end-to-end on a synthetic introduced-silence noise pair (plus the
      over-firing guard: a real dropout is not abut and still patches);
      `is_retryable_patch_skip_excludes_shoulders_abut` covers the third;
      `gap_tags.rs`'s arm is explicit with a comment.
- [x] The locks are measured **once**: `characterize_region`'s locks are the
      ones the fit consumes (`DualFitAttempt` bundles them with the input, and
      `dual_fit_from_locks` is the shared body of the two rejections). Without
      this the abut decision and the fill could disagree about a lag, and every
      dual-fit gap would pay the per-shoulder ±600 ms search twice.
- [x] `splice_dualfit_at` + `dual_fit_rescue_flag` (already declines
      `bridge_frames <= 0`) on a rescued-but-abut fixture. **Done 2026-08-28** —
      `splice_dualfit_rescues_a_far_post_shoulder_and_reads_the_abut` is that
      fixture (−1200 ms post lock, rescued, `bridge_frames ≈ 0`), asserted on
      both the envelope-on and envelope-off arms. The `dual_fit_rescue_flag`
      half needed nothing new: `dual_fit_rescue_requires_every_try_dual_fit_condition`
      already pins `bridge_frames` of `0` and `−100` to `Some(false)`.
- [x] `ShouldersAbut` reason: serde, all four wildcard-free matches, human
      format, JSON + repair-guide tables, vocab Hole 2 “detection” line. Add the
      8th case to `cell_for_skip_reason_is_exhaustive_and_correct` by hand — its
      fixed array cannot detect the new variant (§8.2). Payload stamps `pre_z` /
      `post_z`, envelope rescue, `clamped`, `edge_pinned`, `program_quiet` —
      still one variant; a clamp-only fixture is `shoulders_abut` with
      `clamped: true`, not a new reason. **Done 2026-08-28** for the code half —
      six wildcard-free matches, not four (§8.2), and the 8th test case is in.
      The **JSON + repair-guide tables and the vocab Hole 2 line are still open**;
      they are step 9's docs pass, not this entry's.
- [x] `try_dual_fit`'s six test callers rebuilt on `SeamLocks` (§8.2); every
      in-reach numeric value unchanged. **Done 2026-08-28** — `try_dual_fit`
      and `dual_fit_at_nominal` both take `&SeamLocks`; the six callers
      (`domain/dual_fit.rs` ×4, `application/patch_audio/region/dual_fit.rs`
      ×2) build locks and pass, numerics unchanged.
- [x] Capture + post-splice visit: write-mode fixture with one abut skip and
      one real fill — visit runs **after** splice, A length unchanged, abut
      frames still in range, `patch.shoulders_abut` has that gap; preview has
      the list and never visits; visit is a named function
      [TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
      fills in. **Done 2026-08-28** —
      `abut_capture_survives_splice_and_reaches_the_summary` +
      `preview_reports_the_abut_without_pcm` in
      `tests/abut_skip_integration.rs`; the named function is
      `patch_audio::visit_shoulders_abut`.
- [x] **Tier 1 re-fingerprint** — **done 2026-08-29**, n = 2 `{11/g9, 31/g7}` over 51
      pairs / 1073 gaps; Tier 0's 31/g7 survives and 11/g9 is explained by the
      1 s → 250 ms placement change. Numbers and the two false leads in §8.1.
- [x] **Provenance on the wire** — **done 2026-08-29**; `SeamLocalTags` carries
      `rescued` / `edge_pinned` / `clamped`, populated from the locks on the production
      dual-fit path. Pairs 11 + 31 re-fingerprinted at the fixed binary: both abuts are
      **post-rescued**, `edge_pinned` is **0 of 52 locks**, no measurement moved.
- [ ] **Tier 2 outcome differ** — build it (join on `a_start_secs` at 1 ms within a pair
      label, never on index; report unjoined gaps both sides). Needs no media. §8.1.
- [ ] **Tier 2 run, two arms** — arm A default, arm B `--lag-envelope-max-lag-ms 0`, same
      HEAD binary, 51 pairs; then the secondary diff against `2026-08-13-repeat-terms` for
      step 7's retry population. §8.1 acceptance.
- [ ] **31/g7 ear check on the CURRENT fill** — production patches it today at
      `confidence: high` with a 2.13× gain. Listen to what the Change removes; an audible
      artifact is positive evidence the refusal is right. Higher information than 31/g9.
- [ ] **31/g9 ear check** — Tier 1.5 predicts step 4 gives it a *new* dual-fit
      fill (skip → patch, rescued locks 0.965/0.957 at pair 31's map step). Not a
      regression on its face, but same pair and class as 31/g10's unexplained
      blip. Listen before accepting; if it is bad, step 4 needs a narrower gate,
      not a waiver.
- [ ] 31/g7 production: `shoulders_abut`, not High keep (licensed listen WAVs
      already on disk, 2026-08-18 post-sweep stem).

---

## 9. Exit

- Qualified abutting locks (`gate_pass ∧ bridge_frames ≤ 0`) →
  `shoulders_abut` skip, no fill-search; per-gap JSON payload
  carries lags + `bridge_frames` + `gap_frames` plus the §4 lookalike
  tags (`z`, envelope rescue, `clamped`, `edge_pinned`, `program_quiet`).
  Still one reason. Run-level `patch.shoulders_abut` is the same set with
  `gap_index` + A frames.
- Write / `--patch-only`: after splice, `visit_shoulders_abut` runs once per
  captured skip; A length unchanged. Preview: list present, visit not called.
- In-reach two-lag fillable silence-splice still reaches `try_dual_fit` with
  the same seam locks it would have searched; fill bytes unchanged.
- Nominal-recoverable shoulders never consult envelope.
- **A lock that passes the gate at its nominal lag is never relocated.** The
  patch path triages at `min_fill_correlation`, so the `[0.35, 0.5)` band is
  empty by construction and the envelope can only add a declined fill, never move
  an existing one (§7 step 1). Regression fixture: a shoulder at ~0.40 with a
  live partner and a strong decoy elsewhere in B keeps its nominal lag.
  **Landed 2026-08-28** as `a_gate_passing_nominal_lock_is_never_relocated`
  (`domain/seam_locks.rs`): the shoulder reads 0.405 at nominal — inside the very
  `[0.35, 0.5)` band §7 step 1 measured 3 corpus gaps in — with a verbatim copy of
  A's border planted 3 s away in B and an envelope wide enough to reach it. At
  `triage_r = min_fill_correlation` the lock is not rescued and keeps its lag; the
  test also asserts the same reading falls below a **local** `0.5` — deliberately not any
  shipped constant, so the fixture does not couple itself to an unrelated instrument's
  line. That second arm is what makes it a real test of the parameter: at a triage line
  above the gate floor this same shoulder *would* be a rescue candidate, so the assertion
  pins the behavior to the argument rather than to a constant. (Until 2026-08-29 this read
  "so the fingerprint's line and the patch path's cannot quietly converge on one number" —
  wrong: they are both `min_fill_correlation`.)
  This is the test that must fail before the envelope may be pointed at production.
- `splice_dualfit` on an abutting gap reports the rescued locks and
  `dual_fit_rescue: false`.
- `--no-dual-fit` still collapse-skips 31/g7.
- Sub-floor locks never skip: a gap whose shoulders fail `gate_pass` takes
  today’s path whatever `bridge_frames` says.
- `lag_envelope_max_lag_ms = 0` **in the config file** → 31/g7 post stays dead
  at ±600 → no abut skip from this path (keep may still High, same as today).
  ~~The CLI flag stays dump-gated (§1), so this criterion is exercised through
  config, not `--lag-envelope-max-lag-ms`.~~ **Stale since the 2026-08-28
  relaxation** — the flag now sets the same field on any run, so exercise the
  criterion either way; config remains the canonical form because it is what a
  production run uses.
- A non-positive `lag_envelope_window_secs` in the config file is rejected at
  load, not silently rescue-free.
- **Tier 2 clean**: the two-arm `--patch-only` diff (arm A default vs arm B
  `--lag-envelope-max-lag-ms 0`, same HEAD binary) shows the Tier-1 abut set
  `{11/g9, 31/g7}` flipped to `shoulders_abut` and **zero outcome changes on every
  non-abut gap**, with the secondary diff against `gap-files/2026-08-13-repeat-terms`
  covering step 7's retry population (§8.1). **Open.**
- Tier 1's abut set contains Tier 0's `{31/g7}`, and any *additional* member is
  explained by the placement change Tier 0 cannot model — 1 s template at
  `b_mapped` → 250 ms template at the seam (F14, §8.1). A new member with no such
  explanation is unaccounted for and blocks the merge. A Tier 1 run that **loses**
  31/g7 falsifies §5 and blocks the merge outright. **Met 2026-08-29**: the run kept
  31/g7 and added exactly one member, 11/g9, which carries that explanation.
