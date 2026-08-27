# TEMP — Local paired-shoulder fill gain (replace `compute_fill_gain`)

**Status:** draft, 2026-08-26. **Startable** as a **separate Change** from
[TEMP-program-gain-plan.md](archive/TEMP-program-gain-plan.md) — do not bundle. The
measurement it reuses (`paired_level_offset`) and the program prior
(`program_levels.offset`) already exist on the scan report. Reviewed against
the landed program-gain tree the same day: § 6 (scan threading + helper), § 7
(`stack_fill_gain`), § 8 (apply signature), § 9 (`normalize_window_secs`) are
the corrections; nothing in the estimator itself moved.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill normalize boosts against a
non-local reference* → *Local paired-shoulder gain*.

**Companions:** program scalar
[TEMP-program-gain-plan.md](archive/TEMP-program-gain-plan.md) § 5 (stacking this
discharges); capture
[archive/TEMP-ab-program-levels-plan.md](archive/TEMP-ab-program-levels-plan.md);
local-reference defect
[TEMP-fill-normalize-local-reference-2026-08-25.md](archive/TEMP-fill-normalize-local-reference-2026-08-25.md);
[settled.md](settled.md) § A **B-relative / donor-shoulder fill gain** (refuted —
this plan is **not** that idea, see § 2); latched attenuation-half of *RMS* local
normalize (§ C — also not this idea, see § 5).

**Media hygiene:** pair index only; no filenames / titles / paths in the repo.

---

## 1. Goal / non-goals

**Goal**

When `normalize_fill` is on, stop matching whole-fill RMS to a 5 s + 5 s A-border
energy pool. Instead, at each gap, run the **same paired B−A block difference**
the program scalar uses, restricted to **collar blocks around the hole**, and
apply only the **residual against the whole-program offset**. That is the
hole-local master relationship, informed by the file average, not a second copy
of it.

**Non-goals (v1)**

- Bundling with the program-gain Change (apply site overlap; two knobs vs this
  estimator).
- Flipping the `normalize_fill` default. Program-gain defaults it **off**; this
  Change only replaces the estimator behind the flag. Whether default-on becomes
  safe again is a later decision, after this estimator has a roll-up.
- Keeping `BlockLevel` timelines on `GapReport`.
- Recomputing the collar from patch PCM / refined edges.
- Matching the fill to **B’s gap shoulders** (refuted — § 2).
- Pairing **inside** the gap core (A is inactive; those blocks never pair).
- Shrinkage / James–Stein blend toward the prior when `n_pairs` is tiny — v1
  abstains below a floor (§ 4). Revisit if the floor is hit often.
- Per-side “quieter of pre/post”. One median over the pooled collar.
- ITU / LUFS, peak limiting.
- Narrowing `normalize_window_secs` or otherwise patching `compute_a_border_rms`.
  That function loses its **fill-gain** caller. The field stays the A-border
  template / correlate window (§ 9) — do not treat it as unused.

---

## 2. Why this is not the refuted idea

| | Refuted (settled § A) | This plan |
|---|---|---|
| Reference | B’s **shoulders at the hole**, A ignored | Paired **B minus A** on the same mapped collar blocks |
| What it does | Reproduces the level the fill already has (the fill *is* B) | Reads the local A↔B master relationship, then scales the fill by the inverse residual |
| Fill RMS | unused | **unused here too** — that is the point |
| Same-master, quiet passage | leaves 19/g13’s over-boost untouched | A quiet vs B quiet → `d_local ≈ offset_program ≈ 0` → residual 0 |

The fill is never the reference. The collar is undamaged program on **both**
sides, the same predicate as the whole-file reduce. B’s gap interior is not
read.

---

## 3. What “relevant blocks” are

`paired_level_offset` requires A **active** and **steady**. The gap core fails
both: A is inactive, and the first active block that *abuts* the silent core
fails steadiness (`|Δrms|` ≫ `STEADY_TOLERANCE_DB`). That walk-off is wanted
(same job as `border_standoff`). Do not special-case it.

**Collar (A timeline), default `local_pair_collar_secs = 2.0`:**

```text
pre  = [gap.start − W, gap.start)
post = (gap.end, gap.end + W]
```

