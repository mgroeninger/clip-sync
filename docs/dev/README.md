# clip-sync developer docs

Build, test, corpus, and design plans. Operator guides and output contracts stay in [../](../) (parent `docs/`).

## Where things go

Four venues, one destination each. Putting a thing in the wrong one is how the backlog turned into
an index of dead ideas.

| Venue | Holds | Does **not** hold |
|-------|-------|-------------------|
| [../../BACKLOG.md](../../BACKLOG.md) | Work someone could start tomorrow: problem, next action, done-when | History, rationale, prohibitions, blocked items |
| [settled.md](settled.md) | **The verdict ledger** — refuted ideas, deferrals latched behind a trigger, items blocked on a precondition, and standing measurement cautions. One row each | The reasoning that produced the verdict |
| [archive/](archive/) | Closed plans in full: narrative, palimpsests, why a thing was decided | Current behaviour |
| [gap-vocabulary.md](gap-vocabulary.md), [gap-fingerprint.md](gap-fingerprint.md), [repair-perf.md](repair-perf.md), [shoulder-sweep.md](shoulder-sweep.md) | Durable facts about the domain and current behaviour | Opinions about ideas |

**Closing a plan is a three-step move**, and the third step is the one that gets skipped:

1. Move the `TEMP-*.md` to [archive/](archive/) and add its row to [archive/README.md](archive/README.md).
2. Add a row to [settled.md](settled.md) — the verdict plus the guard, one line.
3. **Delete the corresponding text from [BACKLOG.md](../../BACKLOG.md).** A refuted idea leaves no
   trace there; a blocked one moves to `settled.md` § D.

Before proposing anything to BACKLOG, read [settled.md](settled.md). Most settled ideas are
plausible on their face — that is exactly why they keep coming back.

## Testing & development

| Doc | Covers |
|-----|--------|
| [test-tiers.md](test-tiers.md) | **How to run test tiers** — `test-tier.ps1`, composite profiles, prerequisites |
| [development.md](development.md) | Build, features, integration binary matrix, `#[ignore]` scheduling |
| [test-tier-remainder.md](test-tier-remainder.md) | Deferred tiers (2b, ignore follow-ups, nextest, validate crate) |
| [corpus-validation.md](corpus-validation.md) | Test corpus tiers, acceptance, energy-signature corpus |
| [corpus-matrix.md](corpus-matrix.md) | Alignment corpus matrix |
| [test-acceptance-glossary.md](test-acceptance-glossary.md) | SD/SP/EC/RG acceptance IDs |

## Performance

| Doc | Covers |
|-----|--------|
| [repair-perf.md](repair-perf.md) | **Where repair time goes** — current measured baseline, how to measure (`measure-repair-perf.ps1`), settled/refuted candidates, open candidates, media-hygiene rule |

## Domain / corpus tooling

| Doc | Covers |
|-----|--------|
| [gap-vocabulary.md](gap-vocabulary.md) | **Gap numbering** (0-based data / 1-based display), gap cells, silence-character pre-gate, fixture mapping |
| [gap-fingerprint.md](gap-fingerprint.md) | Gap fingerprint dump schema and corpus format; bulk dump via `measure-gap-fingerprints.ps1` |
| [shoulder-sweep.md](shoulder-sweep.md) | **`shoulder-sweep` bin contract** — the independent wide-window (±2 s) waveform lag sweep of both gap shoulders on the `--gap-listen` WAVs. Shares no correlation code with the `lag_decision` it checks; measures and records, does not classify |
| [settled.md](settled.md) | **Verdict ledger** — what has been refuted, latched behind a trigger, or blocked, and the evidence that settled it. Read before proposing work |

## Plans

**One deliverable per plan.** Each active plan carries a verification rule: a `file:line` reference
belongs only in a checklist item that is about to be executed; design sections state the decision and
its reason. Closed plans move to [archive/](archive/) — see § *Where things go* for the three-step
move, and do not treat archive docs as current behaviour.

Every `TEMP-*.md` in this folder appears below. A plan with **no** BACKLOG row and **no** settled row
is untracked — that is a bug in the catalog, not a state a doc may sit in.

### Active drafts

| Draft | What it is | Tracked as |
|-------|-----------|-----------|
| [TEMP-patch-audio-region-module-split-plan.md](TEMP-patch-audio-region-module-split-plan.md) | Folder split of `region.rs` (3633 lines). Byte-preserving; never bundled into a behaviour-change PR | [BACKLOG.md](../../BACKLOG.md) § *Repair R6* |
| [TEMP-flac-output-plan.md](TEMP-flac-output-plan.md) | In-process `--flac` lossless output, peer of `--wav`. Motivated by the RIFF `u32` data-chunk limit | [BACKLOG.md](../../BACKLOG.md) § *Repair R6* |
| [TEMP-nway-donor-alignment-plan.md](TEMP-nway-donor-alignment-plan.md) | Repair one damaged copy from **multiple** donors with sparse non-overlapping damage | [settled.md](settled.md) § C — latched, no corpus |

### Findings notes (not Changes)

Measurement records — none are open right now. A findings note lives here only while it has an open
question; once every question is promoted to [BACKLOG.md](../../BACKLOG.md) or closed, it archives.
All four notes open on 2026-08-24 (the two flat-3000 runs, the collapsed-bridge census, the pair-50
outcomes) went to [archive/](archive/) that day under exactly that rule, and their surviving questions
are [BACKLOG.md](../../BACKLOG.md) § *Post-join defects (pairs 31 and 50)*.

> Architecture: [../../PLAN.md](../../PLAN.md). Open work: [../../BACKLOG.md](../../BACKLOG.md).
> Settled verdicts: [settled.md](settled.md).

