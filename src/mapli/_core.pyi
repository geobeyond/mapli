from collections.abc import Callable
from enum import Enum
from os import PathLike
from typing import final

from typing_extensions import Buffer

class MapliError(Exception): ...
class UnknownStyleError(MapliError): ...
class StyleLoadError(MapliError): ...
class RenderError(MapliError): ...
class GraphicsUnavailableError(MapliError): ...
class UnknownSchemeError(MapliError): ...

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
    NOT_FOUND = ...
    SERVER = ...
    CONNECTION = ...
    RATE_LIMIT = ...
    OTHER = ...

@final
class Response:
    def __init__(self, data: Buffer) -> None: ...
    @staticmethod
    def no_content() -> Response: ...
    @staticmethod
    def error(
        reason: ErrorReason, message: str, *, retry_after: float | None = None
    ) -> Response: ...

def register_scheme_handler(
    scheme: str, handler: Callable[[RawResourceRequest], Response | Buffer]
) -> None: ...
def unregister_scheme_handler(scheme: str) -> None: ...

class RenderPool:
    def __init__(self, workers: int = 1, max_renderers_per_worker: int = 8) -> None: ...
    def register_style(
        self,
        style_id: str,
        *,
        url: str | None = None,
        path: str | PathLike[str] | None = None,
        json: str | None = None,
    ) -> None: ...
    def has_style(self, style_id: str) -> bool: ...
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
    ) -> bytes: ...
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
    ) -> bytes: ...
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
    ) -> bytes: ...
    async def arender_tile(
        self,
        style_id: str,
        *,
        z: int,
        x: int,
        y: int,
        tile_size: int = 512,
        pixel_ratio: float = 1.0,
    ) -> bytes: ...
