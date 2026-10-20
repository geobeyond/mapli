"""Render a PMTiles archive kept in a private bucket, through a custom URL scheme handler.

The archive is read with [obstore](https://developmentseed.org/obstore/), using the
credentials of the environment, so the style refers to it with a stable `store://` URL that
contains no secret. Every request the handler serves is printed, with its byte range, followed by
the read from the store and how long it took. With `--cache-size`, reads are cached in memory,
and requests served from the cache print no read.

The style is made up from the archive itself: its header gives the bounds and the tile type,
and for vector tiles its metadata lists the layers, which are all drawn.

Run it like this:

```shell
uv sync --group dev

# AWS S3, with credentials from the usual AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION...
xvfb-run -a uv run python examples/private-bucket.py s3://my-bucket tiles/roads.pmtiles

# an S3-compatible store served over plain HTTP, e.g. a local MinIO
AWS_ENDPOINT_URL=http://localhost:9000 AWS_ACCESS_KEY_ID=minioadmin \\
    AWS_SECRET_ACCESS_KEY=minioadmin \\
    xvfb-run -a uv run python examples/private-bucket.py s3://my-bucket roads.pmtiles --allow-http

# Google Cloud Storage, Azure, or a local directory, which needs no credentials
xvfb-run -a uv run python examples/private-bucket.py gs://my-bucket tiles/roads.pmtiles
xvfb-run -a uv run python examples/private-bucket.py file:///data/tiles roads.pmtiles
```

`xvfb-run` is only needed on Linux machines without a display.
"""

import argparse
import functools
import gzip
import itertools
import json
import struct
import time
from pathlib import Path
from typing import Any

import obstore
from obstore.store import ObjectStore, from_url

import mapli

SCHEME = "store"

# PMTiles v3 tile types, from the header
MVT, PNG, JPEG, WEBP, AVIF = 1, 2, 3, 4, 5
COLORS = ["#e6194b", "#3cb44b", "#4363d8", "#f58231", "#911eb4", "#42d4f4", "#f032e6"]


class StoreHandler:
    """A scheme handler that serves `store://<key>` URLs from an obstore store.

    mapli calls the handler with each request. With a `cache_size`, the last `cache_size` reads
    are kept in memory, which is fine for objects that never change, as PMTiles archives usually
    don't.
    """

    def __init__(self, store: ObjectStore, cache_size: int = 0) -> None:
        self.store = store
        # a cache of size 0 keeps nothing, so every request is read from the store
        self.cached_read = functools.lru_cache(maxsize=cache_size)(self.read)

    def __call__(self, request: mapli.ResourceRequest) -> mapli.Response:
        print(f"request {request.kind} {request.url} {request.data_range}")
        key = request.url.removeprefix(f"{SCHEME}://")
        try:
            return mapli.Response(self.cached_read(key, request.data_range))
        except FileNotFoundError:
            return mapli.Response.error(
                mapli.ErrorReason.NOT_FOUND, f"no object at {key}"
            )

    def read(self, key: str, data_range: tuple[int, int] | None) -> obstore.Bytes:
        """Read `key` from the store, raising FileNotFoundError if it is missing.

        This returns data and raises on errors, so that the cache never keeps errors.
        """
        options: obstore.GetOptions = {}
        if data_range is not None:
            start, end = data_range
            # mapli's range is inclusive, obstore's is exclusive
            options["range"] = (start, end + 1)
        started = time.perf_counter()
        data = obstore.get(self.store, key, options=options).bytes()
        elapsed_ms = (time.perf_counter() - started) * 1000
        print(f"  fetched {key} {data_range}: {len(data)} bytes in {elapsed_ms:.1f} ms")
        return data


