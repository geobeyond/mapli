from collections.abc import Callable
from enum import Enum
from os import PathLike
from typing import (
    Literal,
    final,
)

from typing_extensions import Buffer

MAX_LAT: float
"""Latitude limit of Web Mercator, in degrees: bounding boxes must be within ±`MAX_LAT`."""

class MapliError(Exception):
    """Base class of the exceptions raised by mapli."""

class UnknownStyleError(MapliError):
    """No style is registered under the requested style id."""

class StyleLoadError(MapliError):
    """MapLibre Native failed to load a style.

    Styles are loaded when they are first used for rendering, not when they are registered, so
    this is raised by the render methods.
    """

class RenderError(MapliError):
    """MapLibre Native failed to render an image.

    The message includes the reason given by MapLibre Native. This is also raised when a scheme
    handler raises an exception or returns `Response.error`.
    """

class GraphicsUnavailableError(MapliError):
    """The system cannot provide the OpenGL (EGL) context needed for rendering.

    Raised when creating a `RenderPool`. The message says which EGL step failed.
    """

class UnknownSchemeError(MapliError):
    """No scheme handler is registered for the scheme."""

class UrlRulesClosedError(MapliError):
    """The URL rules can no longer be set.

    Raised by `restrict_resource_urls` once a `RenderPool` has been created, or if the rules
    have already been set.
    """

@final
class RawResourceRequest:
    @property
    def url(self) -> str: ...
    @property
    def kind(self) -> str: ...
    @property
    def data_range(self) -> tuple[int, int] | None: ...

@final
class ErrorReason(Enum):
    """Why a scheme handler failed to serve a request, see `Response.error`."""

    NOT_FOUND = ...
    """The requested resource does not exist."""
    SERVER = ...
    """The server holding the resource failed."""
    CONNECTION = ...
    """The server holding the resource could not be reached."""
    RATE_LIMIT = ...
    """The request was rate limited."""
    OTHER = ...
    """Any other reason."""

@final
class Response:
    """Response of a scheme handler to a request: data, no content or an error."""

    def __init__(self, data: Buffer) -> None:
        """Create a successful response carrying `data`.

        Args:
            data: The response body, as any bytes-like object, such as `bytes`, `bytearray`,
                `memoryview` or obstore's `Bytes`.
        """
    @staticmethod
    def no_content() -> Response:
        """Create a successful, empty response.

        Used e.g. for a missing tile, which is then drawn as empty instead of failing the render.
        """
    @staticmethod
    def error(
        reason: ErrorReason, message: str, *, retry_after: float | None = None
    ) -> Response:
        """Create a failed response, which fails the render with a `RenderError`.

        Args:
            reason: Why the request failed.
            message: A description of the failure, included in the `RenderError` message.
            retry_after: For rate limited requests, seconds from now after which the request
                can be retried.
        """

def register_scheme_handler(
    scheme: str, handler: Callable[[RawResourceRequest], Response | Buffer]
) -> None: ...
def unregister_scheme_handler(scheme: str) -> None: ...
def restrict_resource_urls(allowed_url_prefixes: list[str]) -> None: ...

