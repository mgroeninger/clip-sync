//! `--gap-listen` end-to-end: one decode → fingerprint JSON → A/B surround WAVs → production
//! patch → patched WAV.
//!
//! Tier: **integration**. Stereo sine + gap WAV fixtures, real `SymphoniaMediaReader` decode.
//!
//! Run: `cargo test -p clip-sync-repair --features calibration --test gap_listen_integration`
#![cfg(feature = "calibration")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::{MediaError, MediaReader, MediaSource, SymphoniaMediaReader};
use clip_sync_repair::application::align_bridge::scan_alignment_from_result;
use clip_sync_repair::application::gap_listen::{run_gap_listen, GapListenRequest};
use clip_sync_repair::application::patch_audio::PatchAudioResult;
use clip_sync_repair::application::test_support::zero_offset_alignment;
use clip_sync_repair::domain::gap::{Gap, GapReport};
use clip_sync_repair::domain::GapSelectionMode;
use clip_sync_repair::infrastructure::wav_writer::WavPatchedAudioWriter;
use clip_sync_repair_harness::patch_audio::{
    patch_request_with_options, stereo_identical_compat, write_stereo_sine_with_gap,
    PatchTestOptions,
};
use hound::WavReader;

const SAMPLE_RATE: u32 = 44_100;
const TOTAL_SECS: u32 = 20;
const CONTEXT_SECS: f64 = 3.0;
const CROSSFADE_MS: u64 = 50;

/// Counts `open` calls while delegating to the real decoder, so "one decode, not two" is measured
/// against genuine Symphonia decode behavior rather than a fake that might not decode at all.
struct CountingMediaReader {
    inner: SymphoniaMediaReader,
    opens: AtomicUsize,
}

impl CountingMediaReader {
    fn new() -> Self {
        Self {
            inner: SymphoniaMediaReader,
            opens: AtomicUsize::new(0),
        }
    }

    fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }
}

impl MediaReader for CountingMediaReader {
    type Session = <SymphoniaMediaReader as MediaReader>::Session;

    fn open(&self, source: &MediaSource) -> Result<Self::Session, MediaError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        self.inner.open(source)
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    report: GapReport,
    fingerprint_dir: PathBuf,
    wav_dir: PathBuf,
    /// A-timeline spans of every gap written into A, in report order.
    gap_spans: Vec<(f64, f64)>,
}

fn gap_at(start: f64, end: f64) -> Gap {
    Gap {
        video_a_start_secs: start,
        video_a_end_secs: end,
        video_b_start_secs: Some(start),
        video_b_end_secs: Some(end),
        b_has_energy: true,
    }
}

/// A = sine with silent gaps, B = the same sine intact. Zero offset, so each gap's donor sits at
/// the identical timestamp in B.
fn build_fixture(gap_spans: &[(f64, f64)]) -> Fixture {
    build_fixture_with_b(gap_spans, |path| {
        write_stereo_sine_with_gap(path, SAMPLE_RATE, TOTAL_SECS, 0, 0, 440.0, 16_000.0);
    })
}

/// `build_fixture` with a caller-supplied B, for the cases where the *donor* is the variable: a B
/// that cannot pass the seam gate, or one whose channel layout does not match A's.
fn build_fixture_with_b(gap_spans: &[(f64, f64)], write_b: impl Fn(&Path)) -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    let fingerprint_dir = temp.path().join("corpus");
    let wav_dir = temp.path().join("wav");

    write_sine_with_gaps(&path_a, gap_spans);
    write_b(&path_b);

    let report = GapReport {
        video_a: path_a,
        video_b: path_b,
        track_compatibility: Some(stereo_identical_compat(SAMPLE_RATE)),
        alignment: scan_alignment_from_result(&zero_offset_alignment(f64::from(TOTAL_SECS))),
        gaps: gap_spans.iter().map(|&(s, e)| gap_at(s, e)).collect(),
        gap_equivalence: Vec::new(),
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
        limit_fill_to_mapped_region: false,
        b_scanned_end_secs: None,
        b_scan_truncated: false,
        audio_timeline_skew: None,
    };

    Fixture {
        _temp: temp,
        report,
        fingerprint_dir,
        wav_dir,
        gap_spans: gap_spans.to_vec(),
    }
}