def read_header(store: ObjectStore, key: str) -> dict[str, Any]:
    """Read the fields of the PMTiles v3 header that the style needs.

    See https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md#3-header
    """
    try:
        header = bytes(obstore.get_range(store, key, start=0, length=127))
    except FileNotFoundError:
        raise SystemExit(f"no archive at {key}") from None
    if header[:7] != b"PMTiles" or header[7] != 3:
        raise SystemExit(f"{key} is not a PMTiles v3 archive")
    metadata_offset, metadata_length = struct.unpack_from("<QQ", header, 24)
    internal_compression, tile_type = header[97], header[99]
    min_zoom, max_zoom = header[100], header[101]
    west, south, east, north = (
        v / 1e7 for v in struct.unpack_from("<iiii", header, 102)
    )

    metadata = bytes(
        obstore.get_range(store, key, start=metadata_offset, length=metadata_length)
    )
    if internal_compression == 2:  # gzip
        metadata = gzip.decompress(metadata)
    return {
        "tile_type": tile_type,
        "min_zoom": min_zoom,
        "max_zoom": max_zoom,
        "bbox": (west, south, east, north),
        "metadata": json.loads(metadata) if metadata else {},
    }


def make_style(key: str, header: dict[str, Any]) -> dict[str, Any]:
    """A style that draws everything in the archive, over a white background."""
    source: dict[str, Any] = {
        "url": f"pmtiles://{SCHEME}://{key}",
        "minzoom": header["min_zoom"],
        "maxzoom": header["max_zoom"],
    }
    layers: list[dict[str, Any]] = [
        {
            "id": "background",
            "type": "background",
            "paint": {"background-color": "#fff"},
        }
    ]

    if header["tile_type"] == MVT:
        source["type"] = "vector"
        vector_layers = header["metadata"].get("vector_layers", [])
        if not vector_layers:
            raise SystemExit("the archive's metadata does not list its vector layers")
        for layer, color in zip(vector_layers, itertools.cycle(COLORS)):
            common = {"source": "archive", "source-layer": layer["id"]}
            layers += [
                {
                    **common,
                    "id": f"{layer['id']}-fill",
                    "type": "fill",
                    "filter": ["==", ["geometry-type"], "Polygon"],
                    "paint": {"fill-color": color, "fill-opacity": 0.3},
                },
                {
                    **common,
                    "id": f"{layer['id']}-line",
                    "type": "line",
                    "filter": ["!=", ["geometry-type"], "Point"],
                    "paint": {"line-color": color, "line-width": 1},
                },
                {
                    **common,
                    "id": f"{layer['id']}-circle",
                    "type": "circle",
                    "filter": ["==", ["geometry-type"], "Point"],
                    "paint": {"circle-color": color, "circle-radius": 3},
                },
            ]
    elif header["tile_type"] in (PNG, JPEG, WEBP, AVIF):
        source["type"] = "raster"
        layers.append({"id": "archive", "type": "raster", "source": "archive"})
    else:
        raise SystemExit(f"unsupported PMTiles tile type {header['tile_type']}")

    return {"version": 8, "sources": {"archive": source}, "layers": layers}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "store_url",
        help="URL of the bucket, e.g. s3://my-bucket or s3://my-bucket/prefix",
    )
    parser.add_argument("key", help="path of the .pmtiles archive within the store")
    parser.add_argument(
        "--bbox",
        nargs=4,
        type=float,
        metavar=("WEST", "SOUTH", "EAST", "NORTH"),
        help="area to render, defaults to the bounds of the archive",
    )
    parser.add_argument("--width", type=int, default=1024)
    parser.add_argument("--height", type=int, default=768)
    parser.add_argument("--output", type=Path, default=Path("private-bucket.png"))
    parser.add_argument(
        "--cache-size",
        type=int,
        default=0,
        help="number of reads to keep in memory, none by default",
    )
    parser.add_argument(
        "--allow-http",
        action="store_true",
        help="allow plain HTTP endpoints, e.g. a local MinIO",
    )
    args = parser.parse_args()

    # local stores (file://) take no client options
    if args.allow_http:
        store = from_url(args.store_url, client_options={"allow_http": True})
    else:
        store = from_url(args.store_url)
    header = read_header(store, args.key)
    print(f"archive header: {header | {'metadata': '...'}}")

    # Process-wide, and must happen before the first render pool is created
    mapli.register_scheme_handler(SCHEME, StoreHandler(store, args.cache_size))

    pool = mapli.RenderPool(workers=1)
    pool.register_style("archive", json=make_style(args.key, header))
    png = pool.render_from_bounds(
        "archive",
        bbox=args.bbox or header["bbox"],
        width=args.width,
        height=args.height,
        padding=16,
    )
    args.output.write_bytes(png)
    print(f"wrote {args.output} ({len(png)} bytes)")


if __name__ == "__main__":
    main()
