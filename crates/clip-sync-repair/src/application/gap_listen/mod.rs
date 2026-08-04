//! `--gap-listen`: fingerprint JSON + listenable A/B/patched WAVs for selected gaps, from **one**
//! decode.
//!
//! The point is to hear what the numbers describe. For each selected gap this writes the gap plus
//! context from A, the mapped donor span plus the same context from B, and — when the **production**
//! gate patches — the identical A window after the splice. The three clips share a filename stem
//! with the gap's fingerprint JSON, so ears and numbers join.
//!
//! **The patched clip must come from the production engine.** The audio is produced by
//! `characterize_region` → `execute_region_spec` → `splice_into_a`, the same path `--wav` uses to
//! decide patch vs skip. The fingerprint oracle is a *diagnostic* characterization whose `any_ok`
//! can disagree with the production gate by design; splicing from it would answer a different
//! question than "is the repair this tool would actually make any good?". On a production skip no
//! patched WAV is written — an invented fill would be worse than a missing file.
//!
//! Ordering is load-bearing: fingerprint (shared borrow of the decode) → export A/B (shared borrow)
//! → move the decode into the patch (mutable). Anything exported after the splice reads patched
//! audio while claiming to be the "before".
//!
//! See `docs/dev/gap-fingerprint.md` (§ `--gap-listen`); design record:
//! `docs/dev/archive/TEMP-gap-listen-wav-plan.md`.

mod export;

use std::collections::HashMap;
use std::path::PathBuf;

use clip_sync::{MediaReader, ProgressReporter};

use crate::application::error::RepairError;
use crate::application::gap_fingerprint::{
    characterize_gaps_from_decode, entry_stem, write_corpus_dir, CharacterizeAbPcm,
    IncomparableReason,
};
use crate::application::patch_audio::{
    decode_ab, log_patch_preamble, patch_audio_span, PatchAudio, PatchAudioRequest,
    PatchAudioResult,
};
use crate::application::ports::PatchedAudioWriter;
use crate::domain::gap_fill::build_gap_fill_plan;
use crate::domain::gap_tags::format_plan_skip_reason;
use crate::domain::patch_result::{
    format_gap_patch_not_applied_reason, format_gap_patch_skip_reason, GapPatchStatus,
};

use export::{frame_range, slice_window, window_digest, write_window_wav, FrameRange};

const A_SURROUND_SUFFIX: &str = "_a_surround.wav";
const B_SURROUND_SUFFIX: &str = "_b_surround.wav";
const A_PATCHED_SUFFIX: &str = "_a_patched.wav";

/// Gap count above which an unbounded listen run gets a warning before it starts working.
///
/// `--gap-listen` with no `--fingerprint-gap` selects every gap in the pair, and each one costs
/// three WAVs *and* a production characterize — the dominant term in a repair run. On a feature-
/// length pair that is a multi-hour job producing gigabytes, from a command that looks identical to
/// the three-gap one. 25 is a judgment call, not a measurement: comfortably above any deliberate
/// hand-picked set, comfortably below a real corpus pair's gap count.
const UNBOUNDED_LISTEN_WARN_GAPS: usize = 25;

/// Total audio seconds a listen run will write, for the [`UNBOUNDED_LISTEN_WARN_GAPS`] warning.
///
/// Three clips per gap, each the gap plus `context` on both sides. Deliberately computed from the
/// report rather than the decode so the warning lands **before** the decode — a warning that
/// arrives after the expensive part has started is just a log line.
fn projected_export_secs(gaps: &[crate::domain::gap::Gap], select: &[usize], context: f64) -> f64 {
    select
        .iter()
        .filter_map(|&i| gaps.get(i))
        .map(|gap| (gap.video_a_end_secs - gap.video_a_start_secs).max(0.0) + 2.0 * context)
        .sum::<f64>()
        * 3.0
}