/// `write_stereo_sine_with_gap` handles one gap; multi-gap fixtures need several, so write directly.
fn write_sine_with_gaps(path: &Path, gaps: &[(f64, f64)]) {
    use std::f32::consts::TAU;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(SAMPLE_RATE) * u64::from(TOTAL_SECS);
    for frame in 0..total_frames {
        let t = frame as f64 / f64::from(SAMPLE_RATE);
        let silent = gaps.iter().any(|&(s, e)| t >= s && t < e);
        let sample = if silent {
            0i16
        } else {
            (f32::sin(TAU * 440.0 * (frame as f32 / SAMPLE_RATE as f32)) * 16_000.0).round() as i16
        };
        writer.write_sample(sample).expect("write L");
        writer.write_sample(sample).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

/// A donor at a *different* frequency, so the seam waveform disagrees with A's borders.
///
/// Pairs with `disable_structure_trust`: with structure trust on (the default) the engine reports
/// `pre/post_correlation: 1.0` without consulting the waveform at all, so a mismatched donor alone
/// still patches. This mirrors `patch_audio_integration`'s
/// `patch_audio_skips_when_structure_trust_disabled_and_waveform_disagrees`.
fn write_disagreeing_sine(path: &Path) {
    write_stereo_sine_with_gap(path, SAMPLE_RATE, TOTAL_SECS, 0, 0, 2200.0, 16_000.0);
}

/// A **mono** intact sine, for the layout-mismatch refusal. A stays stereo.
fn write_mono_sine(path: &Path) {
    use std::f32::consts::TAU;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for frame in 0..u64::from(SAMPLE_RATE) * u64::from(TOTAL_SECS) {
        let sample =
            (f32::sin(TAU * 440.0 * (frame as f32 / SAMPLE_RATE as f32)) * 16_000.0).round() as i16;
        writer.write_sample(sample).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn listen_options(selection: GapSelectionMode) -> PatchTestOptions {
    PatchTestOptions {
        gap_signature_context_secs: CONTEXT_SECS,
        gap_selection: selection,
        ..Default::default()
    }
}

/// Returns the reader (for decode counting) and the production result the run hands back — the
/// object `composition` folds into `RepairRunOutcome` so the ordinary gap table reports it.
fn run_listen(
    fixture: &Fixture,
    selection: GapSelectionMode,
) -> (CountingMediaReader, PatchAudioResult) {
    run_listen_with(fixture, listen_options(selection))
}

/// `run_listen` for cases that need to reach past the selection — notably
/// `disable_structure_trust`, the only way to make the engine consult the seam waveform at all.
fn run_listen_with(
    fixture: &Fixture,
    options: PatchTestOptions,
) -> (CountingMediaReader, PatchAudioResult) {
    let reader = CountingMediaReader::new();
    let request = patch_request_with_options(fixture.report.clone(), true, 1.0, 0.5, options);
    let result = run_gap_listen(
        &reader,
        &WavPatchedAudioWriter,
        &FakeProgressReporter,
        request,
        GapListenRequest {
            fingerprint_dir: fixture.fingerprint_dir.clone(),
            wav_dir: fixture.wav_dir.clone(),
            fingerprint_diagnostics: false,
            crossfade_ms: CROSSFADE_MS,
        },
    )
    .expect("gap-listen run should succeed");
    (reader, result)
}

/// `run_listen` for the cases where the refusal *is* the assertion.
fn try_run_listen(
    fixture: &Fixture,
    selection: GapSelectionMode,
) -> Result<PatchAudioResult, clip_sync_repair::application::error::RepairError> {
    let request = patch_request_with_options(
        fixture.report.clone(),
        true,
        1.0,
        0.5,
        listen_options(selection),
    );
    run_gap_listen(
        &SymphoniaMediaReader,
        &WavPatchedAudioWriter,
        &FakeProgressReporter,
        request,
        GapListenRequest {
            fingerprint_dir: fixture.fingerprint_dir.clone(),
            wav_dir: fixture.wav_dir.clone(),
            fingerprint_diagnostics: false,
            crossfade_ms: CROSSFADE_MS,
        },
    )
}

fn wav_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read wav dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".wav"))
        .collect();
    names.sort();
    names
}

fn read_wav_frames(path: &Path) -> (Vec<i32>, u16) {
    let mut reader = WavReader::open(path).expect("open wav");
    let channels = reader.spec().channels;
    let samples: Vec<i32> = reader
        .samples::<i32>()
        .map(|s| s.expect("sample"))
        .collect();
    (samples, channels)
}

#[test]
fn gap_listen_writes_surround_and_patched_wavs_joined_to_the_corpus() {
    let fixture = build_fixture(&[(6.0, 9.0)]);
    let _ = run_listen(&fixture, GapSelectionMode::All);

    let names = wav_names(&fixture.wav_dir);
    assert_eq!(
        names.len(),
        3,
        "one patched gap should yield A + B + patched, got {names:?}"
    );

    // Every WAV must be joinable back to the fingerprint that describes it — the whole point of
    // the shared stem (§7.5). A drifting `entry_stem` refactor breaks exactly this.
    for name in &names {
        let stem = ["_a_surround.wav", "_b_surround.wav", "_a_patched.wav"]
            .iter()
            .find_map(|suffix| name.strip_suffix(suffix))
            .unwrap_or_else(|| panic!("unexpected WAV name {name}"));
        let json = fixture.fingerprint_dir.join(format!("{stem}.json"));
        assert!(
            json.is_file(),
            "WAV {name} has no corpus JSON sibling at {}",
            json.display()
        );
    }

    assert!(names.iter().any(|n| n.ends_with("_a_surround.wav")));
    assert!(names.iter().any(|n| n.ends_with("_b_surround.wav")));
    assert!(
        names.iter().any(|n| n.ends_with("_a_patched.wav")),
        "a clean sine gap should patch under the production gate"
    );
    assert!(fixture.fingerprint_dir.join("corpus.json").is_file());
    assert!(fixture.fingerprint_dir.join("manifest.json").is_file());
}

/// The length-preserving splice (§2.1) is what makes the before/after pair comparable. If a future
/// fill mode resizes A, the two windows stop describing the same instants and this fails.
#[test]
fn pre_and_patched_windows_are_sample_aligned_and_differ_only_inside_the_gap() {
    let (gap_start, gap_end) = (6.0, 9.0);
    let fixture = build_fixture(&[(gap_start, gap_end)]);
    let _ = run_listen(&fixture, GapSelectionMode::All);

    let names = wav_names(&fixture.wav_dir);
    let pre_name = names
        .iter()
        .find(|n| n.ends_with("_a_surround.wav"))
        .expect("A surround");
    let patched_name = names
        .iter()
        .find(|n| n.ends_with("_a_patched.wav"))
        .expect("patched A");

    let (pre, channels) = read_wav_frames(&fixture.wav_dir.join(pre_name));
    let (patched, patched_channels) = read_wav_frames(&fixture.wav_dir.join(patched_name));

    assert_eq!(channels, patched_channels);
    assert_eq!(
        pre.len(),
        patched.len(),
        "splice_into_a is length-preserving, so the two windows must be identical in length"
    );

    // The window starts `CONTEXT_SECS` before the gap (clamped at 0), so the gap sits at a known
    // offset inside it. Compare on frames, allowing the crossfade to bleed past each edge.
    let window_start_secs = (gap_start - CONTEXT_SECS).max(0.0);
    let crossfade_secs = CROSSFADE_MS as f64 / 1000.0;
    let ch = channels as usize;
    let frame_of = |secs: f64| ((secs - window_start_secs) * f64::from(SAMPLE_RATE)) as usize;
    let gap_lo = frame_of(gap_start - crossfade_secs);
    let gap_hi = frame_of(gap_end + crossfade_secs).min(pre.len() / ch);

    let mut differing_inside = 0usize;
    for frame in 0..pre.len() / ch {
        let same = (0..ch).all(|c| pre[frame * ch + c] == patched[frame * ch + c]);
        if frame >= gap_lo && frame < gap_hi {
            if !same {
                differing_inside += 1;
            }
        } else {
            assert!(
                same,
                "frame {frame} outside the gap±crossfade changed; the splice must not touch \
                 audio beyond the gap"
            );
        }
    }
    assert!(
        differing_inside > 0,
        "the patched window must actually differ inside the gap"
    );

    // The pre window really was silent there — otherwise "differs inside" proves nothing. Check the
    // *true* gap interior, not the crossfade-expanded band: the latter reaches into real sine on
    // both sides by design.
    let silent_before = (frame_of(gap_start)..frame_of(gap_end))
        .all(|frame| (0..ch).all(|c| pre[frame * ch + c] == 0));
    assert!(silent_before, "fixture gap should be digital silence in A");
}

/// Guards the decode budget: a regression to the old dump-then-repair shape would double it.
#[test]
fn gap_listen_decodes_the_pair_exactly_once() {
    let fixture = build_fixture(&[(6.0, 9.0)]);
    let (reader, _) = run_listen(&fixture, GapSelectionMode::All);
    assert_eq!(
        reader.opens(),
        2,
        "one decode opens A and B once each; 4 means the corpus and the patch each decoded"
    );
}

/// The corpus and the patch plan must be driven by one resolved selection (§6 step 2). Two
/// independent lists would silently fingerprint one set of gaps and patch another.
#[test]
fn one_selector_drives_both_the_corpus_and_the_patch() {
    let fixture = build_fixture(&[(3.0, 5.0), (9.0, 11.0), (15.0, 17.0)]);
    // 1-based token for the middle gap.
    let (_, result) = run_listen(&fixture, GapSelectionMode::Only(vec!["2".to_string()]));

    let corpus: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(fixture.fingerprint_dir.join("corpus.json")).expect("corpus.json"),
    )
    .expect("parse corpus");
    let indices: Vec<u64> = corpus["gaps"]
        .as_array()
        .expect("gaps array")
        .iter()
        .map(|g| g["index"].as_u64().expect("index"))
        .collect();
    assert_eq!(indices, vec![1], "only the selected gap should be dumped");

    // ...and the WAVs cover that same gap and no other. `g001` is the 0-based stem segment.
    let names = wav_names(&fixture.wav_dir);
    assert!(!names.is_empty(), "selected gap should produce WAVs");
    for name in &names {
        assert!(
            name.contains("_g001_"),
            "WAV {name} is not the selected gap; corpus and plan have drifted"
        );
    }
    assert_eq!(
        fixture.gap_spans.len(),
        3,
        "fixture sanity: three gaps existed to choose from"
    );

    // The returned summary is what the caller folds into `RepairRunOutcome`, so it must be a
    // *whole-report* table, not just the selected gap: an unselected gap has to appear as filtered
    // out rather than vanish, or the printed table would silently under-report the run.
    assert_eq!(
        result.summary.gaps.len(),
        3,
        "summary must cover every report gap, not just the selected one"
    );
    assert!(
        matches!(
            result.summary.gaps[1].status,
            clip_sync_repair::domain::patch_result::GapPatchStatus::Patched { .. }
        ),
        "the selected gap should carry the production verdict, got {:?}",
        result.summary.gaps[1].status
    );
    for unselected in [0usize, 2] {
        assert!(
            !matches!(
                result.summary.gaps[unselected].status,
                clip_sync_repair::domain::patch_result::GapPatchStatus::Patched { .. }
            ),
            "gap #{} was not selected and must not read as patched",
            unselected + 1
        );
    }
}

