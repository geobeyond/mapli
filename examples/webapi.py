"""Example FastAPI web API that uses mapli to render map images.

Run it like this:

```shell
uv sync --group dev
uv run fastapi dev examples/webapi.py
```
"""

from contextlib import asynccontextmanager
from typing import (
    Annotated,
    Literal,
)

from fastapi import (
    FastAPI,
    HTTPException,
    Path,
    Query,
)
from fastapi.responses import Response

import mapli

_STYLES = {
    "demo": "https://demotiles.maplibre.org/style.json",
    "liberty": "https://tiles.openfreemap.org/styles/liberty",
    "positron": "https://tiles.openfreemap.org/styles/positron",
}


@asynccontextmanager
async def manage_render_pool(app: FastAPI):
    pool = mapli.RenderPool(workers=4)
    for id_, url in _STYLES.items():
        pool.register_style(id_, url=url)
    app.state.mapli_render_pool = pool
    yield
    del app.state.mapli_render_pool  # dropping the pool shuts down the workers


app = FastAPI(lifespan=manage_render_pool)


@app.get(
    "/maps/{style_id}",
    response_model=None,
    response_class=Response,
    responses={
        "200": {
            "content": {
                "image/png": {},
            }
        }
    },
)
async def render_map(
    style_id: Literal["demo", "liberty", "positron"] = "demo",
    lon: Annotated[float, Query(ge=-180, le=180)] = -9.14,
    lat: Annotated[float, Query(ge=-85.05, le=85.05)] = 38.72,
    zoom: Annotated[float, Query(ge=0, le=24)] = 3.0,
    width: Annotated[int, Query(ge=1, le=2048)] = 512,
    height: Annotated[int, Query(ge=1, le=2048)] = 512,
) -> Response:
    """Return a map image in PNG format."""
    try:
        png_bytes = await app.state.mapli_render_pool.arender(
            style_id, lon=lon, lat=lat, zoom=zoom, width=width, height=height
        )
    except mapli.UnknownStyleError:
        raise HTTPException(status_code=404, detail=f"unknown style {style_id!r}")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except mapli.MapliError as e:
        raise HTTPException(status_code=502, detail=str(e))
    return Response(content=png_bytes, media_type="image/png")


@app.get(
    "/tiles/{style_id}/{z}/{x}/{y}.png",
    response_model=None,
    response_class=Response,
    responses={
        "200": {
            "content": {
                "image/png": {},
            }
        }
    },
)
async def render_tile(
    style_id: Literal["demo", "liberty", "positron"],
    z: Annotated[int, Path(ge=0, le=24)],
    x: Annotated[int, Path(ge=0)],
    y: Annotated[int, Path(ge=0)],
    tile_size: Annotated[int, Query(ge=256, le=512, multiple_of=256)] = 512,
    pixel_ratio: Annotated[float, Query(gt=0, le=4)] = 1.0,
) -> Response:
    """Return a map tile in PNG format, in the XYZ scheme."""
    try:
        png_bytes = await app.state.mapli_render_pool.arender_tile(
            style_id, z=z, x=x, y=y, tile_size=tile_size, pixel_ratio=pixel_ratio
        )
    except mapli.UnknownStyleError:
        raise HTTPException(status_code=404, detail=f"unknown style {style_id!r}")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except mapli.MapliError as e:
        raise HTTPException(status_code=502, detail=str(e))
    return Response(content=png_bytes, media_type="image/png")
