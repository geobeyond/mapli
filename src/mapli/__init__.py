"""mapli: Render MapLibre styles to images in-process."""

import json as _json
from collections.abc import Iterable
from os import PathLike
from typing import Any

from . import _core
from ._core import (
    GraphicsUnavailableError,
    MapliError,
    RenderError,
    StyleLoadError,
    UnknownStyleError,
)

__all__ = [
    "GraphicsUnavailableError",
    "MapliError",
    "RenderError",
    "RenderPool",
    "StyleLoadError",
    "UnknownStyleError",
]


class RenderPool(_core.RenderPool):
    """A pool of renderers for MapLibre styles.

    This class is a thin wrapper around the core RenderPool class, providing
    a more user-friendly API for rendering MapLibre styles to images.
    """

    def register_style(
        self,
        style_id: str,
        *,
        url: str | None = None,
        path: str | PathLike[str] | None = None,
        json: str | dict[str, Any] | None = None,
    ) -> None:
        if isinstance(json, dict):
            json_str = _json.dumps(json)
            super().register_style(style_id, url=url, path=path, json=json_str)
        else:
            super().register_style(style_id, url=url, path=path, json=json)

    def render_from_bounds(
        self,
        style_id: str,
        *,
        bbox: Iterable[float],
        width: int,
        height: int,
        padding: float = 0.0,
        bearing: float = 0.0,
        pitch: float = 0.0,
        pixel_ratio: float = 1.0,
    ) -> bytes:
        """Render a static map image, fitted to a bounding box, to PNG bytes.

        `bbox` is `(west, south, east, north)` in degrees, as in shapely's `bounds` or
        geopandas' `total_bounds`. The zoom is chosen so that the bounds fit in the image,
        leaving `padding` on every side. `width`, `height` and `padding` are in logical pixels
        (the image is `width * pixel_ratio` by `height * pixel_ratio` pixels). `bearing` is in
        degrees clockwise from north, `pitch` in degrees from straight down (0-60). A `west`
        greater than `east` means the bounds cross the antimeridian, so swapped coordinates
        render most of the world rather than raising an error.
        """
        return super().render_from_bounds(
            style_id,
            bbox=_to_bbox(bbox),
            width=width,
            height=height,
            padding=padding,
            bearing=bearing,
            pitch=pitch,
            pixel_ratio=pixel_ratio,
        )

    async def arender_from_bounds(
        self,
        style_id: str,
        *,
        bbox: Iterable[float],
        width: int,
        height: int,
        padding: float = 0.0,
        bearing: float = 0.0,
        pitch: float = 0.0,
        pixel_ratio: float = 1.0,
    ) -> bytes:
        """Asynchronously render a static map image, fitted to a bounding box, to PNG bytes.

        Takes the same arguments as `render_from_bounds`.
        """
        return await super().arender_from_bounds(
            style_id,
            bbox=_to_bbox(bbox),
            width=width,
            height=height,
            padding=padding,
            bearing=bearing,
            pitch=pitch,
            pixel_ratio=pixel_ratio,
        )


def _to_bbox(bbox: Iterable[float]) -> tuple[float, float, float, float]:
    """Accept any iterable of 4 numbers (e.g. a list or a numpy array) as a bbox tuple."""
    values = tuple(float(v) for v in bbox)
    if len(values) != 4:
        raise ValueError(
            f"bbox must have 4 values: west, south, east, north (got {len(values)})"
        )
    west, south, east, north = values
    return west, south, east, north
