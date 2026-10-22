# Development

## Architecture

mapli is a thin wrapper around MapLibre Native, which it uses through the [maplibre_native] Rust
crate. It has three layers:

- A `mapli-core` Rust crate that uses `maplibre_native` to render images and tiles. 

    It runs a pool of worker threads and their renderers, and passes the requests for 
    custom URL schemes to the registered handlers. 

    This has no Python dependency. 

- A set of [PyO3] bindings in `src/lib.rs`. These expose `mapli-core` to Python as the `mapli._core` 
  extension module, converting arguments, results and errors, and releasing the GIL while rendering. 

- The Python package in `src/mapli/` is the public API: it re-exports `mapli._core` and adds what 
  is simpler to write in Python, such as accepting a `dict` as a style or any iterable as a 
  bounding box, and passing `ResourceRequest` objects to scheme handlers.

## Repository layout

| Path | Contents |
|---|---|
| `mapli-core/` | Rust crate that wraps the [maplibre_native] crate: render pool, workers, scheme handlers |
| `src/lib.rs` | Rust crate with the PyO3 bindings, compiled into the `mapli._core` Python extension module |
| `src/mapli/` | Python package: the public API (`__init__.py`) and the type stub of the extension module (`_core.pyi`) |
| `tests/` | Python tests |
| `examples/` | Example programs, see [Examples](../examples/index.md) |
| `docs/` | This documentation |
| `scripts/` | Wheel build script used by CI |

## Build dependencies

Building mapli compiles MapLibre Native from source, which needs a C++ toolchain and several
development packages. On Debian/Ubuntu:

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

The first build takes around 15 minutes on a GitHub Actions runner. Setting
`MLN_CMAKE_CXX_LAUNCHER=ccache` routes the C++ compilation through ccache, which reduces the time
of later builds.

Running the tests also needs the [runtime dependencies](../user-guide/system-requirements.md),
including a display or Xvfb.

## Rust crates

The Rust crates are built, tested and documented with `cargo` as usual:

```shell
cargo build
xvfb-run -a cargo test
cargo doc --open
```

## Python package

The Python package is built with [maturin], and managed with [uv]. `uv sync` builds the extension
module with the `dev` Cargo profile, as set by `editable-profile = "dev"` in the `[tool.maturin]`
section of `pyproject.toml`, and installs the package in editable mode:

```shell
uv sync --group dev
```

`uv sync` rebuilds the extension module when a Rust source file, `Cargo.toml` or `Cargo.lock`
changes (see `[tool.uv] cache-keys` in `pyproject.toml`).

`uv sync --no-editable --reinstall-package mapli` builds a release-mode version of the package,
e.g. for benchmarking.

## Tests and checks

```shell
xvfb-run -a uv run pytest
uv run ruff check
uv run ruff format --check
uv run ty check
cargo fmt --check
cargo clippy -- --deny warnings
```

`xvfb-run -a` is only needed on machines without a display. If Mesa fails to create an EGL
context under Xvfb, set `LIBGL_ALWAYS_SOFTWARE=1`.

The same checks run as [prek] hooks, configured in `prek.toml`:

```shell
prek install
```

## Documentation

This site is built with [zensical], configured in `zensical.toml`. The pages are in `docs/`.

```shell
uv run --group docs zensical serve  # serves the site at http://localhost:8000, rebuilding on changes
uv run --group docs zensical build  # builds the site into site/
```

The [API reference](../reference/mapli/index.md) is generated from the docstrings of the public
`mapli` module, with zensical's api-autonav and mkdocstrings support. Docstrings use the
[Google style].

The docstrings of the objects defined in Rust (`RenderPool`'s render methods, `Response`,
`ErrorReason`, the exceptions) are written in the type stub, `src/mapli/_core.pyi`, not in the
Rust code. The documentation is built from the stub only, without importing the compiled
extension module (`allow_inspection = false` in `zensical.toml`), so the extension does not need
to be built to build the documentation. The `///` comments in `src/lib.rs` document the Rust
code for Rust developers, and are not part of the API reference.

The site is built on every pull request, and deployed to GitHub Pages on every push to `main`,
by the `.github/workflows/docs.yml` workflow.


[maplibre_native]: https://docs.rs/maplibre_native/
[maturin]: https://www.maturin.rs/
[uv]: https://docs.astral.sh/uv/
[PyO3]: https://pyo3.rs/main/getting-started
[prek]: https://github.com/j178/prek
[zensical]: https://zensical.org/
[Google style]: https://mkdocstrings.github.io/griffe/reference/docstrings/#google-style
