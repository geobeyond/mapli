"""mapli: Render MapLibre styles to images in-process."""

import dataclasses as _dataclasses
import json as _json
from collections.abc import Iterable
from os import PathLike
from typing import (
    TYPE_CHECKING,
    Any,
    Literal,
    Protocol,
    cast,
)

from . import _core
from ._core import (
    ErrorReason,
    GraphicsUnavailableError,
    MapliError,
    RenderError,
    Response,
    StyleLoadError,
    UnknownSchemeError,
    UnknownStyleError,
)

if TYPE_CHECKING:
    from typing_extensions import Buffer

__all__ = [
    "ErrorReason",
    "GraphicsUnavailableError",
    "MapliError",
    "RenderError",
    "RenderPool",
    "ResourceKind",
    "ResourceRequest",
    "Response",
    "SchemeHandlerProtocol",
    "StyleLoadError",
    "UnknownSchemeError",
    "UnknownStyleError",
    "register_scheme_handler",
    "unregister_scheme_handler",
]


ResourceKind = Literal[
    "style",
    "source",
    "tile",
    "glyphs",
    "sprite_image",
    "sprite_json",
    "image",
    "unknown",
]


@_dataclasses.dataclass(frozen=True, slots=True, kw_only=True)
class ResourceRequest:
    """A request that MapLibre Native makes for a URL of a registered scheme."""

    url: str
    kind: ResourceKind
    data_range: tuple[int, int] | None = None


class SchemeHandlerProtocol(Protocol):
    """Serves the requests for the URLs of a scheme, see `register_scheme_handler`.

    Any callable taking a `ResourceRequest` will do: a function, a lambda or an object with a
    `__call__` method. It returns a `Response`, or the data as a bytes-like object.

    Notes:

    - Handlers run on threads owned by mapli, so requests are served concurrently when the
      handler's I/O releases the GIL, as obstore and the standard library's sockets and
      files do.

    - Handlers can block while they fetch data, but must be thread-safe, and values in
      a `threading.local()` do not persist between calls.

    - An exception raised by a handler fails the render with a `RenderError`.

    - Identical requests (same URL and byte range) made while a handler is already serving
      one wait for it and get the same response, instead of calling the handler again.

    - mapli does not cache responses, beyond what MapLibre Native keeps on its own, so a
      handler that wants to avoid reading the same data again must cache it itself.
    """

    def __call__(self, request: ResourceRequest, /) -> "Response | Buffer": ...


def register_scheme_handler(scheme: str, handler: SchemeHandlerProtocol) -> None:
    """Register `handler` for the URLs of `scheme`, process-wide.

    `handler` is called with a `ResourceRequest` and returns a `Response`, or `bytes`. It runs
    on a thread owned by mapli. Handlers can be registered at any time, before or after render
    pools are created, and registering a scheme again replaces its handler for the requests made
    from then on. Data that renderers have already loaded is not fetched again.

    Note that schemes served by MapLibre Native itself cannot be overridden with a different handler.
    """
    if not callable(handler):
        raise TypeError("handler must be callable")

    def inner_handler(request: _core.RawResourceRequest) -> "Response | Buffer":
        return handler(
            ResourceRequest(
                url=request.url,
                kind=cast(ResourceKind, request.kind),
                data_range=request.data_range,
            )
        )

    _core.register_scheme_handler(scheme, inner_handler)


def unregister_scheme_handler(scheme: str) -> None:
    """Remove the handler of `scheme`, process-wide.

    Requests made from then on are no longer served by the handler, while the ones it is already
    serving complete. Raises `UnknownSchemeError` if `scheme` has no handler.
    """
    _core.unregister_scheme_handler(scheme)


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
