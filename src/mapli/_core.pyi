from os import PathLike

class MapliError(Exception): ...
class UnknownStyleError(MapliError): ...
class StyleLoadError(MapliError): ...
class RenderError(MapliError): ...
class GraphicsUnavailableError(MapliError): ...

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