/// Item 1: the production verdicts behind the clips must survive the call. Without this the run
/// wrote WAVs and threw away the only record of *why* each gap was or wasn't patched.
#[test]
fn the_run_returns_the_production_summary_for_reporting() {
    let fixture = build_fixture(&[(6.0, 9.0)]);
    let (_, result) = run_listen(&fixture, GapSelectionMode::All);

    assert!(
        !result.preview,
        "a listen run executes the real patch; reporting it as a preview would misdescribe it"
    );
    assert_eq!(result.summary.gaps.len(), 1);
    assert!(
        result.summary.has_patches(),
        "the fixture gap is fillable, so the summary must record the patch"
    );
}

/// Item 12: a gap the **production gate** refuses is a different case from one the plan drops, and
/// it is the case the margin-band experiment cares about most. It must still get both surround
/// clips — you cannot judge a refusal without hearing what was on offer — and must not get a
/// patched clip, because no patch exists. Synthesizing one from the oracle would be the exact
/// "misleading diagnostics" failure this feature was built to avoid.
///
/// `disable_structure_trust` is load-bearing, not a convenience: with structure trust on, the
/// engine reports `pre/post_correlation: 1.0` **without looking at the waveform**, so no choice of
/// donor content can reach a refusal. Same lever as
/// `patch_audio_integration::patch_audio_skips_when_structure_trust_disabled_and_waveform_disagrees`.
///
/// # Why `#[ignore]`
///
/// ~350 s, and **none of it is the thing under test**. `CLIP_SYNC_SPAN_TIMING` breakdown:
///
/// ```text
/// anchor_matchability   time.busy=42.0s  x5 brackets   <- fingerprint dump oracle
///   local_anchor_xcorr  time.busy=21.9s
/// patch_audio           time.busy=654ms                <- the refusal being asserted
///   char_gate_search    time.busy=128ms
/// ```
///
/// The gate refuses in 0.65 s. The rest is the per-bracket anchor oracle in the fingerprint dump
/// that `--gap-listen` runs first, which a mismatched donor sends down its exhaustive path. It is
/// not bounded by the fixture timeline (a 7 s fixture gives the same ~20 s per `local_anchor_xcorr`
/// as a 20 s one) and it does not respond to the fill-search knobs — both were measured and both
/// missed. Shrinking it means making the *dump* cheaper on a non-matching donor, which is a real
/// finding about `--gap-listen` on real media (every gate-refused gap costs minutes of oracle) but
/// is not this test's job.
///
/// Ignored rather than tiered: `validation-tests` is for real-codec calibration sweeps and
/// `diagnostic-tests` is for human-review dumps, and this is neither — it is an ordinary contract
/// test on which clips get written. Run it when `gap_listen`'s clip-writing branches change:
///
/// ```text
/// cargo test -p clip-sync-repair --features calibration --test gap_listen_integration -- \
///     --ignored a_gate_refused_gap_gets_both_surround_clips_and_no_patched_clip
/// ```
///
/// The cheap half of the contract — "no patched clip when there is no patch" — stays in the default
/// run via [`unplanned_gap_still_gets_an_a_only_clip`]; only the *gate-refusal* route is ignored.
#[test]
#[ignore = "~350s, and ~99.8% of it is the fingerprint anchor oracle, not the gate refusal asserted here"]
fn a_gate_refused_gap_gets_both_surround_clips_and_no_patched_clip() {
    let fixture = build_fixture_with_b(&[(6.0, 9.0)], write_disagreeing_sine);
    let (_, result) = run_listen_with(
        &fixture,
        PatchTestOptions {
            disable_structure_trust: true,
            ..listen_options(GapSelectionMode::All)
        },
    );

    // Reached the gate and was refused there — not dropped before it.
    assert!(
        matches!(
            result.summary.gaps[0].status,
            clip_sync_repair::domain::patch_result::GapPatchStatus::Skipped { .. }
        ),
        "a donor whose seam waveform disagrees should be refused by the gate, got {:?}",
        result.summary.gaps[0].status
    );

    let names = wav_names(&fixture.wav_dir);
    assert!(
        names.iter().any(|n| n.ends_with("_a_surround.wav")),
        "the refused gap still needs its A window, got {names:?}"
    );
    assert!(
        names.iter().any(|n| n.ends_with("_b_surround.wav")),
        "a gate refusal is only judgeable against the donor that was refused, got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.ends_with("_a_patched.wav")),
        "no patch happened, so no patched clip may exist, got {names:?}"
    );
}

