# scripts/

Two groups, split by whether a script can run on a clean checkout.

## Repo tooling (this directory)

Hermetic — no licensed media, no prior run required. These are the ones CI and day-to-day
development call.

| Script | What it does |
| --- | --- |
| [`test-tier.ps1`](test-tier.ps1) | Test tier selector (`unit`, `pr`, `validation`, …). See [docs/dev/test-tiers.md](../docs/dev/test-tiers.md). |
| [`check-repair-test-manifest.ps1`](check-repair-test-manifest.ps1) | Verifies every `[[test]]` in `clip-sync-repair` is actually run by some tier. |
| [`test-container-seek.ps1`](test-container-seek.ps1) / [`.sh`](test-container-seek.sh) | Container seek/extent regression tests (needs ffmpeg on PATH). |
| [`generate_corpus.ps1`](generate_corpus.ps1) / [`.sh`](generate_corpus.sh) | Regenerates committed Tier-B WAV fixtures under `tests/corpus/wav/`. |
| [`fetch_corpus_sources.ps1`](fetch_corpus_sources.ps1) / [`.sh`](fetch_corpus_sources.sh) | Downloads optional third-party corpus sources, SHA-256 verified. |
| [`bump-version.ps1`](bump-version.ps1) | Bumps the workspace semver in root `Cargo.toml`. |
| [`repair-directory-pairs.ps1`](repair-directory-pairs.ps1) | Discovers pairs in a directory by the `<name>` + `<name>.2` convention and **runs the repair**, keeping per-pair logs. Needs media, but it does the work rather than studying it — see below. |

## Investigation harnesses ([`measure/`](measure/))

The `gap-files/` loop: bulk runs over media pairs and the analyses built on their output. None of
these run in CI, all of them need licensed media (directly, or via a dump some earlier run wrote).

| Script | Input | Produces |
| --- | --- | --- |
| [`scan-registration.ps1`](measure/scan-registration.ps1) | manifest | Scan-only JSON per pair — `donor_registration` for every gap. Cheapest of the four: no post-scan arm, so the per-bracket anchor oracle never runs. |
| [`measure-patch-outcomes.ps1`](measure/measure-patch-outcomes.ps1) | manifest | Full patch path via `--patch-only` — every per-gap outcome (`fill_level`, `align_adjustment_secs`, tier/confidence, tags). Needs the splice, so `--repair-preview` cannot substitute. |
| [`measure-gap-fingerprints.ps1`](measure/measure-gap-fingerprints.ps1) | manifest | `--gap-fingerprints` corpus dumps, optionally listenable WAVs (`--gap-listen`). The expensive one — per-bracket oracle on every selected gap. |
| [`measure-repair-perf.ps1`](measure/measure-repair-perf.ps1) | manifest, or `-Logs` | Span-timing tree with exclusive costs. Reference numbers live in [docs/dev/repair-perf.md](../docs/dev/repair-perf.md). |
| [`census-residual-measured.ps1`](measure/census-residual-measured.ps1) | `-DumpDir` | Counts `probe_non_finite` residual sides over dumps the two above already wrote. Consumer only — opens no media. |

### Shared manifest format

The four manifest-driven harnesses and `repair-directory-pairs.ps1` all read the same file. CSV or
TSV; blank lines and `#` comments ignored; no header row; one pair per line:

```
label , path/to/A.mkv , path/to/B.m4v [, extra per-pair args]
```

- Delimiter is auto-picked from the extension (`.tsv` → tab, else comma); override with `-Delimiter`.
- Fields may be quoted (`"C:\my movies\a.mkv"`), so paths with spaces or commas are fine.
- The 4th field **runs to end of line** — `extra` is free-form CLI args, so unquoted delimiters
  inside it are rejoined rather than read as further columns. That is what makes
  `label,A,B,--fingerprint-gap 3,7,12` select all three gaps.

### Media hygiene

Nothing under `measure/` produces a committable artifact.

- `gap-files/` is gitignored (`/gap-files/`) and is where corpora, reports, and listen WAVs default.
  Point an output elsewhere **inside** the repo and that protection is gone.
- Logs default to `$env:TEMP` — outside the repo, and therefore not gitignored at all. Every log
  and every manifest contains absolute paths to licensed media.
- When recording results in docs, carry over derived numbers keyed by the manifest's label only —
  never media paths, filenames, or titles.

See [docs/dev/repair-perf.md](../docs/dev/repair-perf.md) § "Media handling".
