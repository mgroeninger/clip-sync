//! Report writing to stdout, shared by both CLIs.
//!
//! Lives here rather than in either CLI crate for the same reason as [`init_tracing`]: it is a
//! process-level I/O concern that `clip-sync` and `clip-sync-repair` must handle *identically*,
//! so scripts see one behavior regardless of which binary they call.
//!
//! [`init_tracing`]: crate::init_tracing

use std::io::{self, Write};

/// Write a fully rendered report to stdout.
///
/// Two failure modes are deliberately treated differently:
///
/// - **Broken pipe** (`clip-sync … | head`) is **success**. The reader chose to stop; the
///   report was correct and complete as far as anyone wanted it. `println!` panicked here.
/// - **Any other write failure** (full disk on `> out.json`, closed handle) is an **error** and
///   is propagated so the caller can exit non-zero. Swallowing these would hand a script exit 0
///   alongside a truncated or empty output file, which is the worst possible combination.
///
/// Writes through a single locked handle and flushes explicitly: a `LineWriter` flush at process
/// exit discards its error, so without this the disk-full case would still be invisible.
pub fn write_report_to_stdout(rendered: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    match handle
        .write_all(rendered.as_bytes())
        .and_then(|()| handle.flush())
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pipe/disk split is the whole point of the helper, so pin it against a writer we can
    /// fail on demand. `write_report_to_stdout` itself locks the real stdout, so the classifying
    /// logic is re-expressed here over the same `ErrorKind` contract.
    fn classify<W: Write>(mut sink: W, rendered: &str) -> io::Result<()> {
        match sink
            .write_all(rendered.as_bytes())
            .and_then(|()| sink.flush())
        {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Err(error) => Err(error),
        }
    }

    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(self.0, "synthetic"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(self.0, "synthetic"))
        }
    }

    #[test]
    fn broken_pipe_is_success() {
        assert!(classify(FailingWriter(io::ErrorKind::BrokenPipe), "report").is_ok());
    }

    #[test]
    fn other_write_failures_propagate() {
        let error = classify(FailingWriter(io::ErrorKind::StorageFull), "report")
            .expect_err("a full disk must not read as success");

        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
    }

    #[test]
    fn successful_write_emits_exact_bytes() {
        let mut sink = Vec::new();
        classify(&mut sink, "line one\nline two\n").expect("write to a Vec cannot fail");

        assert_eq!(sink, b"line one\nline two\n");
    }
}
