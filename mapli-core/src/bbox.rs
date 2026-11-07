//! Planning of renders that cover a bounding box exactly.
//!
//! A bbox render shows exactly the requested bounds, stretched to the requested image size, as
//! OGC API Maps and WMS GetMap require. MapLibre Native only renders square pixels, so this is
//! done in two steps: render the bounds with square pixels, at a zoom where they fit in the
//! render, then crop them out of the render and stretch them to the output size.
//!
//! This module computes the first step: the camera, the size of the render, and where the
//! bounds land in it.

use std::f64::consts::PI;
use std::num::NonZeroU32;

use crate::types::{Bounds, Camera, ImageSpec, MAX_ZOOM};

/// MapLibre zoom levels are based on 512 px tiles: at zoom `z` the world is `512 * 2^z`
/// logical pixels wide.
const TILE_SIZE: f64 = 512.0;

/// A rectangle in pixels, with fractional edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PixelRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl PixelRect {
    /// Scale from one pixel grid to another, e.g. from logical to image pixels.
    pub fn scaled(self, sx: f64, sy: f64) -> Self {
        Self {
            left: self.left * sx,
            top: self.top * sy,
            width: self.width * sx,
            height: self.height * sy,
        }
    }

    /// Whether all edges are within `tolerance` of the edges of an image of `width × height`.
    pub fn covers(&self, width: u32, height: u32, tolerance: f64) -> bool {
        let close = |a: f64, b: u32| (a - f64::from(b)).abs() <= tolerance;
        close(self.left, 0)
            && close(self.top, 0)
            && close(self.left + self.width, width)
            && close(self.top + self.height, height)
    }
}

/// How to render a bbox: the render to make, and where the bbox lands in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BboxRenderPlan {
    pub camera: Camera,
    /// Size of the render. This is the requested size, except when the world is too small to
    /// fill a render of that size at the chosen zoom (see [`plan_bbox_render`]).
    pub render_spec: ImageSpec,
    /// The bbox within the render, in logical pixels of `render_spec`.
    pub crop: PixelRect,
}

/// Plan a render that covers `bounds` exactly, for an output image of `spec`.
///
/// The zoom is the highest at which the bounds fit in the output image, clamped to
/// `0..=MAX_ZOOM`, so the stretch never has to throw away more detail than necessary.
///
/// MapLibre Native constrains the camera so the render never extends past the north or south
/// edge of the world (`ConstrainMode::HeightOnly`). The camera center is therefore clamped
/// the same way here, and the crop accounts for it, instead of assuming that the bounds are
/// centered in the render. When the world is shorter than the requested height at the chosen
/// zoom (which includes zooms below 0, where the world does not even fit the bounds), the
/// render is instead made at the bounds' own size, at zoom 0 or above.
pub(crate) fn plan_bbox_render(bounds: Bounds, spec: ImageSpec) -> BboxRenderPlan {
    let west_x = lon_to_x(bounds.west());
    let east_x = lon_to_x(bounds.unwrapped_east());
    let north_y = lat_to_y(bounds.north());
    let south_y = lat_to_y(bounds.south());
    let dx = east_x - west_x;
    let dy = south_y - north_y;

    let width = f64::from(spec.width.get());
    let height = f64::from(spec.height.get());
    // logical pixels per world width at which the bounds fill the output along one axis
    let fit = (width / dx).min(height / dy);
    let max_world = TILE_SIZE * MAX_ZOOM.exp2();
    let mut world = fit.clamp(TILE_SIZE, max_world);

    let (render_width, render_height) = if fit >= TILE_SIZE && world >= height {
        (spec.width, spec.height)
    } else {
        // Render only the bounds. Their height in pixels is at most the world's, since dy <= 1,
        // but rounding it up could exceed a fractional world height, so the world is then
        // grown to that whole number of pixels.
        let mut h = (dy * world).ceil();
        if h > world {
            world = h;
            h = (dy * world).ceil();
        }
        (at_least_one(dx * world), at_least_one(h))
    };
    let render_w = f64::from(render_width.get());
    let render_h = f64::from(render_height.get());

    let center_x = (west_x + east_x) / 2.0;
    let half_height = render_h / (2.0 * world);
    let center_y = ((north_y + south_y) / 2.0).clamp(half_height, 1.0 - half_height);

    let mut lon = center_x * 360.0 - 180.0;
    if lon > 180.0 {
        lon -= 360.0;
    }
    let camera = Camera {
        lon,
        lat: y_to_lat(center_y),
        zoom: (world / TILE_SIZE).log2(),
        bearing: 0.0,
        pitch: 0.0,
    };

    BboxRenderPlan {
        camera,
        render_spec: ImageSpec {
            width: render_width,
            height: render_height,
            pixel_ratio: spec.pixel_ratio,
        },
        crop: PixelRect {
            left: render_w / 2.0 - (center_x - west_x) * world,
            top: render_h / 2.0 - (center_y - north_y) * world,
            width: dx * world,
            height: dy * world,
        },
    }
}