/// Item 13: §10.1's layout-mismatch row. A stereo / B mono yields a provenance-only corpus with no
/// gap entries, hence no stems. The run must refuse outright rather than emit unnamed clips that
/// cannot be joined back to any fingerprint.
#[test]
fn a_layout_mismatched_pair_refuses_the_whole_run() {
    let fixture = build_fixture_with_b(&[(6.0, 9.0)], write_mono_sine);

    let err = try_run_listen(&fixture, GapSelectionMode::All)
        .err()
        .expect("a layout-mismatched pair must not produce a listen run");
    let message = err.to_string();
    assert!(
        message.contains("channel layout mismatch"),
        "the refusal must say why, got {message}"
    );

    // The trap this guards: a directory of clips with no fingerprint to join them to.
    assert!(
        !fixture.wav_dir.exists() || wav_names(&fixture.wav_dir).is_empty(),
        "refusing must leave no orphan WAVs, got {:?}",
        wav_names(&fixture.wav_dir)
    );
}

/// The empty-plan short-circuit returns a real summary too. "No gap reached the plan" is a finding
/// with a per-gap reason; returning an empty table there would print a run that looks like it had
/// no gaps at all.
#[test]
fn an_empty_plan_still_returns_a_reason_per_gap() {
    let mut fixture = build_fixture(&[(6.0, 9.0)]);
    fixture.report.gaps[0].video_b_start_secs = None;
    fixture.report.gaps[0].video_b_end_secs = None;

    let (_, result) = run_listen(&fixture, GapSelectionMode::All);

    assert_eq!(
        result.summary.gaps.len(),
        1,
        "the gap must still appear in the table"
    );
    assert!(
        matches!(
            result.summary.gaps[0].status,
            clip_sync_repair::domain::patch_result::GapPatchStatus::NotPlanned { .. }
        ),
        "an unmapped gap is NotPlanned, with a reason, got {:?}",
        result.summary.gaps[0].status
    );
    assert!(!result.summary.has_patches());
}

/// A selected gap with no donor mapping never reaches the plan, so there is no B window and no
/// patch — but it still has a fingerprint entry, hence a stem, hence an A clip (§10.1).
#[test]
fn unplanned_gap_still_gets_an_a_only_clip() {
    let mut fixture = build_fixture(&[(6.0, 9.0)]);
    fixture.report.gaps[0].video_b_start_secs = None;
    fixture.report.gaps[0].video_b_end_secs = None;

    let _ = run_listen(&fixture, GapSelectionMode::All);

    let names = wav_names(&fixture.wav_dir);
    assert!(
        names.iter().any(|n| n.ends_with("_a_surround.wav")),
        "A-only clip should still be written, got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.ends_with("_a_patched.wav")),
        "an unplanned gap cannot be patched, got {names:?}"
    );
    // "A-only" is the decision (plan §10.1), not just "no patched clip": with no donor mapping
    // there is no B span to cut a comparable window from, so emitting one would invent geometry.
    assert!(
        !names.iter().any(|n| n.ends_with("_b_surround.wav")),
        "an unmapped gap has no donor window to export, got {names:?}"
    );
}
