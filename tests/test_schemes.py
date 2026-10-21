"""Custom URL scheme handlers, through the `memory://` scheme registered in conftest.py."""

import asyncio
import dataclasses
import gc
import json
import time
from collections import Counter
from collections.abc import Callable
from typing import Any

import pytest
from conftest import (
    Handler,
    SetMemoryHandler,
    build_pmtiles,
    pmtiles_handler,
)

import mapli

BOX = {
    "type": "Polygon",
    "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]],
}
VIEW: dict[str, Any] = {"lon": 0.0, "lat": 0.0, "zoom": 0.0, "width": 64, "height": 64}


def geojson_style(data: object) -> dict[str, Any]:
    return {
        "version": 8,
        "sources": {"box": {"type": "geojson", "data": data}},
        "layers": [
            {"id": "bg", "type": "background", "paint": {"background-color": "#fff"}},
            {
                "id": "box",
                "type": "fill",
                "source": "box",
                "paint": {"fill-color": "#f00"},
            },
        ],
    }


def raster_style(source: dict[str, Any]) -> dict[str, Any]:
    """Style drawing a raster source, given by its `tiles` or its `url`, over white."""
    return {
        "version": 8,
        "sources": {"tiles": {"type": "raster", "tileSize": 512, **source}},
        "layers": [
            {"id": "bg", "type": "background", "paint": {"background-color": "#fff"}},
            {"id": "tiles", "type": "raster", "source": "tiles"},
        ],
    }


def background_style(color: str) -> dict[str, Any]:
    return {
        "version": 8,
        "sources": {},
        "layers": [
            {"id": "bg", "type": "background", "paint": {"background-color": color}}
        ],
    }


@pytest.fixture
def pool() -> mapli.RenderPool:
    return mapli.RenderPool(workers=2)


def test_style_and_source_from_handler(
    pool: mapli.RenderPool, memory_handler: SetMemoryHandler
) -> None:
    served = {
        "memory://style.json": json.dumps(
            geojson_style("memory://box.geojson")
        ).encode(),
        "memory://box.geojson": mapli.Response(json.dumps(BOX).encode()),
    }
    requests: list[mapli.ResourceRequest] = []

    def handler(request: mapli.ResourceRequest) -> object:
        requests.append(request)
        return served[request.url]

    memory_handler(handler)
    pool.register_style("served", url="memory://style.json")
    pool.register_style("inline", json=geojson_style(BOX))

    assert pool.render("served", **VIEW) == pool.render("inline", **VIEW)
    assert all(type(r) is mapli.ResourceRequest for r in requests)
    assert mapli.ResourceRequest(url="memory://style.json", kind="style") in requests
    assert mapli.ResourceRequest(url="memory://box.geojson", kind="source") in requests


def test_resource_request_is_a_frozen_dataclass() -> None:
    """Handlers get a plain dataclass, which tests can also build without rendering."""
    request = mapli.ResourceRequest(
        url="memory://a.pmtiles", kind="source", data_range=(0, 126)
    )
    assert request == mapli.ResourceRequest(
        url="memory://a.pmtiles", kind="source", data_range=(0, 126)
    )
    assert len({request, dataclasses.replace(request)}) == 1
    assert dataclasses.replace(request, data_range=None).data_range is None
    assert repr(request) == (
        "ResourceRequest(url='memory://a.pmtiles', kind='source', data_range=(0, 126))"
    )
    match request:
        case mapli.ResourceRequest(kind="source", data_range=(start, end)):
            assert (start, end) == (0, 126)
        case _:
            pytest.fail("request did not match")
    with pytest.raises(dataclasses.FrozenInstanceError):
        request.url = "memory://b.pmtiles"  # ty: ignore[invalid-assignment]
    with pytest.raises(TypeError):
        mapli.ResourceRequest("memory://a.pmtiles", "source")  # ty: ignore[missing-argument, too-many-positional-arguments]


def test_handlers_satisfy_the_protocol() -> None:
    """Functions, lambdas and callable objects are handlers, whatever their parameter's name.

    This is checked by the type checker, the test only makes sure that they run.
    """

    def function(req: mapli.ResourceRequest) -> bytes:
        return req.url.encode()

    class Handler:
        def __call__(self, request: mapli.ResourceRequest) -> mapli.Response:
            return mapli.Response.no_content()

    handlers: list[mapli.SchemeHandlerProtocol] = [
        function,
        Handler(),
        lambda request: bytearray(request.url.encode()),
    ]
    request = mapli.ResourceRequest(url="memory://a", kind="source")
    returned = [handler(request) for handler in handlers]
    assert returned[0] == returned[2] == b"memory://a"
    assert isinstance(returned[1], mapli.Response)


