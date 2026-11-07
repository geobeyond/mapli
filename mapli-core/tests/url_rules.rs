//! End-to-end rendering of styles that name URLs which the URL rules allow and deny.
//!
//! The URL rules are process-global and can only be set before the first render pool is
//! created, so this file has a single test, which runs in a process of its own.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mapli_core::{
    Camera, ImageSpec, MapliError, OutputFormat, PoolConfig, RenderPool, Rendered, RgbaImage,
    SchemeHandler, SchemeRequest, SchemeResponse, StaticRequest, Style, register_scheme_handler,
    restrict_resource_urls,
};

const RED: [u8; 4] = [255, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

const BOX: &str = r#"{"type": "Polygon", "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]]}"#;

/// Style that paints the GeoJSON at `data_url` red over a white background.
fn build_style_json(data_url: &str) -> String {
    format!(
        r##"{{
            "version": 8,
            "sources": {{"box": {{"type": "geojson", "data": "{data_url}"}}}},
            "layers": [
                {{"id": "bg", "type": "background", "paint": {{"background-color": "#ffffff"}}}},
                {{"id": "box", "type": "fill", "source": "box",
                  "paint": {{"fill-color": "#ff0000", "fill-antialias": false}}}}
            ]
        }}"##
    )
}

/// Serves `BOX` for every URL, and records the requested URLs.
#[derive(Default)]
struct BoxHandler {
    urls: Mutex<Vec<String>>,
}

impl SchemeHandler for BoxHandler {
    fn handle(&self, request: &SchemeRequest) -> SchemeResponse {
        self.urls.lock().unwrap().push(request.url.clone());
        SchemeResponse::Data(BOX.as_bytes().to_vec())
    }
}

fn render_style(pool: &RenderPool, style: &str) -> mapli_core::Result<RgbaImage> {
    let req = StaticRequest {
        style: style.into(),
        view: Camera::new(0.0, 0.0, 0.0).unwrap().into(),
        spec: ImageSpec::new(64, 64).unwrap(),
        format: OutputFormat::Raw,
    };
    match pool.render(req)? {
        Rendered::Raw(img) => Ok(img),
        Rendered::Png(_) => unreachable!("requested raw output"),
    }
}

/// Render the style drawing the GeoJSON at `data_url`, which must succeed.
fn assert_url_renders(pool: &RenderPool, data_url: &str) {
    pool.register_style("style", Style::Json(build_style_json(data_url)));
    let img = render_style(pool, "style").unwrap_or_else(|e| panic!("{data_url}: {e}"));
    assert_eq!(img.get_pixel(32, 32).0, RED, "{data_url}");
    assert_eq!(img.get_pixel(0, 0).0, WHITE, "{data_url}");
}

/// Render the style drawing the GeoJSON at `data_url`, which the rules must deny.
fn assert_url_denied(pool: &RenderPool, data_url: &str) {
    pool.register_style("style", Style::Json(build_style_json(data_url)));
    let detail = match render_style(pool, "style") {
        Err(MapliError::RenderFailed(detail)) => detail,
        other => panic!("{data_url}: expected RenderFailed, got {other:?}"),
    };
    assert!(
        detail.contains("access denied by URL rules"),
        "{data_url}: {detail:?}"
    );
}

/// A directory of its own under the system's temporary directory, with `BOX` in `box.geojson`.
fn create_data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mapli-url-rules-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("box.geojson"), BOX).unwrap();
    dir
}

#[test]
fn url_rules_deny_urls_outside_the_allowed_prefixes() {
    let allowed_dir = create_data_dir("allowed");
    let other_dir = create_data_dir("other");
    let allowed_dir_url = format!("file://{}/", allowed_dir.display());

    // invalid prefixes are refused, and do not set the rules
    let result = restrict_resource_urls(&["mem://data/allowed/", "not a prefix"]);
    assert!(
        matches!(result, Err(MapliError::InvalidInput(_))),
        "{result:?}"
    );

    restrict_resource_urls(&["MEM://data/allowed/", allowed_dir_url.as_str()]).unwrap();
    // the rules cannot be set again
    let result = restrict_resource_urls(&["mem://"]);
    assert!(
        matches!(result, Err(MapliError::UrlRulesClosed(_))),
        "{result:?}"
    );

    let handler = Arc::new(BoxHandler::default());
    register_scheme_handler("mem", handler.clone()).unwrap();

    let pool = RenderPool::new(PoolConfig::default()).unwrap();

    // URLs of a scheme with a handler
    assert_url_renders(&pool, "mem://data/allowed/box.geojson");
    assert_url_denied(&pool, "mem://data/denied/box.geojson");
    assert_url_denied(&pool, "mem://data/allowed/../denied/box.geojson");
    {
        let urls = handler.urls.lock().unwrap();
        assert_eq!(*urls, ["mem://data/allowed/box.geojson"]);
    }
    // allowed URLs still render after denied ones, and are checked once dot segments are resolved
    assert_url_renders(&pool, "mem://data/allowed/other/../box2.geojson");

    // local files
    let file_url = |dir: &PathBuf, path: &str| format!("file://{}/{path}", dir.display());
    assert_url_renders(&pool, &file_url(&allowed_dir, "box.geojson"));
    assert_url_denied(&pool, &file_url(&other_dir, "box.geojson"));
    let other_dir_name = other_dir.file_name().unwrap().to_str().unwrap();
    assert_url_denied(
        &pool,
        &file_url(&allowed_dir, &format!("../{other_dir_name}/box.geojson")),
    );
    assert_url_denied(
        &pool,
        &file_url(
            &allowed_dir,
            &format!("%2e%2e/{other_dir_name}/box.geojson"),
        ),
    );

    // remote URLs, which are denied before any connection is attempted
    assert_url_denied(&pool, "https://example.org/box.geojson");

    // styles loaded from a URL are checked too
    pool.register_style(
        "from-url",
        Style::Url("mem://data/denied/style.json".parse().unwrap()),
    );
    let result = render_style(&pool, "from-url");
    assert!(
        matches!(&result, Err(MapliError::StyleLoadFailed { message, .. })
            if message.contains("access denied by URL rules")),
        "{result:?}"
    );

    // the rules are closed once a pool exists
    let result = restrict_resource_urls(&["mem://"]);
    assert!(
        matches!(result, Err(MapliError::UrlRulesClosed(_))),
        "{result:?}"
    );

    let _ = std::fs::remove_dir_all(&allowed_dir);
    let _ = std::fs::remove_dir_all(&other_dir);
}
