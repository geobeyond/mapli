//! End-to-end rendering of resources served by a custom URL scheme handler.
//!
//! Scheme handler registration is process-global and closes once a pool is created, so the
//! steps that depend on that order run in a single test.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mapli_core::{
    Camera, ErrorReason, ImageSpec, MapliError, OutputFormat, PoolConfig, RenderPool, Rendered,
    RgbaImage, SchemeHandler, SchemeRequest, SchemeResponse, StaticRequest, Style,
    register_scheme_handler,
};

const RED: [u8; 4] = [255, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// Style that paints the GeoJSON at `data_url` red over a white background.
fn style_json(data_url: &str) -> String {
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

const BOX: &str = r#"{"type": "Polygon", "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]]}"#;

/// Serves `resources` by URL, answering everything else with `NotFound`, and records requests.
#[derive(Default)]
struct MemoryHandler {
    resources: HashMap<String, Vec<u8>>,
    requests: Mutex<Vec<SchemeRequest>>,
}

impl SchemeHandler for MemoryHandler {
    fn handle(&self, request: &SchemeRequest) -> SchemeResponse {
        self.requests.lock().unwrap().push(request.clone());
        match self.resources.get(&request.url) {
            Some(data) => SchemeResponse::Data(data.clone()),
            None => SchemeResponse::error(ErrorReason::NotFound, "no such resource"),
        }
    }
}

fn render(pool: &RenderPool, style: &str) -> mapli_core::Result<RgbaImage> {
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

#[test]
fn scheme_handler_serves_style_and_sources() {
    // reserved and malformed schemes are refused
    for scheme in ["https", "PMTiles", "", "1mem"] {
        let result = register_scheme_handler(scheme, Arc::new(MemoryHandler::default()));
        assert!(
            matches!(result, Err(MapliError::InvalidInput(_))),
            "{scheme:?} was accepted"
        );
    }

    let handler = Arc::new(MemoryHandler {
        resources: HashMap::from([
            (
                "mem://style.json".to_string(),
                style_json("mem://box.geojson").into_bytes(),
            ),
            ("mem://box.geojson".to_string(), BOX.as_bytes().to_vec()),
        ]),
        ..Default::default()
    });
    register_scheme_handler("MEM", handler.clone()).unwrap();

    let pool = RenderPool::new(PoolConfig::default()).unwrap();

    // the style and its source both come from the handler
    pool.register_style("box", Style::Url("mem://style.json".parse().unwrap()));
    let img = render(&pool, "box").unwrap();
    assert_eq!(img.get_pixel(32, 32).0, RED);
    assert_eq!(img.get_pixel(0, 0).0, WHITE);
    {
        let requests = handler.requests.lock().unwrap();
        let urls: Vec<&str> = requests.iter().map(|r| r.url.as_str()).collect();
        assert!(urls.contains(&"mem://style.json"), "{urls:?}");
        assert!(urls.contains(&"mem://box.geojson"), "{urls:?}");
    }

    // a handler error reaches the render, with the URL, the reason and the handler's message
    pool.register_style("missing", Style::Json(style_json("mem://missing.geojson")));
    let detail = match render(&pool, "missing") {
        Err(MapliError::RenderFailed(detail)) => detail,
        other => panic!("expected RenderFailed, got {other:?}"),
    };
    for expected in ["mem://missing.geojson", "not found", "no such resource"] {
        assert!(detail.contains(expected), "{expected:?} not in {detail:?}");
    }

    // registration is closed once a pool exists
    let result = register_scheme_handler("late", Arc::new(MemoryHandler::default()));
    assert!(matches!(result, Err(MapliError::SchemeRegistrationClosed(s)) if s == "late"));
}
