# Comparison with other projects

Several other projects integrate MapLibre with Python, at different layers.

## Client-side rendering

These packages generate maps that are rendered in a web browser, with MapLibre GL JS or deck.gl.
They do not produce raster images in Python.

- [py-maplibregl] (`maplibre` on PyPI) generates HTML and JavaScript for MapLibre GL JS, with
  Jupyter and Shiny integrations.
- [leafmap] includes a MapLibre backend built on py-maplibregl.
- [lonboard] renders with deck.gl over a MapLibre basemap.

## Server-side rendering

These projects, like mapli, render images with MapLibre Native.

- [pymgl] provides in-process nanobind bindings to a C++ wrapper around MapLibre Native, for
  rendering styles to PNG. It ships wheels for Linux and macOS arm64. Its API is documented as
  subject to change.
- [mlnative] uses the [maplibre_native] Rust crate in a separate renderer binary, driven from
  Python as a long-lived subprocess over stdin/stdout. The renderer is isolated from the Python
  process, and each request and response is serialized across the process boundary.
- [maplibre-native-ffi] is a MapLibre project that defines a C API over MapLibre Native, with
  bindings for several languages. Its Python bindings are a PyO3 extension over that C API, with
  a broad, low-level API in which the caller manages the graphics context. The project is pre-1.0
  and its ABI is not yet stable.
- [tileserver-gl] is a Node.js HTTP service that rasterizes tiles and images with MapLibre
  Native. It runs as a separate service.

mapli binds the [maplibre_native] Rust crate in-process, with PyO3. It exposes a pool of worker
threads with a configurable size, reuses renderers per style, and provides both blocking and
`async` render methods. Its API covers rendering static images and tiles, and serving data
through custom URL schemes; it does not expose the rest of MapLibre Native's API.


[py-maplibregl]: https://github.com/eodaGmbH/py-maplibregl
[leafmap]: https://github.com/opengeos/leafmap
[lonboard]: https://github.com/developmentseed/lonboard
[pymgl]: https://github.com/brendan-ward/pymgl
[mlnative]: https://github.com/adonm/mlnative
[maplibre-native-ffi]: https://github.com/maplibre/maplibre-native-ffi
[tileserver-gl]: https://github.com/maptiler/tileserver-gl
[maplibre_native]: https://docs.rs/maplibre_native/
