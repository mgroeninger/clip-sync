# Test tier remainder (post–repair migration)

> **Living reference:** [development.md](development.md) (commands, features, tier decision rule,
> repair binary matrix, ignore scheduling). **Migration history:**
> [archive/test-tier-plan.md](archive/test-tier-plan.md).

This doc tracks **open or deferred** test-infrastructure work after the repair-crate tier migration.

---

## Landed (2026-06-26) — repair ignore hygiene

Phases **1–3** (ignore conventions + `test-tier.ps1` wiring for `clip-sync-repair`):

- **`tier:oracle` / `tier:validation` / `tier:diagnostic`** on all repair `#[ignore]` rows
- **Oracle tier:** `oracle_energy` then `oracle_energy -- --ignored` (no per-test name list)
- **Validation tier:** `validate_*` binaries + gap_corpus substring filters + `cli_mux_integration -- --ignored` when ffmpeg on PATH
- **Diagnostic tier:** feature-gated `diag_*` / `seam_residual_oracle`, then named `--ignored` rows (`broadband_oracle_veto_rescue_patches_marginal`, `write_full_surface_repair_golden`, `mux_reports_progress_for_short_fixture`) — no blanket `--lib --ignored`
- **Docs:** repair integration binary matrix, SP binary split in glossary, corpus-validation CI commands
- **Test headers:** `//!` Tier / PR / Run on all 21 repair `[[test]]` binaries

See [development.md § Ignore scheduling](development.md#ignore-scheduling) for the filter table.

---

## Open — `clip-sync` ignore cleanup (~1 h)

Repair crate is done; **`clip-sync` still uses legacy `#[ignore]` strings** and has gaps in
`test-tier.ps1` diagnostic/validation filters.

| Task | Files | Notes |
|------|-------|-------|
| Normalize `#[ignore]` to `tier:validation` / `tier:diagnostic` | `corpus_fixtures.rs`, `offset_refinement.rs`, `locate_query_spike.rs` | Match repair convention |
| Add `locate_query_spike` filter | `test-tier.ps1` `Invoke-ClipSyncDiagnostic` | `locate_query` today hits non-ignored unit tests |
| Add `diagnose_wav_leader` filter | `test-tier.ps1` `Invoke-ClipSyncDiagnostic` | 2 ignored rows in `offset_refinement.rs` not in script |

Subsumed by Phase **2b** if you start the full `clip-sync` binary split soon; otherwise worth doing
standalone for symmetry with repair.

---

## Separate track: `clip-sync` align (Phase 2b)

Not started. `test-tier.ps1` stubs error for:

- `validation-align`, `diagnostic-align`
- `clip-sync` tiers beyond `pr-align` (`corpus_committed` filter on lib today)

**Target:** `autotests = false` on `clip-sync`, explicit `[[test]]` binaries, corpus/symphonia
splits, wire `pr-align` to binaries instead of lib `corpus_` filters. Full inventory in
[archive/test-tier-plan.md § Phase 2b](archive/test-tier-plan.md#phase-2b--physical-separation-clip-sync).

**When to start:** create `docs/TEMP-test-tier-2b-plan.md` (or similar) — multi-day; link from
here and [BACKLOG.md](../BACKLOG.md) Active plans.

---

## Policy decisions (optional — not implementation)

| Item | Question |
|------|----------|
| **`integration_energy_patch` on PR** | Add SP01–SP03 to `pr-repair`? Today PR uses `corpus_scan_patch_smoke` + SD domain rows only. |
| **`pr-repair-extended` in CI** | Path filter on `clip-sync-repair/**` for ~15 min sine seam grid on every PR? |

---

## Optional polish (repair crate)

| Item | Notes |
|------|--------|
| **`diag_repair_golden`** | Deferred binary for `write_full_surface_repair_golden` (today: `--lib` + diagnostic tier name filter). |
| **Doc grep** | Opportunistic fixes in `archive/residual-gate-findings.md`, catalog READMEs for pre-migration paths (e.g. `energy_signature_production.rs`). |

---

## Explicitly deferred (no action required)

| Item | Decision |
|------|----------|
| **Nightly validation CI** | No runner infrastructure (ffmpeg, corpus fetch, wall time). Run `.\scripts\test-tier.ps1 -Tier validation` locally. |
| **cargo-nextest** | Optional; adopt only if `test-tier.ps1` filtering becomes painful. |
| **`clip-sync-repair-validate` crate** | Defer until validation compile time or packaging warrants a separate crate. |
| **Phase 2c — `align_videos` bulk move** | Deferred; see [archive/test-tier-plan.md § Phase 2c](archive/test-tier-plan.md). |

---

## Landed repair migration (closure checklist)

For the record — all complete as of 2026-06-25 unless noted above:

- Phases **1–3.5**: `test-tier.ps1`, feature gates, `clip-sync-repair-harness`, explicit `[[test]]`
- **integration_gap_corpus**, **validate_patch_audio**, **diag_patch_audio**, **integration_energy_patch**
- **seam_residual_oracle** → `diagnostic-tests`
- CI: `pr` tier, manifest check (`check-repair-test-manifest.ps1`), Clippy (repair + harness)
- Oracle hygiene: `#[ignore]` + oracle tier for slow production-geometry rows (no script `--skip`s)
- **2026-06-26:** ignore hygiene phases 1–3 (see [Landed (2026-06-26)](#landed-2026-06-26--repair-ignore-hygiene))
