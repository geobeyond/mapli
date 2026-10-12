//! End-to-end rendering of static images from a bounding box.
//!
//! Uses an inline style with a red area covering exactly the bounds over a white background,
//! so the result can be checked pixel-wise without network access.

use mapli_core::{
    Bounds, BoundsToFit, Camera, ImageSpec, OutputFormat, PoolConfig, RenderPool, Rendered,
    RgbaImage, StaticRequest, Style,
};

const RED: [u8; 4] = [255, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// A `(west, south, east, north)` rectangle
type Rect = (f64, f64, f64, f64);

/// Style that paints `rects` red over a white background.
fn style_json(rects: &[Rect]) -> String {
    let polygons = rects
        .iter()
        .map(|&(w, s, e, n)| {
            format!("[[[{w}, {s}], [{e}, {s}], [{e}, {n}], [{w}, {n}], [{w}, {s}]]]")
        })
        .collect::<Vec<_>>()
        .join(", ");
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
                {{"id": "bg", "type": "background", "paint": {{"background-color": "#ffffff"}}}},
                {{"id": "box", "type": "fill", "source": "box",
                  "paint": {{"fill-color": "#ff0000", "fill-antialias": false}}}}
            ]
        }}"##
    )
}

fn render(rects: &[Rect], view: impl Into<mapli_core::View>) -> RgbaImage {
    let pool = RenderPool::new(PoolConfig::default()).unwrap();
    pool.register_style("box", Style::Json(style_json(rects)));
    let req = StaticRequest {
        style: "box".into(),
        view: view.into(),
        spec: ImageSpec::new(200, 100).unwrap(),
        format: OutputFormat::Raw,
    };
    match pool.render(req).unwrap() {
        Rendered::Raw(img) => img,
        Rendered::Png(_) => unreachable!("requested raw output"),
    }
}

/// Check that the red area was fitted into a 200x100 image with 20 px of padding.
fn assert_fitted(img: &RgbaImage) {
    assert_eq!(img.dimensions(), (200, 100));
    assert_eq!(img.get_pixel(100, 50).0, RED, "bounds should be centered");
    // the padding is outside the bounds on every side
    for (x, y) in [(100, 5), (100, 94), (5, 50), (194, 50)] {
        assert_eq!(
            img.get_pixel(x, y).0,
            WHITE,
            "pixel ({x}, {y}) is in the padding"
        );
    }
    // the bounds fill the image along the limiting dimension, up to the padding
    let limited_vertically = img.get_pixel(100, 25).0 == RED && img.get_pixel(100, 75).0 == RED;
    let limited_horizontally = img.get_pixel(25, 50).0 == RED && img.get_pixel(175, 50).0 == RED;
    assert!(limited_vertically || limited_horizontally);
}

#[test]
fn bounds_are_fitted_inside_padding() {
    let fit = BoundsToFit::new(Bounds::new(-10.0, -5.0, 10.0, 5.0).unwrap())
        .with_padding(20.0)
        .unwrap();
    assert_fitted(&render(&[(-10.0, -5.0, 10.0, 5.0)], fit));
}

#[test]
fn bounds_crossing_antimeridian_are_fitted() {
    // Fiji: 20 degrees wide, split at the antimeridian as RFC 7946 recommends.
    // Fitting the hull of the corners instead would show the 340 degrees in between, where the
    // red area would be a sliver at either edge.
    let fiji = [(170.0, -20.0, 180.0, -15.0), (-180.0, -20.0, -170.0, -15.0)];
    let fit = BoundsToFit::new(Bounds::new(170.0, -20.0, -170.0, -15.0).unwrap())
        .with_padding(20.0)
        .unwrap();
    assert_fitted(&render(&fiji, fit));
}

#[test]
fn camera_view_still_renders() {
    let img = render(
        &[(-10.0, -5.0, 10.0, 5.0)],
        Camera::new(0.0, 0.0, 3.0).unwrap(),
    );
    assert_eq!(img.get_pixel(100, 50).0, RED);
}

#[test]
fn plain_bounds_fill_the_image() {
    // no padding, so the bounds reach the image edges along the limiting dimension
    let img = render(
        &[(-10.0, -5.0, 10.0, 5.0)],
        Bounds::new(-10.0, -5.0, 10.0, 5.0).unwrap(),
    );
    let limited_vertically = img.get_pixel(100, 1).0 == RED && img.get_pixel(100, 98).0 == RED;
    let limited_horizontally = img.get_pixel(1, 50).0 == RED && img.get_pixel(198, 50).0 == RED;
    assert!(limited_vertically || limited_horizontally);
}
