//! Custom URL scheme handlers.
//!
//! Applications can register a [`SchemeHandler`] for a URL scheme of their choosing, e.g.
//! `store`. MapLibre Native then hands every request for a URL of that scheme (e.g.
//! `store://tiles/roads.pmtiles`) to the handler, which returns the bytes. URLs of any other
//! scheme keep going to MapLibre Native's own sources.
//!
//! All handlers are served by a single dispatcher, registered in MapLibre Native's `Asset` file
//! source slot. MapLibre Native asks that slot before any other, and the dispatcher only claims
//! the URLs of registered schemes, so everything else falls through to the default sources. The
//! cost is that `asset://` URLs are no longer served once a handler is registered.
//!
//! Registration is process-global, like MapLibre Native's own file source registration, and is
//! refused once a [`RenderPool`](crate::RenderPool) has been created: MapLibre Native does not
//! update file sources it has already created.
//!
//! Handlers never run on MapLibre Native's threads. Requests are queued to a pool of handler
//! threads owned by this module, and their responses are delivered from there.
//!
//! MapLibre Native often makes several identical requests at the same time, e.g. for the same
//! PMTiles directory while loading several tiles. While a handler serves a request, identical
//! ones wait for it and get the same response, instead of calling the handler again. Nothing is
//! cached once the handler returns.

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::SystemTime;

use crossbeam_channel::{Receiver, Sender};
use maplibre_native::file_source::{ErrorReason as MlnErrorReason, Response};
use maplibre_native::{
    FileSource, FileSourceType, RequestHandle, ResourceKind as MlnResourceKind, ResourceRequest,
    Responder, register_file_source,
};

use crate::error::{MapliError, Result};

/// Number of threads that run scheme handlers.
pub const HANDLER_THREADS: usize = 8;

/// Schemes that MapLibre Native serves itself, which cannot be given a handler.
///
/// The dispatcher is asked before every other source, so a handler for one of these would take
/// over all of its requests, e.g. every HTTP request, or bypass the built-in PMTiles support.
pub const RESERVED_SCHEMES: &[&str] = &["http", "https", "file", "asset", "mbtiles", "pmtiles"];

/// What kind of resource MapLibre Native is requesting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Unknown,
    Style,
    /// A source descriptor (e.g. TileJSON or GeoJSON data), and also the byte-range reads that
    /// the PMTiles source makes for headers, directories and tiles
    Source,
    Tile,
    Glyphs,
    SpriteImage,
    SpriteJson,
    Image,
}

impl ResourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceKind::Unknown => "unknown",
            ResourceKind::Style => "style",
            ResourceKind::Source => "source",
            ResourceKind::Tile => "tile",
            ResourceKind::Glyphs => "glyphs",
            ResourceKind::SpriteImage => "sprite_image",
            ResourceKind::SpriteJson => "sprite_json",
            ResourceKind::Image => "image",
        }
    }

    fn from_mln(kind: MlnResourceKind) -> Self {
        // cxx shared enums are open, so a wildcard arm is required
        match kind {
            MlnResourceKind::Style => ResourceKind::Style,
            MlnResourceKind::Source => ResourceKind::Source,
            MlnResourceKind::Tile => ResourceKind::Tile,
            MlnResourceKind::Glyphs => ResourceKind::Glyphs,
            MlnResourceKind::SpriteImage => ResourceKind::SpriteImage,
            MlnResourceKind::SpriteJSON => ResourceKind::SpriteJson,
            MlnResourceKind::Image => ResourceKind::Image,
            _ => ResourceKind::Unknown,
        }
    }
}

/// A request for a URL of a registered scheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemeRequest {
    pub url: String,
    pub kind: ResourceKind,
    /// Inclusive `(start, end)` byte range, present for partial reads such as PMTiles'
    pub data_range: Option<(u64, u64)>,
}

impl SchemeRequest {
    fn from_mln(request: &ResourceRequest) -> Self {
        Self {
            url: request.url.clone(),
            kind: ResourceKind::from_mln(request.kind),
            data_range: request
                .data_range
                .as_ref()
                .map(|range| (*range.start(), *range.end())),
        }
    }
}

/// Why a scheme handler failed to serve a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorReason {
    NotFound,
    Server,
    Connection,
    RateLimit,
    Other,
}

