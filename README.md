# mapli

mapli is a Python package that renders [MapLibre styles] to PNG images. Rendering is done by
[MapLibre Native], which mapli runs inside the Python process through the [maplibre_native] Rust
crate.

mapli renders static map images, given a center and zoom level or a bounding box, and map tiles
in the XYZ tiling scheme. Render calls are available as blocking and as `async` methods. Data can
also be served to MapLibre Native from Python, through custom URL scheme handlers.

**Documentation: <https://geobeyond.github.io/mapli/>**


## Installation

mapli is distributed as prebuilt wheels attached to each
[GitHub release](https://github.com/geobeyond/mapli/releases). Install the wheel that matches the
machine's architecture by URL. For release 0.4.0 on x86_64:

```shell
uv add "https://github.com/geobeyond/mapli/releases/download/v0.4.0/mapli-0.4.0-cp39-abi3-manylinux_2_34_x86_64.whl"
```

```shell
pip install "https://github.com/geobeyond/mapli/releases/download/v0.4.0/mapli-0.4.0-cp39-abi3-manylinux_2_34_x86_64.whl"
```

For ARM64, replace `x86_64` with `aarch64` in the URL.

mapli currently runs on Linux only (x86_64 and ARM64, glibc 2.34 or newer). Rendering needs an
EGL driver, libcurl and a display (or Xvfb), see
[System requirements](https://geobeyond.github.io/mapli/user-guide/system-requirements/).


## Usage

```python
from pathlib import Path

import mapli

pool = mapli.RenderPool(workers=2)
pool.register_style("demo", url="https://demotiles.maplibre.org/style.json")

png_bytes = pool.render("demo", lon=-9.14, lat=38.72, zoom=5, width=512, height=512)
Path("lisbon.png").write_bytes(png_bytes)
```


## Development

See [Development](https://geobeyond.github.io/mapli/development/) in the documentation.


## License

See [LICENSE](https://github.com/geobeyond/mapli/blob/main/LICENSE).


[MapLibre styles]: https://maplibre.org/maplibre-style-spec/
[MapLibre Native]: https://github.com/maplibre/maplibre-native
[maplibre_native]: https://docs.rs/maplibre_native/
