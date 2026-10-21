# mapli

Python bindings for the [maplibre_native] Rust crate.

This project provides a simple API to render map images and tiles from Python. It is a thin wrapper around 
the [maplibre_native] crate, which is a Rust implementation of the MapLibre GL Native library.


## Installation

At the moment this project is not on PyPi. However, we upload prebuilt wheels as github release artifacts. This
means you can point either `uv` or `pip` to them in order to get this installed:

```shell
uv add "https://github.com/geobeyond/mapli/releases/download/v0.2.0/mapli-0.2.0-cp39-abi3-manylinux_2_34_x86_64.whl"
```

```shell
pip install "https://github.com/geobeyond/mapli/releases/download/v0.2.0/mapli-0.2.0-cp39-abi3-manylinux_2_34_x86_64.whl"
```


## Examples

There are two early usage examples in the `/examples` dir:

- A script that concurrently renders map thumbnails for some cities;
- A small FastAPI app that renders map PNGs and serves XYZ map tiles.


### Static image mode

```python
import asyncio
from pathlib import Path
import mapli


async def main() -> None:
    render_pool = mapli.RenderPool(workers=2)
    render_pool.register_style("demo", url="https://demotiles.maplibre.org/style.json")
    png_bytes = await render_pool.arender(
        "demo", lon=-9.14, lat=38.72, zoom=5, width=256, height=256
    )
    Path(f"mapli-lisbon.png").write_bytes(png_bytes)
    print(f"wrote mapli-lisbon.png ({len(png_bytes)} bytes)")


if __name__ == "__main__":
    asyncio.run(main())
```

Instead of a center and zoom, you can also pass a bounding box. mapli picks the zoom that fits it
in the image:

```python
png = pool.render_from_bounds(  # or `await pool.arender_from_bounds(...)`
    "demo", bbox=(-9.6, 36.9, -6.1, 42.2), width=800, height=600, padding=16
)
```

`bbox` is `(west, south, east, north)`, the same order as shapely's `bounds` and geopandas'
`total_bounds`, which can be passed directly. `padding` is the space, in pixels, to leave around
the bounds on every side. A `west` greater than `east` means the bounds cross the antimeridian,
e.g. `bbox=(170, -20, -170, -15)` for Fiji.

### Tile mode

Besides static images, mapli can render map tiles in the XYZ (WebMercatorQuad) scheme:

```python
import mapli

pool = mapli.RenderPool(workers=2)
pool.register_style("demo", url="https://demotiles.maplibre.org/style.json")

png = pool.render_tile("demo", z=2, x=1, y=1)  # or `await pool.arender_tile(...)`
```

`tile_size` defaults to 512 px, which is MapLibre's native tile size. Most XYZ clients (Leaflet,
OpenLayers) expect 256 px tiles, which you get with `tile_size=256`. Because MapLibre zoom levels
are based on 512 px tiles, a 256 px tile is drawn one zoom level lower, so 256 px tiles cannot be
rendered at `z=0`. Use `pixel_ratio=2` for high-DPI ("@2x") tiles.

### Custom URL schemes

mapli lets you register a custom Python handler for a URL scheme of your choosing. You can then 
refer to data with URLs of that scheme, and mapli hands every request that uses the scheme to the
handler, which becomes responsible for returning the data bytes. URLs of other schemes are still
fetched by MapLibre Native itself, as usual.

This is useful for data that MapLibre Native cannot fetch on its own, such as data in a private
object store bucket. 

<details>

<summary>Using a custom URL scheme for fetching S3 private data</summary>

