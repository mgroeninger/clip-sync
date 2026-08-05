# Gap scan

How `clip-sync-repair` finds the **silent gaps** in A and decides which ones B can fill. This is **phase 2** of the [repair pipeline](pipeline.md), between alignment and the fill plan.

Output is a `GapReport`: a list of `Gap`s (each with A start/end, the mapped B start/end, and a `b_has_energy` flag) plus the alignment and scan parameters. The fill plan (phase 3) and patch (phase 4) consume it.

Source: `application/scan_gaps.rs`; silence/energy helpers in `domain` (`b_has_energy_from_levels`, `check_gap_offset_agreement_in_overlap`).

---

## Detection

A's audio is decoded in chunks (`decode_chunk_secs`) and scanned for **silent runs**:

1. **Block level** — the timeline is measured in `scan_block_ms` blocks (default 100 ms).
2. **Silence floor** — a block counts as silent when its level is below the floor: peak under `silence_peak_fraction` of full scale (default 0.01 = 1 %), with `absolute_silence_rms` as an additional RMS floor (default ≈ **0.001007** normalized / CLI scale **33**, ~−60 dBFS). The RMS floor is what lets a near-silent low-level dropout count as a gap even if it isn't digital zero.
3. **Hold** — `silence_hold_blocks` (from configured `silence_hold_ms`, default 500 ms, quantized to whole blocks) bridges brief blips so a single non-silent block doesn't split one dropout into two.
4. **Minimum length** — a run must be ≥ `min_gap_ms` (default 500 ms) to be reported as a gap. Shorter dips are ignored.

The default recipe is **sensitive** (2026-07-20): `min_gap_ms=500`, `scan_block_ms=100` — it catches sub-second
dropouts, and the equivalence gate (`skip_equivalent_gaps`, **on by default**) drops the
mutual/ambient-silence extras that surfaces so only real dropouts reach patch. The gate is
**measured at scan time and decided at plan time**: scan always records a verdict per gap, and
`build_gap_fill_plan` (phase 3) is what acts on it — see [§ Silence character](#silence-character-the-equivalence-gate).

The scan summary line echoes the active parameters:

```text
Gap scan: 6 silent run(s) ≥500ms — block 100ms, silence 1.0% peak, hold 500ms, decode 10s chunks, scan-both on, rms floor 33 (at -60 dBFS)
```

JSON (`--format json`) also echoes the full scan recipe as flat keys on the scan object. The JSON
`silence_hold_ms` is the **effective** hold (`silence_hold_blocks × scan_block_ms`), not the TOML
`silence_hold_ms` — a configured hold of 450 ms at a 100 ms block therefore appears as `500`.

## Mapping to B and fillability

Each A gap is mapped onto B with the alignment offset (`b = a + recommended_offset_secs`). Gap geometry uses the refined silent-run extent; **absolute occupancy** (`b_has_energy`) reads B's per-block [`BlockLevel::silent`] bit over the mapped **core** (same window as the equivalence gate) — the scanner's peak-domain, per-channel predicate recorded before hold bridging. Do not re-threshold interleaved `rms_db` against the abs floor: that raises the bar vs the original silence path and dilutes center-only dialogue on multi-channel layouts.

- **`b_has_energy = true`** → the mapped core lies fully in the reviewed B scan prefix **and** at least one B analysis block there has `silent == false` → candidate **fillable** dropout.
- **`b_has_energy = false`** → B is also silent there (a shared pause), levels missing, the mapped core starts before B, or the core extends past what B's scan measured → **unfillable** (nothing to copy / not reviewed).

This absolute check is distinct from the equivalence gate's `donor_silence_fraction` (fraction of B blocks that are scanner-silent or quieter than A's gap floor), which decides mutual/ambient quiet vs repairable dropout after fillability. Both signals honor `BlockLevel::silent` so they stay in the same measurement domain.

## Silence character (the equivalence gate)

`b_has_energy` answers *is there anything to copy*. The equivalence gate answers the separate
question *does this gap need patching at all* — the **silence-character pre-gate**, which runs
before the seam/donor stages. Scan records one `GapEquivalenceVerdict` per gap in
`GapReport.gap_equivalence`, **index-parallel to `gaps`**, from two signals: A's level relative to
its own local noise floor (`dropout_margin_db`) and `donor_silence_fraction` over the donor window
(`donor_silence_thresh`).

