//! Error type for mapli-core.
//!
//! The errors carry plain `String` messages instead of wrapping `maplibre_native`'s
//! error types. This is done because:
//!
//! - errors cross thread boundaries, which means they must be `Send`able. Since we
//!   are not in control of `maplibre_native`'s errors, this makes it easier to
//!   enforce this desired property
//! - we keep `maplibre_native`'s types out of our own public API, which isolates it better
use thiserror::Error;

use crate::types::StyleId;

#[derive(Debug, Error)]
pub enum MapliError {
    /// Caller supplied an out of range or malformed value.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Request referenced a style ID that has not been registered.
    #[error("unknown style: {0}")]
    UnknownStyle(StyleId),

    /// MapLibre Native failed to load a registered style.
    #[error("failed to load style {id}: {message}")]
    StyleLoadFailed { id: StyleId, message: String },

    /// MapLibre Native failed while rendering.
    #[error("render failed: {0}")]
    RenderFailed(String),

    /// Encoding to PNG failed.
    #[error("failed to encode PNG: {0}")]
    PngEncodingFailed(String),

    /// The system cannot provide the OpenGL (EGL) context MapLibre Native renders with, e.g.
    /// because no EGL driver is installed or no display is available.
    #[error(
        "no usable OpenGL (EGL) context: {0}. On Linux, mapli needs Mesa's EGL drivers and a \
         display (e.g. run under Xvfb); see the README"
    )]
    GraphicsUnavailable(String),

    /// No handler is registered for this scheme.
    #[error("no handler is registered for scheme {0:?}")]
    UnknownScheme(String),

    /// OS refused to start a worker thread
    #[error("failed to spawn render worker: {0}")]
    WorkerSpawnFailed(String),

    /// The pool has been shut down; workers are not accepting commands.
    #[error("render pool is shut down")]
    PoolClosed,

    /// A worker has exited (e.g. panicked) before sending a reply.
    #[error("render worker exited before replying")]
    WorkerGone,
}

pub type Result<T, E = MapliError> = std::result::Result<T, E>;