As an example, the handler below reads a private S3 bucket with [obstore](https://developmentseed.org/obstore/):

```python
import mapli
import obstore
from obstore.store import S3Store

store = S3Store.from_url("s3://my-private-bucket", region="eu-west-1")


def read_from_store(request: mapli.ResourceRequest) -> mapli.Response:
    key = request.url.removeprefix("store://")
    options: obstore.GetOptions = {}
    if request.data_range is not None:
        start, end = request.data_range
        options["range"] = (start, end + 1)
    try:
        result = obstore.get(store, key, options=options)
    except FileNotFoundError:
        return mapli.Response.error(mapli.ErrorReason.NOT_FOUND, f"no object at {key}")
    return mapli.Response(result.bytes())


mapli.register_scheme_handler("store", read_from_store)
pool = mapli.RenderPool(workers=2)
pool.register_style(
    "roads",
    json={
        "version": 8,
        "sources": {
            # MapLibre Native's PMTiles support reads the archive through the handler
            "roads": {"type": "vector", "url": "pmtiles://store://tiles/roads.pmtiles"},
            # other schemes are fetched by MapLibre Native as usual
            "basemap": {
                "type": "raster",
                "url": "https://tiles.example.org/tiles.json",
            },
        },
        "layers": [
            {"id": "basemap", "type": "raster", "source": "basemap"},
            {"id": "roads", "type": "line", "source": "roads", "source-layer": "roads"},
        ],
    },
)
```

</details>


A handler is called with a `ResourceRequest` and returns one of:

- `mapli.Response(data)`, or just the `bytes`;
- `mapli.Response.no_content()`, e.g. for a missing tile, which is drawn as empty;
- `mapli.Response.error(reason, message)`, which fails the render with a `RenderError`

**Notes:**

- Handlers must be registered before the first `RenderPool` is created. MapLibre Native
  does not pick up file sources registered later. Registering afterwards raises
  `SchemeRegistrationError`.

- mapli's implementation hijacks MapLibre Native's builtin handling of the `asset` scheme. This 
  means that mapli does not support MapLibre's `asset` scheme, which is usually useful on 
  mobile platforms

- Schemes that MapLibre Native serves itself (`http`, `https`, `file`, `mbtiles`, `pmtiles`) 
  cannot be given a handler.

- MapLibre Native often makes several identical requests at the same time. While your handler
  is serving one, other identical requests wait for it and get the same response, which 
  means the handler is only called once. Note that mapli does not cache responses after that. 
  However, since each renderer keeps the tiles it has loaded, and MapLibre Native's PMTiles 
  support keeps archive headers and directories there is some degree of caching, but not a 
  full caching implementation.
  
  If you also want to avoid reading the same data again across renderers, you will need to 
  cache it in the handler (see below for an example).

- Handlers run on threads owned by mapli. This means that requests can be served concurrently if 
  the handler's I/O is able to release the Python GIL - obstore, sockets and files do this. 
  
- A request that is no longer needed is skipped if its handler has not started yet, but once 
  started, it runs to the end.

- Values stored in a `threading.local()` do not persist from one handler call to the next, as
  each call gets a fresh Python thread state. This means that if your handler is not thread-safe 
  then you cannot share state between sucessive handler calls. Ensure whatever clients your 
  handler uses are thread-safe so that they can be shared - obstore is a good example. 

<details>

<summary>Caching example</summary>

To cache, keep the fetching in a function that returns the data and raises on errors, so that
errors are never cached, and wrap it in e.g. `functools.lru_cache`. This suits objects that never
change, as PMTiles archives usually don't:

```python
from functools import lru_cache
import obstore
import mapli
from obstore.store import S3Store

store = S3Store.from_url("s3://my-private-bucket", region="eu-west-1")


@lru_cache(maxsize=1024)
def read_data(key: str, data_range: tuple[int, int] | None) -> obstore.Bytes:
    options: obstore.GetOptions = {}
    if data_range is not None:
        start, end = data_range
        options["range"] = (start, end + 1)
    return obstore.get(store, key, options=options).bytes()


def read_from_store(request: mapli.ResourceRequest) -> mapli.Response:
    key = request.url.removeprefix("store://")
    try:
        return mapli.Response(read_data(key, request.data_range))
    except FileNotFoundError:
        return mapli.Response.error(mapli.ErrorReason.NOT_FOUND, f"no object at {key}")
```

`examples/private-bucket.py` puts this together, and prints what is read from the store.

</details>




## System requirements

> [!IMPORTANT]
>
> For now only Linux is supported. 
> Prebuilt wheels are available for x86_64 and ARM64 (aarch64) and need glibc 2.34 or newer
> (e.g. Ubuntu 22.04+, Debian 12+, RHEL 9+).
>
> macOS on Apple Silicon should also be possible via MapLibre Native's Metal 
> backend but is not yet built or tested. 
>
> Windows is not supported and mapli will only be able to enable support for it if the 
> [upstream maplibre-native-rs] project ever decides to implement it.


[upstream maplibre-native-rs]: https://github.com/maplibre/maplibre-native-rs#platform-support

mapli renders with MapLibre Native's OpenGL backend, which on Linux creates an EGL context in the
process. 

The wheels bundle most of the libraries MapLibre Native needs (ICU, libpng, libjpeg, libwebp, libuv).
At runtime, mapli additionally needs:

- An EGL driver - A GPU is not required: Mesa's `llvmpipe` software renderer works fine on
  servers and in containers. On Debian/Ubuntu:

  ```shell
  sudo apt-get install libegl1 libegl-mesa0 libgl1-mesa-dri
  ```

- libcurl - Used to fetch remote styles, tiles, fonts and sprites. It is not bundled, so that it uses
  your system's CA certificates. Most systems already have it. On Debian/Ubuntu:

  ```shell
  sudo apt-get install libcurl4t64  # libcurl4 on Ubuntu 22.04 and Debian 12
  ```

- A display - This can be your normal display (on a desktop machine) or a headless server, 
  like xvfb

  ```shell
  # only needed if there is no graphical display installed
  sudo apt-get install xvfb
  xvfb-run -a python your_app.py
  ```

  For long-running services, you can instead run `Xvfb` as its own service and set the `DISPLAY`
  environment variable for the process using mapli to the display number Xvfb was started with:

  ```shell
  Xvfb :99 -screen 0 1280x1024x24 -nolisten tcp &
  DISPLAY=:99 python your_app.py
  ```

  Desktop machines that already have a display don't need this.

If the EGL driver or the display is missing, creating a `RenderPool` raises `mapli.GraphicsUnavailableError` with details
about which EGL step failed:

```python
import mapli

try:
    pool = mapli.RenderPool(workers=2)
except mapli.GraphicsUnavailableError as exc:
    print(f"cannot render on this machine: {exc}")
```


## Development

### rust crates

Use `cargo` to build, test, show docs, etc. as usual.


Building mapli compiles MapLibre Native from source, which needs a C++ toolchain and several
development packages. On Debian/Ubuntu, ensure you have these installed:

```shell
sudo apt-get install \
    build-essential \
    ccache \
    cmake \
    glslang-dev \
    glslang-tools \
    libcurl4-openssl-dev \
    libegl1-mesa-dev \
    libfontconfig-dev \
    libgl1-mesa-dev \
    libgl1-mesa-dri \
    libicu-dev \
    libjpeg-turbo8-dev \
    libpng-dev \
    libuv1-dev \
    libwebp-dev \
    libz-dev \
    pkg-config
```

The first build takes a while (around 15 minutes on a GitHub Actions runner). Setting
`MLN_CMAKE_CXX_LAUNCHER=ccache` routes the C++ compilation through ccache, which makes rebuilds
much faster.


### Python package

The repo contains a Python package that uses the rust crates. Checkout the `pyproject.toml` file for more info
on how it is set up. In short, it makes use of [maturin] as the build system for the rust crate, and uses [uv]
as a build tool for the Python package.

`uv sync` builds the project with a development build of the `mapli` crate. This works because we have
`editable-profile = "dev"` in the `[tool.maturin]` section in `pyproject.toml`. The default sync way of uv is to
perform an editable build of the project.

This means we can simply invoke `uv` as usual.

Running `uv sync --no-editable --reinstall-package` causes uv to build a non-debug version of the package, which can
be useful for benchmarking.


### Building distributable wheels

Distributable wheels can be built inside a [manylinux] `manylinux_2_34` container, which links against an older glibc 
and bundles the remaining libraries with [auditwheel]. The script included in `scripts/build-manylinux-wheel.sh` can 
be used for this - it is used in CI too. 

Run it locally with:

```shell
docker run --rm -v "$PWD":/io -w /io \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    -e CARGO_TARGET_DIR=/build/target -v mapli-manylinux-build:/build \
    quay.io/pypa/manylinux_2_34_x86_64 scripts/build-manylinux-wheel.sh
```

The wheel and sdist end up in `dist/`. The `mapli-manylinux-build` volume keeps the build cache between runs, so
only the first build compiles MapLibre Native from scratch.

In CI, the `build_wheel` job builds the wheel on pushes to `main` and on release tags (it can also be started
manually from the Actions tab), and the `test_wheel` job then runs the test suite against it on a plain Ubuntu
runner that only has the runtime dependencies listed above installed.

### Making a release

The project version is set in a single place: the `[workspace.package]` section of the root `Cargo.toml`. Both
rust crates inherit it and the Python package gets it from there too. To make a release:

1. Bump `version` in the root `Cargo.toml` (e.g. `0.3.0`) and refresh the lockfiles with `cargo update --workspace` 
   and `uv lock`
2. Update `CHANGELOG.md`
3. Commit, then push an annotated tag with the same version, prefixed with `v`:

   ```shell
   git tag -a v0.3.0 -m "Release v0.3.0"
   git push origin v0.3.0
   ```

The tag triggers the release workflow, which runs CI, checks that the built wheel's version matches the tag and
then creates a GitHub release with the wheel and sdist attached.

After the release, put the version back into a development state by bumping it to the next expected version with a
`-dev.0` suffix (e.g. `0.4.0-dev.0`) in the root `Cargo.toml`, refreshing the lockfiles and committing:

```shell
# after editing the version in Cargo.toml
cargo update --workspace
uv lock
git commit -am "Bump version to 0.4.0-dev.0"
```

maturin converts this to the
Python version `0.4.0.dev0`, so wheels built from `main` in the meantime are clearly marked as development builds.
When making the next release, step 1 above then just drops the suffix (or picks a different version).


## Comparison to other MapLibre Native related projects

Several other Python packages integrate with MapLibre, at different layers. 

[py-maplibregl] (`maplibre` on PyPI) generates HTML/JavaScript that runs MapLibre GL JS in a browser, with Jupyter 
and Shiny integrations; it targets interactive client-side maps and does not produce raster output in Python. 

[leafmap] and [lonboard] follow the same client-side model (leafmap includes a MapLibre backend built on 
py-maplibregl; lonboard renders with deck.gl over a MapLibre basemap). 

For server-side rendering, [pymgl] provides in-process nanobind bindings to a C++ wrapper around
MapLibre Native for rendering styles to PNG. It ships wheels for Linux and macOS arm64, and its API
is documented as subject to change. 

[mlnative] uses the [maplibre_native] crate in a separate renderer binary, driven from Python as a long-lived 
subprocess over stdin/stdout. This aproach isolates the renderer from the Python process (at the cost of 
serializing each request and response across the process boundary).

[maplibre-native-ffi] is a MapLibre project that defines a C API over MapLibre Native, with bindings for several
languages. Its Python bindings are also a PyO3 extension over that C API, exposing a broad, low-level surface (the host
manages the graphics context), and the project is pre-1.0 with an unstable ABI. 

Out-of-process HTTP services such as [tileserver-gl] (Node.js, rasterizing with MapLibre
Native) are another option, which adds a separate service to deploy. 

mapli binds the same [maplibre_native] crate in-process through PyO3, exposing a pooled renderer
with configurable worker threads, renderer reuse per style, and both synchronous and native `async` render calls. 
Compared with the subprocess and HTTP approaches, it avoids IPC overhead. Compared with the FFI bindings, it exposes 
a narrower, task-specific API.

[maplibre_native]: https://docs.rs/maplibre_native/0.10.0/maplibre_native/index.html
[maturin]: https://www.maturin.rs/
[uv]: https://docs.astral.sh/uv/
[manylinux]: https://github.com/pypa/manylinux
[auditwheel]: https://github.com/pypa/auditwheel
[py-maplibregl]: https://github.com/eodaGmbH/py-maplibregl
[leafmap]: https://github.com/opengeos/leafmap
[lonboard]: https://github.com/developmentseed/lonboard
[pymgl]: https://github.com/brendan-ward/pymgl
[tileserver-gl]: https://github.com/maptiler/tileserver-gl
[mlnative]: https://github.com/adonm/mlnative
[maplibre-native-ffi]: https://github.com/maplibre/maplibre-native-ffi
