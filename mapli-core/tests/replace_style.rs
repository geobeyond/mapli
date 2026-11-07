//! Registering a style again under the same id replaces it in renderers already cached for it.
//!
//! The pool has a single worker, so the renders after the replacement go to the worker that
//! cached a renderer for the old style.

use std::num::NonZeroU32;

use mapli_core::{
    Camera, ImageSpec, OutputFormat, PixelRatio, PoolConfig, RenderPool, Rendered, RgbaImage,
    StaticRequest, Style, TileCoord, TileRequest,
};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// Style that paints the whole map in `color`.
fn build_background_style(color: &str) -> Style {
    Style::Json(format!(
        r#"{{
            "version": 8,
            "sources": {{}},
            "layers": [{{"id": "bg", "type": "background", "paint": {{"background-color": "{color}"}}}}]
        }}"#
    ))
}

fn extract_raw_image(rendered: Rendered) -> RgbaImage {
    match rendered {
        Rendered::Raw(img) => img,
        Rendered::Png(_) => unreachable!("requested raw output"),
    }
}

fn render_static(pool: &RenderPool) -> RgbaImage {
    let req = StaticRequest {
        style: "bg".into(),
        view: Camera::new(0.0, 0.0, 1.0).unwrap().into(),
        spec: ImageSpec::new(8, 8).unwrap(),
        format: OutputFormat::Raw,
    };
    extract_raw_image(pool.render(req).unwrap())
}

fn render_tile(pool: &RenderPool) -> RgbaImage {
    let req = TileRequest {
        style: "bg".into(),
        tile: TileCoord::new(1, 0, 0).unwrap(),
        tile_size: NonZeroU32::new(256).unwrap(),
        pixel_ratio: PixelRatio::default(),
        format: OutputFormat::Raw,
    };
    extract_raw_image(pool.render_tile(req).unwrap())
}

#[test]
fn replaced_style_is_used_by_cached_renderers() {
    let pool = RenderPool::new(PoolConfig::default()).unwrap();
    pool.register_style("bg", build_background_style("#ff0000"));
    assert_eq!(render_static(&pool).get_pixel(4, 4).0, RED);
    assert_eq!(render_tile(&pool).get_pixel(128, 128).0, RED);

    pool.register_style("bg", build_background_style("#0000ff"));
    assert_eq!(render_static(&pool).get_pixel(4, 4).0, BLUE);
    assert_eq!(render_tile(&pool).get_pixel(128, 128).0, BLUE);
}

#[test]
fn failed_replacement_does_not_fall_back_to_old_style() {
    let pool = RenderPool::new(PoolConfig::default()).unwrap();
    pool.register_style("bg", build_background_style("#ff0000"));
    assert_eq!(render_static(&pool).get_pixel(4, 4).0, RED);

    pool.register_style("bg", Style::Json("not a style".into()));
    let req = StaticRequest {
        style: "bg".into(),
        view: Camera::new(0.0, 0.0, 1.0).unwrap().into(),
        spec: ImageSpec::new(8, 8).unwrap(),
        format: OutputFormat::Raw,
    };
    assert!(pool.render(req).is_err());
}