/// Where a listen run puts things, plus the dump settings it shares with `--gap-fingerprints`.
///
/// `fingerprint_dir` and `wav_dir` are separate because the JSON corpus is the artifact of record
/// and the WAVs are licensed-media derivatives that must stay gitignored; they may resolve to the
/// same directory, which is the default.
pub struct GapListenRequest {
    pub fingerprint_dir: PathBuf,
    pub wav_dir: PathBuf,
    pub fingerprint_diagnostics: bool,
    pub crossfade_ms: u64,
}

/// One gap's A window, held from the pre-splice export until after the patch.
///
/// The **frame range is captured before the splice and reused verbatim afterwards**. That is what
/// makes the before/after pair sample-aligned by construction: `splice_into_a` is length-preserving
/// and in place, so the same frames describe the same instants in both buffers. It also means the
/// anchored-retry pass — which can replace a patch after pass 2 — cannot invalidate the window,
/// because nothing here is derived from a spec.
struct PendingWindow {
    gap_index: usize,
    stem: String,
    a_range: FrameRange,
    /// Whether this gap reached the fill plan. Gaps dropped at plan time already reported *why* at
    /// export time, so the post-patch pass stays quiet about them rather than re-reporting the same
    /// gap as a gate decision it never reached.
    planned: bool,
    /// Digest of the pre-splice A window, so the patched window can be checked for having changed
    /// nothing. See the `Patched` arm of the post-patch loop for why that is possible at all.
    a_pre_digest: u64,
}

