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

**`try_dual_fit` no longer calls `seam_local_peak`.** It takes those locks and
does the rest: `gate_pass`, collapse (same numbers, skip path), step-real,
donor, assemble. If the early scan already collapsed, you never call it.

**After splice** (length-preserving fills are in; A still original clock): loop
the captured list and `visit_shoulders_abut`. This Change’s visit is
record-only.
[TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
warns in that same loop (drain only under `--collapse-introduced-silence`).

Cheap search lives in **one** place. Dual-fit stays “accept and build a fill
from locks we already have.”

One sentence: **characterize computes the seam locks first; keep may skip on
collapse; dual-fit and `splice_dualfit` consume the same locks and do not
search; abut skips are captured and visited after splice.**

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

Three things follow from those fields being fingerprint-only **today**, and
each is a step in §7, not an assumption:

- They are **config-file settable but CLI-unreachable on a repair run**:
  `--lag-envelope-max-lag-ms` / `--lag-envelope-window-secs` exist only under
  the `calibration` feature and are rejected without `--gap-fingerprints DIR`
  (`cli/mod.rs:336`, `:342`). This Change does **not** relax that (§6) — the
  production knob is the config file, and every exit criterion below is written
  against a config value, not a flag.
- `RepairConfig::validate` has **no arm** for `lag_envelope_window_secs`; the
  positivity guard lives only in the CLI path a config-file run never reaches.
  Once production reads the field, a non-positive value silently disables every
  rescue. §7 adds the arm.
- Both doc comments say “Fingerprint-only … production repair never reads it.”
  Both become wrong on merge; §7 rewrites them.

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

New `GapPatchSkipReason` object (per-gap status):

```json
{
  "shoulders_abut": {
    "pre_lag_ms": -38.0,
    "post_lag_ms": -1160.3,
    "bridge_frames": -48,
    "gap_frames": 53702
  }
}
```

`cell_for_skip_reason` / `reason_admits_cell` / `skip_cell_from_tags`: this
reason is SilenceSplice only (wildcard-free match arms).

Human line (stdout / `-v` / `--gap-listen` manifest) includes the four numbers:

`shoulders abut (bridge=−48 frames, gap=53702, pre=−38.0 ms, post=−1160.3 ms)`

**Capture** (final dispositions after anchored retry): a `ShouldersAbutSkip` per
abut — `gap_index`, `a_start_frame`, `a_end_frame`, and the lock payload. Hold
that vec through splice. Do not cut, and do not visit, during characterize or
inside the splice loop. `splice_into_a` is length-preserving, so those A frames
still name the hole after neighbouring fills are in.

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
PCM only under `--collapse-introduced-silence`; mux is the current path
(video was not padded). This Change ships the capture, the post-splice
visit, and the summary list. It does not shorten A.

---

## 5. 31/g7 under this plan

A is silent ~1.12 s; B’s match to pre and B’s match to post abut. At the post
seam, ±600 is dead (`peak_r` ~0.12) so envelope consults; the shipped 1 s
census already reports **−1160 ms** with envelope on. Fine retry is the 250 ms
post seam recentered there.

After this Change: early locks → `bridge_frames` ≤ 0 → `shoulders_abut`,
captured. Unified-fit ±10 s never runs. No −977 keep. After splice, the visit
sees the still-silent hole on original-clock A (other gaps already filled).

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
- New CLI. Envelope knobs already exist as config fields; relaxing
  `validate_fingerprint_flags` so `--lag-envelope-*` works without
  `--gap-fingerprints DIR` is a separate Change (§1). Preview does not splice
  and does not run the visit.
- Unqualified abut. A collapse read off sub-floor locks decides nothing (§0).

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
(`lag.rs:275`) read it. One number, two callers.

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
                         wide_a: Option<&WideA>, floors) -> Option<SeamLocks>
