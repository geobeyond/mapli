//! Public value types for mapli-core.
//!
//! These types are all plain data. This means they are `Send + Sync`-able and can be used
//! in threaded code. Also importantly, their public API is free of `maplibre_native` types:
//! conversions to and from them are crate-private.
//! Validation is done via constructors, which means that instances are always implicitly valid.
//!

use std::fmt;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use image::RgbaImage;
use maplibre_native::file_source::{ErrorReason as MlnErrorReason, Response};
use maplibre_native::{ResourceKind as MlnResourceKind, ResourceRequest};
use url::Url;

use crate::encode::encode_png;
use crate::error::{MapliError, Result};

/// Web Mercator latitude limit.
/// https://www.coordinately.org/learn/web-mercator-projection
pub const MAX_LAT: f64 = 85.051_128_779_806_59;

/// Upper zoom bound we accept.
pub const MAX_ZOOM: f64 = 24.0;

/// Upper pitch bound in degrees. Verify against MapLibre Native's current limit.
pub const MAX_PITCH: f64 = 60.0;

/// Highest tile zoom level; 2^24 still fits comfortably in a u32.
pub const MAX_TILE_ZOOM: u8 = 24;

/// Identifier under which a style is registered in the pool
/// Cloning is a refcount bump, as IDs are cloned into every request and cache key
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StyleId(Arc<str>);

impl StyleId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StyleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<&str> for StyleId {
    fn from(s: &str) -> Self {
        StyleId(Arc::from(s))
    }
}

impl From<String> for StyleId {
    fn from(s: String) -> Self {
        StyleId(Arc::from(s))
    }
}

#[derive(Debug, Clone)]
pub enum Style {
    Url(Url),
    Path(PathBuf),
    Json(String),
}

/// Map camera, in lon/lat order
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Degrees
    pub lon: f64,
    /// Degrees
    pub lat: f64,
    pub zoom: f64,
    /// Degrees clockwise from north
    pub bearing: f64,
    /// Degrees from straight down, in [0, MAX_PITCH]
    pub pitch: f64,
}

impl Camera {
    pub fn new(lon: f64, lat: f64, zoom: f64) -> Result<Self> {
        if !(-180.0..=180.0).contains(&lon) {
            return Err(MapliError::InvalidInput(format!(
                "lon {lon} not in valid range of [-180, 180]"
            )));
        }
        if !(-MAX_LAT..=MAX_LAT).contains(&lat) {
            return Err(MapliError::InvalidInput(format!(
                "lat {lat} not in valid range of [-{MAX_LAT}, {MAX_LAT}]"
            )));
        }
        if !(0.0..=MAX_ZOOM).contains(&zoom) {
            return Err(MapliError::InvalidInput(format!(
                "zoom {zoom} not in valid range of [0, {MAX_ZOOM}]"
            )));
        }
        Ok(Self {
            lon,
            lat,
            zoom,
            bearing: 0.0,
            pitch: 0.0,
        })
    }

    /// Initialize with an input bearing, in degrees; Accepts any finite value, which gets normalized to [0, 360].
    pub fn with_bearing(mut self, bearing: f64) -> Result<Self> {
        self.bearing = validate_bearing(bearing)?;
        Ok(self)
    }

    /// Initialize with an input pitch, in degrees from straight down; Accepts [0, MAX_PITCH].
    pub fn with_pitch(mut self, pitch: f64) -> Result<Self> {
        self.pitch = validate_pitch(pitch)?;
        Ok(self)
    }

    pub fn lon(&self) -> f64 {
        self.lon
    }
    pub fn lat(&self) -> f64 {
        self.lat
    }
    pub fn zoom(&self) -> f64 {
        self.zoom
    }
    pub fn bearing(&self) -> f64 {
        self.bearing
    }
    pub fn pitch(&self) -> f64 {
        self.pitch
    }

    /// Convert to a maplibre_native CameraUpdate
    pub(crate) fn to_camera_update(self) -> maplibre_native::CameraUpdate {
        maplibre_native::CameraUpdate::new()
            .center(maplibre_native::LatLng {
                lat: self.lat,
                lng: self.lon,
            })
            .zoom(self.zoom)
            .bearing(self.bearing)
            .pitch(self.pitch)
    }
}

