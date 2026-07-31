# Equivalence-divergence fixtures

Originally: curated single-gap fingerprints where the **scan** equivalence verdict and the **diagnostic**
one disagreed. Committed so the disagreement (and, once closed, the closure) is pinned media-free —
the `gap-files/` corpora these come from are gitignored and deletable.

These are **not** gap *cells* and deliberately live outside `../curated/`: a cell is a property of a
gap, while a divergence is a property of the *pair of front-ends* reading it. Adding them to the
curated manifest would force a fake `GapCellType` and drag in unrelated per-cell assertions.

Non-identifying, like every committed fingerprint: hashed source ids, numbers, and enum names only —
no samples, filenames, titles, or paths. Assertions live in `tests/equivalence_divergence.rs`.

## `band_donor.json` — **regression** (converged 2026-07-30 / re-harvested 2026-07-31)

g4 of the F15 pair. The **band mechanism** behind F15: a donor whose level fell *between* the two
paths' `gap_floor_db` definitions, so the same donor read silent to one path and occupied to the
other. That mechanism is **closed**. This artifact now records the post-F15 + post-I1 verdicts, where
the two paths **agree**.

### Current (re-harvested from `silence-floor/fp_i1_bin_convergence/`)

| | scan | diagnostic |
|---|---|---|
| `gap_floor_db` | −79.50 | **−79.50** |
| `donor_silence_fraction` | 0.474 (< 0.5 ⇒ occupied) | **0.476** (< 0.5 ⇒ occupied) |
| `noise_floor_db` | −44.86 | −45.64 |
| class | `repairable_dropout` (**keep**) | `repairable_dropout` (**keep**) |

Floors match exactly. Donor fractions differ by one block of window alignment (still both occupied).
The residual noise-floor split (~0.78 dB) is the accepted I2 context-window term — safe direction,
does not flip the class.

Provenance: corpus `silence-floor/fp_i1_bin_convergence`, same pair / gap index 4, original filename
`0c47aa95_6548_t00-42-53_g004_full_patch.json`. Background:
`docs/dev/archive/TEMP-equivalence-divergence-findings.md` § F15;
`docs/dev/archive/TEMP-equivalence-instrument-convergence.md` § I1.

### Pre-fix numbers (retained as documentation — `fp_post_F14_fix`)

| | scan | diagnostic |
|---|---|---|
| `gap_floor_db` | −79.50 | −58.39 |
| `donor_silence_fraction` | 0.474 (< 0.5 ⇒ occupied) | 1.000 (⇒ silent) |
| `noise_floor_db` | −44.86 | −54.21 |
| class | `repairable_dropout` (**keep**) | `shared_silence` (**drop**) |

The donor's `donor_interior_nominal.rms_db` is **−66.94** — unchanged across harvests. Pre-fix it sat
below the diagnostic path's whole-span floor and above scan's silent-core floor. That floor was the max over *all*
bins in the gap span (a content peak, not a floor); scan's was the max over A's *silent* blocks. That
single definitional difference flipped the donor axis and with it the class. Divergence was in the
**safe** direction — scan kept what the diagnostic path would drop.

Those pre-fix numbers live as constants in `tests/equivalence_divergence.rs` (`pre_fix` module) so
the mechanism stays evidenced after the JSON was re-harvested.

### Closure path (do not reverse)

1. **F15** (2026-07-30) — silent-core floor + A RMS, interleaved reduction, block-confirmed span.
   Floor collapsed −58.39 → −76.66 on an intermediate re-dump; the *class* did not yet converge
   because the classifier consumes the per-bin donor *fraction*, and granularity (50 ms vs 100 ms)
   still pushed the diagnostic fraction over 0.5. Lesson: **do not predict a donor verdict from a mean level**.
2. **I1** (2026-07-30) — equivalence overlay bins at `scan_block_ms`. g4 class returns to
   `repairable_dropout` on both paths; floors agree exactly. That is this harvest.
3. **I3** (2026-07-31) — the diagnostic donor gains scan's silence disjunct. **No effect on this pair** (lossy
   AAC never reaches the −120 digital-silence floor); no further re-dump required.

### What the tests assert now

| test | role |
|---|---|
| `band_donor_now_agrees_on_repairable_dropout` | both paths keep; floors equal; both donors occupied |
| `closed_band_mechanism_no_longer_straddles_donor` | live floors cannot band-straddle the donor; pre-fix constants still would |
| `diag_noise_floor_reads_lower_than_scan` | I2 residual, sign pinned |
| `divergence_is_never_in_the_dangerous_direction` | safety invariant (holds under agreement too) |

Wrong responses if something goes red: relaxing the assertions, or re-harvesting a *different*
still-diverging gap into this file to restore a diverge shape. Either discards the closure evidence.

### The donor axis here is one block from the threshold

Scan's `donor_silence_fraction` of **0.474** is `9/19` blocks, so one `scan_block_ms` is worth
**0.053** of the fraction and the threshold is 0.026 away. Scan's donor window is also ~1 block
narrower than the gap (block-grid truncation), which is true on every gap of this pair. Read this as
a caution against treating the donor fraction as a smooth quantity near 0.5 — not as a defect. Only
two gaps in the pair straddle that way (this one and g6); the rest sit ≥ 0.4 from the threshold.
