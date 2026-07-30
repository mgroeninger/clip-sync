# Equivalence divergence — open findings ledger

**Opened:** 2026-07-30. **Status:** both findings OPEN, neither diagnosed in source.

Split out of [archive/TEMP-silence-floor-findings.md](archive/TEMP-silence-floor-findings.md) when
that ledger was archived (2026-07-30). Everything else in it is closed; these two are not, and both
came out of its §5 follow-up rather than its original F1–F12 sweep. Finding IDs **F14/F15** are kept
from the parent ledger so its text still resolves.

Both are recorded from **measurement only**. Neither has been traced to a code path — that is the
next step for each, and the hypotheses below are labelled as such.

Media: an uncatalogued licensed 5.1 pair (A ≈ 6900 s, AAC-LC 48 kHz 5.1). Per the media-hygiene
rule the pair is referred to only by these properties; timestamps are numeric, raw logs stay in
gitignored `gap-files/`.

**Verification rule.** Re-read any `file:line` reference before acting on it — the references below
were read 2026-07-30.

---

## F15 — Scan-time and fill-time equivalence disagree on the same gap, post-fix
**Severity: high. Status: OPEN, found 2026-07-30.**

The gap at **2585.11–2586.25 s** carries both verdicts in one fingerprint file, and they are
opposites:

| | `scan_equivalence` | `equivalence` (fill-time) |
|---|---|---|
| class | `repairable_dropout` | **`shared_silence`** |
| `drop` | false | **true** |
| `donor_silence_fraction` | **0.10** | **0.8696** |
| `a_gap_rms_db` | −82.27 | −60.68 |
| `noise_floor_db` | −45.85 | −64.83 |

**The fill-time value is the correct one**, corroborated twice independently:

- `donor_interior_nominal` — over the same nominal mapped span — is `silence_fraction: 0.8696`
  (exactly matching the fill-time fraction), `longest_silence_ms: 650`, `continuous: false`.
- dual-fit's separate measurement of the aligned bridge reports `silence_fraction: 0.5833`,
  `longest_silence_ms: 350`, also non-continuous.

Three measurements agree the donor is largely silent; scan-time's `0.10` is the outlier. This is
**F1's exact shape on a surviving path** — a scan-time donor fraction that contradicts the audio —
and the R2 unification (both signals reading `BlockLevel::silent`) evidently did not reach it.

**Operational consequence.** `skip_equivalent_gaps` is on by default and consumes the *scan-time*
verdict, so this gap is admitted to the fill plan as a `repairable_dropout`, runs the full bracket
search and dual-fit, and then hard-skips — while the fill-time analysis of the same gap says
`shared_silence, drop: true`. Wasted work, and an operator-facing label that is the opposite of the
truth.

This is why the parent ledger's §0 premise — two signals off the same B audio disagreeing — is
**not fully closed**. F1–F12 fixed the instances then in evidence, not the class.

**Hypothesis (unconfirmed).** The two fractions are computed at different granularities: scan blocks
100 ms vs `donor_interior` bins 50 ms. That is a candidate cause but does **not** by itself explain
0.10 vs 0.87 — an 8× disagreement needs more than a bin-size difference.

**Next step.** Find where the scan-time donor fraction is computed for the `skip_equivalent_gaps`
path and check whether it reads `BlockLevel::silent` at all, or still thresholds RMS against a gap
floor (the pre-R2 shape). Compare against the fill-time computation that produces 0.8696.

---

## F14 — Fingerprint `outcome` records `skip` where production patches (dual-fit rescues invisible)
**Severity: medium-high (calibration-oracle integrity). Status: OPEN, found 2026-07-30.**

The gap at **1050.82 s**, fingerprinted and previewed from the **same binary with the same flags**:

| | fingerprint corpus | production (`--repair-preview`) |
|---|---|---|
| decision | `outcome.tier: skip` | `patched` |
| reason | `skip_reason: correlation_below_threshold` | `dual_fit_used: true`, `patch_tier: high`, `confidence: high` |
| seams | `splice_dualfit` 0.9972 / 0.9821, `gate_pass: true` | `pre 0.9947 / post 0.9821` |
| filename | `..._g001_full_timing_offset.json` | — |

The measurements agree; only the **recorded decision** disagrees. The corpus even carries
`splice_dualfit.gate_pass: true` in the same file whose `outcome` says `skip`, so the dual-fit
rescue was measured and then not reflected in the outcome axis.

Two reasons this is not cosmetic:

1. The corpus is the **oracle** for calibration sweeps (17-pair fingerprint runs, gap-vocabulary
   analysis, the pre-gate work). A `skip` recorded where production patches biases every roll-up
   computed over it, and the effect is concentrated on exactly the dual-fit-rescued gaps that
   recent work targets.
2. The per-gap **filename** encodes the same wrong verdict (`..._full_timing_offset.json` for a gap
   that patches), so directory listings mislead before anything is even parsed.

Note the `--repair-preview` help says the fingerprint path uses `any_ok` — i.e. it should be *more*
permissive and skip *less*. The divergence runs the other way, so that note does not explain it.

**Next step.** Read how `gap_fingerprint` populates `outcome` versus where `patch_audio::region`
applies the dual-fit rescue. Relevant: `gap_fingerprint/measure.rs:584-588` records that the
dual-fit validators are **published rather than acted on** — the outcome axis may simply predate
the rescue path.

---

## Reproducing these runs

Cost two failed runs to rediscover, so it is recorded here as well as in the run-protocol note.

- Build with **`--features calibration,he-aac`**. `calibration` gates the `--gap-fingerprints` /
  `--fingerprint-gap` / `--fingerprint-diagnostics` flags (otherwise
  `unexpected argument '--gap-fingerprints'`). `he-aac` is required **even for plain AAC-LC media**:
  `codec_registry.rs:34-47` registers *any* AAC decoder only under `he-aac`/`ac3`, so a `default =
  []` build fails with `alignment failed: no decodable audio tracks` — presenting as exit 0, an
  empty scan JSON, and zero fingerprints.
- **One pair at a time.** Peak RSS is ~15 GB for a ~2.3 h 5.1/48 kHz pair (characterization
  materializes the whole B track: `secs × rate × channels × 4 B`). Two concurrent runs OOM with
  `memory allocation of N bytes failed`.
- Recipe knobs need no flags — the defaults (`min_gap` 500 ms, block 100 ms, rms 33/32767, hold
  500 ms) already equal the 2026-07-26 reference `scan_recipe`. Pin `--silence-hold-ms 500`
  explicitly anyway, because the manifest's `scan_recipe` does not record it (that is F11, tracked
  in [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md)).
- `--fingerprint-gap N` is **1-based** on the gap-table `#`, and emits **0-based** filenames.
- Use `RUST_LOG=debug`, not `RUST_LOG=clip_sync_repair=debug`, when the question might involve the
  `clip_sync` crate (alignment, seek, decode) — the narrower filter hides those errors.

Both findings above are reproducible from one command per finding: `--fingerprint-gap 6` for F15,
`--fingerprint-gap 2` plus a `--repair-preview` run for F14.