/// Validate a bearing in degrees; Accepts any finite value, which gets normalized to [0, 360].
fn validate_bearing(bearing: f64) -> Result<f64> {
    if !bearing.is_finite() {
        return Err(MapliError::InvalidInput(format!(
            "bearing {bearing} is not finite"
        )));
    }
    Ok(bearing.rem_euclid(360.0))
}

fn validate_pitch(pitch: f64) -> Result<f64> {
    if !(0.0..=MAX_PITCH).contains(&pitch) {
        return Err(MapliError::InvalidInput(format!(
            "pitch {pitch} is not in valid range of [0, {MAX_PITCH}]"
        )));
    }
    Ok(pitch)
}

/// Geographic bounding box, in lon/lat order
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    west: f64,
    south: f64,
    east: f64,
    north: f64,
}

impl Bounds {
    /// Bounds with `west > east` cross the antimeridian, as in GeoJSON (RFC 7946, section 5.2).
    /// For example, `Bounds::new(170.0, -20.0, -170.0, -15.0)` is 20 degrees wide.
    pub fn new(west: f64, south: f64, east: f64, north: f64) -> Result<Self> {
        for lon in [west, east] {
            if !(-180.0..=180.0).contains(&lon) {
                return Err(MapliError::InvalidInput(format!(
                    "lon {lon} not in valid range of [-180, 180]"
                )));
            }
        }
        for lat in [south, north] {
            if !(-MAX_LAT..=MAX_LAT).contains(&lat) {
                return Err(MapliError::InvalidInput(format!(
                    "lat {lat} not in valid range of [-{MAX_LAT}, {MAX_LAT}]"
                )));
            }
        }
        // catches west == east and also 180 -> -180, which are zero-width either way
        if unwrap_east(west, east) == west {
            return Err(MapliError::InvalidInput(format!(
                "bounds from west {west} to east {east} have zero width"
            )));
        }
        if south >= north {
            return Err(MapliError::InvalidInput(format!(
                "south {south} must be less than north {north}"
            )));
        }
        Ok(Self {
            west,
            south,
            east,
            north,
        })
    }

    pub fn west(&self) -> f64 {
        self.west
    }
    pub fn south(&self) -> f64 {
        self.south
    }
    pub fn east(&self) -> f64 {
        self.east
    }
    pub fn north(&self) -> f64 {
        self.north
    }

    pub fn crosses_antimeridian(&self) -> bool {
        self.west > self.east
    }

    /// Convert to a maplibre_native LatLngBounds
    ///
    /// MapLibre builds the bounds as the hull of both corners, which would turn an
    /// antimeridian-crossing box into one spanning the rest of the globe. Unwrapping `east`
    /// past 180 keeps the box on the intended side.
    pub(crate) fn to_lat_lng_bounds(self) -> maplibre_native::LatLngBounds {
        maplibre_native::LatLngBounds {
            southwest: maplibre_native::LatLng {
                lat: self.south,
                lng: self.west,
            },
            northeast: maplibre_native::LatLng {
                lat: self.north,
                lng: unwrap_east(self.west, self.east),
            },
        }
    }
}

/// Shift `east` past 180 when the bounds cross the antimeridian, so that `west < east`.
fn unwrap_east(west: f64, east: f64) -> f64 {
    if west > east { east + 360.0 } else { east }
}

/// How to frame `Bounds` in the rendered image
///
/// The zoom is not known up front: it is derived at render time from the bounds, the padding
/// and the size of the output image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundsToFit {
    bounds: Bounds,
    padding: f64,
    bearing: f64,
    pitch: f64,
}

