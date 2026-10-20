# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Custom URL scheme handlers: `mapli.register_scheme_handler()` registers a Python callable that
  serves the URLs of a scheme, e.g. to read data from private object store buckets. Also adds
  `ResourceRequest`, `Response`, `ErrorReason` and `SchemeRegistrationError`
  ([#4](https://github.com/ricardogsilva/mapli/issues/4)). `examples/private-bucket.py` renders a
  PMTiles archive from an object store with [obstore](https://developmentseed.org/obstore/).
  Identical requests made at the same time are served by a single handler call

### Changed
- `RenderError` messages now include MapLibre Native's reason for the failure, instead of just
  "render failed"
- CI: test steps now time out (10 minutes, or 45 for the Rust tests, which also compile) instead
  of hanging for GitHub's 6 hour default, and pytest dumps every thread's traceback when a test
  runs for over 60 seconds (`faulthandler_timeout`), to help diagnose hangs


## [0.3.1] - 2026-10-12

### Fixed
- CI: rendering tests failed intermittently on some ARM runners, where Mesa tried an unsupported
  hardware path under Xvfb. Tests now force Mesa's software renderer (`LIBGL_ALWAYS_SOFTWARE=1`),
  and the Rust tests also run under Xvfb


## [0.3.0] - 2026-10-12 [YANKED]

Tagged but never published: the release CI failed. Use 0.3.1, which has the same changes.

### Added
- `RenderPool.render_from_bounds()` and `RenderPool.arender_from_bounds()`, which render static
  images fitted to a bounding box
- mapli-core: render static images from a bounding box, fitted to the image size with
  optional padding, bearing and pitch (`Bounds`, `BoundsToFit`). Bounds crossing the
  antimeridian (`west > east`) are supported

### Changed
- mapli-core: `StaticRequest.camera` was replaced by `StaticRequest.view`, which takes either a
  `Camera` or bounds to fit (`View`)


## [0.2.2] - 2026-10-12

### Changed
- Lowered the minimum supported Python version to 3.11



## [0.2.1] - 2026-10-12

### Added
- Prebuilt wheels for ARM64 (aarch64) Linux


## [0.2.0] - 2026-10-09

### Added
- First release




[unreleased]: https://github.com/geobeyond/mapli
[0.3.1]: https://github.com/geobeyond/mapli/releases/tag/v0.3.1
[0.3.0]: https://github.com/geobeyond/mapli/releases/tag/v0.3.0
[0.2.2]: https://github.com/geobeyond/mapli/releases/tag/v0.2.2
[0.2.1]: https://github.com/geobeyond/mapli/releases/tag/v0.2.1
[0.2.0]: https://github.com/geobeyond/mapli/releases/tag/v0.2.0
