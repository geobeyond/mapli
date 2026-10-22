"""Render MapLibre styles to PNG images with MapLibre Native, inside the Python process."""

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
"""What a `ResourceRequest` asks for. PMTiles archive reads are `"source"` requests."""


@_dataclasses.dataclass(frozen=True, slots=True, kw_only=True)
class ResourceRequest:
    """A request that MapLibre Native makes for a URL of a registered scheme.

    Scheme handlers are called with an instance of this class, see `register_scheme_handler`.
    """

    url: str
    """The full URL, including the scheme."""
    kind: ResourceKind
    """What is being requested."""
    data_range: tuple[int, int] | None = None
    """Inclusive `(start, end)` byte range for partial reads, as made for PMTiles archives,
    else `None`."""


class SchemeHandlerProtocol(Protocol):
    """The interface of a scheme handler, see `register_scheme_handler`.

    Any callable taking a `ResourceRequest` satisfies it: a function, a lambda or an object with
    a `__call__` method. It returns a `Response`, or the data as a bytes-like object, which is
    the same as returning `Response(data)`.

    Behaviour:

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

    MapLibre Native then passes every request for a URL of `scheme` (e.g. `store://tiles/a.pmtiles`
    for `scheme="store"`) to `handler`, on a thread owned by mapli. Handlers can be registered at
    any time, before or after render pools are created. Registering a scheme again replaces its
    handler for the requests made from then on; data that renderers have already loaded is not
    fetched again.

    Args:
        scheme: The URL scheme, without `://`. Schemes that MapLibre Native serves itself
            (`http`, `https`, `file`, `mbtiles`, `pmtiles`) cannot be given a handler.
        handler: A callable that serves the requests, see `SchemeHandlerProtocol`.

    Raises:
        TypeError: If `handler` is not callable.
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
    serving complete.

    Args:
        scheme: The URL scheme, without `://`.

    Raises:
        UnknownSchemeError: If `scheme` has no handler.
    """
    _core.unregister_scheme_handler(scheme)


class RenderPool(_core.RenderPool):
    """A pool of worker threads that render MapLibre styles to PNG images.

    Styles are registered under an id with `register_style`, and then rendered by id. Each
    worker keeps a MapLibre Native renderer per style, which is reused across render calls.
    Dropping the last reference to the pool stops its workers.
    """

    def register_style(
        self,
        style_id: str,
        *,
        url: str | None = None,
        path: str | PathLike[str] | None = None,
        json: str | dict[str, Any] | None = None,
    ) -> None:
        """Register a MapLibre style under `style_id`.

        Pass exactly one of `url`, `path` or `json`. The style is loaded by each worker when it
        is first used for rendering, so loading errors are raised by the render methods, as
        `StyleLoadError`.

        Args:
            style_id: The id to render the style by.
            url: URL of a style JSON document.
            path: Path of a local style JSON file.
            json: The style itself, as a JSON string or as a dict.

        Raises:
            ValueError: If not exactly one of `url`, `path` or `json` is given, or if `url` is
                not a valid URL.
        """
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

        The zoom is chosen so that the bounds fit in the image, leaving `padding` on every side.
        Blocks until the image is rendered. The GIL is released while waiting.

        Args:
            style_id: The id of a registered style.
            bbox: `(west, south, east, north)`, in degrees, as in shapely's `bounds` or
                geopandas' `total_bounds`. Any iterable of 4 numbers is accepted. A `west`
                greater than `east` means the bounds cross the antimeridian, so swapped
                coordinates render most of the world rather than raising an error.
            width: Width of the image, in logical pixels.
            height: Height of the image, in logical pixels.
            padding: Space to leave around the bounds on every side, in logical pixels.
            bearing: Rotation of the map, in degrees clockwise from north.
            pitch: Tilt of the map, in degrees from straight down (0-60).
            pixel_ratio: Number of image pixels per logical pixel. The image is
                `width * pixel_ratio` by `height * pixel_ratio` pixels.

        Returns:
            The image, PNG encoded.

        Raises:
            ValueError: If `bbox` does not have 4 values, or an argument is out of range, e.g.
                a `padding` that leaves no room for the bounds.
            UnknownStyleError: If no style is registered under `style_id`.
            StyleLoadError: If MapLibre Native fails to load the style.
            RenderError: If MapLibre Native fails to render the image.
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

        Takes the same arguments, and raises the same exceptions, as `render_from_bounds`. The
        request is submitted to the pool when the returned coroutine is first awaited, which
        must happen inside an asyncio event loop. Cancelling the coroutine skips the render if
        no worker has started it yet.
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