impl ErrorReason {
    fn to_mln(self) -> MlnErrorReason {
        match self {
            ErrorReason::NotFound => MlnErrorReason::NotFound,
            ErrorReason::Server => MlnErrorReason::Server,
            ErrorReason::Connection => MlnErrorReason::Connection,
            ErrorReason::RateLimit => MlnErrorReason::RateLimit,
            ErrorReason::Other => MlnErrorReason::Other,
        }
    }
}

impl fmt::Display for ErrorReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ErrorReason::NotFound => "not found",
            ErrorReason::Server => "server error",
            ErrorReason::Connection => "connection error",
            ErrorReason::RateLimit => "rate limited",
            ErrorReason::Other => "error",
        })
    }
}

/// What a scheme handler answers a request with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemeResponse {
    Data(Vec<u8>),
    /// Successful but empty, e.g. a missing tile, which is drawn as empty instead of failing
    /// the render
    NoContent,
    Error {
        reason: ErrorReason,
        message: String,
        /// For rate limited requests, when they can be retried
        retry_after: Option<SystemTime>,
    },
}

impl SchemeResponse {
    pub fn error(reason: ErrorReason, message: impl Into<String>) -> Self {
        SchemeResponse::Error {
            reason,
            message: message.into(),
            retry_after: None,
        }
    }

    /// Convert to a MapLibre Native response.
    ///
    /// Error messages are prefixed with the URL and the reason, as MapLibre Native only passes
    /// the message on to render errors.
    fn into_mln(self, url: &str) -> Response {
        match self {
            SchemeResponse::Data(data) => Response::data(data),
            SchemeResponse::NoContent => Response::no_content(),
            SchemeResponse::Error {
                reason,
                message,
                retry_after,
            } => {
                let response =
                    Response::error(reason.to_mln(), format!("{url}: {reason}: {message}"));
                match retry_after {
                    Some(at) => response.with_retry_after(at),
                    None => response,
                }
            }
        }
    }
}

/// Serves the requests for the URLs of one scheme.
///
/// `handle` runs on one of the handler threads, and can block while it fetches the data.
pub trait SchemeHandler: Send + Sync + 'static {
    fn handle(&self, request: &SchemeRequest) -> SchemeResponse;
}

/// Register `handler` for the URLs of `scheme`, process-wide.
///
/// Must be called before the first [`RenderPool`](crate::RenderPool) is created. Registering an
/// already registered scheme replaces its handler. Schemes are case-insensitive.
pub fn register_scheme_handler(scheme: &str, handler: Arc<dyn SchemeHandler>) -> Result<()> {
    let scheme = validate_scheme(scheme)?;

    // held for the whole registration, so that a pool cannot be created halfway through it
    let pool_created = POOL_CREATED.lock().unwrap_or_else(|p| p.into_inner());
    if *pool_created {
        return Err(MapliError::SchemeRegistrationClosed(scheme));
    }

    let dispatch = match DISPATCH.get() {
        Some(dispatch) => dispatch,
        None => {
            let dispatch = Dispatch::start()?;
            // no race: setting only happens here, while holding the `POOL_CREATED` lock
            let dispatch = DISPATCH.get_or_init(|| dispatch);
            register_file_source(FileSourceType::Asset, Dispatcher);
            dispatch
        }
    };
    let mut handlers = dispatch.handlers.write().unwrap_or_else(|p| p.into_inner());
    handlers.insert(scheme, handler);
    Ok(())
}

/// Close registration. Called when a render pool is created, as MapLibre Native would not pick
/// up file sources registered after that.
pub(crate) fn mark_pool_created() {
    *POOL_CREATED.lock().unwrap_or_else(|p| p.into_inner()) = true;
}

/// Check that `scheme` is a valid URI scheme (RFC 3986, section 3.1) which is not reserved, and
/// return it lowercased.
fn validate_scheme(scheme: &str) -> Result<String> {
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return Err(MapliError::InvalidInput(format!(
            "{scheme:?} is not a valid URL scheme: it must start with a letter, followed by \
             letters, digits, '+', '-' or '.'"
        )));
    }
    let scheme = scheme.to_ascii_lowercase();
    if RESERVED_SCHEMES.contains(&scheme.as_str()) {
        return Err(MapliError::InvalidInput(format!(
            "scheme {scheme:?} is served by MapLibre Native and cannot be given a handler"
        )));
    }
    Ok(scheme)
}

/// Lowercased scheme of `url`, if it has one.
fn scheme_of(url: &str) -> Option<String> {
    url.split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
}

