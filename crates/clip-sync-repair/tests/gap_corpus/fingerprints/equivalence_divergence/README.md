# Equivalence-divergence fixtures

Curated single-gap fingerprints where the **scan** equivalence verdict and the **fine** one disagree.
Committed so the disagreement is pinned media-free — the `gap-files/` corpora these come from are
gitignored and deletable.

These are **not** gap *cells* and deliberately live outside `../curated/`: a cell is a property of a
gap, while a divergence is a property of the *pair of front-ends* reading it. Adding them to the
curated manifest would force a fake `GapCellType` and drag in unrelated per-cell assertions.

Non-identifying, like every committed fingerprint: hashed source ids, numbers, and enum names only —
no samples, filenames, titles, or paths. Assertions live in `tests/equivalence_divergence.rs`.

## `band_donor.json`

The **band mechanism** behind F15: a donor whose level falls *between* the two paths' `gap_floor_db`
definitions, so the same donor reads silent to one path and occupied to the other.

| | scan | fine |
|---|---|---|
| `gap_floor_db` | −79.50 | −58.39 |
| `donor_silence_fraction` | 0.474 (< 0.5 ⇒ occupied) | 1.000 (⇒ silent) |
| `noise_floor_db` | −44.86 | −54.21 |
| class | `repairable_dropout` (**keep**) | `shared_silence` (**drop**) |

The donor's `donor_interior_nominal.rms_db` is **−66.94** — below fine's floor, above scan's. Fine's
floor is the max over *all* bins in the gap span (a content peak, not a floor); scan's is the max over
A's *silent* blocks. That single definitional difference flips the donor axis and with it the class.

The divergence is in the **safe** direction — scan keeps what fine would drop. Both known input
differences bias fine toward `drop`, and this fixture carries both: the floor split above, and a
noise floor 9.35 dB lower on the fine side.

Provenance: corpus `silence-floor/fp_post_F14_fix`, single-pair run, gap index 4, original filename
`0c47aa95_6548_t00-42-53_g004_full_patch.json`. Background:
`docs/dev/archive/TEMP-equivalence-divergence-findings.md` § F15.

### ⚠ This fixture is expected to stop diverging — do not "fix" the test when it does

