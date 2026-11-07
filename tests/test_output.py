"""The `output` argument of the render methods: PNG bytes or raw RGBA pixels."""

import asyncio
import math
from collections.abc import Callable
from typing import Any

import pytest

import mapli

# half-transparent red, to tell straight alpha (255, 0, 0, 128) from premultiplied (128, 0, 0, 128)
STYLE = {
    "version": 8,
    "sources": {},
    "layers": [
        {
            "id": "bg",
            "type": "background",
            "paint": {"background-color": "rgba(255, 0, 0, 0.5)"},
        }
    ],
}
RED_HALF_ALPHA = bytes([255, 0, 0, 128])
VIEW: dict[str, Any] = {"lon": 0, "lat": 0, "zoom": 1, "width": 7, "height": 5}
BBOX = (-10.0, -10.0, 10.0, 10.0)
TILE: dict[str, Any] = {"z": 2, "x": 1, "y": 1, "tile_size": 256}


@pytest.fixture(name="pool")
def create_pool() -> mapli.RenderPool:
    pool = mapli.RenderPool(workers=2)
    pool.register_style("bg", json=STYLE)
    return pool


# every render method, as a function taking the pool and the extra keyword arguments, returning
# the image, and the image size in logical pixels
RENDERERS: dict[str, tuple[Callable[..., bytes], tuple[int, int]]] = {
    "render": (
        lambda pool, **kw: pool.render("bg", **VIEW, **kw),
        (VIEW["width"], VIEW["height"]),
    ),
    "arender": (
        lambda pool, **kw: asyncio.run(pool.arender("bg", **VIEW, **kw)),
        (VIEW["width"], VIEW["height"]),
    ),
    "render_from_bounds": (
        lambda pool, **kw: pool.render_from_bounds(
            "bg", bbox=BBOX, width=VIEW["width"], height=VIEW["height"], **kw
        ),
        (VIEW["width"], VIEW["height"]),
    ),
    "arender_from_bounds": (
        lambda pool, **kw: asyncio.run(
            pool.arender_from_bounds(
                "bg", bbox=BBOX, width=VIEW["width"], height=VIEW["height"], **kw
            )
        ),
        (VIEW["width"], VIEW["height"]),
    ),
    "render_tile": (
        lambda pool, **kw: pool.render_tile("bg", **TILE, **kw),
        (TILE["tile_size"], TILE["tile_size"]),
    ),
    "arender_tile": (
        lambda pool, **kw: asyncio.run(pool.arender_tile("bg", **TILE, **kw)),
        (TILE["tile_size"], TILE["tile_size"]),
    ),
}


@pytest.mark.parametrize("method", RENDERERS)
def test_png_is_the_default(pool: mapli.RenderPool, method: str) -> None:
    render, _ = RENDERERS[method]
    png = render(pool)
    assert png.startswith(b"\x89PNG")
    assert render(pool, output="png") == png


@pytest.mark.parametrize("method", RENDERERS)
@pytest.mark.parametrize("pixel_ratio", [1.0, 1.3, 2.0])
def test_rgba_is_straight_alpha_pixels(
    pool: mapli.RenderPool, method: str, pixel_ratio: float
) -> None:
    render, (width, height) = RENDERERS[method]
    rgba = render(pool, output="rgba", pixel_ratio=pixel_ratio)
    # the image size is rounded down to whole pixels
    pixels = math.floor(width * pixel_ratio) * math.floor(height * pixel_ratio)
    assert rgba == RED_HALF_ALPHA * pixels


@pytest.mark.parametrize("method", RENDERERS)
@pytest.mark.parametrize("output", ["jpeg", "PNG", "RGBA", ""])
def test_invalid_output(pool: mapli.RenderPool, method: str, output: str) -> None:
    render, _ = RENDERERS[method]
    with pytest.raises(ValueError, match="output"):
        render(pool, output=output)