static POOL_CREATED: Mutex<bool> = Mutex::new(false);
static DISPATCH: OnceLock<Dispatch> = OnceLock::new();

/// Registered handlers, the queue to the handler threads and the requests being served.
struct Dispatch {
    handlers: RwLock<HashMap<String, Arc<dyn SchemeHandler>>>,
    jobs: Sender<Job>,
    in_flight: InFlight<RequestKey, Waiter>,
}

/// Requests with the same key get the same response.
type RequestKey = (String, Option<(u64, u64)>);

impl Dispatch {
    fn start() -> Result<Self> {
        let (jobs, rx) = crossbeam_channel::unbounded::<Job>();
        for i in 0..HANDLER_THREADS {
            let rx = rx.clone();
            // the threads are never joined: they live for as long as the process
            std::thread::Builder::new()
                .name(format!("mapli-scheme-{i}"))
                .spawn(move || run_handler_thread(rx))
                .map_err(|e| MapliError::WorkerSpawnFailed(e.to_string()))?;
        }
        Ok(Self {
            handlers: RwLock::default(),
            jobs,
            in_flight: InFlight::default(),
        })
    }

    fn handler_for(&self, url: &str) -> Option<Arc<dyn SchemeHandler>> {
        let scheme = scheme_of(url)?;
        let handlers = self.handlers.read().unwrap_or_else(|p| p.into_inner());
        handlers.get(&scheme).cloned()
    }
}

/// A handler call, serving every request waiting on `key`.
struct Job {
    key: RequestKey,
    request: SchemeRequest,
    handler: Arc<dyn SchemeHandler>,
}

/// A request waiting for the response to a job.
struct Waiter {
    responder: Responder,
    cancelled: Arc<AtomicBool>,
}

impl Waiter {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

fn run_handler_thread(rx: Receiver<Job>) {
    for job in rx.iter() {
        // threads start before `DISPATCH` is set, but jobs are only sent after that
        let dispatch = DISPATCH
            .get()
            .expect("jobs are only sent once DISPATCH is set");
        // skip jobs whose requests have all been cancelled. Their responders are dropped, which
        // does not respond to cancelled requests
        if dispatch
            .in_flight
            .abandon_if_all(&job.key, Waiter::is_cancelled)
        {
            continue;
        }
        let response = catch_unwind(AssertUnwindSafe(|| job.handler.handle(&job.request)))
            .unwrap_or_else(|_| SchemeResponse::error(ErrorReason::Other, "handler panicked"))
            .into_mln(&job.request.url);
        // completing is a no-op for requests cancelled while the handler ran
        for waiter in dispatch.in_flight.finish(&job.key) {
            waiter.responder.complete(response.clone());
        }
    }
}

/// Jobs being served, by key, each with the waiters for its response.
struct InFlight<K, W> {
    jobs: Mutex<HashMap<K, Vec<W>>>,
}

impl<K, W> Default for InFlight<K, W> {
    fn default() -> Self {
        Self {
            jobs: Mutex::default(),
        }
    }
}

impl<K: Eq + Hash, W> InFlight<K, W> {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<K, Vec<W>>> {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Add `waiter` to the job for `key`. Returns true if there was no such job, in which case
    /// the caller must start it.
    fn join(&self, key: K, waiter: W) -> bool {
        let mut jobs = self.lock();
        let waiters = jobs.entry(key).or_default();
        waiters.push(waiter);
        waiters.len() == 1
    }

    /// End the job for `key`, returning its waiters.
    fn finish(&self, key: &K) -> Vec<W> {
        self.lock().remove(key).unwrap_or_default()
    }

    /// End the job for `key` if `predicate` holds for all its waiters, dropping them. Returns
    /// whether it did. Checking and ending at once means that no waiter can join in between.
    fn abandon_if_all(&self, key: &K, predicate: impl Fn(&W) -> bool) -> bool {
        let mut jobs = self.lock();
        let abandon = jobs
            .get(key)
            .is_none_or(|waiters| waiters.iter().all(predicate));
        if abandon {
            jobs.remove(key);
        }
        abandon
    }
}

/// The `FileSource` registered with MapLibre Native, which passes requests on to the handlers.
struct Dispatcher;

impl FileSource for Dispatcher {
    fn can_request(&self, request: &ResourceRequest) -> bool {
        DISPATCH
            .get()
            .is_some_and(|dispatch| dispatch.handler_for(&request.url).is_some())
    }

