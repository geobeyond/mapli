import asyncio
from collections.abc import Coroutine
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
VIEW: dict[str, Any] = {
    "lon": -9.14,
    "lat": 38.72,
    "zoom": 4,
    "width": 64,
    "height": 64,
}


@pytest.fixture(name="pool")
def create_pool() -> mapli.RenderPool:
    pool = mapli.RenderPool(workers=2)
    pool.register_style("bg", json=STYLE)
    return pool


def test_arender_returns_png(pool: mapli.RenderPool) -> None:
    png = asyncio.run(pool.arender("bg", **VIEW))
    assert png.startswith(b"\x89PNG")
    assert png == pool.render("bg", **VIEW)


def test_arender_is_a_coroutine(pool: mapli.RenderPool) -> None:
    # PyO3 coroutines implement the coroutine protocol, but are not native Python coroutines,
    # so `inspect.iscoroutine()` is False while `asyncio.iscoroutine()` is True
    coro = pool.arender("bg", **VIEW)
    assert isinstance(coro, Coroutine)
    assert asyncio.iscoroutine(coro)
    coro.close()


def test_arender_concurrent(pool: mapli.RenderPool) -> None:
    async def main() -> list[bytes]:
        return await asyncio.gather(*(pool.arender("bg", **VIEW) for _ in range(8)))

    results = asyncio.run(main())
    assert len(results) == 8
    assert all(r.startswith(b"\x89PNG") for r in results)


def test_arender_unknown_style(pool: mapli.RenderPool) -> None:
    with pytest.raises(mapli.UnknownStyleError):
        asyncio.run(pool.arender("nope", **VIEW))


def test_arender_invalid_input(pool: mapli.RenderPool) -> None:
    view: dict[str, Any] = {**VIEW, "width": 0}
    with pytest.raises(ValueError):
        asyncio.run(pool.arender("bg", **view))


def test_arender_cancellation_leaves_pool_usable(pool: mapli.RenderPool) -> None:
    async def main() -> bytes:
        tasks = [asyncio.create_task(pool.arender("bg", **VIEW)) for _ in range(8)]
        await asyncio.sleep(0)  # let the tasks submit their requests
        for t in tasks:
            t.cancel()
        results = await asyncio.gather(*tasks, return_exceptions=True)
        assert all(isinstance(r, (asyncio.CancelledError, bytes)) for r in results)
        return await pool.arender("bg", **VIEW)

    assert asyncio.run(main()).startswith(b"\x89PNG")
