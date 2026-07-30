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
