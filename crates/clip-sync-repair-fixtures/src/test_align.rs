//! Re-exports of repair [`test_support`](clip_sync_repair::application::test_support) aligner
//! stubs and alignment builders for fixture / harness callers.

pub use clip_sync_repair::application::{
    no_op_alignment, oracle_injected_alignment, start_clip_alignment, zero_offset_alignment,
    NeverCalledAligner,
};
