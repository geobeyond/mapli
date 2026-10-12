//! Render a map using the mapli-core library.
//!
//! This example demonstrates how to render a map using the Mapli Core library. It creates a
//! render pool, registers a style from a URL, sets the camera position, and renders the map to
//! an image. Finally, it saves the rendered image as "map.png".
//!
//! Notably, there is no trace of the wrapped MapLibre Native, as mapli-core abstracts away the
//! underlying implementation details.

use mapli_core::{Camera, ImageSpec, OutputFormat, PoolConfig, RenderPool, StaticRequest, Style};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = RenderPool::new(PoolConfig::default())?;
    pool.register_style(
        "demo",
        Style::Url("https://demotiles.maplibre.org/style.json".parse()?),
    );

    let req = StaticRequest {
        style: "demo".into(),
        view: Camera::new(-9.14, 38.72, 4.0)?.into(),
        spec: ImageSpec::new(512, 512)?,
        format: OutputFormat::Png,
    };

    // the worker thread already encoded the image, so this is a no-op
    let png = pool.render(req)?.into_png()?;
    std::fs::write("map.png", &png)?;
    println!("Wrote map.png ({} bytes)", png.len());
    Ok(())
}
