# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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




[unreleased]: https://github.com/ricardogsilva/mapli
[0.2.2]: https://github.com/ricardogsilva/mapli/releases/tag/v0.2.2
[0.2.1]: https://github.com/ricardogsilva/mapli/releases/tag/v0.2.1
[0.2.0]: https://github.com/ricardogsilva/mapli/releases/tag/v0.2.0
