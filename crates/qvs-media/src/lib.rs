// Media decoding — uses ffmpeg/ffprobe binaries via subprocess for frame extraction.
// When ffmpeg development headers are available, prefer ffmpeg-next for better performance.
#![allow(
    clippy::missing_errors_doc,
    clippy::derivable_impls,
    clippy::double_must_use,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    unused_assignments,
    dead_code,
    ambiguous_glob_reexports,
    clippy::must_use_candidate,
    clippy::map_unwrap_or,
    clippy::needless_pass_by_value,
    clippy::redundant_else,
    clippy::items_after_test_module,
    clippy::unwrap_used,
    clippy::needless_continue
)]

pub mod decoder;
pub mod demuxer;
pub mod format;
pub mod renderer;
pub mod resampler;
pub mod subprocess;
pub mod sync;

pub use decoder::*;
pub use demuxer::*;
pub use format::*;
pub use renderer::*;
pub use resampler::*;
pub use subprocess::*;
pub use sync::*;
