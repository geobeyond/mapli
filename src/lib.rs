//! PyO3 bindings for mapli-core.
//!
//! This crate is deliberatly thin: it converts Python arguments into mapli-core types,
//! releases the GIL around anything slow, and converts results and errors back. All real logic
//! is in mapli-core.

use std::mem::ManuallyDrop;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::{PyBytes, PyMemoryView};

use mapli_core::{
    Bounds, BoundsToFit, Camera, ImageSpec, OutputFormat, PixelRatio, PoolConfig, Rendered,
    SchemeHandler, SchemeRequest, SchemeResponse, StaticRequest, Style, TileCoord, TileRequest,
};

// We use the name `mapli` (and not `mapli-core`) for the Python module because users
// import these from the `mapli` python package
create_exception!(
    mapli,
    MapliError,
    PyException,
    "Base exception for mapli errors"
);
create_exception!(
    mapli,
    UnknownStyleError,
    MapliError,
    "The style id has not been registered"
);
create_exception!(
    mapli,
    StyleLoadError,
    MapliError,
    "MapLibre Native failed to load a style."
);
create_exception!(
    mapli,
    GraphicsUnavailableError,
    MapliError,
    "The system cannot provide the OpenGL (EGL) context needed for rendering."
);
create_exception!(
    mapli,
    RenderError,
    MapliError,
    "MapLibre Native failed render."
);
create_exception!(
    mapli,
    SchemeRegistrationError,
    MapliError,
    "A scheme handler was registered after the first render pool was created."
);

fn to_py_err(err: mapli_core::MapliError) -> PyErr {
    use mapli_core::MapliError as E;
    let msg = err.to_string();
    match err {
        E::InvalidInput(_) => PyValueError::new_err(msg),
        E::UnknownStyle(_) => UnknownStyleError::new_err(msg),
        E::StyleLoadFailed { .. } => StyleLoadError::new_err(msg),
        E::RenderFailed(_) | E::PngEncodingFailed(_) => RenderError::new_err(msg),
        E::GraphicsUnavailable(_) => GraphicsUnavailableError::new_err(msg),
        E::SchemeRegistrationClosed(_) => SchemeRegistrationError::new_err(msg),
        _ => MapliError::new_err(msg),
    }
}

/// A pool of MapLibre Native render workers.
///
/// `frozen`: Python only ever gets an immutable reference to the pool, so PyO3 is able to
/// skip runtime borrow tracking, and the object can be shared freely across Python threads.
/// This works because mapli_core::RenderPool is `Send+Sync` and only needs `&self`.
///
/// `subclass`: allows Python subclasses to be created, which is useful for adding async stuff,
/// testing, etc.
#[pyclass(frozen, subclass, module = "mapli._core")]
struct RenderPool {
    /// Dropped by hand, see `Drop`
    inner: ManuallyDrop<mapli_core::RenderPool>,
}

impl Drop for RenderPool {
    /// Dropping the pool joins its workers, and a worker may be waiting on a scheme handler,
    /// which needs the GIL to run: release the GIL while joining, or both would wait forever.
    fn drop(&mut self) {
        // SAFETY: `inner` is not used again after being taken here
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        Python::attach(|py| py.detach(move || drop(inner)));
    }
}

#[pymethods]
impl RenderPool {
    #[new]
    #[pyo3(signature = (workers = 1, max_renderers_per_worker = 8))]
    fn new(workers: usize, max_renderers_per_worker: usize) -> PyResult<Self> {
        let workers = NonZeroUsize::new(workers)
            .ok_or_else(|| PyValueError::new_err("workers must be >= 1"))?;
        let max_renderers_per_worker = NonZeroUsize::new(max_renderers_per_worker)
            .ok_or_else(|| PyValueError::new_err("max_renderers_per_worker must be >= 1"))?;
        let inner = mapli_core::RenderPool::new(PoolConfig {
            workers,
            max_renderers_per_worker,
        })
        .map_err(to_py_err)?;
        Ok(Self {
            inner: ManuallyDrop::new(inner),
        })
    }