    /// Called on a MapLibre Native thread, which must not wait for the handler: queue the
    /// request to the handler threads instead.
    fn request(&self, request: ResourceRequest, responder: Responder) -> RequestHandle {
        let Some(dispatch) = DISPATCH.get() else {
            responder.complete(Response::error(MlnErrorReason::Other, "no scheme handlers"));
            return RequestHandle::Done;
        };
        let Some(handler) = dispatch.handler_for(&request.url) else {
            let message = format!("{}: no handler registered for its scheme", request.url);
            responder.complete(Response::error(MlnErrorReason::Other, message));
            return RequestHandle::Done;
        };

        let request = SchemeRequest::from_mln(&request);
        let key = (request.url.clone(), request.data_range);
        let cancelled = Arc::new(AtomicBool::new(false));
        let waiter = Waiter {
            responder,
            cancelled: Arc::clone(&cancelled),
        };
        // only the first of identical requests starts a job, the others wait for its response
        if dispatch.in_flight.join(key.clone(), waiter) {
            let url = request.url.clone();
            let job = Job {
                key,
                request,
                handler,
            };
            if let Err(crossbeam_channel::SendError(job)) = dispatch.jobs.send(job) {
                // only if every handler thread has exited, which they don't
                let error = Response::error(
                    MlnErrorReason::Other,
                    format!("{url}: scheme handler threads are gone"),
                );
                for waiter in dispatch.in_flight.finish(&job.key) {
                    waiter.responder.complete(error.clone());
                }
                return RequestHandle::Done;
            }
        }
        RequestHandle::pending(move || cancelled.store(true, Ordering::Release))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_schemes_are_lowercased() {
        assert_eq!(validate_scheme("store").unwrap(), "store");
        assert_eq!(validate_scheme("My-Store+v1.2").unwrap(), "my-store+v1.2");
    }

    #[test]
    fn invalid_schemes_are_rejected() {
        for scheme in ["", "1store", "st ore", "store:", "st/ore", "stóre"] {
            assert!(validate_scheme(scheme).is_err(), "{scheme:?} was accepted");
        }
    }

    #[test]
    fn reserved_schemes_are_rejected() {
        for scheme in RESERVED_SCHEMES.iter().copied().chain(["HTTPS", "PMTiles"]) {
            assert!(validate_scheme(scheme).is_err(), "{scheme:?} was accepted");
        }
    }

    #[test]
    fn scheme_of_url() {
        assert_eq!(scheme_of("Store://a/b").as_deref(), Some("store"));
        assert_eq!(scheme_of("pmtiles://store://a").as_deref(), Some("pmtiles"));
        assert_eq!(scheme_of("no-scheme"), None);
    }

    #[test]
    fn error_message_carries_url_and_reason() {
        let response = SchemeResponse::error(ErrorReason::NotFound, "no object at a/b")
            .into_mln("store://a/b");
        let error = response.error.unwrap();
        assert_eq!(error.reason, MlnErrorReason::NotFound);
        assert_eq!(error.message, "store://a/b: not found: no object at a/b");
    }

    #[test]
    fn in_flight_merges_identical_jobs() {
        let in_flight: InFlight<&str, u32> = InFlight::default();
        assert!(in_flight.join("a", 1));
        assert!(!in_flight.join("a", 2));
        assert!(in_flight.join("b", 3)); // other keys are separate jobs
        assert_eq!(in_flight.finish(&"a"), vec![1, 2]);
        assert!(in_flight.finish(&"a").is_empty());
        assert!(in_flight.join("a", 4)); // a finished job is not reused
        assert_eq!(in_flight.finish(&"b"), vec![3]);
    }

    #[test]
    fn in_flight_abandons_only_if_all_waiters_match() {
        let in_flight: InFlight<&str, bool> = InFlight::default();
        in_flight.join("a", true);
        in_flight.join("a", false);
        assert!(!in_flight.abandon_if_all(&"a", |&cancelled| cancelled));
        assert_eq!(in_flight.finish(&"a"), vec![true, false]);

        in_flight.join("a", true);
        assert!(in_flight.abandon_if_all(&"a", |&cancelled| cancelled));
        assert!(in_flight.join("a", false)); // starts a new job
    }

    #[test]
    fn data_and_no_content_convert() {
        let response = SchemeResponse::Data(vec![1, 2]).into_mln("store://a");
        assert_eq!(response.data, Some(vec![1, 2]));
        assert!(SchemeResponse::NoContent.into_mln("store://a").no_content);
    }
}
