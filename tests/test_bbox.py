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
# square in degrees, so taller than wide in Web Mercator
LISBON = (-9.5, 38.5, -9.0, 39.0)
VIEW_SIZE: dict[str, Any] = {"width": 64, "height": 48}
VIEW: dict[str, Any] = {"bbox": LISBON, **VIEW_SIZE}


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
    ("pixel_ratio", "expected"), [(1.0, (64, 48)), (2.0, (128, 96))]
)
def test_render_bbox_size(
    pool: mapli.RenderPool, pixel_ratio: float, expected: tuple[int, int]
) -> None:
    png = pool.render_bbox("bg", **VIEW, pixel_ratio=pixel_ratio)
    assert read_png_size(png) == expected


def test_render_bbox_rgba_output(pool: mapli.RenderPool) -> None:
    rgba = pool.render_bbox("bg", **VIEW, output="rgba")
    assert len(rgba) == 64 * 48 * 4
    assert rgba[:4] == b"\xff\x00\x00\xff"


def test_arender_bbox_matches_render_bbox(pool: mapli.RenderPool) -> None:
    png = asyncio.run(pool.arender_bbox("bg", **VIEW))
    assert png == pool.render_bbox("bg", **VIEW)


@pytest.mark.parametrize(
    "bbox",
    [LISBON, list(LISBON), (v for v in LISBON), (-10, 38, -9, 39)],
    ids=["tuple", "list", "generator", "ints"],
)
def test_render_bbox_accepts_iterables(pool: mapli.RenderPool, bbox: Any) -> None:
    png = pool.render_bbox("bg", bbox=bbox, width=64, height=48)
    assert read_png_size(png) == (64, 48)


def test_max_lat_is_accepted_and_beyond_it_is_not(pool: mapli.RenderPool) -> None:
    whole_world = (-180, -mapli.MAX_LAT, 180, mapli.MAX_LAT)
    png = pool.render_bbox("bg", bbox=whole_world, **VIEW_SIZE)
    assert read_png_size(png) == (64, 48)
    with pytest.raises(ValueError):
        pool.render_bbox(
            "bg", bbox=(-180, -mapli.MAX_LAT, 180, mapli.MAX_LAT + 1e-9), **VIEW_SIZE
        )


@pytest.mark.parametrize(
    "bbox",
    [(170, -20, -170, -15), (-180, -85, 180, 85), (0, 0, 1e-6, 1e-6)],
    ids=["antimeridian", "whole-world", "deeper-than-max-zoom"],
)
def test_render_bbox_edge_cases(pool: mapli.RenderPool, bbox: Any) -> None:
    png = pool.render_bbox("bg", bbox=bbox, width=64, height=48)
    assert read_png_size(png) == (64, 48)


@pytest.mark.parametrize(
    "kwargs",
    [
        {**VIEW, "bbox": (-9.5, 38.5, -9.0)},
        {**VIEW, "bbox": (-181, 38.5, -9.0, 39.0)},
        {**VIEW, "bbox": (-9.5, 86.0, -9.0, 87.0)},
        {**VIEW, "bbox": (-9.5, 39.0, -9.0, 38.5)},
        {**VIEW, "bbox": (-9.5, 38.5, -9.5, 39.0)},
        {**VIEW, "width": 0},
        {**VIEW, "pixel_ratio": 0.0},
        {**VIEW, "output": "jpeg"},
    ],
    ids=[
        "3-values",
        "lon-out-of-range",
        "lat-out-of-range",
        "south-above-north",
        "zero-width",
        "zero-image-width",
        "zero-pixel-ratio",
        "unknown-output",
    ],
)
def test_render_bbox_invalid_input(
    pool: mapli.RenderPool, kwargs: dict[str, Any]
) -> None:
    with pytest.raises(ValueError):
        pool.render_bbox("bg", **kwargs)
    with pytest.raises(ValueError):
        asyncio.run(pool.arender_bbox("bg", **kwargs))


def test_render_bbox_unknown_style(pool: mapli.RenderPool) -> None:
    with pytest.raises(mapli.UnknownStyleError):
        pool.render_bbox("nope", **VIEW)
    with pytest.raises(mapli.UnknownStyleError):
        asyncio.run(pool.arender_bbox("nope", **VIEW))