class RenderPool:
    def __init__(self, workers: int = 1, max_renderers_per_worker: int = 8) -> None:
        """Creating a pool starts its worker threads.

        Args:
            workers: Number of worker threads. Each worker has its own MapLibre Native run loop,
                OpenGL context and renderers, so memory use grows with this number.
            max_renderers_per_worker: Number of renderers each worker keeps. A worker keeps one
                renderer per style, render mode and pixel ratio, whatever the image size; when
                this limit is reached, the least recently used renderer is discarded.

        Raises:
            ValueError: If `workers` or `max_renderers_per_worker` is lower than 1.
            GraphicsUnavailableError: If an OpenGL (EGL) context cannot be created.
        """
    def register_style(
        self,
        style_id: str,
        *,
        url: str | None = None,
        path: str | PathLike[str] | None = None,
        json: str | None = None,
    ) -> None: ...
    def has_style(self, style_id: str) -> bool:
        """Check whether a style is registered under `style_id`.

        Args:
            style_id: The id of the style.

        Returns:
            `True` if the style is registered, else `False`.
        """
    def render(
        self,
        style_id: str,
        *,
        lon: float,
        lat: float,
        zoom: float,
        width: int,
        height: int,
        bearing: float = 0.0,
        pitch: float = 0.0,
        pixel_ratio: float = 1.0,
        output: Literal["png", "rgba"] = "png",
    ) -> bytes:
        """Render a static map image, centered on a point, to PNG bytes or raw pixels.

        Blocks until the image is rendered. The GIL is released while waiting.

        Args:
            style_id: The id of a registered style.
            lon: Longitude of the center of the image, in degrees.
            lat: Latitude of the center of the image, in degrees.
            zoom: MapLibre zoom level.
            width: Width of the image, in logical pixels.
            height: Height of the image, in logical pixels.
            bearing: Rotation of the map, in degrees clockwise from north.
            pitch: Tilt of the map, in degrees from straight down (0-60).
            pixel_ratio: Number of image pixels per logical pixel. The image is
                `width * pixel_ratio` by `height * pixel_ratio` pixels.
            output: `"png"` for PNG-encoded bytes, or `"rgba"` for the raw pixels, which skips
                PNG encoding, e.g. to hand the image to Pillow or numpy.

        Returns:
            The image: PNG encoded with `output="png"`, or with `output="rgba"`, its raw pixels,
                row by row from the top-left, 4 bytes (red, green, blue, alpha) per pixel, with
                straight (not premultiplied) alpha. The image is `floor(width * pixel_ratio)` by
                `floor(height * pixel_ratio)` pixels.

        Raises:
            ValueError: If an argument is out of range, e.g. a `pitch` outside 0-60, or
                `output` is neither `"png"` nor `"rgba"`.
            UnknownStyleError: If no style is registered under `style_id`.
            StyleLoadError: If MapLibre Native fails to load the style.
            RenderError: If MapLibre Native fails to render the image.
        """
    async def arender(
        self,
        style_id: str,
        *,
        lon: float,
        lat: float,
        zoom: float,
        width: int,
        height: int,
        bearing: float = 0.0,
        pitch: float = 0.0,
        pixel_ratio: float = 1.0,
        output: Literal["png", "rgba"] = "png",
    ) -> bytes:
        """Asynchronously render a static map image, centered on a point.

        Takes the same arguments, and raises the same exceptions, as `render`. The request is
        submitted to the pool when the returned coroutine is first awaited, which must happen
        inside an asyncio event loop. Cancelling the coroutine skips the render if no worker has
        started it yet.
        """
    def render_from_bounds(
        self,
        style_id: str,
        *,
        bbox: tuple[float, float, float, float],
        width: int,
        height: int,
        padding: float = 0.0,
        bearing: float = 0.0,
        pitch: float = 0.0,
        pixel_ratio: float = 1.0,
        output: Literal["png", "rgba"] = "png",
    ) -> bytes: ...
    async def arender_from_bounds(
        self,
        style_id: str,
        *,
        bbox: tuple[float, float, float, float],
        width: int,
        height: int,
        padding: float = 0.0,
        bearing: float = 0.0,
        pitch: float = 0.0,
        pixel_ratio: float = 1.0,
        output: Literal["png", "rgba"] = "png",
    ) -> bytes: ...
    def render_bbox(
        self,
        style_id: str,
        *,
        bbox: tuple[float, float, float, float],
        width: int,
        height: int,
        pixel_ratio: float = 1.0,
        output: Literal["png", "rgba"] = "png",
    ) -> bytes: ...
    async def arender_bbox(
        self,
        style_id: str,
        *,
        bbox: tuple[float, float, float, float],
        width: int,
        height: int,
        pixel_ratio: float = 1.0,
        output: Literal["png", "rgba"] = "png",
    ) -> bytes: ...
    def render_tile(
        self,
        style_id: str,
        *,
        z: int,
        x: int,
        y: int,
        tile_size: int = 512,
        pixel_ratio: float = 1.0,
        output: Literal["png", "rgba"] = "png",
    ) -> bytes:
        """Render a map tile in the XYZ (WebMercatorQuad) tiling scheme to PNG bytes or raw pixels.

        Blocks until the tile is rendered. The GIL is released while waiting.

        Args:
            style_id: The id of a registered style.
            z: Zoom level of the tile.
            x: Column of the tile, from 0 at the antimeridian, increasing eastwards.
            y: Row of the tile, from 0 at the north edge, increasing southwards.
            tile_size: Width and height of the tile, in logical pixels. MapLibre zoom levels
                are based on 512 px tiles, so a 256 px tile is drawn one zoom level lower, and
                256 px tiles cannot be rendered at `z=0`.
            pixel_ratio: Number of image pixels per logical pixel. The tile is
                `tile_size * pixel_ratio` pixels wide.
            output: `"png"` for PNG-encoded bytes, or `"rgba"` for the raw pixels, which skips
                PNG encoding, e.g. to hand the image to Pillow or numpy.

        Returns:
            The tile: PNG encoded with `output="png"`, or with `output="rgba"`, its raw pixels,
                row by row from the top-left, 4 bytes (red, green, blue, alpha) per pixel, with
                straight (not premultiplied) alpha. The tile is `floor(tile_size * pixel_ratio)`
                pixels wide and high.

        Raises:
            ValueError: If an argument is out of range, e.g. `x` or `y` outside the tile grid of
                zoom level `z`, a 256 px tile at `z=0`, or `output` is neither `"png"` nor
                `"rgba"`.
            UnknownStyleError: If no style is registered under `style_id`.
            StyleLoadError: If MapLibre Native fails to load the style.
            RenderError: If MapLibre Native fails to render the tile.
        """
    async def arender_tile(
        self,
        style_id: str,
        *,
        z: int,
        x: int,
        y: int,
        tile_size: int = 512,
        pixel_ratio: float = 1.0,
        output: Literal["png", "rgba"] = "png",
    ) -> bytes:
        """Asynchronously render a map tile.

        Takes the same arguments, and raises the same exceptions, as `render_tile`. Behaves like
        `arender`: the request is submitted to the pool when the coroutine is first awaited.
        """