    /// Register a style with the pool under `style_id`. Pass exactly one of `url`, `path` or `json`
    #[pyo3(signature = (style_id, *, url=None, path=None, json=None))]
    fn register_style(
        &self,
        style_id: &str,
        url: Option<&str>,
        path: Option<PathBuf>,
        json: Option<String>,
    ) -> PyResult<()> {
        let style = match (url, path, json) {
            (Some(u), None, None) => Style::Url(
                u.parse()
                    .map_err(|e| PyValueError::new_err(format!("invalid url: {u:?}: {e}")))?,
            ),
            (None, Some(p), None) => Style::Path(p),
            (None, None, Some(j)) => Style::Json(j),
            _ => {
                return Err(PyValueError::new_err(
                    "pass exactly one of url, path or json",
                ));
            }
        };
        self.inner.register_style(style_id, style);
        Ok(())
    }

    /// Whether a style has been registered under `style_id`.
    fn has_style(&self, style_id: &str) -> bool {
        self.inner.has_style(&style_id.into())
    }

    /// Render a static map image to PNG bytes.
    ///
    /// `lon` and `lat` are in degrees, `width` and `height` in logical pixels (the image is
    /// `width * pixel_ratio` by `height * pixel_ratio` pixels). `bearing` is in degrees clockwise
    /// from north, `pitch` in degrees from straight down (0-60).
    #[pyo3(signature = (
        style_id, *, lon, lat, zoom ,width, height,
        bearing = 0.0, pitch = 0.0, pixel_ratio = 1.0,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn render<'py>(
        &self,
        py: Python<'py>,
        style_id: &str,
        lon: f64,
        lat: f64,
        zoom: f64,
        width: u32,
        height: u32,
        bearing: f64,
        pitch: f64,
        pixel_ratio: f32,
    ) -> PyResult<Bound<'py, PyBytes>> {
        // build and validate the request while holding the GIL, so we can return a
        // PyErr if something is wrong
        let req = build_static_request(
            style_id,
            lon,
            lat,
            zoom,
            width,
            height,
            bearing,
            pitch,
            pixel_ratio,
        )
        .map_err(to_py_err)?;

        // now release the GIL while the worker renders and encodes, since both are slow and
        // don't touch Python objects.
        let png = py
            .detach(|| self.inner.render(req).and_then(Rendered::into_png))
            .map_err(to_py_err)?;

        // we have the GIL again: copy the bytes into a Python `bytes` object and return it
        Ok(PyBytes::new(py, &png))
    }

    /// Asynchronously render a static map image to PNG bytes.
    ///
    /// Takes the same arguments as `render`. Returns a coroutine that must be awaited inside an
    /// asyncio event loop. Nothing is submitted to the pool until it is first awaited.
    /// Cancelling it skips the render if a worker has not picked it up yet.
    #[pyo3(signature = (
        style_id, *, lon, lat, zoom ,width, height,
        bearing = 0.0, pitch = 0.0, pixel_ratio = 1.0,
    ))]
    #[allow(clippy::too_many_arguments)]
    async fn arender(
        &self,
        style_id: String,
        lon: f64,
        lat: f64,
        zoom: f64,
        width: u32,
        height: u32,
        bearing: f64,
        pitch: f64,
        pixel_ratio: f32,
    ) -> PyResult<Py<PyBytes>> {
        let req = build_static_request(
            &style_id,
            lon,
            lat,
            zoom,
            width,
            height,
            bearing,
            pitch,
            pixel_ratio,
        )
        .map_err(to_py_err)?;

        // This future is polled on the event loop thread, so it must not do CPU work.
        // The worker already encoded the PNG, which makes `into_png` a no-op here.
        let png = self
            .inner
            .render_async(req)
            .await
            .and_then(Rendered::into_png)
            .map_err(to_py_err)?;

        Ok(Python::attach(|py| PyBytes::new(py, &png).unbind()))
    }

    /// Render a static map image, fitted to a bounding box, to PNG bytes.
    ///
    /// `bbox` is `(west, south, east, north)`, in degrees. The zoom is chosen so that the bounds
    /// fit in the image, leaving `padding` on every side. `width`, `height` and `padding` are in
    /// logical pixels, `bearing` and `pitch` in degrees as in `render`. A `west` greater than
    /// `east` means the bounds cross the antimeridian, so swapped coordinates render most of the
    /// world rather than raising an error.
    #[pyo3(signature = (
        style_id, *, bbox, width, height,
        padding = 0.0, bearing = 0.0, pitch = 0.0, pixel_ratio = 1.0,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn render_from_bounds<'py>(
        &self,
        py: Python<'py>,
        style_id: &str,
        bbox: (f64, f64, f64, f64),
        width: u32,
        height: u32,
        padding: f64,
        bearing: f64,
        pitch: f64,
        pixel_ratio: f32,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let req = build_bounds_request(
            style_id,
            bbox,
            width,
            height,
            padding,
            bearing,
            pitch,
            pixel_ratio,
        )
        .map_err(to_py_err)?;
        let png = py
            .detach(|| self.inner.render(req).and_then(Rendered::into_png))
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &png))
    }

    /// Asynchronously render a static map image, fitted to a bounding box, to PNG bytes.
    ///
    /// Behaves like `arender`: returns a coroutine that submits the request when first awaited.
    #[pyo3(signature = (
        style_id, *, bbox, width, height,
        padding = 0.0, bearing = 0.0, pitch = 0.0, pixel_ratio = 1.0,
    ))]
    #[allow(clippy::too_many_arguments)]
    async fn arender_from_bounds(
        &self,
        style_id: String,
        bbox: (f64, f64, f64, f64),
        width: u32,
        height: u32,
        padding: f64,
        bearing: f64,
        pitch: f64,
        pixel_ratio: f32,
    ) -> PyResult<Py<PyBytes>> {
        let req = build_bounds_request(
            &style_id,
            bbox,
            width,
            height,
            padding,
            bearing,
            pitch,
            pixel_ratio,
        )
        .map_err(to_py_err)?;
        let png = self
            .inner
            .render_async(req)
            .await
            .and_then(Rendered::into_png)
            .map_err(to_py_err)?;

        Ok(Python::attach(|py| PyBytes::new(py, &png).unbind()))
    }

    /// Render a map tile in the XYZ (WebMercatorQuad) scheme to PNG bytes.
    ///
    /// `tile_size` is in logical pixels; the image is `tile_size * pixel_ratio` pixels wide.
    /// MapLibre zoom levels are based on 512 px tiles, so 256 px tiles cannot be rendered at z=0.
    #[pyo3(signature = (style_id, *, z, x, y, tile_size = 512, pixel_ratio = 1.0))]
    #[allow(clippy::too_many_arguments)]
    fn render_tile<'py>(
        &self,
        py: Python<'py>,
        style_id: &str,
        z: u8,
        x: u32,
        y: u32,
        tile_size: u32,
        pixel_ratio: f32,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let req =
            build_tile_request(style_id, z, x, y, tile_size, pixel_ratio).map_err(to_py_err)?;
        let png = py
            .detach(|| self.inner.render_tile(req).and_then(Rendered::into_png))
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &png))
    }

    /// Asynchronously render a map tile to PNG bytes.
    ///
    /// Behaves like `arender`: returns a coroutine that submits the request when first awaited.
    #[pyo3(signature = (style_id, *, z, x, y, tile_size = 512, pixel_ratio = 1.0))]
    #[allow(clippy::too_many_arguments)]
    async fn arender_tile(
        &self,
        style_id: String,
        z: u8,
        x: u32,
        y: u32,
        tile_size: u32,
        pixel_ratio: f32,
    ) -> PyResult<Py<PyBytes>> {
        let req =
            build_tile_request(&style_id, z, x, y, tile_size, pixel_ratio).map_err(to_py_err)?;
        let png = self
            .inner
            .render_tile_async(req)
            .await
            .and_then(Rendered::into_png)
            .map_err(to_py_err)?;

        Ok(Python::attach(|py| PyBytes::new(py, &png).unbind()))
    }

    fn __repr__(&self) -> String {
        "<mapli.RenderPool>".to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn build_static_request(
    style_id: &str,
    lon: f64,
    lat: f64,
    zoom: f64,
    width: u32,
    height: u32,
    bearing: f64,
    pitch: f64,
    pixel_ratio: f32,
) -> mapli_core::Result<StaticRequest> {
    let camera = Camera::new(lon, lat, zoom)?
        .with_bearing(bearing)?
        .with_pitch(pitch)?;
    let spec = ImageSpec::new(width, height)?.with_pixel_ratio(PixelRatio::new(pixel_ratio)?);
    Ok(StaticRequest {
        style: style_id.into(),
        view: camera.into(),
        spec,
        format: OutputFormat::Png,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_bounds_request(
    style_id: &str,
    (west, south, east, north): (f64, f64, f64, f64),
    width: u32,
    height: u32,
    padding: f64,
    bearing: f64,
    pitch: f64,
    pixel_ratio: f32,
) -> mapli_core::Result<StaticRequest> {
    let fit = BoundsToFit::new(Bounds::new(west, south, east, north)?)
        .with_padding(padding)?
        .with_bearing(bearing)?
        .with_pitch(pitch)?;
    let spec = ImageSpec::new(width, height)?.with_pixel_ratio(PixelRatio::new(pixel_ratio)?);
    let req = StaticRequest {
        style: style_id.into(),
        view: fit.into(),
        spec,
        format: OutputFormat::Png,
    };
    // check that the padding fits in the image now, while we can still raise a ValueError
    req.validate()?;
    Ok(req)
}

fn build_tile_request(
    style_id: &str,
    z: u8,
    x: u32,
    y: u32,
    tile_size: u32,
    pixel_ratio: f32,
) -> mapli_core::Result<TileRequest> {
    let tile_size = NonZeroU32::new(tile_size)
        .ok_or_else(|| mapli_core::MapliError::InvalidInput("tile_size must be > 0".into()))?;
    Ok(TileRequest {
        style: style_id.into(),
        tile: TileCoord::new(z, x, y)?,
        tile_size,
        pixel_ratio: PixelRatio::new(pixel_ratio)?,
        format: OutputFormat::Png,
    })
}

/// A request that MapLibre Native makes for a URL of a registered scheme.
#[pyclass(frozen, module = "mapli._core")]
struct ResourceRequest {
    /// The full URL, including the scheme
    #[pyo3(get)]
    url: String,
    /// What is being requested: "style", "source", "tile", "glyphs", "sprite_image",
    /// "sprite_json", "image" or "unknown". PMTiles reads are "source" requests.
    #[pyo3(get)]
    kind: &'static str,
    /// Inclusive `(start, end)` byte range for partial reads, as PMTiles makes, else `None`
    #[pyo3(get)]
    data_range: Option<(u64, u64)>,
}

#[pymethods]
impl ResourceRequest {
    fn __repr__(&self) -> String {
        let data_range = match self.data_range {
            Some((start, end)) => format!("({start}, {end})"),
            None => "None".to_string(),
        };
        format!(
            "ResourceRequest(url={:?}, kind={:?}, data_range={data_range})",
            self.url, self.kind
        )
    }
}

impl From<&SchemeRequest> for ResourceRequest {
    fn from(req: &SchemeRequest) -> Self {
        Self {
            url: req.url.clone(),
            kind: req.kind.as_str(),
            data_range: req.data_range,
        }
    }
}

/// Why a scheme handler failed to serve a request.
#[pyclass(eq, eq_int, frozen, from_py_object, module = "mapli._core")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ErrorReason {
    #[pyo3(name = "NOT_FOUND")]
    NotFound,
    #[pyo3(name = "SERVER")]
    Server,
    #[pyo3(name = "CONNECTION")]
    Connection,
    #[pyo3(name = "RATE_LIMIT")]
    RateLimit,
    #[pyo3(name = "OTHER")]
    Other,
}

impl From<ErrorReason> for mapli_core::ErrorReason {
    fn from(reason: ErrorReason) -> Self {
        match reason {
            ErrorReason::NotFound => Self::NotFound,
            ErrorReason::Server => Self::Server,
            ErrorReason::Connection => Self::Connection,
            ErrorReason::RateLimit => Self::RateLimit,
            ErrorReason::Other => Self::Other,
        }
    }
}

/// What a scheme handler answers a request with: data, no content or an error.
#[pyclass(frozen, module = "mapli._core")]
struct Response {
    inner: SchemeResponse,
}

#[pymethods]
impl Response {
    /// A successful response carrying `data`, which can be any bytes-like object.
    #[new]
    fn new(data: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            inner: SchemeResponse::Data(extract_data(data)?),
        })
    }

    /// A successful, empty response, e.g. for a missing tile, which is drawn as empty instead
    /// of failing the render.
    #[staticmethod]
    fn no_content() -> Self {
        Self {
            inner: SchemeResponse::NoContent,
        }
    }

    /// A failed response, which fails the render. `retry_after` is in seconds from now, for
    /// rate limited requests.
    #[staticmethod]
    #[pyo3(signature = (reason, message, *, retry_after = None))]
    fn error(reason: ErrorReason, message: String, retry_after: Option<f64>) -> PyResult<Self> {
        let retry_after = retry_after
            .map(|seconds| {
                Duration::try_from_secs_f64(seconds)
                    .map(|delay| SystemTime::now() + delay)
                    .map_err(|_| {
                        PyValueError::new_err(format!(
                            "retry_after {seconds} must be finite and >= 0"
                        ))
                    })
            })
            .transpose()?;
        Ok(Self {
            inner: SchemeResponse::Error {
                reason: reason.into(),
                message,
                retry_after,
            },
        })
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            SchemeResponse::Data(data) => format!("Response(<{} bytes>)", data.len()),
            SchemeResponse::NoContent => "Response.no_content()".to_string(),
            SchemeResponse::Error {
                reason, message, ..
            } => format!("Response.error({reason:?}, {message:?})"),
        }
    }
}

