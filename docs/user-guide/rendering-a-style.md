# Rendering a style

This page shows how to create a render pool, register a style and render it to a PNG image.

## Example

```python
from pathlib import Path

import mapli

pool = mapli.RenderPool(workers=2)
pool.register_style("demo", url="https://demotiles.maplibre.org/style.json")

png_bytes = pool.render("demo", lon=-9.14, lat=38.72, zoom=5, width=512, height=512)
Path("lisbon.png").write_bytes(png_bytes)
```

## The render pool

`RenderPool` owns a number of worker threads, set with `workers` (default `1`). Each render
call is handed to a worker, so a pool renders up to `workers` images at the same time. Each
worker has its own MapLibre Native run loop and OpenGL context, so memory use grows with the
number of workers.

A worker keeps one MapLibre Native renderer per style and render mode (static image or tile), and
reuses it across render calls. `max_renderers_per_worker` (default `8`) limits how many renderers
a worker keeps; when the limit is reached, the least recently used one is discarded.

A pool can be shared between threads. The render methods release the GIL while they wait for the
image. The worker threads stop when the last reference to the pool is dropped.

## Registering styles

A style is registered under an id, which is then passed to the render methods. A style is given
by exactly one of:

- `url`: the URL of a style JSON document;
- `path`: the path of a local style JSON file;
- `json`: the style itself, as a JSON string or as a `dict`.

```python
pool.register_style("demo", url="https://demotiles.maplibre.org/style.json")
pool.register_style("local", path="styles/basic.json")
pool.register_style(
    "background",
    json={
        "version": 8,
        "sources": {},
        "layers": [
            {"id": "bg", "type": "background", "paint": {"background-color": "#88c"}},
        ],
    },
)

assert pool.has_style("demo")
```

The style is not fetched or parsed when it is registered. Each worker loads it the first time it
renders it.

## Output

The render methods return the image as PNG-encoded `bytes`. The image size is given in logical
pixels. `pixel_ratio` (default `1.0`) sets the number of image pixels per logical pixel: with
`width=512, height=512, pixel_ratio=2`, the image is 1024 x 1024 pixels, with labels and line
widths drawn at twice their size, as for high-density displays.

## Errors

| Exception | Raised when |
|---|---|
| `ValueError` | `register_style` is not given exactly one of `url`, `path`, `json`; or a render argument is out of range |
| `UnknownStyleError` | no style is registered under the given id |
| `StyleLoadError` | MapLibre Native fails to load the style, e.g. the URL cannot be fetched or the JSON is invalid |
| `RenderError` | MapLibre Native fails to render the image |

All mapli exceptions derive from `MapliError`.
