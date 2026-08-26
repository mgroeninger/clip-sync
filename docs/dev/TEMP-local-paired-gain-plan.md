# TEMP — Local paired-shoulder fill gain (replace `compute_fill_gain`)

**Status:** draft, 2026-08-26. **Startable** as a **separate Change** from
[TEMP-program-gain-plan.md](TEMP-program-gain-plan.md) — do not bundle. The
measurement it reuses (`paired_level_offset`) and the program prior
(`program_levels.offset`) already exist on the scan report.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill normalize boosts against a
non-local reference* → *Local paired-shoulder gain*.

**Companions:** program scalar
[TEMP-program-gain-plan.md](TEMP-program-gain-plan.md) § 5 (stacking this
discharges); capture
[archive/TEMP-ab-program-levels-plan.md](archive/TEMP-ab-program-levels-plan.md);
local-reference defect
[TEMP-fill-normalize-local-reference-2026-08-25.md](TEMP-fill-normalize-local-reference-2026-08-25.md);
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
  That function loses its production caller.

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
`region`.

---

## 5. Who gets `g_local`

Today dual-fit / SilenceSplice hard-code local unity because `compute_fill_gain`
needs fill RMS. **This estimator does not** — the collar is scan-time, fill-agnostic.

| Path | `g_program` (other plan) | `g_local` (this plan) |
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

`n_pre` / `n_post` are provenance (is this a one-sided collar?), not a gate.
No `chunks`. `channels_comparable` lives on the program object; do not
duplicate it — if program says channels are incomparable, local pairing is the
same confound: **do not stamp a collar** (or stamp and refuse at apply; prefer
not stamping so JSON absence means “no pairing”, same as whole-file).

Home: `Gap.collar_level: Option<CollarLevelOffset>` (`skip_serializing_if =
None`). Index-parallel extra vec is the wrong shape — this *is* the gap’s
level reading. Mechanical `Gap { … }` literals get `collar_level: None`.

Extract a shared pair-collection helper from `paired_level_offset` that takes
an optional A-time filter (`fn(block) -> bool` or a list of windows). Whole-file
passes “all blocks”; the collar passes the union of pre/post. One median/MAD
summarizer.

Patch reads `request.report.gaps[i].collar_level` plus the already-decided
`g_program` / applied `offset_db`. No second reduce.

---

## 7. Stacking at the splice (single multiply)

Same as program-gain § 5: one `RegionPatch.gain`.

```text
g_program  = <other plan, or 1.0 if that Change has not landed>
g_local    = normalize_fill ? collar_residual_gain(...) : 1.0
g_total    = g_program * g_local
```

**Do not** compute local from `rms(fill * g_program)` — that was the fix for
the *RMS-ratio* estimator double-counting. The residual formula is the
double-count guard for *this* estimator.

If this Change lands **before** program-gain: `g_program = 1`, prior = 0,
`g_local = 10^(-d_local/20)`. A constant-gain donor is then matched from the
collar (holes with enough pairs) and left unmatched where the collar abstains.
That is acceptable; program-gain later fills those abstentions. A same-master
quiet hole still gets residual 0.

`--no-normalize` remains local-off. It must not clear `collar_level` from the
scan report (measurement stays; apply does not).

---

## 8. Reporting

Keep `normalize_gain` = `g_total` (splice multiplier; `fill_level` contract).

| JSON | Value |
|---|---|
| `scan.gaps[].collar_level` | measurement; omit when `None` |
| `status.patched.normalize_gain` | `g_total` (existing) |
| `status.patched.program_gain` | other plan, if present |
| `status.patched.local_gain` | `g_local` (`1.0` when off or abstained), **unconditional** once this ships |

`g_local = normalize_gain / program_gain` only when program-gain has shipped and
is non-zero. Emit `local_gain` anyway — dividing is how the last plan avoided a
third field, but this estimator’s abstention vs “on and unity residual” are
different facts; `1.0` from residual-zero and `1.0` from abstention would
collapse. **Stamp the reason** on the patch record: `applied` / `abstained_n` /
`refused_residual` / `flag_off` / `channels`. A small enum, serialized as a
string, next to `local_gain`.

---

## 9. Config / CLI

```toml
[repair]
normalize_fill = false          # unchanged by this Change (program-gain owns the default)
local_pair_collar_secs = 2.0    # new; config-only
# MIN_LOCAL_PAIRS = 8 is a code constant, not a knob, until something asks to tune it
max_fill_gain_db = 12.0         # reuse: refuse |residual| past this, do not clamp to it
```

`normalize_window_secs` becomes dead for production. Leave the field (don’t
break TOML) but stop reading it; document it as unused. No new CLI. `--normalize`
/ `--no-normalize` stay the on/off for this path.

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
- Residual: `offset_prior = 6`, `d_local = 6` → `g_local = 1`.
- Residual: `offset_prior = 0`, `d_local = 6` → `g_local ≈ 0.501`.
- Refuse, do not clamp: `residual_db = 13` → `g_local = 1.0`.

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
| 45/g4, 45/g5, 19/g13, 5/g6 | `local_gain ≈ 1` (same-master quiet collar, residual 0) |
| 24/g46 | not crushed (scalar from shoulders, not fill RMS) |
| 15/g6, 2/g6 | **may sit at unity**; not a failure |

Default flags (local off) are program-gain’s acceptance, not this plan’s.

---

## 11. Implement checklist

- [ ] Extract pair-collection + summary from `paired_level_offset`; add window filter
- [ ] `CollarLevelOffset` + `MIN_LOCAL_PAIRS` + collar window helper (pre/post, exclude core)
- [ ] Stamp `Gap.collar_level` in `ScanGaps` next to `program_levels` (timelines still live)
- [ ] `GapScanJson` / gap object table in [json-output.md](../json-output.md); omit when `None`
- [ ] Pure `collar_residual_gain(collar, offset_prior, max_db) -> LocalGainDecision`
- [ ] Replace `compute_fill_gain` production call; thread `g_local` onto bracket **and** dual-fit
- [ ] Emit `local_gain` + reason; `normalize_gain` remains the product
- [ ] Config `local_pair_collar_secs`; stop reading `normalize_window_secs` in patch
- [ ] Docs: [pipeline.md](../pipeline.md) § Fill normalization, README (`--normalize` now this estimator)
- [ ] Tests in § 10; `cargo test`; clippy clean
- [ ] Do **not** bundle with program-gain. Do **not** keep `BlockLevel` arrays. Do **not** read B gap interior

---

## 12. Done when

Scan JSON for a normal A+B run carries `collar_level` on gaps that had eight
steady pairs; a synthetic quiet-hole-in-loud-program fixture reports `offset_db
≈ 0` on that collar (the RMS pool’s failure mode); `--normalize` on a
same-master pair does not revive the n=3 over-boost; a constant-gain donor with
program-gain off is matched from the collar where it has pairs; and
`g_program * g_local` equals `10^(-d_local/20)` when both apply, not the square.

---

## Related

- [TEMP-program-gain-plan.md](TEMP-program-gain-plan.md) — file-wide scalar, stacking identity
- [archive/TEMP-ab-program-levels-plan.md](archive/TEMP-ab-program-levels-plan.md) — pairing rules
- [TEMP-fill-normalize-local-reference-2026-08-25.md](TEMP-fill-normalize-local-reference-2026-08-25.md) — defect this replaces the RMS fix for
- [settled.md](settled.md) § A B-relative gain; § C RMS-attenuation latch
- [pipeline.md](../pipeline.md) — current boost-only RMS normalize