```

Per shoulder: `seam_local_peak` at nominal → if `peak_r <
SEAM_LOCK_RECOVERABLE_R` and envelope on, coarse envelope → `seam_local_peak`
recentered → keep the retry only if it is now recoverable, else the honest
nominal. Returns pre/post `(r, lag_frames, z)`, `b_pre_seam`, `b_post_seam`,
`bridge_frames`, and `gate_pass` (`min(pre_r, post_r)` vs both floors, computed
with `try_dual_fit`’s exact NaN polarity — `!(smin < floor)`, so NaN passes).
`abut()` is `gate_pass && b_post_seam <= b_pre_seam`. `None` only when a seam
window is out of range.

`wide_a`: on the patch path A is fully decoded in `a_pcm`, so the wider
envelope window is a plain slice — no lazy-fetch closure needed. The parameter
exists for the fingerprint’s `wide_a_border` closure, which cannot slice.

**3 — `try_dual_fit` consumes locks.** Delete its two `seam_local_peak` calls
and its `gate_pass` / collapse blocks (`dual_fit.rs:106–160`); take `SeamLocks`
instead. Keep the collapse test as a `debug_assert`-grade guard — the caller
should never invoke it on an abut. Its remaining body (step-real, donor,
trim, assemble) is unchanged, so in-reach fills stay byte-identical.
`build_dual_fit_input` keeps building `DualFitRepairInput` but sources its
templates from `SeamLockTemplates`.

**4 — Plumb the two knobs to production.** Add
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
`locks.abut()` → `shoulders_abut` skip via `skip_region_spec`, capture, return
— no `evaluate_seam_gate`, no `try_dual_fit`. Otherwise carry the locks into
`skip_or_dual_fit` → `try_dual_fit`. The locks are computed from the **baseline**
`refined`, in the same place and for the same reason `dual_fit_input` already is
(`characterize.rs:381`) — the gate may mutate `refined` afterwards.

Under `--no-dual-fit` the lock scan still runs (§3); only
`build_dual_fit_input`’s heavier buffers stay flag-gated.

**6 — `splice_dualfit_at`.** Replace its template build + two
`seam_local_peak`s with `SeamLockTemplates` + `nominal_first_seam_locks`;
keep the prominence measurements (`pre_seam_prom` / `post_seam_prom`) and
`post_seam_global_r`, which are fingerprint-only. `bridge_frames`,
`trim_frames`, `gate_pass` come from the locks. `lag_decision` keeps its 1 s
census and now calls the moved envelope primitive.

**7 — Wire the reason.** `GapPatchSkipReason::ShouldersAbut { … }`
(`domain/patch_result.rs:127`), formatters, `cell_for_skip_reason` /
`reason_admits_cell` / `skip_cell_from_tags`, JSON contract
([json-output.md](../json-output.md), [gap-repair-guide.md](../gap-repair-guide.md)),
Hole 2 detection sentence in [gap-vocabulary.md](gap-vocabulary.md).
Add `ShouldersAbut => false` to `is_retryable_patch_skip`
(`anchor_retry.rs:88` selects retries by that predicate) — otherwise anchored
retry re-characterizes every abut gap and re-abuts it, which is §3’s “does not
retry abut skips” by luck rather than by construction.

**8 — Capture + visit.** Collect `Vec<ShouldersAbutSkip>` from the final skip
specs (after the anchored-retry pass, where `region_results` settles). After
the splice loop in `run_with_decoded`, call `visit_shoulders_abut` for each.
Stamp `PatchSummary.shoulders_abut`. Preview stamps the list, skips the visit.

---

## 8. Checklist

- [ ] `domain::lag_envelope` move: fingerprint `rescue_shoulder` uses the moved
      primitive (no second envelope), `lag.rs` re-exports, existing lag tests
      pass unedited. One `SEAM_LOCK_RECOVERABLE_R`; one 100 ms bin const.
- [ ] `domain::seam_locks`: `SeamLockTemplates::build` is the only range guard
      (both old copies deleted); `nominal_first_seam_locks` unit tests —
      `gate_pass` NaN polarity matches `try_dual_fit`, envelope not consulted
      when nominal is recoverable, rescued-but-unrecoverable falls back to the
      honest nominal.
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
      re-characterize an abut gap).
- [ ] `splice_dualfit_at` + `dual_fit_rescue_flag` (already declines
      `bridge_frames <= 0`) on a rescued-but-abut fixture.
- [ ] `ShouldersAbut` reason: serde, `cell_for_skip_reason` exhaustive tests,
      human format, JSON + repair-guide tables, vocab Hole 2 “detection” line.
- [ ] Capture + post-splice visit: write-mode fixture with one abut skip and
      one real fill — visit runs **after** splice, A length unchanged, abut
      frames still in range, `patch.shoulders_abut` has that gap; preview has
      the list and never visits; visit is a named function
      [TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
      fills in.
- [ ] 31/g7 production: `shoulders_abut`, not High keep (licensed listen WAVs
      already on disk, 2026-08-18 post-sweep stem).

---

## 9. Exit

- Qualified abutting locks (`gate_pass ∧ bridge_frames ≤ 0`) →
  `shoulders_abut` skip, no fill-search; per-gap JSON payload
  carries lags + `bridge_frames` + `gap_frames`; run-level
  `patch.shoulders_abut` is the same set with `gap_index` + A frames.
- Write / `--patch-only`: after splice, `visit_shoulders_abut` runs once per
  captured skip; A length unchanged. Preview: list present, visit not called.
- In-reach two-lag fillable silence-splice still reaches `try_dual_fit` with
  the same seam locks it would have searched; fill bytes unchanged.
- Nominal-recoverable shoulders never consult envelope.
- `splice_dualfit` on an abutting gap reports the rescued locks and
  `dual_fit_rescue: false`.
- `--no-dual-fit` still collapse-skips 31/g7.
- Sub-floor locks never skip: a gap whose shoulders fail `gate_pass` takes
  today’s path whatever `bridge_frames` says.
- `lag_envelope_max_lag_ms = 0` **in the config file** → 31/g7 post stays dead
  at ±600 → no abut skip from this path (keep may still High, same as today).
  The CLI flag stays dump-gated (§1), so this criterion is exercised through
  config, not `--lag-envelope-max-lag-ms`.
- A non-positive `lag_envelope_window_secs` in the config file is rejected at
  load, not silently rescue-free.
