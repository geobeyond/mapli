# Selecting the map area: bounding box or center and zoom

The area of the map shown in a static image is set either by a bounding box or by a center
point and a zoom level. Each option has its own `RenderPool` method.

| Method | Area shown | Pixels |
|---|---|---|
| `render_bbox` | Exactly the bounding box | Square only when the aspect of the bounding box matches the aspect of the image |
| `render_from_bounds` | The bounding box, plus extra area along one axis when the aspects differ | Square |
| `render` | The area around a center point, at a zoom level | Square |

Each method has an `async` version: `arender_bbox`, `arender_from_bounds` and `arender`. See
[Async rendering](async.md). The [API reference](../reference/mapli/index.md) lists all the
arguments of each method.

## Bounds

The bounds methods take a `bbox` argument, which follows these rules:

- `bbox` is `(west, south, east, north)`, in degrees (WGS 84). This is the same order as
  shapely's `bounds` and geopandas' `total_bounds`, which can be passed directly. Any iterable of
  four numbers is accepted.
- Latitudes must be within ±85.0511°, the limits of Web Mercator. A `south` or `north` outside
  this range raises `ValueError`; the bounding box is not clamped to the range. To render up to
  the edge of the map, clamp the latitudes to ±`mapli.MAX_LAT` before passing them.
- A `west` greater than `east` means that the bounds cross the antimeridian (180° longitude). For
  example, `bbox=(170, -20, -170, -15)` covers Fiji. Swapped coordinates are therefore not an
  error: they render the area that goes the other way around the world.

The two methods differ when the aspect of the bounding box in Web Mercator differs from the
aspect of the image.

### Exact bounds

`RenderPool.render_bbox` renders exactly the bounding box, projected to Web Mercator, and
stretches it to `width` by `height`. OGC API Maps and WMS GetMap behave the same way: clients
request adjacent bounding boxes and place the resulting images side by side.

```python
png_bytes = pool.render_bbox(
    "demo",
    bbox=(-9.5, 38.5, -9.0, 39.0),
    width=256,
    height=256,
)
```

- When the aspects differ, pixels are not square. The bounding box in the example is square in
  degrees but, at this latitude, about 1.28 times taller than wide in Web Mercator, so the map is
  stretched horizontally.
- When the aspects match, the image is the same as the one `render_from_bounds` renders.
- There are no `bearing` or `pitch` arguments, so the map cannot be rotated or tilted.

### Contained bounds

`RenderPool.render_from_bounds` chooses the center and zoom level so that the bounding box fits
in the image.

```python
png_bytes = pool.render_from_bounds(
    "demo",
    bbox=(-9.6, 36.9, -6.1, 42.2),
    width=800,
    height=600,
    padding=16,
)
```

- Pixels are square. When the aspects differ, the bounding box fills the image along one axis,
  and the image shows more than the bounding box along the other.
- `padding` is the space to leave around the bounding box on every side, in logical pixels. A
  padding that leaves no room for the bounding box raises `ValueError`.
- `bearing` and `pitch` rotate and tilt the map.

## Center and zoom

`RenderPool.render` takes the center of the image as `lon` and `lat`, in degrees (WGS 84), and a
MapLibre `zoom` level. At zoom level `z`, the width of the world is `512 * 2**z` logical pixels.

```python
png_bytes = pool.render(
    "demo",
    lon=-9.14,
    lat=38.72,
    zoom=5,
    width=800,
    height=600,
)
```

`bearing` and `pitch` rotate and tilt the map.
