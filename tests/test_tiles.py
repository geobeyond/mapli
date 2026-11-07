import asyncio
import struct
from typing import Any

import pytest

import mapli

STYLE = {
    "version": 8,
    "sources": {},
    "layers": [
        {"id": "bg", "type": "background", "paint": {"background-color": "#f00"}}
    ],
}
TILE: dict[str, Any] = {"z": 2, "x": 1, "y": 1}


def read_png_size(png: bytes) -> tuple[int, int]:
    """Read width and height from the PNG IHDR chunk."""
    assert png.startswith(b"\x89PNG")
    return struct.unpack(">II", png[16:24])


@pytest.fixture(name="pool")
def create_pool() -> mapli.RenderPool:
    pool = mapli.RenderPool(workers=2)
    pool.register_style("bg", json=STYLE)
    return pool


@pytest.mark.parametrize(
    ("tile_size", "pixel_ratio", "expected"),
    [(512, 1.0, 512), (256, 1.0, 256), (256, 2.0, 512)],
)
def test_render_tile_size(
    pool: mapli.RenderPool, tile_size: int, pixel_ratio: float, expected: int
) -> None:
    png = pool.render_tile("bg", **TILE, tile_size=tile_size, pixel_ratio=pixel_ratio)
    assert read_png_size(png) == (expected, expected)


def test_render_tile_defaults_to_512(pool: mapli.RenderPool) -> None:
    assert read_png_size(pool.render_tile("bg", z=0, x=0, y=0)) == (512, 512)


def test_arender_tile_matches_render_tile(pool: mapli.RenderPool) -> None:
    png = asyncio.run(pool.arender_tile("bg", **TILE))
    assert png == pool.render_tile("bg", **TILE)


@pytest.mark.parametrize(
    "kwargs",
    [
        {"z": 2, "x": 4, "y": 0},
        {"z": 2, "x": 0, "y": 4},
        {**TILE, "tile_size": 0},
        {"z": 0, "x": 0, "y": 0, "tile_size": 256},
        {**TILE, "pixel_ratio": 0.0},
    ],
)
def test_render_tile_invalid_input(
    pool: mapli.RenderPool, kwargs: dict[str, Any]
) -> None:
    with pytest.raises(ValueError):
        pool.render_tile("bg", **kwargs)
    with pytest.raises(ValueError):
        asyncio.run(pool.arender_tile("bg", **kwargs))


def test_render_tile_unknown_style(pool: mapli.RenderPool) -> None:
    with pytest.raises(mapli.UnknownStyleError):
        pool.render_tile("nope", **TILE)
    with pytest.raises(mapli.UnknownStyleError):
        asyncio.run(pool.arender_tile("nope", **TILE))
