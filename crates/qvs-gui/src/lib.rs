// GUI convenience allows for UI patterns
#![allow(
    clippy::cast_precision_loss,
    clippy::missing_errors_doc,
    clippy::derivable_impls,
    clippy::must_use_unit,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::match_same_arms,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::let_and_return,
    clippy::expect_used,
    clippy::doc_markdown,
    clippy::assigning_clones,
    clippy::manual_let_else,
    clippy::single_match_else,
    clippy::redundant_pattern_matching,
    clippy::too_many_lines,
    unused_must_use
)]

pub mod app;
pub mod client;
pub mod controls;
pub mod fonts;
pub mod overlay;
pub mod player;
pub mod playlist;
pub mod settings;
pub mod skin;
pub mod status;
pub mod theme;
