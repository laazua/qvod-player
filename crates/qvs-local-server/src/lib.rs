// HTTP server convenience patterns are intentional
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unwrap_used,
    clippy::used_underscore_binding,
    clippy::unused_async,
    clippy::match_same_arms,
    clippy::needless_pass_by_value
)]

pub mod config;
pub mod handler;
pub mod middleware;
pub mod range;
pub mod server;
pub mod stream;

pub use config::*;
pub use handler::*;
pub use middleware::*;
pub use range::*;
pub use server::*;
pub use stream::*;