fn at_least_one(pixels: f64) -> NonZeroU32 {
    // `as` saturates, and the sizes here are at most a world at zoom 0, i.e. 512 px
    NonZeroU32::new(pixels.ceil() as u32).unwrap_or(NonZeroU32::MIN)
}

/// Web Mercator x, from 0 at 180°W to 1 at 180°E (and beyond, for unwrapped longitudes).
fn lon_to_x(lon: f64) -> f64 {
    (lon + 180.0) / 360.0
}

/// Web Mercator y, from 0 at the north edge of the world to 1 at the south edge.
fn lat_to_y(lat: f64) -> f64 {
    let phi = lat.to_radians();
    (1.0 - (PI / 4.0 + phi / 2.0).tan().ln() / PI) / 2.0
}

fn y_to_lat(y: f64) -> f64 {
    (PI * (1.0 - 2.0 * y)).sinh().atan().to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MAX_LAT, PixelRatio};

    const EPS: f64 = 1e-6;

    fn plan(bbox: (f64, f64, f64, f64), width: u32, height: u32) -> BboxRenderPlan {
        let bounds = Bounds::new(bbox.0, bbox.1, bbox.2, bbox.3).unwrap();
        plan_bbox_render(bounds, ImageSpec::new(width, height).unwrap())
    }

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < EPS, "{a} != {b}");
    }

    #[test]
    fn mercator_round_trips() {
        assert_close(lat_to_y(0.0), 0.5);
        assert_close(lat_to_y(MAX_LAT), 0.0);
        assert_close(lat_to_y(-MAX_LAT), 1.0);
        for lat in [-80.0, -38.7, 0.0, 12.3, 85.0] {
            assert_close(y_to_lat(lat_to_y(lat)), lat);
        }
    }

    #[test]
    fn square_bbox_in_wide_image_is_stretched_horizontally() {
        // a quarter of the world wide, an eighth high, centered on the equator
        let y = y_to_lat(0.5 - 1.0 / 16.0);
        let p = plan((-45.0, -y, 45.0, y), 512, 512);
        // fits vertically: 512 px for an eighth of the world
        assert_close(p.camera.zoom, 2.0);
        assert_close(p.camera.lon, 0.0);
        assert_close(p.camera.lat, 0.0);
        assert_eq!(p.render_spec, ImageSpec::new(512, 512).unwrap());
        assert_close(p.crop.left, 0.0);
        assert_close(p.crop.width, 512.0);
        assert_close(p.crop.top, 128.0);
        assert_close(p.crop.height, 256.0);
    }

    #[test]
    fn bbox_with_image_aspect_needs_no_stretch() {
        let y = y_to_lat(0.5 - 1.0 / 16.0);
        let p = plan((-45.0, -y, 45.0, y), 800, 400);
        assert!(p.crop.covers(800, 400, EPS));
    }

    #[test]
    fn antimeridian_bbox_is_centered_on_it() {
        let p = plan((170.0, -10.0, -170.0, 10.0), 256, 256);
        assert_close(p.camera.lon.abs(), 180.0);
        let p = plan((160.0, -10.0, -170.0, 10.0), 256, 256);
        assert_close(p.camera.lon, 175.0);
        let p = plan((170.0, -10.0, -160.0, 10.0), 256, 256);
        assert_close(p.camera.lon, -175.0);
        // 20 or 30 degrees wide, not 340
        assert!(p.crop.width <= 256.0 + EPS);
    }

    #[test]
    fn whole_world_in_small_image_renders_at_zoom_0() {
        let p = plan((-180.0, -MAX_LAT, 180.0, MAX_LAT), 256, 128);
        assert_close(p.camera.zoom, 0.0);
        assert_eq!(p.render_spec, ImageSpec::new(512, 512).unwrap());
        assert!(p.crop.covers(512, 512, EPS));
    }

    #[test]
    fn deep_bbox_is_clamped_to_max_zoom() {
        let p = plan((0.0, 0.0, 1e-6, 1e-6), 256, 256);
        assert_close(p.camera.zoom, MAX_ZOOM);
        assert_eq!(p.render_spec, ImageSpec::new(256, 256).unwrap());
        // the crop is a centered speck of the render
        assert!(p.crop.width < 256.0 && p.crop.height < 256.0);
        assert_close(p.crop.left + p.crop.width / 2.0, 128.0);
        assert_close(p.crop.top + p.crop.height / 2.0, 128.0);
    }

    #[test]
    fn polar_bbox_center_is_clamped_inside_the_world() {
        // the bounds touch the north edge and are wide, so they fit at zoom 0, where they are
        // only ~90 px high: a render centered on them would extend past the north edge
        let p = plan((0.0, 75.0, 180.0, MAX_LAT), 256, 256);
        assert_close(p.camera.zoom, 0.0);
        assert_eq!(p.render_spec, ImageSpec::new(256, 256).unwrap());
        let world = TILE_SIZE * p.camera.zoom.exp2();
        assert_close(lat_to_y(p.camera.lat), 128.0 / world);
        // so the bounds are at the top of the render, not in the middle
        assert_close(p.crop.top, 0.0);
        assert!(p.crop.top + p.crop.height < 256.0);
    }

    #[test]
    fn world_shorter_than_image_renders_bbox_size() {
        // a band around the whole world, in an image taller than the world at zoom 0
        let p = plan((-180.0, -60.0, 180.0, 60.0), 600, 2000);
        assert_close(p.camera.zoom, (600.0 / TILE_SIZE).log2());
        assert_eq!(p.render_spec.width.get(), 600);
        assert!(p.render_spec.height.get() < 600);
        assert!(p.crop.covers(600, p.render_spec.height.get(), 1.0 + EPS));
    }

    #[test]
    fn fractional_world_height_is_rounded_up() {
        // the bounds fit at a world 1263.6 px wide, too short for the image, and are then the
        // whole world high, so their render would be 1264 px high
        let p = plan((0.0, -MAX_LAT, 100.0, MAX_LAT), 351, 2000);
        assert_eq!(p.render_spec.height.get(), 1264);
        let world = TILE_SIZE * p.camera.zoom.exp2();
        assert!(f64::from(p.render_spec.height.get()) <= world + EPS);
        assert!(p.crop.top >= -EPS);
        assert!(p.crop.top + p.crop.height <= f64::from(p.render_spec.height.get()) + EPS);
    }

    #[test]
    fn pixel_ratio_is_kept() {
        let bounds = Bounds::new(-10.0, -10.0, 10.0, 10.0).unwrap();
        let spec = ImageSpec::new(100, 100)
            .unwrap()
            .with_pixel_ratio(PixelRatio::new(2.0).unwrap());
        let p = plan_bbox_render(bounds, spec);
        assert_eq!(p.render_spec, spec);
    }
}
