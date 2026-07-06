// Media decoding layer.
// When "ffmpeg-native" feature is active, uses ffmpeg-next (statically linked FFmpeg)
// for frame decoding and media probing.
// When "ffmpeg-subprocess" feature is active, uses ffmpeg/ffprobe binaries via subprocess.
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

#[cfg(feature = "ffmpeg-native")]
pub mod native;
#[cfg(feature = "ffmpeg-native")]
pub use native::NativeFrameReader as FrameReader;
#[cfg(feature = "ffmpeg-native")]
#[allow(unused_imports)]
pub use native::*;

#[cfg(feature = "ffmpeg-subprocess")]
pub mod subprocess;
#[cfg(feature = "ffmpeg-subprocess")]
pub use subprocess::FfmpegFrameReader as FrameReader;
#[cfg(feature = "ffmpeg-subprocess")]
#[allow(unused_imports)]
pub use subprocess::*;

pub mod decoder;
pub mod demuxer;
pub mod format;
pub mod renderer;
pub mod resampler;
pub mod sync;

pub use decoder::*;
pub use demuxer::*;
pub use format::*;
pub use renderer::*;
pub use resampler::*;
pub use sync::*;
