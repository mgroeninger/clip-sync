# clip-sync docs

Reference and operational documentation for `clip-sync` (analyzer) and `clip-sync-repair`. For architecture (crates, hexagonal layers, ports) see [../PLAN.md](../PLAN.md); for open work see [../BACKLOG.md](../BACKLOG.md).

## Pipeline & internals

Start here for "how does it work?":

| Doc | Covers |
|-----|--------|
| [pipeline.md](pipeline.md) | **Map of the repair pipeline** — align → scan → fill plan → per-gap patch → write/mux, with links to each phase |
| [alignment.md](alignment.md) | Phase 1 — fingerprint alignment: modes, clip windows, refinement, query-reference, drift |
| [gap-scan.md](gap-scan.md) | Phase 2 — silence detection and fillable/unfillable classification |
| [seam-scoring.md](seam-scoring.md) | Phase 4d — how `pre`/`post` seams are identified and scored |

## Operator guides

How to read and steer a run:

| Doc | Covers |
|-----|--------|
| [gap-repair-guide.md](gap-repair-guide.md) | Classifying gaps, tiers, seam shapes, signature modes, profiles, vocabulary |
| [gap-fill-modes.md](gap-fill-modes.md) | `fit` vs `gate`, flag interactions, performance, config keys |

## Output contracts

| Doc | Covers |
|-----|--------|
| [cli-output.md](cli-output.md) | Human report layout, gap outcomes, timeline/duration warnings |
| [json-output.md](json-output.md) | `--format json` schema, `GapPatchStatus`, fields |
| [error-mapping.md](error-mapping.md) | Errors → exit codes |

## Testing & development

| Doc | Covers |
|-----|--------|
| [development.md](development.md) | Build, features, test commands, fixture regeneration |
| [corpus-validation.md](corpus-validation.md) | Test corpus tiers, acceptance, energy-signature corpus |
| [corpus-matrix.md](corpus-matrix.md) | Alignment corpus matrix |

## Plans

- **Active drafts:** `TEMP-*.md` in this folder (e.g. [TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md), [TEMP-repair-profiles-plan.md](TEMP-repair-profiles-plan.md), [TEMP-ac3-backend-plan.md](TEMP-ac3-backend-plan.md)).
- **Shipped:** [archive/](archive/) — historical design records (do not treat as current behavior; links may be stale).

> **Conventions:** living reference docs sit flat in `docs/`; active plans use the `TEMP-` prefix; shipped plans move to `archive/` (frozen, original links). See [pipeline.md](pipeline.md) for the execution flow vs. [gap-repair-guide.md](gap-repair-guide.md) for the operator-decision lens vs. [../PLAN.md](../PLAN.md) for architecture.