/// Scan → one decode → fingerprint JSON → A/B WAVs → production patch → patched WAVs.
///
/// Returns the production [`PatchAudioResult`] so the caller can report it. The clips alone say
/// *that* a gap was or wasn't patched; the summary says **why**, in the gate's own words, and it is
/// the same object a `--repair-preview` run prints — so a listen run gets the ordinary gap table
/// rather than a second, listen-specific rendering of the same verdicts. Returned even when the
/// patch produced no PCM: "nothing was patched" is a finding, and the reasons live in the summary.
pub fn run_gap_listen<MR: MediaReader, PW: PatchedAudioWriter>(
    media_reader: &MR,
    wav_writer: &PW,
    progress: &dyn ProgressReporter,
    patch_request: PatchAudioRequest,
    listen: GapListenRequest,
) -> Result<PatchAudioResult, RepairError> {
    // ── Step 1: one selector, two consumers ────────────────────────────────────────────────────
    // The fingerprint `select` is *derived* from the resolved patch selection rather than parsed
    // separately, so the corpus and the patch plan can never cover different gap sets.
    let select = patch_request.gap_selection.selected_indices();
    progress.phase(&format!(
        "gap-listen: {} gap(s) selected for fingerprint + WAV export",
        select.len()
    ));
    if select.len() > UNBOUNDED_LISTEN_WARN_GAPS {
        // Before the decode, so it is still actionable. `eprintln!` for the same reason as the
        // no-op-splice warning: this must not be suppressible by `--progress off`, because the
        // whole point is to let the user abort a run that is about to cost hours.
        eprintln!(
            "warning: gap-listen: {} gaps selected — this writes ~{} WAVs (~{:.0} min of audio) \
             and runs a production characterize on every one of them, which is the dominant cost \
             of a full repair. Narrow it with --fingerprint-gap if you meant to listen to a few.",
            select.len(),
            select.len() * 3,
            projected_export_secs(
                &patch_request.report.gaps,
                &select,
                patch_request.gap_signature_context_secs,
            ) / 60.0,
        );
    }

    // ── Step 2: the one decode ─────────────────────────────────────────────────────────────────
    let decoded = decode_ab(media_reader, &patch_request.report, progress)?;

    // ── Step 3: fingerprint, pre-splice, shared borrow only ────────────────────────────────────
    // Fingerprint characterize treats empty `select` as "all" (`take_all`). Passing a materialized
    // `0..n` list disables that and turns the filter into O(n²) `Vec::contains`. Same corpus either
    // way — a HashSet of validated indices with `len == gaps.len()` is necessarily the full set.
    // Keep the full `select` for export / warnings; only the characterize argument uses the
    // empty-means-all convention the dump path already uses.
    let fingerprint_select: &[usize] = if select.len() == patch_request.report.gaps.len() {
        &[]
    } else {
        &select
    };
    let corpus = characterize_gaps_from_decode(
        &patch_request.report,
        &CharacterizeAbPcm {
            a_pcm: &decoded.a_pcm,
            b_samples: &decoded.b_samples_full,
            sources: Some(&decoded.sources),
        },
        &patch_request,
        fingerprint_select,
        listen.fingerprint_diagnostics,
        progress,
    );

    // A layout-mismatch pair yields a provenance-only corpus with `gaps` empty: no stems, no plan,
    // no patched WAV. Refuse the whole run rather than emitting unnamed A/B clips — a directory of
    // clips that cannot be joined back to any fingerprint is a trap, not a diagnostic.
    if let Some(IncomparableReason::ChannelLayoutMismatch) = corpus.source.incomparable {
        return Err(RepairError::Config(format!(
            "channel layout mismatch (A {}ch / B {}ch): refused pairwise fingerprint characterize, \
             so --gap-listen has no fingerprints to name WAVs after and no fill plan to patch from",
            decoded.sources.a.native_channels, decoded.sources.b.native_channels,
        )));
    }

    let written = write_corpus_dir(&corpus, &listen.fingerprint_dir).map_err(|e| {
        RepairError::Config(format!(
            "write gap corpus to {}: {e}",
            listen.fingerprint_dir.display()
        ))
    })?;
    progress.phase(&format!(
        "wrote corpus.json + {written} per-gap fingerprints + manifest.json to {}",
        listen.fingerprint_dir.display()
    ));
    // §5: a filtered listen dump covers only the listened gaps. That is intentional (cheap), but
    // the directory looks like a normal corpus — so say so when it is not the full-pair dump.
    let gap_count = patch_request.report.gaps.len();
    if select.len() < gap_count {
        progress.phase(&format!(
            "gap-listen: corpus covers {} of {gap_count} gaps — partial dump, not the corpus of \
             record for band analysis; keep roll-up tooling on a full-pair run",
            select.len(),
        ));
    }

    // The stem is a function of the built fingerprint (tier / verdict / refined start), not of the
    // gap, so it can only be looked up — never derived from the index.
    let stems: HashMap<usize, String> = corpus
        .gaps
        .iter()
        .map(|gap| (gap.index, entry_stem(&corpus.source, gap)))
        .collect();

    std::fs::create_dir_all(&listen.wav_dir).map_err(|e| {
        RepairError::Config(format!(
            "create WAV directory {}: {e}",
            listen.wav_dir.display()
        ))
    })?;

    // ── Step 4: the plan, built ONCE and handed to the patch below ─────────────────────────────
    let plan = build_gap_fill_plan(
        &patch_request.report,
        listen.crossfade_ms,
        patch_request.skip_equivalent_gaps,
        &patch_request.gap_selection,
    );
    let region_by_gap: HashMap<usize, &crate::domain::gap_fill::FillRegion> =
        plan.regions.iter().map(|r| (r.gap_index, r)).collect();
    // Why each dropped gap was dropped. Reporting "no donor mapping" for all of them would be a
    // lie on the case that matters most: with `skip_equivalent_gaps` on (the default) the dominant
    // reason is `already_matches_reference`, and equivalence-dropped gaps are precisely the ones
    // the margin-band experiment exists to listen to.
    let plan_skip_by_gap: HashMap<usize, &crate::domain::patch_result::GapFillSkipReason> = plan
        .skipped
        .iter()
        .map(|s| (s.gap_index, &s.reason))
        .collect();

    // ── Step 5: pre-splice A/B windows, sliced by this caller ──────────────────────────────────
    // Plan geometry, not the production engine's refined edges. Two things move an edge past the
    // plan bounds: edge refinement (GAP_EDGE_REFINE_SECS, 0.75 s) and the seam-fail extension
    // retries (gap_{start,end}_extend_max_ms, 500 ms, both enabled by default) — so up to ~1.25 s
    // per edge. The export pads by `context` (3 s default), so a plan-geometry window still
    // contains the final throat with ≥1.75 s of real context per side. Not worth threading an
    // export callback through the patch hot loop to recover the difference.
    let context = patch_request.gap_signature_context_secs;
    let sample_rate = decoded.a_pcm.sample_rate;
    let a_channels = decoded.a_pcm.channels;
    let a_bit_depth = decoded.a_pcm.source_bit_depth;
    let a_frames = decoded.a_pcm.frames();
    // B's own layout — `decode_ab` resamples B to A's *rate* but never remixes its *channels*.
    // From the decoded buffer, not `sources.b.native_channels`: that is container/probe metadata,
    // and striding a buffer by a number sourced from somewhere else is the §5.1 trap with extra
    // steps. This is the only place in the workspace that strides real PCM by a channel count that
    // did not come from the buffer itself — everywhere else `native_channels` is provenance or the
    // layout-mismatch gate.
    let b_channels = decoded.b_channels;
    let b_bit_depth = decoded.sources.b.bit_depth;
    let b_frames = decoded.b_samples_full.len() / b_channels.max(1) as usize;

    let mut pending: Vec<PendingWindow> = Vec::with_capacity(select.len());
    for &gap_index in &select {
        let Some(stem) = stems.get(&gap_index) else {
            progress.phase(&format!(
                "gap-listen: gap #{} has no fingerprint entry (not characterizable); no WAVs",
                gap_index + 1
            ));
            continue;
        };
        let gap = &patch_request.report.gaps[gap_index];
        let region = region_by_gap.get(&gap_index).copied();

        // Selected but unplanned (unfillable, unmapped, or equivalence-dropped): fall back to the
        // raw scan bounds for an A-only clip. They are unrefined, but the padding swallows the
        // difference — the same argument as plan-vs-refined above.
        let (a_start, a_end) = match region {
            Some(r) => (r.a_start_secs, r.a_end_secs),
            None => (gap.video_a_start_secs, gap.video_a_end_secs),
        };
        let a_range = frame_range(a_start - context, a_end + context, sample_rate, a_frames);
        if a_range.is_empty() {
            progress.phase(&format!(
                "gap-listen: gap #{} A window is empty after clamping; skipped",
                gap_index + 1
            ));
            continue;
        }
        let a_pcm = slice_window(
            &decoded.a_pcm.samples,
            a_channels,
            sample_rate,
            a_bit_depth,
            a_range,
        );
        write_window_wav(wav_writer, &listen.wav_dir, stem, A_SURROUND_SUFFIX, &a_pcm)?;

        match region {
            Some(r) => {
                // Same ±context as A, so the two clips are the same shape and directly comparable
                // by ear. (The fingerprint's own haystack pad is far wider — ~14 s lead / 15 s tail
                // at defaults — which answers "what could the search have drawn from", a debugging
                // question, not a listening one.)
                let b_range = frame_range(
                    r.b_start_secs - context,
                    r.b_end_secs + context,
                    sample_rate,
                    b_frames,
                );
                if b_range.is_empty() {
                    progress.phase(&format!(
                        "gap-listen: gap #{} B window is empty after clamping; A only",
                        gap_index + 1
                    ));
                } else {
                    let b_pcm = slice_window(
                        &decoded.b_samples_full,
                        b_channels,
                        // B was resampled to A's rate, so the window plays back at A's rate.
                        sample_rate,
                        b_bit_depth,
                        b_range,
                    );
                    write_window_wav(wav_writer, &listen.wav_dir, stem, B_SURROUND_SUFFIX, &b_pcm)?;
                }
            }
            None => progress.phase(&format!(
                "gap-listen: gap #{} was dropped at plan time ({}); A only, no patched WAV",
                gap_index + 1,
                plan_skip_by_gap
                    .get(&gap_index)
                    .map_or("not planned", |reason| format_plan_skip_reason(reason)),
            )),
        }

        pending.push(PendingWindow {
            gap_index,
            stem: stem.clone(),
            a_range,
            planned: region.is_some(),
            a_pre_digest: window_digest(&a_pcm.samples),
        });
    }

    if plan.regions.is_empty() {
        progress
            .phase("gap-listen: no selected gap reached the fill plan; wrote surround WAVs only");
        // Not `Ok(())`-shaped even though nothing was patched: the caller reports this summary, and
        // "every gap NotPlanned, with its reason" is precisely what the run found. Built by the same
        // helper the executor's own empty-plan path uses, so skipping the executor here cannot make
        // the table disagree with the one a full run would print.
        return Ok(crate::application::patch_audio::empty_plan_write_result(
            &patch_request,
            &plan,
        ));
    }

    // ── Step 6: the PRODUCTION patch, on the decode we already own ─────────────────────────────
    // `decoded` moves here; every export above took only shared borrows, so the mutable splice
    // strictly follows them and nothing can read patched audio as if it were the "before".
    let patch_result = {
        let _span = patch_audio_span(&plan, &patch_request, false).entered();
        log_patch_preamble(progress, &patch_request, false);
        PatchAudio::new(media_reader, progress).execute_with_decoded(
            patch_request,
            plan,
            decoded,
        )?
    };

    // ── Step 7: post-splice windows, at the frame ranges recorded in step 5 ────────────────────
    if patch_result.pcm.is_none() {
        progress.phase("gap-listen: patch returned no PCM; no patched WAVs");
        return Ok(patch_result);
    }
    let patched = patch_result
        .pcm
        .as_ref()
        .expect("pcm presence checked immediately above");

    let mut patched_count = 0usize;
    let mut unchanged_count = 0usize;
    for window in &pending {
        // `outcomes_in_report_order` emits exactly one outcome per report gap, in report order, so
        // the report index is a valid key here.
        let status = patch_result
            .summary
            .gaps
            .get(window.gap_index)
            .map(|outcome| &outcome.status);
        match status {
            Some(GapPatchStatus::Patched { .. }) => {}
            // A real gate decision: the gap was planned, characterized, and refused. This is the
            // finding the listen run exists to surface, so it carries the gate's own reason. Do NOT
            // synthesize a fill — the missing file is part of the finding.
            Some(GapPatchStatus::Skipped { reason }) => {
                progress.phase(&format!(
                    "gap-listen: gap #{} refused by the production gate: {}; no patched WAV",
                    window.gap_index + 1,
                    format_gap_patch_skip_reason(reason),
                ));
                continue;
            }
            // Already reported with its plan-time reason during export. Re-reporting it here as a
            // gate outcome would attribute a decision to a gate that never saw the gap.
            Some(GapPatchStatus::NotPlanned { reason }) => {
                if window.planned {
                    progress.phase(&format!(
                        "gap-listen: gap #{} was planned but reported not-planned ({}); no patched WAV",
                        window.gap_index + 1,
                        format_plan_skip_reason(reason),
                    ));
                }
                continue;
            }
            // The gate approved this gap and the splice then failed, so A is unchanged across it.
            // Writing `_a_patched.wav` here would hand the listener a file that is byte-identical
            // to `_a.wav` under a name that claims otherwise — the exact trap the digest check
            // below exists to catch. The engine now names the fault, so refuse the file outright.
            Some(GapPatchStatus::NotApplied { reason }) => {
                eprintln!(
                    "warning: gap-listen: gap #{} was approved but NOT APPLIED ({}); A is \
                     unchanged across this gap and no patched WAV was written. This is a bug in \
                     the splice geometry, not a gate decision.",
                    window.gap_index + 1,
                    format_gap_patch_not_applied_reason(reason),
                );
                continue;
            }
            None => {
                progress.phase(&format!(
                    "gap-listen: gap #{} has no patch outcome; no patched WAV",
                    window.gap_index + 1
                ));
                continue;
            }
        }
        let pcm = slice_window(
            &patched.samples,
            a_channels,
            sample_rate,
            a_bit_depth,
            window.a_range,
        );
        // Backstop for "reports Patched, sounds unchanged". `splice_into_a`'s three known no-op
        // paths now surface as `NotApplied` (handled above), but this check does not depend on the
        // engine noticing: it compares bytes. It still fires if a splice reports success and
        // changes nothing, or if a failure could not be attributed to a gap. To the ear the "after"
        // clip is simply the "before" clip, which is the single most misleading thing a listening
        // test can hand you — so it is checked rather than assumed. The window is still written:
        // the identical file *is* the evidence.
        if window_digest(&pcm.samples) == window.a_pre_digest {
            unchanged_count += 1;
            // `eprintln!`, not `progress.phase`: phase output is suppressible
            // (`StderrProgressReporter::new(config.logging.progress)`), and this says the engine's
            // own verdict disagrees with what it wrote. Same precedent as the `--mux` warning.
            eprintln!(
                "warning: gap-listen: gap #{} reports Patched but its A window is byte-identical \
                 before and after the splice — {}{} will sound the same. Suspect a no-op splice \
                 (destination out of range, inverted, or fill shorter than the gap).",
                window.gap_index + 1,
                window.stem,
                A_PATCHED_SUFFIX,
            );
        }
        write_window_wav(
            wav_writer,
            &listen.wav_dir,
            &window.stem,
            A_PATCHED_SUFFIX,
            &pcm,
        )?;
        patched_count += 1;
    }

    progress.phase(&format!(
        "gap-listen: wrote {} gap clip set(s) ({patched_count} with a patched window{}) to {}",
        pending.len(),
        if unchanged_count == 0 {
            String::new()
        } else {
            format!(", {unchanged_count} of them UNCHANGED — see warnings above")
        },
        listen.wav_dir.display()
    ));
    Ok(patch_result)
}

