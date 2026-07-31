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
`docs/dev/TEMP-equivalence-divergence-findings.md` § F15.

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

At that point `band_donor_diverges_on_the_donor_axis` and
`band_donor_divergence_is_attributable_to_the_donor_axis` **will fail**. That failure is the fix's
**acceptance signal**, not a broken test.

The correct response is to convert this into a *regression* fixture — retain the artifact and the
recorded pre-fix numbers, and rewrite the assertions to state that this gap used to diverge and now
agrees, keeping the band arithmetic as documentation of the mechanism that was closed. The wrong
responses, in rough order of how tempting they will look:

- relaxing or deleting the assertions so they pass again — that discards the only committed evidence
  of the mechanism;
- re-harvesting a *different* still-diverging gap into this file to keep the test green — that hides
  the fix's effect and silently changes what the fixture means;
- concluding the fix is wrong because the test went red.

Two assertions here are **not** scheduled to change and should keep passing throughout:
`fine_noise_floor_reads_lower_than_scan` (a separate, still-open axis) and
`divergence_is_never_in_the_dangerous_direction` (a safety invariant that must hold before *and*
after). If either of those breaks, that is a real regression.