Use scan `Gap.video_a_start_secs` / `video_a_end_secs`, not patch-refined
edges (100 ms blocks make tens-of-ms refine noise). A block is in the collar
if it **overlaps** pre or post and does **not** overlap `[gap.start, gap.end]`.

Then the existing pairing steps (active / steady / map / B prefix / B active /
`d_i = rms_B − rms_A`) run **only** on those A blocks. Reuse the loop; do not
copy it. File-edge and one-sided collars are valid — an empty pre is not an
error.

`W = 2.0` matches the registration envelope context, not the 5 s normalize
window. The 5 s pool was the defect; this is a different estimator, not a
shorter RMS. Config-only; no CLI in v1.

Stamping is **scan-time**, so `ScanGapsRequest` must carry `local_pair_collar_secs`
(`composition.rs` today passes no gain knob into scan). Putting the width only
on `PatchRequestSettings` would make a first scan hardcode 2.0 and leave
`--patch-only` unable to change it. Patch never recomputes the collar.

---

## 4. Residual, prior, and who applies

Scan convention stays **B minus A**.

```text
d_local        = median(d_i) over collar pairs          # None if n_pairs < MIN_LOCAL_PAIRS
offset_prior   = offset_db if g_program was Applied, else 0
residual_db    = d_local − offset_prior
g_local_db     = −residual_db
g_local        = 10^(g_local_db / 20)
```

**Why the prior is the *applied* program offset, not the raw measurement.** If
program gain was refused (spread, `|offset| > 6`, no pairing, `--no-program-gain`),
there is no trusted average to subtract. Treating the prior as 0 makes local
`−d_local` — a hole-local master match on its own. Subtracting a *refused*
`offset_db` would bake the phantom (e.g. § 4b’s −8.4 dB compressed-B reading)
into every collar.

**Identity when both apply.** `g_program_db + g_local_db = −d_local`. The file
average cancels; the hole’s own paired offset is what lands on the fill. Program
gain is then the **fallback** for collars that abstain, not a second copy of the
same number.

| Collar | Program | `g_total_db` |
|---|---|---|
| enough pairs | Applied | `−d_local` |
| enough pairs | Refused / off | `−d_local` (prior 0) |
| abstain | Applied | `−offset_db` |
| abstain | Refused / off | 0 |

**`MIN_LOCAL_PAIRS = 8`.** Whole-file `MIN_PAIRS` is 50; a 2 s + 2 s collar at
100 ms is 40 blocks *before* active/steady/B filters. Eight is 0.8 s of accepted
pairs — enough for a median to mean something, low enough that a one-sided
file-edge hole can still speak. Below it: local abstains (`g_local = 1`). No
shrinkage in v1.

**No local spread refusal in v1.** MAD over ~10 samples is not the same
statistic as MAD over ~500. The `n_pairs` floor is the trust gate. Emit
`spread_db` on the collar object so a later Change can gate it.

**Refusal, not clamp, on a huge residual.** `|residual_db| > max_fill_gain_db`
(reuse the existing 12 dB knob as a **bound on the residual**, not as a boost
ceiling on fill RMS) → `g_local = 1.0`, do not apply 12 dB of a number that on
this corpus would mean the collar is still reaching the wrong material. Same
shape as program-gain’s refuse-not-clamp.

**Attenuation is allowed.** A constant scalar from paired shoulders does not
crush intra-fill dynamics (24/g46). That is a different mechanism from the
latched RMS-ratio attenuation (settled § C). Drop the `debug_assert!(gain >= 1.0)`
on the **local** factor; keep it on nothing that this estimator produces. The
product `g_total` may be below 1.

**Boost-only `compute_fill_gain` is no longer on the production path** when this
ships. Leave the function for now (tests / history); do not call it from
`region`. Replace the RMS half of `domain::stack_fill_gain` — do not compose
with it. That helper scales fill RMS into `compute_fill_gain` so the *old*
estimator would not square the program offset; this estimator’s double-count
guard is the residual formula, and feeding it a scaled RMS would be a third
path. `debug_assert!(stacked.local >= 1.0)` in `region/characterize.rs` goes
with it.

---

## 5. Who gets `g_local`

Today dual-fit / SilenceSplice hard-code local unity because `compute_fill_gain`
needs fill RMS. **This estimator does not** — the collar is scan-time, fill-agnostic.

