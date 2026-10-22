# Tiles

`RenderPool.render_tile` renders map tiles in the XYZ tiling scheme, also known as
WebMercatorQuad or "slippy map" tiles. This is the tiling scheme used by web map clients such as
Leaflet, OpenLayers and MapLibre GL JS.

## Example

```python
png_bytes = pool.render_tile("demo", z=2, x=1, y=1)
```

## Tile coordinates

- `z` is the zoom level. At zoom level `z`, the world is divided into `2**z` by `2**z` tiles.
- `x` is the column, from `0` at the west edge (180° W), increasing eastwards.
- `y` is the row, from `0` at the north edge, increasing southwards.

Coordinates outside the grid of zoom level `z` raise `ValueError`.

## Tile size

`tile_size` is the width and height of the tile, in logical pixels. It defaults to `512`, which is
MapLibre's native tile size. Most XYZ clients, such as Leaflet and OpenLayers, expect 256 pixel
tiles, which are rendered with `tile_size=256`:

```python
png_bytes = pool.render_tile("demo", z=3, x=3, y=2, tile_size=256)
```

MapLibre zoom levels are based on 512 pixel tiles, so a 256 pixel tile is drawn one zoom level
lower than its `z`. For this reason, 256 pixel tiles cannot be rendered at `z=0`, which raises
`ValueError`.

## High-density tiles

`pixel_ratio` sets the number of image pixels per logical pixel. `pixel_ratio=2` renders
high-density ("@2x") tiles: a tile with `tile_size=256, pixel_ratio=2` is 512 x 512 pixels and
covers the same area as a 256 pixel tile, with labels and lines drawn at twice their size.

`render_tile` has an `async` version, `arender_tile`, see [Async rendering](async.md).
