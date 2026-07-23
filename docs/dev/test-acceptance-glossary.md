# Test and operator acceptance glossary

Stable IDs for **what** we prove (acceptance rows, operator vocabulary, gate contracts) and how
that relates to **where** tests run (CI tiers — see [development.md](development.md)).

**Rules:**

1. Do not reuse one letter for two meanings (legacy `P*` / `C*` are deprecated in new docs).
2. Do not use **plan** in acceptance IDs — it collides with fill-plan product terms and `TEMP-*-plan.md` docs.
3. New tests and edited docs use the prefixes below; rename code opportunistically (no big-bang pass).
4. **F*** fixture IDs are geometry, not acceptance tier.

**Harness vs acceptance IDs:** SD/SP/EC/RG name *what* a row proves; harness code is split —
fixtures in `clip-sync-repair-fixtures`, runners in `clip-sync-repair-harness`, catalogs in `tests/*_catalog/`,
`#[test]` in tier binaries. See [development.md](development.md) and [archive/test-tier-plan.md](archive/test-tier-plan.md#harness-organization-fixtures-runners-catalogs).

---

## Prefix overview

| Prefix | Name | Purpose | Typical home |
|--------|------|---------|--------------|
| **GK** | Gap kind | Operator/report classification (maps to `plan_kind`) | [gap-repair-guide.md](../gap-repair-guide.md) Layer 1 |
| **CS** | Content shape | Acoustic/editorial seam hint | [gap-repair-guide.md](../gap-repair-guide.md) Layer 2 |
| **F** | Fixture geometry | Synthetic scenario layout (F1, F1-long, F4-decoy, …) | `clip-sync-repair-fixtures/`, guide § Corpus fixtures |
| **SD** | Signature domain | Structure-match oracle on short in-memory fixtures | `tests/oracle_energy.rs` (integration, oracle label) |
| **SP** | Signature patch | Domain + haystack + full `PatchAudio` on 8 s fixtures | `tests/integration_energy_patch.rs` (SP01–SP03), `tests/patch_audio_integration.rs` (SP04), `tests/validate_patch_audio.rs` (SP05) |
| **EC** | Energy corpus | Production-geometry signature acceptance | `tests/oracle_energy.rs` (domain), `tests/integration_energy_smoke.rs` (scan/e2e), `tests/integration_energy_patch.rs` (SP01–SP03 patch), `tests/validate_residual_gate.rs` (EC-6 patch, validation), `tests/diag_energy_matrix.rs` (matrix, diagnostic) |
| **RG** | Residual gate claim | Veto/rescue gate validity contract | [residual_gate_catalog/README.md](../../crates/clip-sync-repair/tests/residual_gate_catalog/README.md), `matrix.toml` |
| **PL** | Placement mode | Who picks B alignment in residual/floor harness | `matrix.toml` `placement` |
| **CHK** | Signature checklist | One-off ship criteria (verbose output, fixture port) | [energy-signature-plan.md](archive/energy-signature-plan.md) |

### Where acceptance code lives (harness)

| Layer | Path | Holds |
|-------|------|-------|
| Fixtures | `crates/clip-sync-repair-fixtures/` | F* builders, production helpers — no `#[test]` |
| Runners | `clip-sync-repair-harness` (dev-dep of repair) | Shared pipeline drivers (floor oracle, residual gate, seam residual, energy matrix) |
| Catalogs | `tests/residual_gate_catalog/`, `tests/floor_oracle/`, `tests/gap_corpus/` | `matrix.toml`, manifests, baselines — not binaries |
| Tests | `tests/<tier>_*.rs` | Thin `#[test]` asserting SD/SP/EC/RG rows |

**CI tiers** (unit / integration / validation / diagnostic) describe **when** tests run, not
acceptance ID families. "oracle" is a *label* (the `oracle_` name prefix for domain-acceptance
rows), not a tier — those rows schedule as integration. See
[development.md](development.md). Rough mapping:

| Tier | Acceptance families |
|------|---------------------|
| unit | SD (in domain modules), pure GK/CS tag logic |
| integration | SP, gap_corpus, patch sine rows; **oracle label** — SD, EC domain, seam score harness |
| validation | RG, EC6, floor oracle, real codec |
| diagnostic | RG05 CSV, energy mode matrix, golden generators |

---

## GK — gap kind (operator vocabulary)

Layer 1 in [gap-repair-guide.md](../gap-repair-guide.md). Describes how a gap appears in scan/fill
output — **not** a TEMP implementation plan.

| ID | Meaning | `plan_kind` / status |
|----|---------|----------------------|
| **GK0** | Below scan floor | not listed (`< min_gap_ms`) |
| **GK1** | Unfillable — no B overlap | `unfillable` |
| **GK2** | Unfillable — B dry | `unfillable` |
| **GK3** | Not planned — outside coverage | `not_planned` |
| **GK4** | Not planned — tracks / layout | `not_planned` |
| **GK5** | Fillable (typical ~1–30 s) | enters patch |
| **GK6** | Fillable long / tail | enters patch; often structure skip |
| **GK7** | Audible hole, not scanned | absent from report |

**Legacy:** guide **P0–P7** → **GK0–GK7**.

---

## CS — content shape (operator vocabulary)

Layer 2 in [gap-repair-guide.md](../gap-repair-guide.md). Acoustic/editorial hint; assumes **GK5**
fillable unless noted.

| ID | Shape |
|----|-------|
| **CS1** | Silence / room tone |
| **CS2** | Music / ambience dropout |
| **CS3** | Boundary gap (music or pause → speech) |
| **CS4** | Speech / dialog dropout |
| **CS5** | Long tail / end-of-file silence |

**Legacy:** guide **C1–C5** (content) → **CS1–CS5**. Not the same as **RG** (residual gate).

---

## SD — signature domain oracle

Short-fixture structure signature proofs at domain layer (`unified_match`, `score_pre_*`).
Defined in [energy-signature-plan.md](archive/energy-signature-plan.md).

| ID | Fixture | Mode / focus |
|----|---------|--------------|
| **SD01** | F1 | energy `score_pre` true > decoy |
| **SD02** | F1 | bool pre ambiguous |
| **SD03** | F1 | energy unified → true offset |
| **SD04** | F1 | bool unified decoy or worse than energy |
| **SD05** | F2 | energy unified → pause₁ |
| **SD05b** | F2_integration | SD05 on 8 s builder |
| **SD05c** | F2 @ 48 kHz | scaled unit geometry |
| **SD06** | F2 | bool ambiguous / nominal pause₂ |
| **SD07** | F3 silence | `auto` → bool |
| **SD08** | F3 drone | energy ≈ bool scores |

**Legacy:** **U1–U8** → **SD01–SD08**; test fns `u1_*` … `u8_*` until renamed (`sig_sd01_*` target).

---

## SP — signature patch pipeline

8 s integration: domain + haystack + full `PatchAudio`. Shared helper:
`assert_energy_integration_patch` in `clip-sync-repair-harness` (`patch_audio` module).

| ID | Fixture | Focus | Binary | PR |
|----|---------|-------|--------|-----|
| **SP01** | F1 | energy — all three layers agree | `integration_energy_patch.rs` (`i1_*`) | no — `integration` tier |
| **SP02** | F1 | bool domain closer to decoy than energy | `integration_energy_patch.rs` (`i2_*`) | no |
| **SP03** | F2 | energy @ pause₁, slide ≈ 0 | `integration_energy_patch.rs` (`i3_*`) | no |
| **SP04** | F3 | `auto` domain ≡ bool | `patch_audio_integration.rs` (`i4_*`) | no — `pr-repair-extended` |
| **SP05** | — | production-default fit smoke (was I5) | `validate_patch_audio.rs` | no — `validation` tier |

PR energy patch coverage uses `corpus_scan_patch_smoke` in `integration_energy_smoke.rs` (EC01
e2e tripwire) plus SD domain rows on `oracle_energy.rs` — not the 8 s SP fixtures.

**Legacy:** **I1–I5** → **SP01–SP05**; test fns `i1_*` … `i4_*` until renamed (`sig_sp01_*` target).

---

## EC — energy corpus acceptance

Production-geometry fixtures (`build_*_production`, F*-long, F4-decoy). Canonical IDs — keep **EC**
prefix; do not add parallel `p1_` numbering in new work.

| ID | Fixture | Assertion |
|----|---------|-----------|
| **EC01** | F1-long | energy unified → true offset (domain); patch: `f1_production_scan_patch_smoke` |
| **EC02** | F2-long | energy @ pause₁ (domain); patch: `f2_production_oracle_patch_smoke` |
| **EC03** | F3-long | `auto` → bool |
| **EC04** | F1-long | `auto` regression vs suite defaults |
| **EC05** | F1-long 120 s | context 30 within wall budget |
| **EC06** | F4-decoy | energy/`auto` → true pause; bool → decoy |

**Legacy:** **EC-1–EC-6** = **EC01–EC06**; lib tests `p1_*` … `p4_*` map to EC01–EC03, EC06.

---

## RG — residual gate contract

Gate validity claims. Full semantics:
[residual_gate_catalog/README.md](../../crates/clip-sync-repair/tests/residual_gate_catalog/README.md).

| ID | Claim |
|----|-------|
| **RG01** | Veto fires on echo/decoy (Pearson OK, residual rejects) |
| **RG02** | Abstain out-of-regime (two-mic, uninformative floor) |
| **RG03** | Never false-veto a truth gap |
| **RG04** | `residual_gate = off` is a true no-op |
| **RG05** | Rescue real-media value (resolved: no; diagnostic only) |

**Legacy:** residual README **C1–C5** → **RG01–RG05**; `matrix.toml` `claims` to migrate when edited.

---

## PL — test placement mode

Who picks B fill placement in residual/floor harness rows (`matrix.toml` `placement`).

| ID | Mode |
|----|------|
| **PL0** | Harness truth frame |
| **PL1** | Oracle nominal (manifest + refine) |
| **PL2** | Search winner (`production_fit`) |
| **PL3+** | Sweep / grid / field |

**Legacy:** residual README **P0–P3** placement → **PL0–PL3**. Not guide **GK2** (B dry).

---

## CHK — signature feature checklist

Historical ship criteria from energy signature Phase 2 — not gap kinds.

| ID | Item |
|----|------|
| **CHK01** | Verbose fill plan includes resolved `signature_mode=` |
| **CHK02** | SP fixtures share helpers with SD |
| **CHK03** | Optional CI: SP01–SP04 with `gap_signature_mode = energy` |

**Legacy:** **P2-1–P2-3** → **CHK01–CHK03**.

---

## Legacy quick lookup

| You see… | Meant… | Use instead |
|----------|--------|-------------|
| U3 | Domain energy unified on F1 | **SD03** |
| I1 | Full patch on F1 energy | **SP01** |
| `p1_f1_*` | EC01 domain test | **EC01** / `sig_ec01_*` |
| P2-1 | Verbose `signature_mode` | **CHK01** |
| Guide P5 | Fillable gap | **GK5** |
| Guide C3 | Boundary content | **CS3** |
| Gate C3 | No false veto | **RG03** |
| Gate P2 placement | Search winner | **PL2** |
| Tier plan “P1/P4 oracles” | Production EC domain rows | **EC01**, **EC06** |

---

## Related docs

- [development.md](development.md) — CI tiers, file layout, `test-tier.ps1` (living reference)
- [archive/test-tier-plan.md](archive/test-tier-plan.md) — migration history
- [gap-repair-guide.md](../gap-repair-guide.md) — GK / CS in operator workflow
- [corpus-validation.md](corpus-validation.md) — alignment corpus + EC rows
- [energy-signature-plan.md](archive/energy-signature-plan.md) — SD / SP / CHK definitions
- [archive/energy-corpus-plan.md](archive/energy-corpus-plan.md) — EC acceptance detail
- [residual_gate_catalog/README.md](../../crates/clip-sync-repair/tests/residual_gate_catalog/README.md) — RG contract