| Path | `g_program` (landed) | `g_local` (this plan) |
|---|---|---|
| Bracket fill | yes | if `normalize_fill` and collar did not abstain |
| Dual-fit / SilenceSplice | yes | **same** — the old reason to skip it is gone |
| Skip / not-applied | n/a | n/a |

---

## 6. When to stamp (scan, not patch)

`ScanGaps` already holds `a_levels`, `b_levels`, `offset_secs`,
`b_scanned_end_secs`, and the gap list at the `program_levels` site, then
**drops** the timelines. Stamp each gap there. Do not keep the arrays. Do not
wait for patch decode.

```rust
/// Collar pairing at one gap. Same sign as AbLevelOffset (B minus A).
pub struct CollarLevelOffset {
    pub offset_db: f64,
    pub spread_db: f64,
    pub n_pairs: usize,
    pub n_pre: usize,
    pub n_post: usize,
    pub first_pair_secs: f64,
    pub last_pair_secs: f64,
}
```

`n_pre` / `n_post` are provenance (is this a one-sided collar?), not a gate —
counts of **accepted pairs** whose A-block start falls in pre vs post.
`first_pair_secs` / `last_pair_secs` are A-timeline, same as `AbLevelOffset`.
No `chunks`. Type home: `program_levels.rs`, next to `AbLevelOffset`.

`channels_comparable` lives on the program object; do not duplicate it. If
program says channels are incomparable, local pairing is the same confound:
**do not stamp a collar**, so JSON absence means “no pairing”, same as
whole-file. Apply still needs a `channels` reason (§ 8): when collar is `None`,
read `program_levels.offset.channels_comparable` (false, or no offset because
channels blocked pairing). Do not infer `channels` from absence alone —
absence is also `n_pairs < 8` and “B not scanned”.

Home: `Gap.collar_level: Option<CollarLevelOffset>` (`skip_serializing_if =
None`). Index-parallel extra vec is the wrong shape — this *is* the gap’s
level reading. Mechanical `Gap { … }` literals get `collar_level: None`.
`GapScanJson` already serializes `Vec<Gap>`, so the field appears without a
DTO change.

Extract a shared pair-collection helper from `paired_level_offset`. The loop
already accumulates `(a.start_secs, d_i)`. Split there:

- **collect** — optional A-block predicate (collar = overlaps pre∪post and
  does not overlap the core; whole-file = all blocks). Returns the pair vec.
- **summarize** — one median/MAD path. Whole-file wraps as `AbLevelOffset`
  with `MIN_PAIRS = 50`; collar wraps as `CollarLevelOffset` with
  `MIN_LOCAL_PAIRS = 8`, then bins the same pair vec into `n_pre` / `n_post`
  by A-start. A `fn(block) -> bool` cannot produce those counts by itself.

Patch reads `request.report.gaps[i].collar_level` plus the already-decided
`g_program` / applied `offset_db`. No second reduce. `--patch-only` against a
scan that predates this field sees `None` on every gap and abstains — same as
“no pairing”.

---

## 7. Stacking at the splice (single multiply)

Same as program-gain § 5: one `RegionPatch.gain`. Program-gain has **already
landed** in this tree (`domain::program_gain`, `stack_fill_gain`,
`request.program_gain: ProgramGainDecision`). Implement against that, replacing
the RMS local half. Do not bundle the two Changes into one commit/PR.

```text
g_program  = request.program_gain.gain()          # 1.0 on any refusal
offset_prior = Applied { offset_db } else 0
g_local    = collar_residual_gain(...)            # 1.0 when flag off / abstain / refuse
g_total    = g_program * g_local
```

**Do not** compute local from `rms(fill * g_program)` — that was the fix for
the *RMS-ratio* estimator double-counting. The residual formula is the
double-count guard for *this* estimator. `stack_fill_gain` today *is* that RMS
path; rewrite it (or replace the call site) so the local factor comes from
`collar_residual_gain`, not `compute_fill_gain`.

If this Change is ever replayed onto a tree **without** program-gain:
`g_program = 1`, prior = 0, `g_local = 10^(-d_local/20)`. A constant-gain donor
is then matched from the collar (holes with enough pairs) and left unmatched
where the collar abstains. That is acceptable; program-gain later fills those
abstentions. A same-master quiet hole still gets residual 0.

`--no-normalize` remains local-off. It must not clear `collar_level` from the
scan report (measurement stays; apply does not).