/// A Python callable serving the requests of a scheme.
struct PyHandler(Py<PyAny>);

impl SchemeHandler for PyHandler {
    fn handle(&self, request: &SchemeRequest) -> SchemeResponse {
        // fails if the interpreter is shutting down, in which case there is nothing to call
        Python::try_attach(
            |py| match self.0.call1(py, (ResourceRequest::from(request),)) {
                Ok(returned) => to_scheme_response(returned.bind(py)),
                Err(err) => SchemeResponse::error(
                    mapli_core::ErrorReason::Other,
                    format!("handler raised {err}"),
                ),
            },
        )
        .unwrap_or_else(|| {
            SchemeResponse::error(
                mapli_core::ErrorReason::Other,
                "the Python interpreter is not available",
            )
        })
    }
}

/// Copy the bytes out of `data`, which can be any object supporting the buffer protocol, such as
/// `bytes`, `bytearray`, `memoryview` or obstore's `Bytes`.
///
/// pyo3 only reads arbitrary buffers directly under the abi3 build of Python >= 3.11, so others
/// go through `memoryview(data).tobytes()`, at the cost of an extra copy.
fn extract_data(data: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(data) = data.extract::<PyBackedBytes>() {
        return Ok(data.to_vec());
    }
    let bytes = PyMemoryView::from(data)?.call_method0("tobytes")?;
    Ok(bytes.cast::<PyBytes>()?.as_bytes().to_vec())
}

