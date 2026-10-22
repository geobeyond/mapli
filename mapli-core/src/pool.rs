//! Thread-safe pool of workers for rendering map tiles.
//!
//! This module implements a thread-safe pool of workers for rendering map images
//! and tiles. The `RenderPool` struct is `Send+Sync` and can be shared between
//! threads. It manages a set of worker threads that can render map images and
//! tiles concurrently.
//! The pool also maintains a registry of styles that can be used by the workers to
//! render the images.
//! All MapLibre state lives in the worker threads, so the pool is just a thin wrapper around them.
use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

use crossbeam_channel::Sender;

use crate::error::{MapliError, Result};
use crate::types::{Rendered, StaticRequest, Style, StyleId, TileRequest};
use crate::worker::{self, Command, Reply};

/// Styles shared between the pool (writer) and workers (readers).
///
/// Values are `Arc<Style>` so a worker can clone one out and release the lock
/// before doing the slow work of loading it.
pub(crate) type StyleRegistry = Arc<RwLock<HashMap<StyleId, Arc<Style>>>>;

type ReplyReceiver = oneshot::Receiver<Result<Rendered>>;

pub struct PoolConfig {
    /// Number of worker threads to spawn in the pool. Each worker thread owns its own
    /// MapLibre run loop, GPU context and renderer cache, which also means memory consumption
    /// grows with this number.
    /// The pool will distribute rendering requests across the workers.
    pub workers: NonZeroUsize,

    /// Maximum number of cached renderers per worker, for each render mode. A worker keeps up to
    /// this many static renderers and, separately, up to this many tile renderers.
    pub max_renderers_per_worker: NonZeroUsize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            workers: NonZeroUsize::MIN,
            max_renderers_per_worker: NonZeroUsize::new(8).unwrap(),
        }
    }
}

pub struct RenderPool {
    tx: Option<Sender<Command>>,
    styles: StyleRegistry,
    workers: Vec<JoinHandle<()>>,
}

impl Drop for RenderPool {
    fn drop(&mut self) {
        // dropping the sender disconnects the channel, which means each worker's `rx.iter()` loop ends
        drop(self.tx.take());
        for handle in self.workers.drain(..) {
            // if a worker had panicked, it would return an error here, but we wouldn't do
            // anything with it during drop, so just ignore
            let _ = handle.join();
        }
    }
}

impl RenderPool {
    /// Create a pool and spawn its workers.
    ///
    /// On Linux, this first checks that an EGL context can be created and returns
    /// [`MapliError::GraphicsUnavailable`] if not. Without the check, MapLibre Native would abort
    /// the process on the first render instead.
    pub fn new(config: PoolConfig) -> Result<Self> {
        #[cfg(target_os = "linux")]
        crate::egl::check()?;

        // MapLibre Native only uses the scheme handlers' dispatcher if it is registered before
        // the first renderer is created
        crate::scheme::install_dispatcher();

        // a shared multi-consumer queue: idle workers pull the next command from it
        let (tx, rx) = crossbeam_channel::unbounded::<Command>();
        let styles: StyleRegistry = Arc::default();
        let mut workers = Vec::with_capacity(config.workers.get());
        for i in 0..config.workers.get() {
            let rx = rx.clone();
            let styles = Arc::clone(&styles);
            let max_renderers = config.max_renderers_per_worker.get();
            let handle = std::thread::Builder::new()
                .name(format!("mapli-worker-{i}"))
                .spawn(move || worker::run(rx, styles, max_renderers))
                .map_err(|e| MapliError::WorkerSpawnFailed(e.to_string()))?;
            workers.push(handle);
        }

        Ok(Self {
            tx: Some(tx),
            styles,
            workers,
        })
    }

    /// Register or replace a style in the pool's style registry.
    ///
    /// This allows workers to access the style when rendering.
    pub fn register_style(&self, id: impl Into<StyleId>, style: Style) {
        let mut styles = self.styles.write().unwrap_or_else(|p| p.into_inner());
        styles.insert(id.into(), Arc::new(style));
    }

    pub fn has_style(&self, id: &StyleId) -> bool {
        let styles = self.styles.read().unwrap_or_else(|p| p.into_inner());
        styles.contains_key(id)
    }

    /// Render a static map image using the pool of workers.
    pub fn render(&self, req: StaticRequest) -> Result<Rendered> {
        req.validate()?;
        self.ensure_style(&req.style)?;
        Self::wait(self.submit(|reply| Command::Static { req, reply })?)
    }

    /// Render a static map image asynchronously, using the pool of workers.
    ///
    /// The request is submitted to the pool immediately, and the returned future will resolve
    /// when the rendering is complete. Submission occurs on the calling thread, but the actual
    /// rendering is done in a worker thread. Submission is immediate upon calling of this method.
    ///
    /// The returned future does not borrow the pool (`use<>`), so it is `'static` and can be
    /// handed to an executor or to Python.
    pub fn render_async(
        &self,
        req: StaticRequest,
    ) -> impl Future<Output = Result<Rendered>> + Send + use<> {
        let submitted = req
            .validate()
            .and_then(|()| self.ensure_style(&req.style))
            .and_then(|()| self.submit(|reply| Command::Static { req, reply }));
        Self::wait_async(submitted)
    }

    /// Render a map tile using the pool of workers.
    pub fn render_tile(&self, req: TileRequest) -> Result<Rendered> {
        req.validate()?;
        self.ensure_style(&req.style)?;
        Self::wait(self.submit(|reply| Command::Tile { req, reply })?)
    }

    /// Render a map tile asynchronously, using the pool of workers.
    ///
    /// Behaves like [`RenderPool::render_async`]: submission is immediate and the returned
    /// future is `'static`.
    pub fn render_tile_async(
        &self,
        req: TileRequest,
    ) -> impl Future<Output = Result<Rendered>> + Send + use<> {
        let submitted = req
            .validate()
            .and_then(|()| self.ensure_style(&req.style))
            .and_then(|()| self.submit(|reply| Command::Tile { req, reply }));
        Self::wait_async(submitted)
    }

    /// Block until a worker replies.
    fn wait(rx: ReplyReceiver) -> Result<Rendered> {
        rx.recv().map_err(|_| MapliError::WorkerGone)?
    }

    /// Wait for a worker reply without blocking. Takes the submission result so that
    /// submission errors are returned from the future.
    async fn wait_async(submitted: Result<ReplyReceiver>) -> Result<Rendered> {
        submitted?.await.map_err(|_| MapliError::WorkerGone)?
    }

    fn ensure_style(&self, id: &StyleId) -> Result<()> {
        if self.has_style(id) {
            Ok(())
        } else {
            Err(MapliError::UnknownStyle(id.clone()))
        }
    }

    /// Submit a command to the pool and return a receiver for the reply.
    fn submit(&self, make_command: impl FnOnce(Reply) -> Command) -> Result<ReplyReceiver> {
        let (reply, rx) = oneshot::channel();
        let tx = self.tx.as_ref().ok_or(MapliError::PoolClosed)?;
        // `send` only fails if the receiver has been dropped, which means the pool is shutting down.
        tx.send(make_command(reply))
            .map_err(|_| MapliError::PoolClosed)?;
        Ok(rx)
    }
}
