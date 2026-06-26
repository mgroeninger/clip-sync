# Test tier remainder (post–repair migration)

> **Archived (2026-06-25):** the repair-crate tier migration is complete. Historical phase
> detail lives in [archive/test-tier-plan.md](archive/test-tier-plan.md). **Living reference:**
> [development.md](development.md) (commands, features, tier decision rule).

This doc tracks **open or deferred** work that was never part of the repair migration closure.

---

## Explicitly deferred (no action required)

| Item | Decision |
|------|----------|
| **Nightly validation CI** | No runner infrastructure (ffmpeg, corpus fetch, wall time). Run `.\scripts\test-tier.ps1 -Tier validation` locally. |
| **Phase 4 — cargo-nextest** | Optional; adopt only if `test-tier.ps1` filtering becomes painful. |
| **Phase 5 — `clip-sync-repair-validate` crate** | Defer until validation compile time or packaging warrants a separate crate. |
| **Phase 2c — `align_videos` bulk move** | Deferred; see [archive/test-tier-plan.md § Phase 2c](archive/test-tier-plan.md). |

---

## Separate track: `clip-sync` align (Phase 2b)

Not started. `test-tier.ps1` stubs error for:

- `validation-align`, `diagnostic-align`
- `clip-sync` tiers beyond `pr-align` (`corpus_committed` filter on lib today)

**Target:** `autotests = false` on `clip-sync`, explicit `[[test]]` binaries, corpus/symphonia
splits, wire `pr-align` to binaries instead of lib `corpus_` filters. Full inventory in
[archive/test-tier-plan.md § Phase 2b](archive/test-tier-plan.md#phase-2b--physical-separation-clip-sync).

---

## Optional polish (repair crate)

| Item | Notes |
|------|--------|
| **`pr-repair-extended` in CI** | Path filter on `clip-sync-repair/**` if you want the sine seam grid on every PR. |
| **`cli_mux_integration`** | Compiles on PR when ffmpeg on PATH; ignored mux rows need `--ignored` locally or in validation. |
| **Lib `#[ignore]` stragglers** | Golden JSON generator (`output.rs`), ffmpeg mux unit test — diagnostic tier runs `--lib --ignored`. |
| **`diag_repair_golden`** | Deferred binary for `write_full_surface_repair_golden`. |
| **Doc grep** | Opportunistic fixes in `residual-gate-findings.md`, catalog READMEs for pre-migration paths. |

---

## Landed repair migration (closure checklist)

For the record — all complete as of 2026-06-25:

- Phases **1–3.5**: `test-tier.ps1`, feature gates, `clip-sync-repair-harness`, explicit `[[test]]`
- **integration_gap_corpus**, **validate_patch_audio**, **diag_patch_audio**, **integration_energy_patch**
- **seam_residual_oracle** → `diagnostic-tests`
- CI: `pr` tier, manifest check (`check-repair-test-manifest.ps1`), Clippy (repair + harness)
- Oracle hygiene: `#[ignore]` + oracle tier for slow production-geometry rows (no script `--skip`s)
