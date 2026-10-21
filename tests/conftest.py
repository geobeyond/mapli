import json
import struct
from collections.abc import Callable

import mapli

# a handler that may return anything, as some tests return invalid values on purpose
Handler = Callable[[mapli.ResourceRequest], object]


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
