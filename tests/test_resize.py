"""Renders at varied sizes reuse one renderer per style and pixel ratio, resized each time."""

import pytest

import mapli

STYLE = {
    "version": 8,
    "sources": {},
    "layers": [
        {"id": "bg", "type": "background", "paint": {"background-color": "#ff0000"}}
    ],
}


@pytest.fixture(name="pool")
def create_pool() -> mapli.RenderPool:
    # a single renderer, so every render below reuses it
    pool = mapli.RenderPool(workers=1, max_renderers_per_worker=1)
    pool.register_style("bg", json=STYLE)
    return pool


@pytest.mark.parametrize("pixel_ratio", [1.0, 2.0])
def test_varied_sizes_render_at_requested_size(pool, pixel_ratio):
    for width, height in [(64, 64), (200, 100), (64, 64), (30, 90)]:
        image = pool.render(
            "bg",
            lon=0,
            lat=0,
            zoom=1,
            width=width,
            height=height,
            pixel_ratio=pixel_ratio,
            output="rgba",
        )
        physical = (int(width * pixel_ratio), int(height * pixel_ratio))
        assert len(image) == physical[0] * physical[1] * 4
        assert image == bytes([255, 0, 0, 255]) * (physical[0] * physical[1])
