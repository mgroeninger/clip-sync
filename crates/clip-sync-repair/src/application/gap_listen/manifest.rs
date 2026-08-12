//! `listen.json` — **production's** disposition for every gap a listen run exported.
//!
//! A WAV directory is otherwise mute about decisions: the clips say what a gap sounds like before
//! and after, and the stem names the gap, but nothing there says whether production patched it,
//! refused it at the gate, dropped it at plan time, or approved it and failed to apply the splice.
//! Until this file existed the only read was an inference from absence — `_a_patched.wav` exists ⇒
//! production patched it — which is sound (the exporter refuses that file on every non-`Patched`
//! status) but collapses four distinct dispositions into "not patched", and the differences are
//! exactly what a listening session is trying to attribute.
//!
//! **Why a sibling file and not the corpus `manifest.json`.** That manifest is written by
//! `write_corpus_dir` *before* the patch runs — dispositions do not exist yet — and it describes the
//! fingerprint dump, which is valid without any patch at all. Rewriting it post-splice would make a
//! `--gap-fingerprints` artifact shape depend on whether `--gap-listen` was passed.
//!
//! **Why not the filename.** `entry_stem` is the single naming authority for the ears ↔ JSON join,
//! and a disposition encoded there would be a second source of truth — precisely the drift that got
//! the old verdict token removed from stems (2026-08-12), with the extra hazard that production's
//! disposition is the *more* authoritative of the two and so would look correct while diverging.

use std::path::Path;

use serde::Serialize;

use crate::application::error::RepairError;
use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::patch_result::{
    format_gap_patch_not_applied_reason, format_gap_patch_skip_reason, GapPatchStatus,
};

pub(super) const LISTEN_MANIFEST_FILE: &str = "listen.json";

/// Stamped at the root of every `listen.json`, because the distinction it names is the one that
/// cost a real investigation: a fingerprint's `outcome.tier` is the **bracket gate**, and a gap it
/// calls `skip` is routinely patched by production's `skip_or_dual_fit`.
const LISTEN_CONTRACT: &str = "disposition is production's GapPatchStatus for this run (the patch \
     gate that wrote the WAVs) — NOT the fingerprint's outcome.tier, which is the bracket-gate \
     any_ok result and disagrees by design: a fingerprint `skip` is often a production `patched`.";

/// One exported gap, keyed by the stem its WAVs and fingerprint JSON share.
#[derive(Serialize)]
pub(super) struct ListenEntry {
    pub(super) stem: String,
    /// 0-based report index — the same `index` the corpus `manifest.json` and gap JSON carry.
    pub(super) index: usize,
    /// Mirrors [`GapPatchStatus`]'s variant names (`patched` / `skipped` / `not_planned` /
    /// `not_applied`), plus `absent` when the run produced no outcome for the gap at all. Mirrored
    /// rather than renamed so this file and the gap table cannot grow separate vocabularies.
    pub(super) disposition: &'static str,
    /// The gate's own words, where the status carries a reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reason: Option<String>,
    /// `high` / `marginal` on a patch — the warn-band a listener most wants flagged before playing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) confidence: Option<&'static str>,
    /// Whether `<stem>_a_patched.wav` was written. Not redundant with `disposition`: an early
    /// return (empty plan, no patched PCM) leaves `patched` gaps with no post-splice clip.
    pub(super) a_patched_wav: bool,
    /// `Some(true)` when the patched A window came back byte-identical to the pre-splice one — the
    /// "reports Patched, sounds unchanged" trap the export loop warns about. `None` when no patched
    /// window was compared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) a_window_unchanged: Option<bool>,
}

impl ListenEntry {
    /// Read the disposition off the production status. `None` ⇒ the run recorded no outcome for
    /// this gap, which is reported as `absent` rather than guessed at.
    pub(super) fn new(stem: String, index: usize, status: Option<&GapPatchStatus>) -> Self {
        let (disposition, reason, confidence) = match status {
            Some(GapPatchStatus::Patched { confidence, .. }) => (
                "patched",
                None,
                Some(match confidence {
                    FillConfidence::High => "high",
                    FillConfidence::Marginal => "marginal",
                }),
            ),
            Some(GapPatchStatus::Skipped { reason }) => {
                ("skipped", Some(format_gap_patch_skip_reason(reason)), None)
            }
            Some(GapPatchStatus::NotPlanned { reason }) => (
                "not_planned",
                Some(crate::domain::gap_tags::format_plan_skip_reason(reason).to_string()),
                None,
            ),
            Some(GapPatchStatus::NotApplied { reason }) => (
                "not_applied",
                Some(format_gap_patch_not_applied_reason(reason)),
                None,
            ),
            None => ("absent", None, None),
        };
        Self {
            stem,
            index,
            disposition,
            reason,
            confidence,
            a_patched_wav: false,
            a_window_unchanged: None,
        }
    }

    /// Record that `<stem>_a_patched.wav` was written, and whether it changed anything.
    pub(super) fn with_patched_wav(mut self, unchanged: bool) -> Self {
        self.a_patched_wav = true;
        self.a_window_unchanged = Some(unchanged);
        self
    }
}

#[derive(Serialize)]
struct ListenManifest<'a> {
    #[serde(rename = "_contract")]
    contract: &'static str,
    a_id: &'a str,
    b_id: &'a str,
    /// Entries in this file — the **exported** gaps, not the pair's gap count. A listen run is
    /// routinely a filtered subset, and gaps that reached no fingerprint entry have no stem to key.
    exported_gap_count: usize,
    entries: Vec<ListenEntry>,
}

/// Write `listen.json` beside the WAVs. Opaque ids only — no titles or paths, same rule as the
/// corpus dump — so a WAV directory can still be joined back to the corpus that named it.
pub(super) fn write_listen_manifest(
    dir: &Path,
    a_id: &str,
    b_id: &str,
    entries: Vec<ListenEntry>,
) -> Result<(), RepairError> {
    let manifest = ListenManifest {
        contract: LISTEN_CONTRACT,
        a_id,
        b_id,
        exported_gap_count: entries.len(),
        entries,
    };
    let path = dir.join(LISTEN_MANIFEST_FILE);
    let file = std::fs::File::create(&path)
        .map_err(|e| RepairError::Config(format!("create {}: {e}", path.display())))?;
    serde_json::to_writer_pretty(file, &manifest)
        .map_err(|e| RepairError::Config(format!("write {}: {e}", path.display())))?;
    Ok(())
}
