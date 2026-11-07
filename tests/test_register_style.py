"""Registering a style under an id that is already registered."""

from typing import Any

import pytest

import mapli

VIEW: dict[str, Any] = {
    "lon": 0,
    "lat": 0,
    "zoom": 1,
    "width": 4,
    "height": 4,
    "output": "rgba",
}
TILE: dict[str, Any] = {"z": 1, "x": 0, "y": 0, "tile_size": 256, "output": "rgba"}
RED = bytes([255, 0, 0, 255])
BLUE = bytes([0, 0, 255, 255])


def background(color: str) -> dict[str, Any]:
    """A style painting the whole map in `color`."""
    return {
        "version": 8,
        "sources": {},
        "layers": [
            {"id": "bg", "type": "background", "paint": {"background-color": color}}
        ],
    }


@pytest.fixture
def pool() -> mapli.RenderPool:
    # a single worker, so renders after the replacement reach the renderers cached before it
    pool = mapli.RenderPool(workers=1)
    pool.register_style("bg", json=background("#f00"))
    return pool


def test_replaced_style_is_rendered(pool: mapli.RenderPool) -> None:
    assert bytes(pool.render("bg", **VIEW)[:4]) == RED
    assert bytes(pool.render_tile("bg", **TILE)[:4]) == RED

    pool.register_style("bg", json=background("#00f"))

    assert bytes(pool.render("bg", **VIEW)[:4]) == BLUE
    assert bytes(pool.render_tile("bg", **TILE)[:4]) == BLUE


def test_failed_replacement_does_not_render_old_style(pool: mapli.RenderPool) -> None:
    pool.render("bg", **VIEW)
    pool.register_style("bg", json="not a style")
    with pytest.raises(mapli.StyleLoadError):
        pool.render("bg", **VIEW)