impl BoundsToFit {
    pub fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            padding: 0.0,
            bearing: 0.0,
            pitch: 0.0,
        }
    }

    /// Initialize with padding, in logical pixels, to leave around the bounds on every side.
    pub fn with_padding(mut self, padding: f64) -> Result<Self> {
        if !padding.is_finite() || padding < 0.0 {
            return Err(MapliError::InvalidInput(format!(
                "padding {padding} must be finite and >= 0.0"
            )));
        }
        self.padding = padding;
        Ok(self)
    }

    /// Initialize with an input bearing, in degrees; Accepts any finite value, which gets normalized to [0, 360].
    pub fn with_bearing(mut self, bearing: f64) -> Result<Self> {
        self.bearing = validate_bearing(bearing)?;
        Ok(self)
    }

    /// Initialize with an input pitch, in degrees from straight down; Accepts [0, MAX_PITCH].
    pub fn with_pitch(mut self, pitch: f64) -> Result<Self> {
        self.pitch = validate_pitch(pitch)?;
        Ok(self)
    }

    pub fn bounds(&self) -> Bounds {
        self.bounds
    }
    pub fn padding(&self) -> f64 {
        self.padding
    }
    pub fn bearing(&self) -> f64 {
        self.bearing
    }
    pub fn pitch(&self) -> f64 {
        self.pitch
    }
}

impl From<Bounds> for BoundsToFit {
    fn from(bounds: Bounds) -> Self {
        BoundsToFit::new(bounds)
    }
}

/// What a static image shows: either an explicit camera or bounds to fit
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    Camera(Camera),
    BoundsToFit(BoundsToFit),
}

impl From<Camera> for View {
    fn from(camera: Camera) -> Self {
        View::Camera(camera)
    }
}

impl From<BoundsToFit> for View {
    fn from(fit: BoundsToFit) -> Self {
        View::BoundsToFit(fit)
    }
}

/// Fit the bounds with no padding, bearing nor pitch
impl From<Bounds> for View {
    fn from(bounds: Bounds) -> Self {
        View::BoundsToFit(bounds.into())
    }
}

/// Device pixel ratio
///
/// `f32` does not implement `Eq` nor `Hash` (because of NaN and -0.0), but we need both in order
/// to be able to incorporate this in cache keys. As such this type rejects non-finite and
/// non-positive values, after which comparing and hashing is possible in a sound way.
#[derive(Debug, Clone, Copy)]
pub struct PixelRatio(f32);

impl PixelRatio {
    pub fn new(ratio: f32) -> Result<Self> {
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err(MapliError::InvalidInput(format!(
                "pixel ratio {ratio} must be finite and > 0.0"
            )));
        }
        Ok(Self(ratio))
    }

    pub fn get(&self) -> f32 {
        self.0
    }
}

impl Default for PixelRatio {
    fn default() -> Self {
        Self(1.0)
    }
}

impl PartialEq for PixelRatio {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for PixelRatio {}

impl Hash for PixelRatio {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

/// Output image dimensions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageSpec {
    /// Logical pixels; the output image is `width * pixel_ratio` pixels wide
    pub width: NonZeroU32,
    /// Logical pixels; the output image is `height * pixel_ratio` pixels high
    pub height: NonZeroU32,
    pub pixel_ratio: PixelRatio,
}

impl ImageSpec {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let w = NonZeroU32::new(width)
            .ok_or_else(|| MapliError::InvalidInput("width must be > 0".into()))?;
        let h = NonZeroU32::new(height)
            .ok_or_else(|| MapliError::InvalidInput("height must be > 0".into()))?;
        Ok(Self {
            width: w,
            height: h,
            pixel_ratio: PixelRatio::default(),
        })
    }

    pub fn with_pixel_ratio(mut self, pixel_ratio: PixelRatio) -> Self {
        self.pixel_ratio = pixel_ratio;
        self
    }
}

/// A tile address in WebMercatorQuad (XYZ) scheme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    z: u8,
    x: u32,
    y: u32,
}

impl TileCoord {
    pub fn new(z: u8, x: u32, y: u32) -> Result<Self> {
        if z > MAX_TILE_ZOOM {
            return Err(MapliError::InvalidInput(format!(
                "tile zoom {z} is not in valid range of [0, {MAX_TILE_ZOOM}]"
            )));
        }

        let n = 1u32 << z;
        if x >= n || y >= n {
            return Err(MapliError::InvalidInput(format!(
                "tile coords {z}/{x}/{y} are out of range"
            )));
        }
        Ok(Self { z, x, y })
    }