F15's decided-but-unimplemented direction is to give the **fine** path a silent-core `gap_floor_db`
(answer **(a)**: max over A's *silent* bins, not over all bins in the span). When that lands, this gap
is **predicted to converge**:

- fine's floor falls from **−58.39** toward scan's **−79.50**;
- the donor at **−66.94** is then *above* the floor, so fine's `rms < gap_floor` predicate reads it
  **occupied** rather than silent;
- fine's class moves `shared_silence` → `repairable_dropout`, matching scan.

**Measured 2026-07-30, before the fix landed.** A silent-core probe over the same gap reads
**−80.96** at 50 ms and **−81.99** at 100 ms — the predicted collapse, 1.5 dB past scan's −79.50. The
donor at −66.94 is 14 dB above it, so it will read occupied. The prediction above is confirmed as far
as it can be without the fix itself.

One caveat that did **not** come out of the probe as expected: the resulting class flip clears the
dropout threshold by only **0.41 dB** (silent-core A RMS −89.62 vs fine's noise floor −54.21 is
−35.41; the margin is 35.0). Convergence here is real but marginal, and it is marginal because the
*noise-floor* axis is still unfixed. Do not read this fixture going green as proof the class is
robustly closed.

Since identified: the dominant term on that axis is the **channel reduction**, not the context
window — fine downmixes (amplitude mean) where scan takes an interleaved power mean, which under-reads
by up to `10·log10(6)` = 7.78 dB on 6-channel material. This gap is a *partial* one (`silent_bins <
total_bins`), and its post-floor-fix residual of −2.49 dB sits inside that band, so the 0.41 dB margin
above is expected to widen once the reduction is fixed too. That is the reason the floor fix should
ship *with* the noise-floor fix rather than before it.

At that point `band_donor_diverges_on_the_donor_axis` and
`band_donor_divergence_is_attributable_to_the_donor_axis` **will fail**. That failure is the fix's
**acceptance signal**, not a broken test.

### ⚠ The fix landed 2026-07-30 and these tests did **not** go red — that is expected, and it is a limit

All three F15 fixes are now implemented in `application/gap_equivalence.rs`. These assertions stayed
green, because **they never execute that code**: they read the numbers recorded in `band_donor.json` and
re-derive the classification from them. A fixture cannot observe a change to the measurement path that
produced it.

So the "acceptance signal" framing above is only true of a **re-harvested** fixture. Until this artifact
is re-dumped from media under the fixed path, the green here means "the pre-fix numbers still say what
they said", not "the fix works".

The fix *is* verified by execution, elsewhere: `band_donor_mechanism_now_classifies_as_repairable` in
`application::gap_equivalence` reproduces this fixture's shape from synthetic PCM — a gap with one loud
bin and a donor in the band between the silent-core floor and the whole-span peak — and asserts the class
is now `repairable_dropout`, with `donor_would_read_silent_against_the_unfixed_whole_span_floor` pinning
that the two floors genuinely disagree on that donor. That pair is the real acceptance signal.

When this fixture is re-harvested, convert it as described below and expect the recorded fine verdict to
change; do not re-harvest a *different* gap to keep it diverging.

The correct response is to convert this into a *regression* fixture — retain the artifact and the
recorded pre-fix numbers, and rewrite the assertions to state that this gap used to diverge and now
agrees, keeping the band arithmetic as documentation of the mechanism that was closed. The wrong
responses, in rough order of how tempting they will look:

- relaxing or deleting the assertions so they pass again — that discards the only committed evidence
  of the mechanism;
- re-harvesting a *different* still-diverging gap into this file to keep the test green — that hides
  the fix's effect and silently changes what the fixture means;
- concluding the fix is wrong because the test went red.

### ⚠ Re-dumped from media under the fixed path — the mechanism closed, the class did **not** converge

Corpus `silence-floor/fp_band_donor_mechanism_now_classifies_as_repairable_check`, same pair, all 10 gaps.

The floor prediction above is **confirmed**: fine's `gap_floor_db` fell **−58.39 → −76.66**, an 18 dB
collapse onto scan's −79.50. The band mechanism that this fixture documents is closed.

The class prediction is **refuted**: fine still reads `shared_silence`, because
`donor_silence_fraction` went **0.474 → 0.610** and stayed on the far side of 0.5.

The prediction was made with the wrong instrument. It reasoned from the donor's
`donor_interior_nominal.rms_db` of **−66.94** — a *mean*, ~10 dB above the new floor, hence "occupied".
But the classifier consumes the **per-bin fraction**, and 61 % of this donor's bins sit below −76.66
despite that mean. The donor is peaky. **Do not predict a donor verdict from a mean level**; the
fraction and the mean can disagree arbitrarily on non-stationary content.

Two residual terms keep it apart, both on the deliberately-open window/bin leg:

- a 2.84 dB floor residual (fine's floor is a max over 50 ms bins, scan's over 100 ms blocks — a max
  over finer bins can only be **greater**, and was on all 10 gaps, 0 negatives);
- a granularity bias in the donor itself: fine's fraction ran **higher** on 5 of the 6 gaps that have a
  donor (+0.136, +0.154, +0.410, +0.030, +0.013, −0.011), because finer bins dip below the floor more
  often. This biases fine toward `drop` — the safe direction, but it is now the dominant remaining term,
  larger than anything the three fixes left behind.

`divergence_is_never_in_the_dangerous_direction` still holds across the whole pair.

### The donor axis here is one block from the threshold

Measured offline 2026-07-30. Scan's `donor_silence_fraction` of **0.474** is `9/19` blocks, so one
`scan_block_ms` is worth **0.053** of the fraction and the threshold is 0.026 away. Scan's donor window
is also ~1 block narrower than the gap (block-grid truncation), which is true on every gap of this pair.

The band mechanism is unaffected — the floor split driving it is 21 dB, orders of magnitude larger than
one block of donor — but the `keep` verdict here is **one block of window alignment from flipping,
independently of the floor**. Read this as a caution against treating the donor fraction as a smooth
quantity near 0.5, not as a defect in the fixture. Only two gaps in the pair straddle that way (this one
and g6); the rest sit ≥ 0.4 from the threshold.

Two assertions here are **not** scheduled to change and should keep passing throughout:
`fine_noise_floor_reads_lower_than_scan` (a separate, still-open axis) and
`divergence_is_never_in_the_dangerous_direction` (a safety invariant that must hold before *and*
after). If either of those breaks, that is a real regression.
