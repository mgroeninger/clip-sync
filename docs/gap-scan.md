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
dropouts, and the scan-time equivalence gate (`skip_equivalent_gaps`, **on by default**) drops the
mutual/ambient-silence extras that surfaces so only real dropouts reach patch.

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

### Truncated B scan

The B silence/level walk is **report-only safe**: a mid-file decode/seek error does not abort the whole repair scan — B returns what it has, marks truncation, and continues. (The seek-loop fallback used by test fakes **propagates** mid-file seek/decode errors to that B handler; near declared EOF it still soft-breaks with `Ok`. Production Symphonia sequential scan skips individual corrupt packets until a consecutive-error limit.) The report records `b_scanned_end_secs` (last successfully fed time on B) and `b_scan_truncated = true` when the walk **hard-failed**. Ending >2 s before declared duration warns (container over-report / soft EOF) but does not alone set the truncated flag — occupancy fail-closes on `scanned_end_secs`, not the flag. Progress/stderr and human output include a line such as:

```text
B silence scan truncated at 118.000s; gaps mapping past that are unfillable (not reviewed)
```

Gaps outside the alignment **overlap / mapped region** are reported but marked unfillable when `limit_fill_to_mapped_region` is set (default) — e.g. A's first seconds before B's coverage starts.

> The final `fillable` / `unfillable` / `not_planned` classification (`plan_kind`) is settled in the **fill plan** (phase 3); scan provides the raw `b_has_energy` signal and the gap geometry.

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
| `skip_equivalent_gaps` | true | Drop mutual/ambient-silence gaps from the fill plan (`--no-skip-equivalent-gaps` to disable) |
| `apply_donor_registration` | true | Measure the gate's donor window at the **registered** lag rather than the nominal offset map (`--no-apply-donor-registration` to classify at the nominal map). Below `min_envelope_r` the gate **abstains** as `not_evaluated` / `donor_registration_unreliable` (keeps the gap) — it does **not** fall back to the nominal window. The §6.10.3 head/tail exclusion for clipped first/last-gap registrations is **not** implemented; see `docs/dev/TEMP-equivalence-band-gate-off-findings.md` §7.4a |
| `gap_offset_tolerance_secs` | 0.5 | Tolerance for A↔B silence agreement |
| `limit_fill_to_mapped_region` | true | Gaps outside B coverage are unfillable |
| `decode_chunk_secs` | 10 | A decode chunk size |

## Code map

| Step | Code |
|------|------|
| Scan orchestration, chunked decode, gap assembly | `application/scan_gaps.rs` |
| B energy check in a mapped range | `domain` `b_has_energy_from_levels` |
| Mutual-silence agreement | `domain` `check_gap_offset_agreement_in_overlap` |
| Gap type | `domain/gap.rs` (`Gap`, `is_fillable`) |

## Related reading

- [pipeline.md](pipeline.md) — where scan sits (phase 2)
- [gap-repair-guide.md](gap-repair-guide.md) — plan-time gap types (P0–P7) the classification feeds
- [corpus-validation.md](dev/corpus-validation.md), [`tests/gap_corpus/README.md`](../crates/clip-sync-repair/tests/gap_corpus/README.md) — scan corpus
- [PLAN.md](../PLAN.md) § Repair workflow