    pub fn z(&self) -> u8 {
        self.z
    }
    pub fn x(&self) -> u32 {
        self.x
    }
    pub fn y(&self) -> u32 {
        self.y
    }
}

/// Output format of a rendered image.
///
/// Encoding happens on the worker thread, so callers receive ready-to-use bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Raw RGBA pixels, no encoding.
    #[default]
    Raw,
    Png,
}

/// A rendered image, in the `OutputFormat` that was requested.
#[derive(Debug, Clone)]
pub enum Rendered {
    Raw(RgbaImage),
    Png(Vec<u8>),
}

impl Rendered {
    /// Return PNG bytes, encoding them first if this is a raw image.
    pub fn into_png(self) -> Result<Vec<u8>> {
        match self {
            Rendered::Raw(img) => encode_png(&img),
            Rendered::Png(bytes) => Ok(bytes),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StaticRequest {
    pub style: StyleId,
    pub view: View,
    pub spec: ImageSpec,
    pub format: OutputFormat,
}

impl StaticRequest {
    /// Check that the view fits in the image.
    ///
    /// Bounds padding must leave some room for the bounds themselves, otherwise MapLibre
    /// cannot compute a zoom level for them.
    pub fn validate(&self) -> Result<()> {
        if let View::BoundsToFit(fit) = self.view {
            let min_side = self.spec.width.min(self.spec.height).get();
            if 2.0 * fit.padding() >= f64::from(min_side) {
                return Err(MapliError::InvalidInput(format!(
                    "padding {} leaves no room for the bounds in a {}x{} image",
                    fit.padding(),
                    self.spec.width,
                    self.spec.height
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct TileRequest {
    pub style: StyleId,
    pub tile: TileCoord,
    /// Tile edge, in logical pixels; the output image is `tile_size * pixel_ratio` pixels wide
    pub tile_size: NonZeroU32,
    pub pixel_ratio: PixelRatio,
    pub format: OutputFormat,
}

impl TileRequest {
    /// Check that the tile can be rendered at its size.
    ///
    /// MapLibre zoom levels are based on 512 px tiles, so a smaller tile at zoom `z` is drawn at
    /// map zoom `z - log2(512 / tile_size)`. MapLibre cannot zoom out below 0, which means e.g. a
    /// 256 px tile at zoom 0 cannot be rendered.
    pub fn validate(&self) -> Result<()> {
        let map_zoom = f64::from(self.tile.z()) - (512.0 / f64::from(self.tile_size.get())).log2();
        if map_zoom < 0.0 {
            return Err(MapliError::InvalidInput(format!(
                "cannot render a {} px tile at zoom {}",
                self.tile_size,
                self.tile.z()
            )));
        }
        Ok(())
    }
}

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
    pub(crate) fn from_mln(request: &ResourceRequest) -> Self {
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
    pub(crate) fn into_mln(self, url: &str) -> Response {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert that everything that crosses the thread boundary is Send+Sync
    #[test]
    fn types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StyleId>();
        assert_send_sync::<Style>();
        assert_send_sync::<StaticRequest>();
        assert_send_sync::<TileRequest>();
        assert_send_sync::<MapliError>();
        assert_send_sync::<Rendered>();
    }

    #[test]
    fn rendered_into_png() {
        let png = Rendered::Raw(RgbaImage::new(1, 1)).into_png().unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        assert_eq!(Rendered::Png(png.clone()).into_png().unwrap(), png);
    }

    #[test]
    fn camera_rejects_out_of_range() {
        assert!(Camera::new(181.0, 0.0, 0.0).is_err());
        assert!(Camera::new(0.0, 89.0, 0.0).is_err());
        assert!(Camera::new(0.0, 0.0, -1.0).is_err());
        assert!(Camera::new(-9.14, 38.72, 12.0).is_ok());
    }

    #[test]
    fn bearing_is_normalised() {
        let cam = Camera::new(0.0, 0.0, 0.0)
            .unwrap()
            .with_bearing(-90.0)
            .unwrap();
        assert_eq!(cam.bearing(), 270.0);
    }

    #[test]
    fn pixel_ratio_rejects_nan_and_zero() {
        assert!(PixelRatio::new(f32::NAN).is_err());
        assert!(PixelRatio::new(0.0).is_err());
        assert!(PixelRatio::new(2.0).is_ok());
    }

    #[test]
    fn tile_coord_bounds() {
        assert!(TileCoord::new(0, 0, 0).is_ok());
        assert!(TileCoord::new(0, 1, 0).is_err());
        assert!(TileCoord::new(3, 7, 7).is_ok());
        assert!(TileCoord::new(3, 8, 0).is_err());
    }

    #[test]
    fn tile_request_rejects_zoom_too_low_for_size() {
        let req = |z, size| TileRequest {
            style: "s".into(),
            tile: TileCoord::new(z, 0, 0).unwrap(),
            tile_size: NonZeroU32::new(size).unwrap(),
            pixel_ratio: PixelRatio::default(),
            format: OutputFormat::Raw,
        };
        assert!(req(0, 256).validate().is_err());
        assert!(req(0, 512).validate().is_ok());
        assert!(req(1, 256).validate().is_ok());
        assert!(req(0, 1024).validate().is_ok());
    }

    #[test]
    fn bounds_rejects_invalid() {
        assert!(Bounds::new(-181.0, 0.0, 0.0, 1.0).is_err());
        assert!(Bounds::new(0.0, -89.0, 1.0, 1.0).is_err());
        assert!(Bounds::new(0.0, 1.0, 1.0, 0.0).is_err()); // south > north
        assert!(Bounds::new(0.0, 0.0, 0.0, 1.0).is_err()); // zero width
        assert!(Bounds::new(180.0, 0.0, -180.0, 1.0).is_err()); // zero width, wrapped
        assert!(Bounds::new(-180.0, 0.0, 180.0, 1.0).is_ok()); // whole world
        assert!(Bounds::new(-9.6, 36.9, -6.1, 42.2).is_ok());
    }

    #[test]
    fn bounds_crossing_antimeridian_are_unwrapped() {
        let b = Bounds::new(170.0, -20.0, -170.0, -15.0).unwrap();
        assert!(b.crosses_antimeridian());
        let ll = b.to_lat_lng_bounds();
        assert_eq!((ll.southwest.lng, ll.northeast.lng), (170.0, 190.0));

        let b = Bounds::new(-10.0, 0.0, 10.0, 1.0).unwrap();
        assert!(!b.crosses_antimeridian());
        let ll = b.to_lat_lng_bounds();
        assert_eq!((ll.southwest.lng, ll.northeast.lng), (-10.0, 10.0));
    }

    #[test]
    fn bounds_to_fit_rejects_invalid_padding() {
        let fit = BoundsToFit::new(Bounds::new(0.0, 0.0, 1.0, 1.0).unwrap());
        assert!(fit.with_padding(-1.0).is_err());
        assert!(fit.with_padding(f64::NAN).is_err());
        assert!(fit.with_padding(16.0).is_ok());
    }

    #[test]
    fn static_request_rejects_padding_too_large_for_image() {
        let req = |padding| StaticRequest {
            style: "s".into(),
            view: BoundsToFit::new(Bounds::new(0.0, 0.0, 1.0, 1.0).unwrap())
                .with_padding(padding)
                .unwrap()
                .into(),
            spec: ImageSpec::new(200, 100).unwrap(),
            format: OutputFormat::Raw,
        };
        assert!(req(49.0).validate().is_ok());
        assert!(req(50.0).validate().is_err());
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
    fn data_and_no_content_convert() {
        let response = SchemeResponse::Data(vec![1, 2]).into_mln("store://a");
        assert_eq!(response.data, Some(vec![1, 2]));
        assert!(SchemeResponse::NoContent.into_mln("store://a").no_content);
    }
}
