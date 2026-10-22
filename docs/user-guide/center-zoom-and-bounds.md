# Center and zoom, or bounds

The area shown in a static map image is given either by a center point and a zoom level, or by a
bounding box that the image must contain.

## Center and zoom

`RenderPool.render` takes the center of the image as `lon` and `lat`, in degrees (WGS 84), and a
MapLibre `zoom` level. At zoom level `z`, the width of the world is `512 * 2**z` logical pixels.

```python
png_bytes = pool.render("demo", lon=-9.14, lat=38.72, zoom=5, width=800, height=600)
```

## Bounds

`RenderPool.render_from_bounds` takes a bounding box, and chooses the center and zoom level so
that the box fits in the image.

```python
png_bytes = pool.render_from_bounds(
    "demo", bbox=(-9.6, 36.9, -6.1, 42.2), width=800, height=600, padding=16
)
```

- `bbox` is `(west, south, east, north)`, in degrees. This is the same order as shapely's
  `bounds` and geopandas' `total_bounds`, which can be passed directly. Any iterable of four
  numbers is accepted.
- `padding` is the space to leave around the bounds on every side, in logical pixels. A padding
  that leaves no room for the bounds raises `ValueError`.
- The aspect ratio of the bounds is not changed, so the image usually shows more than the bounds
  along one axis.
- A `west` greater than `east` means that the bounds cross the antimeridian (180° longitude). For
  example, `bbox=(170, -20, -170, -15)` covers Fiji. Swapped coordinates are therefore not an
  error: they render the area that goes the other way around the world.

## Common arguments

Both methods take these arguments:

| Argument | Unit | Default | Description |
|---|---|---|---|
| `width`, `height` | logical pixels | required | Size of the image |
| `bearing` | degrees | `0.0` | Rotation of the map, clockwise from north |
| `pitch` | degrees | `0.0` | Tilt of the map, from straight down; 0-60 |
| `pixel_ratio` | image pixels per logical pixel | `1.0` | The image is `width * pixel_ratio` by `height * pixel_ratio` pixels |

```python
png_bytes = pool.render(
    "demo",
    lon=-9.14,
    lat=38.72,
    zoom=5,
    width=800,
    height=600,
    bearing=30,
    pitch=45,
    pixel_ratio=2,
)
```

Both methods have `async` versions, `arender` and `arender_from_bounds`, see
[Async rendering](async.md).