Program-gain’s remaining media acceptance still expects `--normalize` to
restore 15/g6 and 2/g6 via the RMS residual. **This plan withdraws that row**
(§ 10). Run program-gain’s default-flags cohort (local off) first if that
table is still owed; once this ships, the `--normalize` regain is obsolete.

---

## 8. Reporting

Keep `normalize_gain` = `g_total` (splice multiplier; `fill_level` contract).

| JSON | Value |
|---|---|
| `scan.gaps[].collar_level` | measurement; omit when `None` |
| `status.patched.normalize_gain` | `g_total` (existing) |
| `status.patched.program_gain` | landed program-gain field |
| `status.patched.local_gain` | `g_local` (`1.0` when off or abstained), **unconditional** once this ships |

`g_local = normalize_gain / program_gain` only when program-gain is present and
non-zero. Emit `local_gain` anyway — dividing is how the last plan avoided a
third field, but this estimator’s abstention vs “on and unity residual” are
different facts; `1.0` from residual-zero and `1.0` from abstention would
collapse. **Stamp the reason** on the patch record next to `local_gain`.

Mirror `program_fill_gain`: flag first (reason, not outcome — every failure is
unity), then preconditions, then the residual bound.

```text
collar_residual_gain(
    collar: Option<&CollarLevelOffset>,
    enabled: bool,                 # normalize_fill
    offset_prior: f64,             # Applied offset_db, else 0
    max_residual_db: f64,          # max_fill_gain_db
    channels_comparable: bool,     # from program_levels.offset; true if no offset object
) -> LocalGainDecision
```

| Reason (`snake_case`) | When |
|---|---|
| `flag_off` | `enabled == false` (collar may still be on the scan JSON) |
| `channels` | collar `None` and `channels_comparable == false` |
| `abstained_n` | collar `None` otherwise (`n_pairs < 8`, B not scanned, old report) |
| `refused_residual` | collar present, `\|d_local − offset_prior\| > max_residual_db` |
| `applied` | collar present, residual inside the bound (including ~0) |

`LocalGainDecision::{Applied { gain }, Refused { reason }}` — same shape as
program gain, per-gap rather than per-run. Thread `gain` + reason onto bracket
`RegionPatch` **and** `GapRepairStrategy::SilenceSplice` (today the splice
only stores `normalize_gain = request.program_gain.gain()`).

---

## 9. Config / CLI

```toml
[repair]
normalize_fill = false          # unchanged by this Change (program-gain owns the default)
local_pair_collar_secs = 2.0    # new; config-only; **scan** reads this (ScanGapsRequest)
# MIN_LOCAL_PAIRS = 8 is a code constant, not a knob, until something asks to tune it
max_fill_gain_db = 12.0         # reuse: refuse |residual| past this, do not clamp to it
normalize_window_secs = 5.0     # **keep reading** — A-border template / correlate_frames /
                                # fingerprint noise_collar_secs. Named for normalization,
                                # used as a border size. Not this estimator’s window.
```

Stop using `normalize_window_secs` for `compute_a_border_rms` / fill gain.
Do **not** drop it from `PatchRequestSettings`, `FillWindowFrames`,
`FillGeometryRecipe`, or the fingerprint probe. A checklist item that says
“stop reading it in patch” is wrong and would break placement. Leave the TOML
field; document that the name is historical. No new CLI. `--normalize` /
`--no-normalize` stay the on/off for this path.

---

## 10. Tests / acceptance

**Unit (synthetic `BlockLevel` timelines, no media)**

- Gap in a flat A=B program: `d_local ≈ 0`, `n_pairs` from both collars, core
  blocks contribute **zero** pairs.
- B = A + 6 dB everywhere: `d_local ≈ 6`, independent of gap length.
- Quiet A passage around the hole, B matches it, loud material 3 s out: `d_local`
  stays near 0 (the 5 s RMS pool would not). This is the defect test.
- Leading-edge gap: pre empty, post pairs, not `None` if `n_post ≥ MIN_LOCAL_PAIRS`.
- Fewer than 8 pairs → `None`.
- Steadiness: the A block abutting the core is excluded; the next steady block
  can pair.
