# mapli

mapli is a Python package that renders [MapLibre styles] to PNG images. A MapLibre style is a JSON
document that describes the data sources of a map and how to draw them. Rendering is done by
[MapLibre Native], the C++ map renderer of the MapLibre project, which mapli runs inside the
Python process through the [maplibre_native] Rust crate.

mapli renders:

- static map images, given a center and zoom level, or a bounding box;
- map tiles in the XYZ tiling scheme.

Render calls are available as blocking methods and as `async` methods. Data can also be served to
MapLibre Native from Python, through custom URL scheme handlers.

!!! note "Platform support"

    mapli currently runs on Linux only (x86_64 and ARM64, glibc 2.34 or newer). Rendering needs
    an EGL driver and a display, see [System requirements](user-guide/system-requirements.md).


## Installation

mapli is published on [PyPI](https://pypi.org/project/mapli/):

=== "uv"

    ```shell
    uv add mapli
    ```

=== "pip"

    ```shell
    pip install mapli
    ```

Prebuilt wheels are available for Linux x86_64 and ARM64 (aarch64), with CPython 3.11 and newer.
The same wheels are also attached to each
[GitHub release](https://github.com/geobeyond/mapli/releases).


## Usage

```python
from pathlib import Path

import mapli

pool = mapli.RenderPool(workers=2)
pool.register_style("demo", url="https://demotiles.maplibre.org/style.json")

png_bytes = pool.render("demo", lon=-9.14, lat=38.72, zoom=5, width=512, height=512)
Path("lisbon.png").write_bytes(png_bytes)
```

This creates a pool of two worker threads, registers a style under the id `demo`, and renders a
512 x 512 pixel image centered on Lisbon at zoom level 5.

The [user guide](user-guide/rendering-a-style.md) describes each feature. The
[API reference](reference/mapli/index.md) lists every class and function.


[MapLibre styles]: https://maplibre.org/maplibre-style-spec/
[MapLibre Native]: https://github.com/maplibre/maplibre-native
[maplibre_native]: https://docs.rs/maplibre_native/