def test_raw_request_is_not_public() -> None:
    assert not hasattr(mapli, "RawResourceRequest")


def test_tiles_from_handler(
    pool: mapli.RenderPool, memory_handler: SetMemoryHandler
) -> None:
    pool.register_style("red-bg", json=background_style("#f00"))
    red_tile = pool.render_tile("red-bg", z=0, x=0, y=0)
    requests: list[mapli.ResourceRequest] = []

    def handler(request: mapli.ResourceRequest) -> bytearray:
        requests.append(request)
        return bytearray(red_tile)

    memory_handler(handler)
    pool.register_style(
        "tiles", json=raster_style({"tiles": ["memory://tiles/{z}/{x}/{y}.png"]})
    )

    assert pool.render_tile("tiles", z=0, x=0, y=0) == red_tile
    # MapLibre Native may ask for a tile more than once
    tile_urls = {r.url for r in requests if r.kind == "tile"}
    assert tile_urls == {"memory://tiles/0/0/0.png"}


def test_pmtiles_through_handler(
    pool: mapli.RenderPool, memory_handler: SetMemoryHandler
) -> None:
    """MapLibre Native's PMTiles support reads an archive served by a handler, in ranges."""
    pool.register_style("red-bg", json=background_style("#f00"))
    red_tile = pool.render_tile("red-bg", z=0, x=0, y=0)
    archive = build_pmtiles(red_tile)
    serve_archive = pmtiles_handler(archive)
    requests: list[mapli.ResourceRequest] = []

    def handler(request: mapli.ResourceRequest) -> bytes:
        requests.append(request)
        return serve_archive(request)

    memory_handler(handler)
    pool.register_style(
        "archive", json=raster_style({"url": "pmtiles://memory://red.pmtiles"})
    )

    assert pool.render_tile("archive", z=0, x=0, y=0) == red_tile
    # the handler only sees range reads of the inner URL, never the pmtiles:// one
    assert {(r.url, r.kind) for r in requests} == {("memory://red.pmtiles", "source")}
    ranges = {r.data_range for r in requests}
    assert None not in ranges
    assert (0, 126) in ranges  # the header
    tile_range = (len(archive) - len(red_tile), len(archive) - 1)
    assert tile_range in ranges


def test_identical_concurrent_requests_call_handler_once(
    pool: mapli.RenderPool, memory_handler: SetMemoryHandler
) -> None:
    """While the handler serves a request, identical ones wait for its response.

    Rendering the archive's single tile into a large image makes MapLibre Native request the
    root directory and the tile several times at once. The handler is slowed down so that those
    requests overlap. The archive has a URL of its own, as MapLibre Native's PMTiles support
    caches headers and directories by URL, for the whole process.
    """
    pool.register_style("red-bg", json=background_style("#f00"))
    red_tile = pool.render_tile("red-bg", z=0, x=0, y=0)
    archive = build_pmtiles(red_tile)
    serve_archive = pmtiles_handler(archive)
    requests: list[mapli.ResourceRequest] = []

    def slow_handler(request: mapli.ResourceRequest) -> bytes:
        requests.append(request)
        time.sleep(0.05)
        return serve_archive(request)

    memory_handler(slow_handler)
    pool.register_style(
        "archive", json=raster_style({"url": "pmtiles://memory://merged.pmtiles"})
    )
    pool.render_from_bounds(
        "archive", bbox=(-180, -85, 180, 85), width=1024, height=768
    )

    calls = Counter(r.data_range for r in requests)
    tile_range = (len(archive) - len(red_tile), len(archive) - 1)
    tile_calls = calls.pop(tile_range)
    # header, root directory and metadata, each read once, though MapLibre Native requests the
    # root directory three times at once
    assert (0, 126) in calls
    assert list(calls.values()) == [1, 1, 1]
    # MapLibre Native requests the tile 6 times, in two rounds: the second round starts after
    # the first one is answered, so it calls the handler again
    assert 1 <= tile_calls <= 2


