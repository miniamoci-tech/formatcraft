//! FormatCraft engine: everything below the user interface.
//!
//! - [`formats`]: the catalogue of output formats.
//! - [`job`]: plain-data job descriptions shared by the UI, CLI and automation.
//! - [`exec::run_job`]: runs one job; [`queue::Queue`] runs many.
//! - Audio/video work is delegated to an external FFmpeg process
//!   ([`ffmpeg`]); images and PDFs are handled natively.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod args;
pub mod atomic;
pub mod disc;
pub mod error;
pub mod exec;
pub mod ffmpeg;
pub mod formats;
pub mod image_conv;
pub mod job;
pub mod pdf;
pub mod probe;
pub mod queue;
pub mod runner;

pub use error::{Error, Result};
pub use exec::run_job;
pub use job::{ImageOptions, Job, MediaOptions, Task};
pub use queue::{Queue, Status};
