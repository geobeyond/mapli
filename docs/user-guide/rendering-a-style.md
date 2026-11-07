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

A worker keeps one MapLibre Native renderer per style, render mode (static image or tile) and
pixel ratio, and reuses it across render calls. Static image renderers are resized for each
render, so requests for different image sizes share a renderer; tile renderers are also kept per
tile size. `max_renderers_per_worker` (default `8`) limits how many renderers a worker keeps;
when the limit is reached, the least recently used one is discarded.

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

Registering a style again under the same id replaces it, and renders submitted afterwards use the
new style. A style registered by `url` is fetched again only if the server's cache headers allow
it: a response with `Cache-Control: max-age` is reused until it expires, even by new pools. To
force a fresh copy, give the style a new URL, e.g. by adding a version to its query string.

## Output

The render methods return the image as `bytes`, in the format chosen with `output`:

- `"png"` (the default): a PNG-encoded image;
- `"rgba"`: the raw pixels, without any encoding. See [Raw pixels](#raw-pixels).

Image size is given in logical pixels and `pixel_ratio`, which defaults to `1.0`, sets the 
number of image pixels per logical pixel: with `width=512, height=512, pixel_ratio=2`, the image is 1024 x 1024
pixels, with labels and line widths drawn at twice their size, as for high-density displays. 

When needed, the size in image pixels is rounded down: the image is `floor(width * pixel_ratio)` by
`floor(height * pixel_ratio)` pixels (for tiles, `floor(tile_size * pixel_ratio)` pixels wide and
high).


### Raw pixels

With `output="rgba"`, the pixels are returned row by row from the top-left, 4 bytes (red, green,
blue, alpha) per pixel, with straight (not premultiplied) alpha. This output format is useful 
whenever there is a need to further process the generated image. The returned bytes can be 
handed directly to libraries such as Pillow or numpy:

```python
import math

import mapli
import numpy as np
from PIL import Image

pool = mapli.RenderPool()
pool.register_style("demo", url="https://demotiles.maplibre.org/style.json")

width, height, pixel_ratio = 512, 256, 2
rgba = pool.render(
    "demo",
    lon=-9.14,
    lat=38.72,
    zoom=5,
    width=width,
    height=height,
    pixel_ratio=pixel_ratio,
    output="rgba",
)

image_width = math.floor(width * pixel_ratio)
image_height = math.floor(height * pixel_ratio)

# encoding encode the image as WebP
image = Image.frombytes("RGBA", (image_width, image_height), rgba)
image.save("lisbon.webp", quality=80)

# using it in numpy, as an array of shape (height, width, 4)
pixels = np.frombuffer(rgba, dtype=np.uint8).reshape(image_height, image_width, 4)
```

## Errors

| Exception | Raised when |
|---|---|
| `ValueError` | `register_style` is not given exactly one of `url`, `path`, `json`; or a render argument is out of range, or `output` is neither `"png"` nor `"rgba"` |
| `UnknownStyleError` | no style is registered under the given id |
| `StyleLoadError` | MapLibre Native fails to load the style, e.g. the URL cannot be fetched or the JSON is invalid |
| `RenderError` | MapLibre Native fails to render the image |

All mapli exceptions derive from `MapliError`.