@pytest.mark.parametrize(
    "wrap",
    [
        bytes,
        bytearray,
        memoryview,
        mapli.Response,
        lambda d: mapli.Response(memoryview(d)),
    ],
)
def test_bytes_like_data(
    pool: mapli.RenderPool,
    memory_handler: SetMemoryHandler,
    wrap: Callable[[bytes], object],
) -> None:
    data = wrap(json.dumps(BOX).encode())
    memory_handler(lambda request: data)
    pool.register_style("served", json=geojson_style("memory://box.geojson"))
    pool.register_style("inline", json=geojson_style(BOX))
    assert pool.render("served", **VIEW) == pool.render("inline", **VIEW)


def test_no_content_draws_nothing(
    pool: mapli.RenderPool, memory_handler: SetMemoryHandler
) -> None:
    memory_handler(lambda request: mapli.Response.no_content())
    pool.register_style(
        "tiles", json=raster_style({"tiles": ["memory://empty/{z}/{x}/{y}.png"]})
    )
    pool.register_style("white", json=background_style("#fff"))
    assert pool.render_tile("tiles", z=0, x=0, y=0) == pool.render_tile(
        "white", z=0, x=0, y=0
    )


def test_error_reaches_render(
    pool: mapli.RenderPool, memory_handler: SetMemoryHandler
) -> None:
    memory_handler(
        lambda request: mapli.Response.error(
            mapli.ErrorReason.NOT_FOUND, "nothing stored here"
        )
    )
    pool.register_style("missing", json=geojson_style("memory://missing.geojson"))
    with pytest.raises(mapli.RenderError) as exc_info:
        pool.render("missing", **VIEW)
    message = str(exc_info.value)
    assert "memory://missing.geojson: not found: nothing stored here" in message


@pytest.mark.parametrize(
    ("handler", "expected"),
    [
        (
            lambda request: 1 / 0,
            "handler raised ZeroDivisionError: division by zero",
        ),
        (
            lambda request: 42,
            "handler returned int, expected Response or a bytes-like object",
        ),
        (
            lambda request: mapli.Response.error(
                mapli.ErrorReason.RATE_LIMIT, "slow down", retry_after=10.0
            ),
            "rate limited: slow down",
        ),
    ],
)
def test_failing_handler(
    pool: mapli.RenderPool,
    memory_handler: SetMemoryHandler,
    handler: Handler,
    expected: str,
) -> None:
    memory_handler(handler)
    pool.register_style("broken", json=geojson_style("memory://broken.geojson"))
    with pytest.raises(mapli.RenderError, match=expected):
        pool.render("broken", **VIEW)


def test_arender_with_handler(
    pool: mapli.RenderPool, memory_handler: SetMemoryHandler
) -> None:
    memory_handler(lambda request: json.dumps(BOX).encode())
    pool.register_style("served", json=geojson_style("memory://box.geojson"))
    pool.register_style("inline", json=geojson_style(BOX))
    png = asyncio.run(pool.arender("served", **VIEW))
    assert png == pool.render("inline", **VIEW)


def test_dropping_pool_while_handler_runs(memory_handler: SetMemoryHandler) -> None:
    """Dropping a pool waits for its workers, which may be waiting on a handler that needs the
    GIL: this must not deadlock."""

    def slow(request: mapli.ResourceRequest) -> bytes:
        time.sleep(0.3)
        return json.dumps(BOX).encode()

    memory_handler(slow)

    async def start_and_cancel(pool: mapli.RenderPool) -> None:
        task = asyncio.ensure_future(pool.arender("slow", **VIEW))
        await asyncio.sleep(0.1)
        task.cancel()

    pool = mapli.RenderPool()
    pool.register_style("slow", json=geojson_style("memory://slow.geojson"))
    asyncio.run(start_and_cancel(pool))
    del pool
    gc.collect()


@pytest.mark.parametrize("scheme", ["https", "PMTiles", "file", "", "1abc", "a b"])
def test_invalid_scheme(scheme: str) -> None:
    with pytest.raises(ValueError):
        mapli.register_scheme_handler(scheme, lambda request: b"")


def test_handler_must_be_callable() -> None:
    with pytest.raises(TypeError):
        mapli.register_scheme_handler("other", b"not callable")  # ty: ignore[invalid-argument-type]


def test_registration_closed_after_pool(pool: mapli.RenderPool) -> None:
    with pytest.raises(mapli.SchemeRegistrationError):
        mapli.register_scheme_handler("late", lambda request: b"")


def test_retry_after_must_be_valid() -> None:
    with pytest.raises(ValueError):
        mapli.Response.error(mapli.ErrorReason.RATE_LIMIT, "x", retry_after=-1.0)
