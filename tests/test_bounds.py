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
PORTUGAL = (-9.6, 36.9, -6.1, 42.2)
VIEW: dict[str, Any] = {"bbox": PORTUGAL, "width": 64, "height": 48}


def png_size(png: bytes) -> tuple[int, int]:
    """Read width and height from the PNG IHDR chunk."""
    assert png.startswith(b"\x89PNG")
    return struct.unpack(">II", png[16:24])


@pytest.fixture
def pool() -> mapli.RenderPool:
    pool = mapli.RenderPool(workers=2)
    pool.register_style("bg", json=STYLE)
    return pool


@pytest.mark.parametrize(
    ("pixel_ratio", "expected"), [(1.0, (64, 48)), (2.0, (128, 96))]
)
def test_render_from_bounds_size(
    pool: mapli.RenderPool, pixel_ratio: float, expected: tuple[int, int]
) -> None:
    png = pool.render_from_bounds("bg", **VIEW, pixel_ratio=pixel_ratio)
    assert png_size(png) == expected


def test_arender_from_bounds_matches_render_from_bounds(
    pool: mapli.RenderPool,
) -> None:
    png = asyncio.run(pool.arender_from_bounds("bg", **VIEW))
    assert png == pool.render_from_bounds("bg", **VIEW)


@pytest.mark.parametrize(
    "bbox",
    [PORTUGAL, list(PORTUGAL), (v for v in PORTUGAL), (-10, 36, -6, 42)],
    ids=["tuple", "list", "generator", "ints"],
)
def test_render_from_bounds_accepts_iterables(
    pool: mapli.RenderPool, bbox: Any
) -> None:
    png = pool.render_from_bounds("bg", bbox=bbox, width=64, height=48)
    assert png_size(png) == (64, 48)


def test_render_from_bounds_crossing_antimeridian(pool: mapli.RenderPool) -> None:
    png = pool.render_from_bounds("bg", bbox=(170, -20, -170, -15), width=64, height=48)
    assert png_size(png) == (64, 48)


@pytest.mark.parametrize(
    "kwargs",
    [
        {**VIEW, "bbox": (-9.6, 36.9, -6.1)},
        {**VIEW, "bbox": (-9.6, 36.9, -6.1, 42.2, 0.0)},
        {**VIEW, "bbox": (-181, 36.9, -6.1, 42.2)},
        {**VIEW, "bbox": (-9.6, 42.2, -6.1, 36.9)},
        {**VIEW, "bbox": (-9.6, 36.9, -9.6, 42.2)},
        {**VIEW, "padding": -1.0},
        {**VIEW, "padding": 24.0},
        {**VIEW, "pitch": 90.0},
        {**VIEW, "width": 0},
    ],
    ids=[
        "3-values",
        "5-values",
        "lon-out-of-range",
        "south-above-north",
        "zero-width",
        "negative-padding",
        "padding-too-large",
        "pitch-out-of-range",
        "zero-image-width",
    ],
)
def test_render_from_bounds_invalid_input(
    pool: mapli.RenderPool, kwargs: dict[str, Any]
) -> None:
    with pytest.raises(ValueError):
        pool.render_from_bounds("bg", **kwargs)
    with pytest.raises(ValueError):
        asyncio.run(pool.arender_from_bounds("bg", **kwargs))


def test_render_from_bounds_unknown_style(pool: mapli.RenderPool) -> None:
    with pytest.raises(mapli.UnknownStyleError):
        pool.render_from_bounds("nope", **VIEW)
    with pytest.raises(mapli.UnknownStyleError):
        asyncio.run(pool.arender_from_bounds("nope", **VIEW))