| Class | What it means | Disposition |
|-------|---------------|-------------|
| `repairable_dropout` | A's signal died (RMS ≥ `dropout_margin_db` below the local noise floor) **and** B carries content | **Keep** — proceeds into the normal seam/donor stages |
| `shared_silence` | B is silent over the donor window (`donor_silence ≥ thresh`) — nothing to fill with | **Drop** |
| `ambient_quiet` | A is only room tone near its own floor, a genuine quiet passage rather than a signal failure, even though B has content | **Drop** — don't inject content into intentional quiet |
| `not_evaluated` | No decision was made (see reasons below). The `Default`, so a default-constructed verdict can never fabricate a drop | **Keep** |

`not_evaluated` carries a reason, and the three are very different refusals:

- `gate_disabled` — `params.enabled == false`.
- `missing_signal` — a needed signal was absent (no A blocks, no donor mapped, empty window).
- `donor_registration_unreliable` — the donor envelope correlated below `min_envelope_r`, so the donor window can't be placed and no statement about B's occupancy is defensible (see `apply_donor_registration` in [§ Config](#config)).

### Donor registration (`apply_donor_registration`)

The donor window is placed by `register_donor_window` (`domain/gap_equivalence.rs`) on the
scanner's existing 100 ms `BlockLevel.rms_db` envelopes — no decode, no seam fit:

1. **Correlate on the shoulders.** Cross-correlate A's and B's dB envelopes over the gap ± context
   (`EQUIVALENCE_CONTEXT_SECS`), searching ±`max_lag_blocks` (default 10 ⇒ ±1.0 s). The **gap core
   is excluded** from the correlation: including it makes registration fail on deep A dropouts
   against live B (the core is exactly where the timelines are expected to differ).
2. **Erode one bin at each gap edge** when reading interior levels (`a_interior_db` /
   `b_interior_db` / `interior_delta_db`). Without erosion, 100 ms grid quantization produces large
   spurious deltas.
3. **`peak_r` is a registration test, not an equivalence test.** Under **Apply** (production
   default via `apply_donor_registration`), `peak_r < min_envelope_r` (default 0.70) abstains as
   `donor_registration_unreliable` and **keeps** the gap — it does **not** fall back to the nominal
   map. Flat / too-short envelopes are "cannot ask": no registration is recorded and the nominal
   map stands (not an abstain that would keep every quiet gap).

`DonorRegistrationMode::Observe` remains the enum default so a caller that opts into registration
without choosing a mode cannot silently move a decision; production selects **Apply** from config.
`--no-apply-donor-registration` computes and records registration but classifies at the nominal map.
Registration (and recorded envelopes, when present) is always emitted either way.

**Head/tail exclusion:** when Apply is on, a gap whose **block-confirmed silent core**
(`a_span_secs` / `SilentRun::core_*`) touches the scanned A extent — first/last `BlockLevel` on A's
level stream, within one scan-block `ε` — still classifies at the **nominal** map (Observe
semantics) while recording registration. Predicate is A-span geometry, not gap index 0 / n−1 and
not a `bins` floor. Do not eyeball refined `Gap` A bounds for this rule (sub-block edge refine can
widen them). Mid-extent cores keep Apply unchanged. See [pipeline.md](pipeline.md) §3 and
[gap-vocabulary.md](dev/gap-vocabulary.md) § Silence-character pre-gate.

**Precedence at plan time** (`domain/gap_fill.rs`): fillability and coverage decide first, then the
equivalence gate, then gap selection. So the gate only ever *drops gaps that were otherwise
fillable* — it can never rescue an unfillable one. A dropped gap is reported as
`not planned: already_matches_reference` (`GapFillSkipReason::AlreadyMatchesReference`). With
`--no-skip-equivalent-gaps` the verdict is still recorded but is **advisory only** and every scanned
gap is planned.

### Truncated B scan

The B silence/level walk is **report-only safe**: a mid-file decode/seek error does not abort the whole repair scan — B returns what it has, marks truncation, and continues. (The seek-loop fallback used by test fakes **propagates** mid-file seek/decode errors to that B handler; near declared EOF it still soft-breaks with `Ok`. Production Symphonia sequential scan skips individual corrupt packets until a consecutive-error limit.) The report records `b_scanned_end_secs` (last successfully fed time on B) and `b_scan_truncated = true` when the walk **hard-failed**. Ending >2 s before declared duration warns (container over-report / soft EOF) but does not alone set the truncated flag — occupancy fail-closes on `scanned_end_secs`, not the flag. Progress/stderr and human output include a line such as:

```text
B silence scan truncated at 118.000s; gaps mapping past that are unfillable (not reviewed)
```

Gaps outside the alignment **overlap / mapped region** are reported but marked unfillable when `limit_fill_to_mapped_region` is set (default) — e.g. A's first seconds before B's coverage starts.

> The final `below_scan_floor` / `unfillable` / `not_planned` / `fillable` classification (`plan_kind`) is settled in the **fill plan** (phase 3); scan provides the gap geometry, the raw `b_has_energy` signal, and the per-gap equivalence verdict.

## Bidirectional scan (`scan_both`)

With `scan_both` (default on), B is also scanned for silence so the two timelines can be cross-checked:

- **Mutual-silence cross-check** — A gaps classified `shared_silence` feed `gap_offset_agreement`; when equivalence is `not_evaluated`, mapped `!b_has_energy` is used as a fallback. Decided repairable/ambient classes never fall back to fillability.
- Plan-time fillability still uses `b_has_energy` separately: A-only dropout (B has energy) is fillable; both-silent is not.

## Output

`GapReport`:

| Field | Meaning |
|-------|---------|
| `gaps: Vec<Gap>` | Detected gaps |
| `Gap.video_a_start_secs` / `_end_secs` | Gap bounds on A (decoded-sample clock) |
| `Gap.video_b_start_secs` / `_end_secs` | Mapped donor range on B (`None` when unmapped) |
| `Gap.b_has_energy` | Whether B has audio in the mapped range |
| `gap_equivalence` | Per-gap silence-character verdict, index-parallel to `gaps` (always populated; empty only on pre-gate / test-constructed reports) |
| `alignment` | The `AlignmentResult` used for mapping |
| `gap_offset_agreement` | Mutual-silence agreement (when `scan_both`) |

> **Clock caveat:** gap times are on the **decoded-sample clock**, not container PTS — see the "shared overlap starts at N.Ns" warning and [cli-output.md](cli-output.md) § Timeline warnings. Prefer a clean source / MKV for exact timestamps.

## Config

| Key | Default | Notes |
|-----|---------|-------|
| `min_gap_ms` | 500 | Minimum silent-run length to report (sensitive default) |
| `scan_block_ms` | 100 | Measurement block (also the equivalence gate's granularity) |
| `silence_peak_fraction` | 0.01 | Peak silence threshold (fraction of full scale) |
| `absolute_silence_rms` | ≈ 0.001007 | Normalized amplitude in `[0, 1)` (~−60 dBFS). CLI `--absolute-silence-rms` uses a 0–32767 i16 scale and converts at the boundary; TOML must use the normalized float (values ≥ 1.0 are rejected) |
| `silence_hold_ms` | 500 | Configured hold (pre-quantization). The JSON / scan-summary echo is the **effective** hold (`silence_hold_blocks × scan_block_ms`) |
| `scan_both` | true | Scan B too for the mutual-silence cross-check |
| `skip_equivalent_gaps` | true | Act on the equivalence verdict — drop `shared_silence` / `ambient_quiet` gaps from the fill plan (`--no-skip-equivalent-gaps` makes it advisory only) |
| `apply_donor_registration` | true | Measure the gate's donor window at the **registered** lag rather than the nominal offset map (`--no-apply-donor-registration` to classify at the nominal map). Below `min_envelope_r` the gate **abstains** as `not_evaluated` / `donor_registration_unreliable` (keeps the gap) — it does **not** fall back to the nominal window. Cores that touch the scanned A extent still classify at the nominal map while recording registration (head/tail exclusion; geometry on the silent core, not gap index / `bins`) |
| `gap_offset_tolerance_secs` | 0.5 | Tolerance for A↔B silence agreement |
| `limit_fill_to_mapped_region` | true | Gaps outside B coverage are unfillable |
| `decode_chunk_secs` | 10 | A decode chunk size |

## Code map

| Step | Code |
|------|------|
| Scan orchestration, chunked decode, gap assembly | `application/scan_gaps.rs` |
| B energy check in a mapped range | `domain` `b_has_energy_from_levels` |
| Mutual-silence agreement | `domain` `check_gap_offset_agreement_in_overlap` |
| Silence-character classes + donor registration | `domain/gap_equivalence.rs` (`GapEquivalenceClass`, `NotEvaluatedReason`, `DonorRegistrationMode`) |
| Plan-time disposition (precedence, skip reasons) | `domain/gap_fill.rs` (`build_gap_fill_plan`) |
| Gap type | `domain/gap.rs` (`Gap`, `is_fillable`) |

## Related reading

- [pipeline.md](pipeline.md) — where scan sits (phase 2)
- [gap-repair-guide.md](gap-repair-guide.md) — plan-time gap types (P0–P7) the classification feeds
- [dev/gap-vocabulary.md](dev/gap-vocabulary.md) — § *Silence-character pre-gate*: where these classes sit among the seam/donor cells
- [corpus-validation.md](dev/corpus-validation.md), [`tests/gap_corpus/README.md`](../crates/clip-sync-repair/tests/gap_corpus/README.md) — scan corpus
- [PLAN.md](../PLAN.md) § Repair workflow
