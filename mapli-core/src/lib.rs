#[cfg(target_os = "linux")]
mod egl;
mod encode;
mod error;
mod pool;
mod scheme;
mod types;
mod worker;

pub use encode::encode_png;
pub use error::{MapliError, Result};
pub use pool::{PoolConfig, RenderPool};
pub use scheme::{
    ErrorReason, HANDLER_THREADS, RESERVED_SCHEMES, ResourceKind, SchemeHandler, SchemeRequest,
    SchemeResponse, register_scheme_handler,
};
pub use types::*;

// Re-export `RgbaImage` from the `image` crate so that users of this crate don't have to depend
// on `image` themselves.
pub use image::RgbaImage;