- Residual: `offset_prior = 6`, `d_local = 6` → `g_local = 1`, reason `applied`.
- Residual: `offset_prior = 0`, `d_local = 6` → `g_local ≈ 0.501`.
- Refuse, do not clamp: `residual_db = 13` → `g_local = 1.0`, reason `refused_residual`.
- `enabled = false` → `g_local = 1`, reason `flag_off`, collar still `Some` on the gap.
- Collar `None`, `channels_comparable = false` → reason `channels`, not `abstained_n`.

**Apply / stacking**

- `normalize_fill` off → `local_gain = 1`, collar still on the scan JSON.
- Product: program Applied at +6 dB, collar `d_local = +6` → `g_total ≈ 0.501`,
  not `0.501²`.
- Dual-fit path receives `g_local` when the collar exists.

**Cohort (after a `--patch-only` roll-up with `--normalize`)**

Q4 of the findings note was written for an RMS-window tweak that had to keep
15/g6 and 2/g6 boosts. **That test is withdrawn** — those boosts were fill RMS
vs a wide A window, which this estimator does not compute.

| Gap | Expect with `--normalize` |
|---|---|
| 45/g4, 45/g5, 19/g13, 5/g6, 15/g3 | `local_gain ≈ 1` (same-master quiet collar, residual 0) |
| 24/g46 | not crushed (scalar from shoulders, not fill RMS) |
| 15/g6, 2/g6 | **may sit at unity**; not a failure |

Default flags (local off) are program-gain’s acceptance, not this plan’s.

---

## 11. Implement checklist

- [ ] Extract collect + summarize from `paired_level_offset`; collar predicate on A blocks; bin `n_pre`/`n_post` from pair A-times
- [ ] `CollarLevelOffset` in `program_levels.rs` + `MIN_LOCAL_PAIRS` + collar window helper (pre/post, exclude core)
- [ ] Thread `local_pair_collar_secs` onto `ScanGapsRequest` (`composition.rs` and every request literal)
- [ ] Stamp `Gap.collar_level` in `ScanGaps` next to `program_levels` (timelines still live); skip when channels incomparable
- [ ] Gap object table in [json-output.md](../json-output.md); omit when `None` (`GapScanJson` serializes `Gap` already)
- [ ] Pure `collar_residual_gain` per § 8 (`enabled`, `offset_prior`, `max_residual_db`, `channels_comparable`)
- [ ] Replace the RMS half of `stack_fill_gain` / `compute_fill_gain` production call; drop the local `debug_assert!(gain >= 1.0)`
- [ ] Thread `g_local` + reason onto bracket **and** dual-fit / `SilenceSplice` (today splice stores only `program_gain`)
- [ ] Emit `local_gain` + reason; `normalize_gain` remains the product
- [ ] Config `local_pair_collar_secs`; **keep** reading `normalize_window_secs` for border geometry / fingerprint collar
- [ ] Docs: [pipeline.md](../pipeline.md) § Fill gain (local half is this estimator), README (`--normalize` now this estimator); `max_fill_gain_db` is a residual refusal, not a boost clamp
- [ ] Tests in § 10; `cargo test`; clippy clean
- [ ] Do **not** bundle with program-gain. Do **not** keep `BlockLevel` arrays. Do **not** read B gap interior. Do **not** stop reading `normalize_window_secs` as a border size

---

## 12. Done when

Scan JSON for a normal A+B run carries `collar_level` on gaps that had eight
steady pairs; a synthetic quiet-hole-in-loud-program fixture reports `offset_db
≈ 0` on that collar (the RMS pool’s failure mode); `--normalize` on a
same-master pair does not revive the n=5 over-boost; a constant-gain donor with
program-gain off is matched from the collar where it has pairs; and
`g_program * g_local` equals `10^(-d_local/20)` when both apply, not the square.

---

## Related

- [TEMP-program-gain-plan.md](archive/TEMP-program-gain-plan.md) — file-wide scalar, stacking identity
- [archive/TEMP-ab-program-levels-plan.md](archive/TEMP-ab-program-levels-plan.md) — pairing rules
- [TEMP-fill-normalize-local-reference-2026-08-25.md](archive/TEMP-fill-normalize-local-reference-2026-08-25.md) — defect this replaces the RMS fix for
- [settled.md](settled.md) § A B-relative gain; § C RMS-attenuation latch
- [pipeline.md](../pipeline.md) — current two-half fill gain (program scalar + RMS local residual)
