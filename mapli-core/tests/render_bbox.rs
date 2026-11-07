//! End-to-end rendering of static images that cover a bounding box exactly.
//!
//! Uses inline styles with red areas over a white (or transparent) background, so the results
//! can be checked pixel-wise without network access.

use std::f64::consts::PI;

use mapli_core::{
    Bounds, BoundsToFit, ImageSpec, MAX_LAT, OutputFormat, PoolConfig, RenderPool, Rendered,
    RgbaImage, StaticRequest, Style, View,
};

const RED: [u8; 4] = [255, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// A `(west, south, east, north)` rectangle
type Rect = (f64, f64, f64, f64);

/// Style that paints `rects` red, over a white background unless `transparent`.
fn style_json(rects: &[Rect], transparent: bool) -> String {
    let polygons = rects
        .iter()
        .map(|&(w, s, e, n)| {
            format!("[[[{w}, {s}], [{e}, {s}], [{e}, {n}], [{w}, {n}], [{w}, {s}]]]")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let background = if transparent {
        ""
    } else {
        r##"{"id": "bg", "type": "background", "paint": {"background-color": "#ffffff"}},"##
    };
    format!(
        r##"{{
            "version": 8,
            "sources": {{
                "box": {{
                    "type": "geojson",
                    "data": {{"type": "MultiPolygon", "coordinates": [{polygons}]}}
                }}
            }},
            "layers": [
                {background}
                {{"id": "box", "type": "fill", "source": "box",
                  "paint": {{"fill-color": "#ff0000", "fill-antialias": false}}}}
            ]
        }}"##
    )
}

fn pool_with(rects: &[Rect], transparent: bool) -> RenderPool {
    let pool = RenderPool::new(PoolConfig::default()).unwrap();
    pool.register_style("box", Style::Json(style_json(rects, transparent)));
    pool
}

fn render(pool: &RenderPool, view: View, width: u32, height: u32) -> RgbaImage {
    let req = StaticRequest {
        style: "box".into(),
        view,
        spec: ImageSpec::new(width, height).unwrap(),
        format: OutputFormat::Raw,
    };
    match pool.render(req).unwrap() {
        Rendered::Raw(img) => img,
        Rendered::Png(_) => unreachable!("requested raw output"),
    }
}

fn render_bbox(pool: &RenderPool, bbox: Rect, width: u32, height: u32) -> RgbaImage {
    let bounds = Bounds::new(bbox.0, bbox.1, bbox.2, bbox.3).unwrap();
    let img = render(pool, View::ExactBounds(bounds), width, height);
    assert_eq!(img.dimensions(), (width, height));
    img
}

fn lat_to_y(lat: f64) -> f64 {
    let phi = lat.to_radians();
    (1.0 - (PI / 4.0 + phi / 2.0).tan().ln() / PI) / 2.0
}

fn y_to_lat(y: f64) -> f64 {
    (PI * (1.0 - 2.0 * y)).sinh().atan().to_degrees()
}

/// Latitude at `fraction` of the way down from `north` to `south`, in Web Mercator.
fn lat_at(north: f64, south: f64, fraction: f64) -> f64 {
    let (yn, ys) = (lat_to_y(north), lat_to_y(south));
    y_to_lat(yn + fraction * (ys - yn))
}

fn pixel(img: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    img.get_pixel(x, y).0
}

/// Fraction of pixels where any channel differs by more than `threshold`.
fn fraction_differing(a: &RgbaImage, b: &RgbaImage, threshold: u8) -> f64 {
    assert_eq!(a.dimensions(), b.dimensions());
    let differing = a
        .pixels()
        .zip(b.pixels())
        .filter(|(p, q)| p.0.iter().zip(q.0).any(|(u, v)| u.abs_diff(v) > threshold))
        .count();
    differing as f64 / f64::from(a.width() * a.height())
}

#[test]
fn corner_lands_at_expected_pixel_on_both_axes() {
    // a square bbox in degrees, stretched into a 2:1 image
    let bbox = (-10.0, -10.0, 10.0, 10.0);
    // the red area's south-east corner is a quarter of the way across and down the bbox
    let corner_lat = lat_at(10.0, -10.0, 0.25);
    let pool = pool_with(&[(-20.0, corner_lat, -5.0, 20.0)], false);
    let img = render_bbox(&pool, bbox, 200, 100);

    // so it is at (50, 25)
    assert_eq!(pixel(&img, 47, 22), RED);
    assert_eq!(pixel(&img, 53, 22), WHITE);
    assert_eq!(pixel(&img, 47, 28), WHITE);
    assert_eq!(pixel(&img, 0, 0), RED);
}

#[test]
fn adjacent_bboxes_stitch_into_their_union() {
    // a diagonal staircase of rectangles crossing the seam at lon 0
    let rects: Vec<Rect> = (0..8)
        .map(|i| {
            let i = f64::from(i);
            (
                -18.0 + 4.5 * i,
                1.0 + 2.2 * i,
                -15.0 + 4.5 * i,
                3.0 + 2.2 * i,
            )
        })
        .collect();
    let pool = pool_with(&rects, false);
    let west = render_bbox(&pool, (-20.0, 0.0, 0.0, 20.0), 100, 100);
    let east = render_bbox(&pool, (0.0, 0.0, 20.0, 20.0), 100, 100);
    let whole = render_bbox(&pool, (-20.0, 0.0, 20.0, 20.0), 200, 100);

    let stitched = RgbaImage::from_fn(200, 100, |x, y| {
        if x < 100 {
            *west.get_pixel(x, y)
        } else {
            *east.get_pixel(x - 100, y)
        }
    });
    let differing = fraction_differing(&stitched, &whole, 64);
    assert!(differing < 0.01, "{differing} of the pixels differ");
}

#[test]
fn bbox_with_image_aspect_matches_fitted_bounds() {
    // an eighth of the world high and a quarter wide, in a 2:1 image: no stretch needed
    let y = y_to_lat(0.5 - 1.0 / 16.0);
    let bbox = (-45.0, -y, 45.0, y);
    let pool = pool_with(
        &[(-30.0, -20.0, 10.0, 15.0), (20.0, 5.0, 40.0, 30.0)],
        false,
    );
    let exact = render_bbox(&pool, bbox, 512, 256);
    let bounds = Bounds::new(bbox.0, bbox.1, bbox.2, bbox.3).unwrap();
    let fitted = render(&pool, BoundsToFit::new(bounds).into(), 512, 256);

    let differing = fraction_differing(&exact, &fitted, 0);
    assert!(differing < 0.005, "{differing} of the pixels differ");
}

#[test]
fn bbox_crossing_antimeridian_is_covered() {
    // red on the west side of the antimeridian only
    let pool = pool_with(&[(170.0, -20.0, 180.0, -15.0)], false);
    let img = render_bbox(&pool, (170.0, -20.0, -170.0, -15.0), 100, 100);
    assert_eq!(pixel(&img, 25, 50), RED);
    assert_eq!(pixel(&img, 75, 50), WHITE);
}

#[test]
fn whole_world_in_small_image() {
    // the world needs 512 px even at zoom 0, so it is rendered at that size and scaled down
    let pool = pool_with(&[(-180.0, -MAX_LAT, 0.0, MAX_LAT)], false);
    let img = render_bbox(&pool, (-180.0, -MAX_LAT, 180.0, MAX_LAT), 128, 64);
    assert_eq!(pixel(&img, 32, 32), RED);
    assert_eq!(pixel(&img, 96, 32), WHITE);
}

#[test]
fn bbox_deeper_than_max_zoom_is_upscaled() {
    let pool = pool_with(&[(-1.0, -1.0, 0.0, 1.0)], false);
    let img = render_bbox(&pool, (-2.5e-6, -2.5e-6, 2.5e-6, 2.5e-6), 256, 256);
    assert_eq!(pixel(&img, 64, 128), RED);
    assert_eq!(pixel(&img, 192, 128), WHITE);
}

#[test]
fn polar_bbox_is_not_shifted() {
    // MapLibre keeps renders inside the world, so a render centered on these bounds would be
    // moved south. These are rendered at zoom 0, where they are only ~90 px high.
    let (south, north) = (75.0, MAX_LAT);
    let boundary = lat_at(north, south, 0.5);
    let pool = pool_with(&[(-10.0, boundary, 190.0, MAX_LAT)], false);
    let img = render_bbox(&pool, (0.0, south, 180.0, north), 256, 256);
    assert_eq!(pixel(&img, 128, 2), RED);
    assert_eq!(pixel(&img, 128, 124), RED);
    assert_eq!(pixel(&img, 128, 132), WHITE);
    assert_eq!(pixel(&img, 128, 253), WHITE);
}

#[test]
fn transparent_map_has_no_dark_fringe() {
    let pool = pool_with(&[(-10.0, -10.0, 0.0, 10.0)], true);
    let img = render_bbox(&pool, (-10.0, -10.0, 10.0, 10.0), 300, 100);
    assert_eq!(pixel(&img, 75, 50), RED);
    assert_eq!(pixel(&img, 225, 50)[3], 0);
    for p in img.pixels().filter(|p| p.0[3] >= 16) {
        let [r, g, b, _] = p.0;
        assert!(r >= 250 && g <= 5 && b <= 5, "{p:?} has a dark fringe");
    }
}
