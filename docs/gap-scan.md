# Gap scan

How `clip-sync-repair` finds the **silent gaps** in A and decides which ones B can fill. This is **phase 2** of the [repair pipeline](pipeline.md), between alignment and the fill plan.

Output is a `GapReport`: a list of `Gap`s (each with A start/end, the mapped B start/end, and a `b_has_energy` flag) plus the alignment and scan parameters. The fill plan (phase 3) and patch (phase 4) consume it.

Source: `application/scan_gaps.rs`; silence/energy helpers in `domain` (`b_has_energy_in_range`, `check_gap_offset_agreement_in_overlap`).

---

## Detection

A's audio is decoded in chunks (`decode_chunk_secs`) and scanned for **silent runs**:

1. **Block level** — the timeline is measured in `scan_block_secs` blocks (default 0.1 s = 100 ms).
2. **Silence floor** — a block counts as silent when its level is below the floor: peak under `silence_peak_fraction` of full scale (default 0.01 = 1 %), with `absolute_silence_rms` as an additional RMS floor (default **33** in production, ~−60 dBFS). The RMS floor is what lets a near-silent low-level dropout count as a gap even if it isn't digital zero.
3. **Hold** — `silence_hold_blocks` (config `silence_hold_ms`, default 500 ms) bridges brief blips so a single non-silent block doesn't split one dropout into two.
4. **Minimum length** — a run must be ≥ `min_gap_secs` (default 0.5 s) to be reported as a gap. Shorter dips are ignored.

The default recipe is **sensitive** (2026-07-20): `min_gap_ms=500`, `scan_block_ms=100` — it catches sub-second
dropouts, and the scan-time equivalence gate (`skip_equivalent_gaps`, **on by default**) drops the
mutual/ambient-silence extras that surfaces so only real dropouts reach patch.

The scan summary line echoes the active parameters:

```text
Gap scan: 6 silent run(s) ≥500ms — block 100ms, silence 1.0% peak, hold 500ms, decode 10s chunks, scan-both on, rms floor 33
```

## Mapping to B and fillability

Each A gap is mapped onto B with the alignment offset (`b = a + recommended_offset_secs`), then checked for **energy on B** in that range (`b_has_energy_in_range`):

- **`b_has_energy = true`** → B has audio where A is silent → the gap is a candidate **fillable** dropout.
- **`b_has_energy = false`** → B is also silent there (a shared pause), or the mapped range is outside B's coverage → **unfillable** (nothing to copy).

Gaps outside the alignment **overlap / mapped region** are reported but marked unfillable when `limit_fill_to_mapped_region` is set (default) — e.g. A's first seconds before B's coverage starts.

> The final `fillable` / `unfillable` / `not_planned` classification (`plan_kind`) is settled in the **fill plan** (phase 3); scan provides the raw `b_has_energy` signal and the gap geometry.

## Bidirectional scan (`scan_both`)

With `scan_both` (default on), B is also scanned for silence so the two timelines can be cross-checked:

- **Mutual-silence cross-check** — co-occurring silence on *both* A and B is a shared pause, not a dropout; it is excluded from fillable gaps. The check feeds `gap_offset_agreement` (and the report's silence-based offset cross-check).
- This is why an A-only dropout (B has energy) is the fillable case, and a both-silent stretch is not.

## Output

`GapReport`:

| Field | Meaning |
|-------|---------|
| `gaps: Vec<Gap>` | Detected gaps |
| `Gap.video_a_start_secs` / `_end_secs` | Gap bounds on A (decoded-sample clock) |
| `Gap.video_b_start_secs` / `_end_secs` | Mapped donor range on B (`None` when unmapped) |
| `Gap.b_has_energy` | Whether B has audio in the mapped range |
| `alignment` | The `AlignmentResult` used for mapping |
| `gap_offset_agreement` | Mutual-silence agreement (when `scan_both`) |

> **Clock caveat:** gap times are on the **decoded-sample clock**, not container PTS — see the "shared overlap starts at N.Ns" warning and [cli-output.md](cli-output.md) § Timeline warnings. Prefer a clean source / MKV for exact timestamps.

## Config

| Key | Default | Notes |
|-----|---------|-------|
| `min_gap_ms` | 500 | Minimum silent-run length to report (sensitive default) |
| `scan_block_ms` | 100 | Measurement block (also the equivalence gate's granularity) |
| `silence_peak_fraction` | 0.01 | Peak silence threshold (fraction of full scale) |
| `absolute_silence_rms` | 33.0 | Additional RMS floor (~−60 dBFS) |
| `silence_hold_ms` | 500 | Bridge brief non-silent blips |
| `scan_both` | true | Scan B too for the mutual-silence cross-check |
| `skip_equivalent_gaps` | true | Drop mutual/ambient-silence gaps from the fill plan (`--no-skip-equivalent-gaps` to disable) |
| `gap_offset_tolerance_secs` | 0.5 | Tolerance for A↔B silence agreement |
| `limit_fill_to_mapped_region` | true | Gaps outside B coverage are unfillable |
| `decode_chunk_secs` | 10 | A decode chunk size |

## Code map

| Step | Code |
|------|------|
| Scan orchestration, chunked decode, gap assembly | `application/scan_gaps.rs` |
| B energy check in a mapped range | `domain` `b_has_energy_in_range` |
| Mutual-silence agreement | `domain` `check_gap_offset_agreement_in_overlap` |
| Gap type | `domain/gap.rs` (`Gap`, `is_fillable`) |

## Related reading

- [pipeline.md](pipeline.md) — where scan sits (phase 2)
- [gap-repair-guide.md](gap-repair-guide.md) — plan-time gap types (P0–P7) the classification feeds
- [corpus-validation.md](corpus-validation.md), [`tests/gap_corpus/README.md`](../crates/clip-sync-repair/tests/gap_corpus/README.md) — scan corpus
- [PLAN.md](../PLAN.md) § Repair workflow
