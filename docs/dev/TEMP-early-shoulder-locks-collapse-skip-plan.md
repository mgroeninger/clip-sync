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
`SEAM_LOCK_RECOVERABLE_R` and have `is_recoverable(&LagSummary)`
(`lag.rs:275`) read it. That const is the **fingerprint's** recoverable line and
the helper's *default* — **not** a number the patch path may inherit; see step 2.

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

- **fingerprint** passes `SEAM_LOCK_RECOVERABLE_R` (0.5) — `lag_decision` and
  `is_recoverable` keep today's behavior exactly;
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

```text
nominal_first_seam_locks(&templates, max_lag_frames, envelope: Option<EnvelopeCfg>,
                         wide_a: Option<&WideA>, floors, triage_r) -> Option<SeamLocks>
```

Per shoulder: `seam_local_peak` at nominal → if `peak_r < triage_r` and envelope
on, coarse envelope → `seam_local_peak` recentered → keep the retry only if it
now clears `triage_r`, else the honest nominal. `triage_r` is
`SEAM_LOCK_RECOVERABLE_R` for the fingerprint and `min_fill_correlation` for the
patch path (step 1). Returns pre/post `(r, lag_frames, z)`, `b_pre_seam`, `b_post_seam`,
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
`SEAM_LOCK_RECOVERABLE_R`, and for the same reason: one definition of "pinned",
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

**6 — `splice_dualfit_at`.** Replace its template build + two
`seam_local_peak`s with `SeamLockTemplates` + `nominal_first_seam_locks`;
keep the prominence measurements (`pre_seam_prom` / `post_seam_prom`) and
`post_seam_global_r`, which are fingerprint-only. `bridge_frames`,
`trim_frames`, `gate_pass` come from the locks. `lag_decision` keeps its 1 s
census and now calls the moved envelope primitive.

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

Tooling gap: the harness reads `gate_pass`
(`gap_fingerprint_corpus/analysis.rs:647`) but **never reads `bridge_frames`** —
no roll-up prints either predicate today. Add a roll-up line to
`gap-fingerprint-stats` only if Tier 0 and Tier 1 should be one command.

**Tier 1 — re-fingerprint after implementing.** Step 6 routes `splice_dualfit`
through `nominal_first_seam_locks`, so the new dump's `bridge_frames` /
`gate_pass` *are* the production predicate, not a mirror of it. Re-run
`--gap-fingerprints` over the 51 pairs (`--features calibration,he-aac`, one
pair at a time, ~15 GB RSS) and diff the abut set against Tier 0. Only here does
the exact set become knowable.

**Tier 1.5 — step 4's blast radius, measured offline 2026-08-28.** Step 4 turns
envelope rescue on in production for the first time, which touches far more than
the abut gap. Sized from the same corpus — gaps with a **dead** seam lock
(`splice_dualfit` r < `SEAM_LOCK_RECOVERABLE_R`, so the new helper *would*
consult the envelope) **and** a `lag_decision` shoulder marked `rescued`:

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
with `--patch-only`, diffed per gap against the `gap-files/2026-08-13-repeat-terms`
baseline. Acceptance: exactly the Tier-1 abut set flips to `shoulders_abut`;
**31/g9 may newly gain a dual-fit fill (Tier 1.5) and must be ear-checked before
it is accepted**; every other gap's outcome unchanged. Two things only Tier 2 catches — the
envelope knobs now firing on the production path can shift **non-abut** locks
and change dual-fit placements (§7 step 4), and `is_retryable_patch_skip`
changes the anchored-retry population (§7 step 7). It pays no anchor-oracle
cost; that per-gap sweep is a Full-tier dump cost, not a production one.

### 8.2 Test blast radius

**Compile-forced (4 sites).** `cell_for_skip_reason`
(`domain/gap_repair_spec.rs:300`), `reason_admits_cell` (`:320`),
`skip_cell_from_tags` (`:344`) are wildcard-free by design (C4b — the
compile-time vocabulary guarantee), plus `format_gap_patch_skip_reason`
(`domain/patch_result.rs:205`). All four take `ShouldersAbut => SilenceSplice`.

**Compile-silent — the real risk (3 sites).**

* `is_retryable_patch_skip` (`domain/patch_anchor.rs:340`) is a `matches!`
  allowlist, so a new variant silently defaults to `false`. That *is* what this
  plan wants, but nothing enforces it: add an explicit assertion or the intent
  is uncaptured.