#[cfg(test)]
mod tests {
    use super::{projected_export_secs, UNBOUNDED_LISTEN_WARN_GAPS};
    use crate::domain::gap::Gap;

    fn gap(start: f64, end: f64) -> Gap {
        Gap {
            video_a_start_secs: start,
            video_a_end_secs: end,
            video_b_start_secs: Some(start),
            video_b_end_secs: Some(end),
            b_has_energy: true,
        }
    }

    #[test]
    fn projection_counts_three_padded_clips_per_selected_gap() {
        let gaps = [gap(10.0, 12.0), gap(30.0, 34.0), gap(50.0, 51.0)];
        // Gaps 0 and 2 only: (2 + 6) + (1 + 6) = 15 s of window, three clips each.
        let secs = projected_export_secs(&gaps, &[0, 2], 3.0);
        assert!(
            (secs - 45.0).abs() < 1e-9,
            "expected 45 s projected, got {secs}"
        );
    }

    /// An out-of-range index must not panic or poison the estimate — the warning exists to inform,
    /// and killing a run over an arithmetic detail in a log line would be worse than a rough number.
    #[test]
    fn projection_ignores_indices_with_no_gap() {
        let gaps = [gap(10.0, 12.0)];
        let secs = projected_export_secs(&gaps, &[0, 99], 3.0);
        assert!((secs - 24.0).abs() < 1e-9, "got {secs}");
    }

    /// A zero-length or inverted gap contributes only its padding, never a negative.
    #[test]
    fn projection_floors_degenerate_gaps_at_their_padding() {
        let gaps = [gap(10.0, 10.0), gap(30.0, 20.0)];
        let secs = projected_export_secs(&gaps, &[0, 1], 2.0);
        assert!((secs - 24.0).abs() < 1e-9, "got {secs}");
    }

    /// The threshold is a judgment call, but it must sit above any plausible hand-picked set and
    /// below a real corpus pair's gap count, or it warns on every run and gets ignored.
    #[test]
    fn warn_threshold_is_in_the_intended_band() {
        assert!((10..=50).contains(&UNBOUNDED_LISTEN_WARN_GAPS));
    }
}
