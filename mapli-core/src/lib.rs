mod bbox;
#[cfg(target_os = "linux")]
mod egl;
mod encode;
mod error;
mod pool;
mod resample;
mod scheme;
mod types;
mod url_rules;
mod worker;

pub use encode::encode_png;
pub use error::{MapliError, Result};
pub use pool::{PoolConfig, RenderPool};
pub use scheme::{
    HANDLER_THREADS, RESERVED_SCHEMES, SchemeHandler, register_scheme_handler,
    unregister_scheme_handler,
};
pub use types::*;
pub use url_rules::restrict_resource_urls;

// Re-export `RgbaImage` from the `image` crate so that users of this crate don't have to depend
// on `image` themselves.
pub use image::RgbaImage;