* `cell_for_skip_reason_is_exhaustive_and_correct`
  (`domain/gap_repair_spec.rs:423`) iterates a **fixed 7-case array**. Despite
  its name and its “C4b backstop” comment it cannot detect an unlisted variant —
  the wildcard-free match is the only real guarantee. Add the 8th case; do not
  rely on it to remind you.
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
fires only if step 6 adds a new derived `splice_dualfit` field; this plan adds
none. If one is added: `CURATED_FIXTURE_BACKFILL=1`, then re-freeze
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
- [ ] `domain::lag_envelope` move: fingerprint `rescue_shoulder` uses the moved
      primitive (no second envelope), `lag.rs` re-exports, existing lag tests
      pass unedited. One `SEAM_LOCK_RECOVERABLE_R`; one 100 ms bin const.
- [ ] `domain::seam_locks`: `SeamLockTemplates::build` is the only range guard
      (both old copies deleted); `nominal_first_seam_locks` unit tests —
      `gate_pass` NaN polarity matches `try_dual_fit`, envelope not consulted
      when nominal is recoverable, rescued-but-unrecoverable falls back to the
      honest nominal, `edge_pinned` is true iff the accepted peak is on
      `±max_lag` of the producing search, `clamped` is true iff unclamped
      seams do not abut and `.max(0)` makes them abut.
- [ ] `seam_local_peak` widening (step 2a): returns `SeamPeak` with the
      post-clamp `lag_lo` / `lag_hi`; one `LAG_EDGE_TOL_MS`, in `domain`,
      re-exported from `measure/lag.rs`; all six call sites updated; **every
      existing value unchanged**. Unit tests: edge peak pinned, interior peak
      not, and a B-edge-clamped search reports the *clamped* bound (the case
      `lag.abs() == max_lag` gets wrong).
- [ ] Step 2's deferred payload: `SeamLock.edge_pinned` from the accepted
      search, `SeamLocks.b_*_seam_unclamped` retained rather than discarded at
      `seam_locks.rs:305–306`.
- [ ] **Unqualified locks decide nothing**: dead-shoulder fixture
      (`min(pre_r, post_r)` below floor) whose noise lags cross produces **no**
      abut skip and reaches `evaluate_seam_gate` — same outcome as today.
- [ ] `try_dual_fit` consumes locks; unit tests: abut never assembles; in-reach
      two-lag fixture matches today’s fill bytes.
- [ ] Plumbing: `patch_settings()` carries both knobs; `characterize_region`
      reads them through `Deref`; `RepairConfig::validate` rejects a
      non-positive `lag_envelope_window_secs`; both doc comments no longer say
      “production repair never reads it”.
- [ ] `characterize_region` skip-before-keep; `--no-dual-fit` still abut-skips;
      `is_retryable_patch_skip(ShouldersAbut) == false` (anchored retry does not
      re-characterize an abut gap). This one is **compile-silent** — the
      `matches!` allowlist defaults a new variant to `false`, so the assertion
      is the only thing that captures the intent (§8.2). `gap_tags.rs:462`'s
      `_ =>` arm is the third silent site — decide `NotApplicable` explicitly.
- [ ] `splice_dualfit_at` + `dual_fit_rescue_flag` (already declines
      `bridge_frames <= 0`) on a rescued-but-abut fixture.
- [ ] `ShouldersAbut` reason: serde, all four wildcard-free matches, human
      format, JSON + repair-guide tables, vocab Hole 2 “detection” line. Add the
      8th case to `cell_for_skip_reason_is_exhaustive_and_correct` by hand — its
      fixed array cannot detect the new variant (§8.2). Payload stamps `pre_z` /
      `post_z`, envelope rescue, `clamped`, `edge_pinned`, `program_quiet` —
      still one variant; a clamp-only fixture is `shoulders_abut` with
      `clamped: true`, not a new reason.
- [ ] `try_dual_fit`'s six test callers rebuilt on `SeamLocks` (§8.2); every
      in-reach numeric value unchanged.
- [ ] Capture + post-splice visit: write-mode fixture with one abut skip and
      one real fill — visit runs **after** splice, A length unchanged, abut
      frames still in range, `patch.shoulders_abut` has that gap; preview has
      the list and never visits; visit is a named function
      [TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
      fills in.
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
- **Tier 2 clean**: the `--patch-only` diff against
  `gap-files/2026-08-13-repeat-terms` shows the Tier-1 abut set flipped to
  `shoulders_abut` and **zero outcome changes on every non-abut gap** (§8.1).
- Tier 1's abut set contains Tier 0's `{31/g7}`, and any *additional* member is
  explained by the placement change Tier 0 cannot model — 1 s template at
  `b_mapped` → 250 ms template at the seam (F14, §8.1). A new member with no such
  explanation is unaccounted for and blocks the merge. A Tier 1 run that **loses**
  31/g7 falsifies §5 and blocks the merge outright.
