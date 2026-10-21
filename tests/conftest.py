import json
import struct
from collections.abc import (
    Callable,
    Iterator,
)

import pytest

import mapli

# Handlers in the tests return `object`, as some return invalid values on purpose
Handler = Callable[[mapli.ResourceRequest], object]
SetMemoryHandler = Callable[[Handler], None]

# the handler of the running test for `memory://` URLs, set with the `memory_handler` fixture
_handler: Handler | None = None


def _forward(request: mapli.ResourceRequest) -> object:
    if _handler is None:
        return mapli.Response.error(mapli.ErrorReason.NOT_FOUND, "no handler set")
    return _handler(request)


# Scheme handlers can only be registered before the first render pool exists, so tests cannot
# register their own. Instead, this one is registered on import, before any test runs, and
# forwards every request to the handler that the running test set
mapli.register_scheme_handler("memory", _forward)  # ty: ignore[invalid-argument-type]


@pytest.fixture
def memory_handler() -> Iterator[SetMemoryHandler]:
    """Set the handler for `memory://` URLs during the test."""
    global _handler

    def set_handler(handler: Handler) -> None:
        global _handler
        _handler = handler

    yield set_handler
    _handler = None


def pmtiles_handler(archive: bytes) -> Callable[[mapli.ResourceRequest], bytes]:
    """A handler serving `archive`, a PMTiles archive, answering range reads."""

    def handler(request: mapli.ResourceRequest) -> bytes:
        if request.data_range is None:
            return archive
        start, end = request.data_range  # inclusive
        return archive[start : end + 1]

    return handler


PMTILES_HEADER_LENGTH = 127
PMTILES_PNG = 2
PMTILES_NO_COMPRESSION = 1


def build_pmtiles(tile: bytes) -> bytes:
    """A PMTiles v3 archive with `tile`, a PNG, as its only tile, at z0, covering the world.

    Nothing in it is compressed. The layout is header, root directory, metadata, tile data. See
    https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md
    """
    # one entry: tile id 0 (z0), run length 1, the tile's length, offset 0 (encoded as offset + 1)
    root = b"".join(_varint(n) for n in (1, 0, 1, len(tile), 0 + 1))
    metadata = json.dumps({"name": "test"}).encode()
    root_offset = PMTILES_HEADER_LENGTH
    metadata_offset = root_offset + len(root)
    data_offset = metadata_offset + len(metadata)

    header = bytearray(PMTILES_HEADER_LENGTH)
    header[0:8] = b"PMTiles\x03"
    struct.pack_into(
        "<8Q",
        header,
        8,
        root_offset,
        len(root),
        metadata_offset,
        len(metadata),
        0,  # leaf directories offset
        0,  # leaf directories length
        data_offset,
        len(tile),
    )
    # addressed tiles, tile entries, tile contents
    struct.pack_into("<3Q", header, 72, 1, 1, 1)
    header[96] = 1  # clustered
    header[97] = PMTILES_NO_COMPRESSION  # internal compression
    header[98] = PMTILES_NO_COMPRESSION  # tile compression
    header[99] = PMTILES_PNG  # tile type
    header[100] = header[101] = 0  # min and max zoom
    # bounds as (west, south, east, north) * 10^7, then center zoom, lon and lat
    struct.pack_into("<4i", header, 102, -1800000000, -850511287, 1800000000, 850511287)
    struct.pack_into("<Bii", header, 118, 0, 0, 0)
    return bytes(header) + root + metadata + tile


def _varint(n: int) -> bytes:
    """Encode `n` as an unsigned LEB128 varint, as PMTiles directories do."""
    out = bytearray()
    while True:
        byte, n = n & 0x7F, n >> 7
        out.append(byte | (0x80 if n else 0))
        if not n:
            return bytes(out)
