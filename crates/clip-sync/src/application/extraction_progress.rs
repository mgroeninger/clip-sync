use std::borrow::Cow;
use std::cell::Cell;

use crate::application::ports::ProgressReporter;

pub const FINGERPRINT_ALIGN_STAGE: &str = "Aligning audio fingerprints...";

const UNITS_PER_CLIP: u64 = 1000;

/// Tracks clip extraction across one or more videos and maps decode progress to a single stage bar.
pub struct ExtractionProgressScope<'a> {
    inner: &'a dyn ProgressReporter,
    stage_label: Cow<'a, str>,
    global_total: Cell<u64>,
    global_done: Cell<u64>,
}

impl<'a> ExtractionProgressScope<'a> {
    pub fn new(inner: &'a dyn ProgressReporter) -> Self {
        Self::with_stage_label(inner, Cow::Borrowed(FINGERPRINT_ALIGN_STAGE))
    }

    pub fn with_stage_label(inner: &'a dyn ProgressReporter, stage_label: Cow<'a, str>) -> Self {
        Self {
            inner,
            stage_label,
            global_total: Cell::new(0),
            global_done: Cell::new(0),
        }
    }

    pub fn register_batch(&self, clip_count: u64) {
        self.global_total
            .set(self.global_total.get() + clip_count);

        if self.inner.detailed_extraction_progress() {
            return;
        }

        let unit_total = self.global_total.get().max(1) * UNITS_PER_CLIP;
        self.inner.progress(
            &self.stage_label,
            self.global_done.get() * UNITS_PER_CLIP,
            unit_total,
        );
    }

    pub fn finish_batch(&self, clip_count: u64) {
        self.global_done
            .set(self.global_done.get() + clip_count);
    }

    pub fn for_clip(&self, clip_in_batch: u64) -> ClipExtractProgress<'_> {
        ClipExtractProgress {
            scope: self,
            clip_in_batch,
        }
    }
}

pub struct ClipExtractProgress<'a> {
    scope: &'a ExtractionProgressScope<'a>,
    clip_in_batch: u64,
}

impl ProgressReporter for ClipExtractProgress<'_> {
    fn phase(&self, message: &str) {
        self.scope.inner.phase(message);
    }

    fn phase_verbose(&self, message: &str) {
        self.scope.inner.phase_verbose(message);
    }

    fn detailed_extraction_progress(&self) -> bool {
        self.scope.inner.detailed_extraction_progress()
    }

    fn progress(&self, label: &str, current: u64, total: u64) {
        if self.scope.inner.detailed_extraction_progress() {
            self.scope.inner.progress(label, current, total);
            return;
        }

        let total_clips = self.scope.global_total.get().max(1);
        let unit_total = total_clips * UNITS_PER_CLIP;
        let clip_total = total.max(1);
        let within = (current.min(clip_total) * UNITS_PER_CLIP) / clip_total;
        let global_clip_index = self.scope.global_done.get() + self.clip_in_batch;
        let global_current = (global_clip_index * UNITS_PER_CLIP + within).min(unit_total);
        self.scope.inner.progress(
            &self.scope.stage_label,
            global_current,
            unit_total,
        );
    }

    fn flush_progress(&self) {
        self.scope.inner.flush_progress();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct RecordingProgress {
        detailed: bool,
        last: RefCell<Option<(String, u64, u64)>>,
    }

    impl RecordingProgress {
        fn new(detailed: bool) -> Self {
            Self {
                detailed,
                last: RefCell::new(None),
            }
        }
    }

    impl ProgressReporter for RecordingProgress {
        fn phase(&self, _message: &str) {}

        fn detailed_extraction_progress(&self) -> bool {
            self.detailed
        }

        fn progress(&self, label: &str, current: u64, total: u64) {
            *self.last.borrow_mut() = Some((label.to_string(), current, total));
        }
    }

    #[test]
    fn register_batch_emits_stage_progress_in_auto_mode() {
        let inner = RecordingProgress::new(false);
        let scope = ExtractionProgressScope::new(&inner);
        scope.register_batch(2);

        assert_eq!(
            inner.last.borrow().as_ref().map(|(label, current, total)| {
                (label.as_str(), *current, *total)
            }),
            Some((FINGERPRINT_ALIGN_STAGE, 0, 2000))
        );
    }

    #[test]
    fn custom_stage_label_is_used_for_aggregated_progress() {
        let inner = RecordingProgress::new(false);
        let scope = ExtractionProgressScope::with_stage_label(
            &inner,
            "Aligning audio fingerprints (video A)...".into(),
        );
        scope.register_batch(1);
        scope.for_clip(0).progress("extract", 500, 1000);

        assert_eq!(
            inner.last.borrow().as_ref().map(|(label, current, total)| {
                (label.as_str(), *current, *total)
            }),
            Some(("Aligning audio fingerprints (video A)...", 500, 1000))
        );
    }

    #[test]
    fn aggregated_progress_uses_stage_label_and_spans_batches() {
        let inner = RecordingProgress::new(false);
        let scope = ExtractionProgressScope::new(&inner);

        scope.register_batch(1);
        scope.for_clip(0).progress("Extracting clip 1/1 (video A)", 500, 1000);
        assert_eq!(
            inner.last.borrow().as_ref().map(|(label, current, total)| {
                (label.as_str(), *current, *total)
            }),
            Some((FINGERPRINT_ALIGN_STAGE, 500, 1000))
        );

        scope.finish_batch(1);
        scope.register_batch(1);
        scope.for_clip(0).progress("Extracting clip 1/1 (video B)", 250, 1000);
        assert_eq!(
            inner.last.borrow().as_ref().map(|(label, current, total)| {
                (label.as_str(), *current, *total)
            }),
            Some((FINGERPRINT_ALIGN_STAGE, 1250, 2000))
        );
    }

    #[test]
    fn detailed_mode_preserves_per_clip_labels() {
        let inner = RecordingProgress::new(true);
        let scope = ExtractionProgressScope::new(&inner);
        scope.register_batch(1);

        scope
            .for_clip(0)
            .progress("Extracting clip 1/1 (video A, 10:00)", 99, 100);

        assert_eq!(
            inner.last.borrow().as_ref().map(|(label, current, total)| {
                (label.as_str(), *current, *total)
            }),
            Some(("Extracting clip 1/1 (video A, 10:00)", 99, 100))
        );
    }
}
