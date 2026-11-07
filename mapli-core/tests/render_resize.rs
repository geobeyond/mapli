//! Static renderers are reused across image sizes, being resized before each render.
//!
//! These tests use a pool with a single worker that keeps a single static renderer, so every
//! render goes through the same renderer, and check that a resized renderer gives the same
//! image as a new one.

use std::num::NonZeroUsize;

use mapli_core::{
    Bounds, BoundsToFit, Camera, ImageSpec, OutputFormat, PixelRatio, PoolConfig, RenderPool,
    Rendered, RgbaImage, StaticRequest, Style, View,
};

/// Style with a red area over a white background, so it renders without network access.
const STYLE: &str = r##"{
    "version": 8,
    "sources": {
        "box": {
            "type": "geojson",
            "data": {"type": "Polygon",
                     "coordinates": [[[-10, -5], [10, -5], [10, 5], [-10, 5], [-10, -5]]]}
        }
    },
    "layers": [
        {"id": "bg", "type": "background", "paint": {"background-color": "#ffffff"}},
        {"id": "box", "type": "fill", "source": "box",
         "paint": {"fill-color": "#ff0000", "fill-antialias": false}}
    ]
}"##;

/// Pool whose single worker keeps a single static renderer.
fn create_single_renderer_pool() -> RenderPool {
    let pool = RenderPool::new(PoolConfig {
        workers: NonZeroUsize::new(1).unwrap(),
        max_renderers_per_worker: NonZeroUsize::new(1).unwrap(),
    })
    .unwrap();
    pool.register_style("box", Style::Json(STYLE.to_owned()));
    pool
}

fn render_image(pool: &RenderPool, view: View, spec: ImageSpec) -> RgbaImage {
    let req = StaticRequest {
        style: "box".into(),
        view,
        spec,
        format: OutputFormat::Raw,
    };
    match pool.render(req).unwrap() {
        Rendered::Raw(img) => img,
        Rendered::Png(_) => unreachable!("requested raw output"),
    }
}

fn build_image_spec(width: u32, height: u32) -> ImageSpec {
    ImageSpec::new(width, height).unwrap()
}

/// Render `view` at each of `sizes` with one pool, and check each image against one from a new
/// pool.
fn assert_resized_matches_fresh(view: View, sizes: &[ImageSpec]) {
    let reused = create_single_renderer_pool();
    for &size in sizes {
        let img = render_image(&reused, view, size);
        let expected = render_image(&create_single_renderer_pool(), view, size);
        assert_eq!(
            img.dimensions(),
            expected.dimensions(),
            "wrong dimensions at {size:?}"
        );
        assert!(
            img == expected,
            "resized renderer differs from a new one at {size:?}"
        );
    }
}

#[test]
fn camera_renders_match_after_resizing() {
    let camera = Camera::new(0.0, 0.0, 3.0).unwrap();
    assert_resized_matches_fresh(
        camera.into(),
        &[
            build_image_spec(64, 64),
            build_image_spec(200, 100),
            build_image_spec(64, 64),
            build_image_spec(100, 300),
        ],
    );
}

#[test]
fn bounds_to_fit_uses_resized_renderer() {
    // the zoom of a fitted camera depends on the renderer size, so it must be resized first
    let fit = BoundsToFit::new(Bounds::new(-10.0, -5.0, 10.0, 5.0).unwrap())
        .with_padding(10.0)
        .unwrap();
    assert_resized_matches_fresh(
        fit.into(),
        &[
            build_image_spec(64, 64),
            build_image_spec(300, 120),
            build_image_spec(80, 200),
        ],
    );
}

#[test]
fn exact_bounds_render_after_resizing() {
    let bounds = Bounds::new(-10.0, -5.0, 10.0, 5.0).unwrap();
    assert_resized_matches_fresh(
        View::ExactBounds(bounds),
        &[
            build_image_spec(64, 64),
            build_image_spec(200, 100),
            build_image_spec(50, 150),
        ],
    );
}

#[test]
fn image_dimensions_follow_pixel_ratio_after_resizing() {
    let pool = create_single_renderer_pool();
    let camera: View = Camera::new(0.0, 0.0, 3.0).unwrap().into();
    let ratio = PixelRatio::new(2.0).unwrap();
    for (width, height) in [(64, 64), (200, 100), (64, 64)] {
        let img = render_image(
            &pool,
            camera,
            build_image_spec(width, height).with_pixel_ratio(ratio),
        );
        assert_eq!(img.dimensions(), (width * 2, height * 2));
    }
}
