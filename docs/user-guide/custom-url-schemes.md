# Custom URL schemes

MapLibre Native fetches the resources referenced by a style (style documents, sources, tiles,
fonts, sprites) by URL. It supports the `http`, `https`, `file`, `mbtiles` and `pmtiles` schemes.

mapli lets a Python function serve the requests for a URL scheme of your choosing. Such a
function is called a scheme handler. A style can then refer to data with URLs of that scheme,
and every request for one of these URLs is passed to the handler, which returns the data. URLs
of other schemes are still fetched by MapLibre Native.

This allows rendering data that MapLibre Native cannot fetch itself, e.g. data in a private
object storage bucket, a database, or data generated in Python.

## Example

The handler below serves `mem://` URLs from a dict:

```python
import mapli

STYLES = {
    "background": b"""{
        "version": 8,
        "sources": {},
        "layers": [
            {"id": "bg", "type": "background", "paint": {"background-color": "#88c"}}
        ]
    }""",
}


def read_from_memory(request: mapli.ResourceRequest) -> mapli.Response:
    key = request.url.removeprefix("mem://")
    try:
        return mapli.Response(STYLES[key])
    except KeyError:
        return mapli.Response.error(mapli.ErrorReason.NOT_FOUND, f"no data at {key}")


mapli.register_scheme_handler("mem", read_from_memory)

pool = mapli.RenderPool()
pool.register_style("background", url="mem://background")
png_bytes = pool.render("background", lon=0, lat=0, zoom=1, width=256, height=256)
```

The [private bucket example](../examples/private-bucket.md) uses a handler to read a PMTiles
archive from an S3 bucket with [obstore].

## Requests

A handler is called with a `ResourceRequest`, which has these attributes:

- `url`: the full URL, including the scheme;
- `kind`: what is requested: `"style"`, `"source"`, `"tile"`, `"glyphs"`, `"sprite_image"`,
  `"sprite_json"`, `"image"` or `"unknown"`;
- `data_range`: for partial reads, the inclusive `(start, end)` byte range to read, else `None`.

### PMTiles archives

[PMTiles] is a single-file archive format for map tiles. MapLibre Native reads PMTiles archives
with partial reads, and the URL of the archive can use a custom scheme. In the style, the source
URL is then `pmtiles://` followed by the custom URL:

```json
{"type": "vector", "url": "pmtiles://store://tiles/roads.pmtiles"}
```

The handler is then called with `url="store://tiles/roads.pmtiles"`, `kind="source"` and a
`data_range` for each part of the archive that MapLibre Native reads. Note that `end` is
inclusive, while many APIs, including Python slicing, take an exclusive end:

```python
start, end = request.data_range
data = archive_bytes[start : end + 1]
```

## Responses

A handler returns one of:

| Return value | Effect |
|---|---|
| `mapli.Response(data)`, or just `data` | The request succeeds with `data`, which can be any bytes-like object (`bytes`, `bytearray`, `memoryview`, obstore's `Bytes`) |
| `mapli.Response.no_content()` | The request succeeds with no data. A missing tile is drawn as empty |
| `mapli.Response.error(reason, message)` | The request fails, which fails the render with a `RenderError` that includes `message` |

`reason` is an `ErrorReason`: `NOT_FOUND`, `SERVER`, `CONNECTION`, `RATE_LIMIT` or `OTHER`. For
`RATE_LIMIT`, `retry_after` gives the number of seconds after which the request can be retried.

An exception raised by the handler also fails the render with a `RenderError`.

## Registration

- Handlers are process-wide: a handler serves the requests of every render pool.
- Handlers can be registered at any time, before or after render pools are created.
- Registering a scheme again replaces its handler. `mapli.unregister_scheme_handler(scheme)`
  removes it. In both cases, only the requests made from then on are affected.
- The schemes that MapLibre Native serves itself (`http`, `https`, `file`, `mbtiles`, `pmtiles`)
  cannot be given a handler.
- mapli receives the requests for custom schemes through MapLibre Native's handling of the
  `asset` scheme. Once a `RenderPool` has been created, `asset://` URLs are therefore not
  supported. The `asset` scheme is mostly used on mobile platforms so not this is not 
  likely to be a relevant issue for mapli usage.

## Threads

- Handlers run on threads owned by mapli.
  Requests can be served concurrently if the custom handler's I/O releases the GIL. [obstore], the
  standard library's sockets all release the GIL and are well suited for being used in a 
  custom handler.
- Handlers can block while they fetch data, and must be thread-safe.
- Values stored in a `threading.local()` do not persist between handler calls, as each call gets
  a fresh Python thread state. Clients that a handler uses across calls must therefore be
  thread-safe, so that they can be shared. obstore's stores are.
- A request that is no longer needed is skipped if its handler has not started yet. Once started,
  the handler runs to the end.

## Caching

MapLibre Native often makes several identical requests (same URL and byte range) at the same
time. Mapli implements a deduplication method that works by making similarly targeted 
requests wait for a currently in-progress one. This reduces the amount of requests that need to
be handled via the custom handler. mapli does not cache responses beyond that. Some data is 
kept by MapLibre Native:

- each renderer keeps the tiles it has loaded;
- MapLibre Native's PMTiles support keeps the header, metadata and directories of each archive
  it has read.

Different renderers (e.g. on different workers) still request the same data separately. To avoid
reading the same data more than once, cache it in the handler.

??? example "Caching in a handler"

    Keep the fetching in a function that returns the data and raises on errors, so that errors
    are not cached, and wrap it in e.g. `functools.lru_cache`. This is suitable for data that
    does not change, as is usually the case for PMTiles archives:

    ```python
    from functools import lru_cache

    import obstore
    from obstore.store import S3Store

    import mapli

    store = S3Store.from_url("s3://my-private-bucket", region="eu-west-1")


    @lru_cache(maxsize=1024)
    def read_data(key: str, data_range: tuple[int, int] | None) -> obstore.Bytes:
        options: obstore.GetOptions = {}
        if data_range is not None:
            start, end = data_range
            options["range"] = (start, end + 1)
        return obstore.get(store, key, options=options).bytes()


    def read_from_store(request: mapli.ResourceRequest) -> mapli.Response:
        key = request.url.removeprefix("store://")
        try:
            return mapli.Response(read_data(key, request.data_range))
        except FileNotFoundError:
            return mapli.Response.error(mapli.ErrorReason.NOT_FOUND, f"no object at {key}")


    mapli.register_scheme_handler("store", read_from_store)
    ```

### Replacing data

Because the data above is cached by URL, replacing a handler, or changing the data it serves,
does not make renderers fetch data they have already loaded. mapli provides no way to discard
that data; MapLibre Native's PMTiles cache is not accessible to mapli.

To make renderers fetch new data, refer to it with a new URL, e.g. by adding a version to the
path (`pmtiles://store://v2/tiles/roads.pmtiles`) and having the handler remove the version
before reading the data.


[obstore]: https://developmentseed.org/obstore/latest/
[PMTiles]: https://docs.protomaps.com/pmtiles/