/// Accept a `Response`, or a bytes-like object as a shortcut for `Response(data)`.
fn to_scheme_response(returned: &Bound<'_, PyAny>) -> SchemeResponse {
    if let Ok(response) = returned.cast::<Response>() {
        return response.get().inner.clone();
    }
    if let Ok(data) = extract_data(returned) {
        return SchemeResponse::Data(data);
    }
    let type_name = returned
        .get_type()
        .name()
        .map_or_else(|_| "?".to_string(), |name| name.to_string());
    SchemeResponse::error(
        mapli_core::ErrorReason::Other,
        format!("handler returned {type_name}, expected Response or a bytes-like object"),
    )
}

/// Register `handler` for the URLs of `scheme`, process-wide.
///
/// `handler` is called with a `ResourceRequest` and returns a `Response`, or `bytes`. It runs
/// on a thread owned by mapli. Handlers must be registered before the first `RenderPool` is
/// created. Schemes served by MapLibre Native itself cannot be given a handler.
#[pyfunction]
fn register_scheme_handler(scheme: &str, handler: Bound<'_, PyAny>) -> PyResult<()> {
    if !handler.is_callable() {
        return Err(PyTypeError::new_err("handler must be callable"));
    }
    mapli_core::register_scheme_handler(scheme, Arc::new(PyHandler(handler.unbind())))
        .map_err(to_py_err)
}

/// The function name must match the last segment of `module-name` in the pyproject.toml file
/// and also the `lib[name] in Cargo.toml.
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<RenderPool>()?;
    m.add_class::<ResourceRequest>()?;
    m.add_class::<Response>()?;
    m.add_class::<ErrorReason>()?;
    m.add_function(wrap_pyfunction!(register_scheme_handler, m)?)?;

    let py = m.py();
    m.add("MapliError", py.get_type::<MapliError>())?;
    m.add("UnknownStyleError", py.get_type::<UnknownStyleError>())?;
    m.add("StyleLoadError", py.get_type::<StyleLoadError>())?;
    m.add("RenderError", py.get_type::<RenderError>())?;
    m.add(
        "SchemeRegistrationError",
        py.get_type::<SchemeRegistrationError>(),
    )?;
    m.add(
        "GraphicsUnavailableError",
        py.get_type::<GraphicsUnavailableError>(),
    )?;
    Ok(())
}
